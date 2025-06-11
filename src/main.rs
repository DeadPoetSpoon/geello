#[cfg(feature = "server")]
use geello::{AreaRenderer, LineRenderer, PointRenderer, RenderOption, RenderRegion};
use rocket::{State, fs::NamedFile};
use ron::ser::PrettyConfig;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::{
    io::{BufReader, BufWriter},
    thread::sleep,
};
use vello::wgpu::Device;
use vello::wgpu::Queue;
use vello::wgpu::Texture;
use vello::wgpu::{self, Extent3d};

use image::{
    DynamicImage, ImageBuffer, ImageReader,
    codecs::png::{PngDecoder, PngEncoder},
};
use image::{ExtendedColorType, ImageEncoder};
use rocket::response::stream::ByteStream;
use rocket::response::stream::ReaderStream;
use std::io::{Cursor, Read};
use tokio::io::BufStream;

use image::ImageFormat;
use rocket::http::ContentType;
#[macro_use]
extern crate rocket;

#[cfg(feature = "server")]
#[get("/<x>/<y>/<z>")]
async fn index(
    x: u32,
    y: u32,
    z: u32,
    device: &State<Device>,
    queue: &State<Queue>,
    texture_vec: &State<Arc<Mutex<Vec<Texture>>>>,
) -> Result<(ContentType, Vec<u8>), String> {
    let geojson = PathBuf::from("assets/test/polygon.geojson");
    let render_option_path = PathBuf::from("assets/test/render_option.ron");
    let option_str = std::fs::read_to_string(render_option_path).map_err(|e| e.to_string())?;
    let mut render_option: RenderOption =
        ron::de::from_str(&option_str).map_err(|e| e.to_string())?;
    render_option.region = RenderRegion::TileIndex(x, y, z);
    let out = PathBuf::from("assets/test/output.png");
    // geello::render_geojson_file_to_image(geojson.as_path(), &render_option, out.as_path())
    //     .await
    //     .map_err(|e| String::from(e.to_string()))?;
    let texture_vec = Arc::clone(texture_vec);
    let mut texture = None;
    loop {
        let mut v = texture_vec.lock().unwrap();
        let x = v.pop();
        if x.is_some() {
            texture = x;
            drop(v);
            break;
        }
        drop(v);
        let time = tokio::time::Duration::from_millis(100);
        sleep(time);
    }
    let texture = texture.unwrap();
    let buffer = geello::render_geojson_file_to_buffer(
        geojson.as_path(),
        device,
        queue,
        &texture,
        &render_option,
    )
    .await
    .map_err(|e| e.to_string())?;
    let mut v = texture_vec.lock().unwrap();
    v.push(texture);
    drop(v);
    // let mut buffer_img = Vec::with_capacity(buffer.len());
    // let dencoder = PngEncoder::new(&mut buffer_img);
    // dencoder
    //     .write_image(&buffer, 256, 256, ExtendedColorType::Rgba32F)
    //     .map_err(|e| e.to_string())?;
    let mut image_buffer = Vec::with_capacity(buffer.len() * 4);
    let mut cursor = Cursor::new(&mut image_buffer);
    let image = image::RgbaImage::from_raw(256, 256, buffer).unwrap();
    image
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok((ContentType::PNG, image_buffer))
}

#[cfg(feature = "server")]
#[launch]
async fn rocket() -> _ {
    let mut rocket = rocket::build().mount("/", routes![index]);
    let mut context = vello::util::RenderContext::new();
    match context.device(None).await {
        Some(device_id) => {
            let texture_vec = Arc::new(Mutex::new(Vec::new()));
            let init = Arc::clone(&texture_vec);
            let mut texture_vec_mut = init.lock().unwrap();
            for i in 0..10 {
                let label = format!("WMTS Rendered Texture {}", i);
                let texture_desc = wgpu::TextureDescriptor {
                    label: Some(&label),
                    size: Extent3d {
                        width: 256,
                        height: 256,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                };
                let texture = context.devices[device_id]
                    .device
                    .create_texture(&texture_desc);
                texture_vec_mut.push(texture);
            }
            let device = context.devices[device_id].device.clone();
            let queen = context.devices[device_id].queue.clone();
            rocket = rocket.manage(device);
            rocket = rocket.manage(queen);
            rocket = rocket.manage(texture_vec);
        }
        None => {
            log::error!("No compatible device found")
        }
    };
    rocket
}
