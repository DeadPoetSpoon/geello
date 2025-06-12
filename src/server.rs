use geello::{AreaRenderer, LineRenderer, PointRenderer, RenderOption, RenderRegion};
use geojson::GeoJson;
use log::LevelFilter;
use rocket::{
    Build, Rocket,
    fairing::AdHoc,
    figment::{
        Figment, Profile,
        providers::{Env, Format, Serialized, Toml},
    },
    fs::FileServer,
    response::Redirect,
};
use rocket::{State, fs::NamedFile};
use ron::ser::PrettyConfig;
use std::sync::{Arc, Mutex};
use std::{fs::File, num::NonZero};
use std::{
    io::{BufReader, BufWriter},
    thread::sleep,
};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
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
use image::{ImageFormat, Rgba};
use rocket::http::ContentType;
use rocket::response::stream::ByteStream;
use rocket::response::stream::ReaderStream;
use std::io::{Cursor, Read};
use tokio::io::BufStream;
use tokio::sync::RwLock;
use vello::Renderer;

#[get("/wmts/real-time/<data_path>/<style_path>/<x>/<y>/<z>/<format>")]
async fn wmts_real_time(
    data_path: &str,
    style_path: &str,
    x: u32,
    y: u32,
    z: u32,
    format: Option<&str>,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    texture_vec: &State<Arc<Mutex<Vec<Texture>>>>,
) -> Result<(ContentType, Vec<u8>), String> {
    let (geojson, mut render_option) = get_data_from_fs(config, data_path, style_path)?;
    render_option.region = RenderRegion::TileIndex(x, y, z);
    let image =
        render_wmts_tile(&geojson, device, queue, config, texture_vec, &render_option).await?;
    let image_format = match format {
        Some(format) => {
            let format = ImageFormat::from_extension(format);
            match format {
                Some(format) => format,
                None => ImageFormat::Png,
            }
        }
        None => ImageFormat::Png,
    };
    let size = image.width() * image.height() * 4;
    let buffer = Vec::with_capacity(size as usize);
    let mut cursor = Cursor::new(buffer);
    image
        .write_to(&mut cursor, image_format)
        .map_err(|e| format!("encode image faild: {}", e.to_string()))?;
    let content_type = ContentType::from_str(image_format.to_mime_type())
        .map_err(|e| format!("error image format: {}", e.to_string()))?;
    Ok((content_type, cursor.into_inner()))
}

// #[get("/wmts/cache/<data_path>/<style_path>/<x>/<y>/<z>")]
// async fn wmts_cache(
//     data_path: &str,
//     style_path: &str,
//     x: u32,
//     y: u32,
//     z: u32,
//     device: &State<Device>,
//     queue: &State<Queue>,
//     config: &State<Config>,
//     texture_vec: &State<Arc<Mutex<Vec<Texture>>>>,
// ) -> Redirect {
// }
fn get_image_path(
    data_path: &str,
    style_path: &str,
    x: u32,
    y: u32,
    z: u32,
    cache_path: &str,
) -> (String, String) {
    (
        format!("{cache_path}/{data_path}/{style_path}/{z}/{x}"),
        format!("{y}.png"),
    )
}
fn get_data_from_fs(
    config: &State<Config>,
    data_path: &str,
    style_path: &str,
) -> Result<(GeoJson, RenderOption), String> {
    let geojson_path = config.data_path.join(data_path);
    if !geojson_path.exists() {
        return Err(format!("can not find {}", data_path));
    }
    let mut file = File::open(geojson_path.as_path())
        .map_err(|e| format!("open {} failed: {}", geojson_path.display(), e.to_string()))?;
    let mut geojson_str = String::new();
    let _ = file.read_to_string(&mut geojson_str);
    let geojson = geojson_str.parse::<GeoJson>().map_err(|e| {
        format!(
            "convert {} failed: {}",
            geojson_path.display(),
            e.to_string()
        )
    })?;

    let style_path = match style_path.ends_with(".ron") {
        true => style_path.to_string(),
        false => format!("{}.ron", style_path),
    };
    let render_option_path = config.data_path.join(style_path);
    if !render_option_path.exists() {
        return Err(format!("can not find {}", render_option_path.display()));
    }
    let option_str = std::fs::read_to_string(render_option_path.as_path()).map_err(|e| {
        format!(
            "open {} failed: {}",
            render_option_path.display(),
            e.to_string()
        )
    })?;
    let render_option: RenderOption = ron::de::from_str(&option_str).map_err(|e| {
        format!(
            "convert {} failed: {}",
            render_option_path.display(),
            e.to_string()
        )
    })?;
    Ok((geojson, render_option))
}

async fn render_wmts_tile(
    geojson: &GeoJson,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    texture_vec: &State<Arc<Mutex<Vec<Texture>>>>,
    render_option: &RenderOption,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, String> {
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
    // vello::Renderer is !Sync can not be shared between threads
    let mut renderer = vello::Renderer::new(
        &device,
        vello::RendererOptions {
            num_init_threads: config.shader_init_threads,
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .expect("Got non-Send/Sync error from creating renderer");

    let buffer = geello::render_geojson_to_buffer(
        geojson,
        device,
        queue,
        &mut renderer,
        &texture,
        &render_option,
    )
    .await
    .map_err(|e| e.to_string())?;
    let mut v = texture_vec.lock().unwrap();
    v.push(texture);
    drop(v);
    image::RgbaImage::from_raw(256, 256, buffer).ok_or(String::from("render image error"))
}

#[get("/<x>/<y>/<z>")]
async fn index(
    x: u32,
    y: u32,
    z: u32,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    texture_vec: &State<Arc<Mutex<Vec<Texture>>>>,
) -> Result<(ContentType, Vec<u8>), String> {
    // TODO: 添加 本地缓存 与 浏览器缓存

    let geojson = PathBuf::from("assets/test/china.geo.json");
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
    // vello::Renderer is !Sync can not be shared between threads
    let mut renderer = vello::Renderer::new(
        &device,
        vello::RendererOptions {
            num_init_threads: config.shader_init_threads,
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .expect("Got non-Send/Sync error from creating renderer");

    let buffer = geello::render_geojson_file_to_buffer(
        geojson.as_path(),
        device,
        queue,
        &mut renderer,
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

pub async fn rocket() -> Rocket<Build> {
    let figment = Figment::from(rocket::Config::default())
        .merge(Serialized::defaults(Config::default()))
        .merge(Toml::file("Geello.toml").nested())
        .merge(Env::prefixed("GEELLO_").global())
        .select(Profile::from_env_or("GEELLO_PROFILE", "default"));
    let mut rocket = rocket::custom(figment);
    rocket = rocket.attach(AdHoc::config::<Config>());
    let config: Config = rocket.figment().extract().expect("read config errors.");
    rocket = rocket.mount("/data", FileServer::from(config.data_path));
    let mut context = vello::util::RenderContext::new();
    match context.device(None).await {
        Some(device_id) => {
            let texture_vec = Arc::new(Mutex::new(Vec::new()));
            let init = Arc::clone(&texture_vec);
            let mut texture_vec_mut = init.lock().unwrap();
            for i in 0..config.wmts_texture_count {
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
            log::error!("No compatible device found, geello need GPU to render");
            panic!("No compatible device found, geello need GPU to render");
        }
    };
    rocket = rocket.mount("/", routes![index, wmts_real_time]);
    rocket
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Config {
    data_path: PathBuf,
    wmts_texture_count: u32,
    shader_init_threads: Option<NonZero<usize>>,
    cache_path: PathBuf,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            data_path: "data".into(),
            wmts_texture_count: 100,
            shader_init_threads: None,
            cache_path: "cache".into(),
        }
    }
}
