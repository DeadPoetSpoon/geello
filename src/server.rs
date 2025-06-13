use geello::{
    RenderOption, RenderRegion, RenderedGeometry,
    utils::{self, transform_4326_to_3857_point},
};
use geo::BoundingRect;
use geojson::GeoJson;
use rocket::{
    Build, Rocket,
    fairing::AdHoc,
    figment::{
        Figment, Profile,
        providers::{Env, Format, Serialized, Toml},
    },
};
use rocket::{State, fs::NamedFile};
use std::thread::sleep;
use std::{fs::File, num::NonZero, path::PathBuf, str::FromStr};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use vello::wgpu::Texture;
use vello::wgpu::{self, Extent3d};
use vello::{Renderer, wgpu::Queue};
use vello::{kurbo::Affine, wgpu::Device};

use image::ImageBuffer;
use image::{ImageFormat, Rgba};
use rocket::http::ContentType;
use std::io::{Cursor, Read};

pub async fn rocket() -> Rocket<Build> {
    let figment = Figment::from(rocket::Config::default())
        .merge(Serialized::defaults(Config::default()))
        .merge(Toml::file("Geello.toml").nested())
        .merge(Env::prefixed("GEELLO_").global())
        .select(Profile::from_env_or("GEELLO_PROFILE", "default"));
    let mut rocket = rocket::custom(figment);
    rocket = rocket.attach(AdHoc::config::<Config>());
    let config: Config = rocket.figment().extract().expect("read config errors.");
    // rocket = rocket.mount("/data", FileServer::from(config.data_path));
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
    rocket = rocket.manage(Instant::now());
    rocket = rocket.mount(
        "/",
        routes![wmts_real_time, wmts_cache, wms_real_time, anim_real_time],
    );
    rocket
}

#[get("/anim?<param..>")]
async fn anim_real_time(
    param: WebMapServiceQueryParam,
    device: &State<Device>,
    queue: &State<Queue>,
    instant: &State<Instant>,
    config: &State<Config>,
) -> Result<(ContentType, Vec<u8>), String> {
    let WebMapServiceQueryParam {
        layers,
        styles,
        width,
        height,
        format,
        bbox,
    } = param;
    let (geojson, mut render_option) = get_data_from_fs(config, &layers, &styles)?;
    render_option.pixel_option.width = width;
    render_option.pixel_option.height = height;
    render_option.region = convert_bbox(bbox, render_option.need_proj_geom);
    let i = (instant.elapsed().as_secs() % 10) as f64;
    render_option
        .renderers
        .iter_mut()
        .for_each(|type_renderer| match type_renderer {
            geello::GeometryRenderer::Point(point_renderer) => {
                point_renderer.radius = point_renderer.radius * i;
            }
            geello::GeometryRenderer::Line(_) => {}
            geello::GeometryRenderer::Area(_) => {}
        });
    let image = render_wms(&geojson, device, queue, config, &mut render_option).await?;
    let image_format = convert_format(format);
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

#[get("/wms?<param..>")]
async fn wms_real_time(
    param: WebMapServiceQueryParam,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
) -> Result<(ContentType, Vec<u8>), String> {
    let WebMapServiceQueryParam {
        layers,
        styles,
        width,
        height,
        format,
        bbox,
    } = param;
    let (geojson, mut render_option) = get_data_from_fs(config, &layers, &styles)?;
    render_option.need_proj_geom = false;
    render_option.pixel_option.width = width;
    render_option.pixel_option.height = height;
    render_option.region = convert_bbox(bbox, render_option.need_proj_geom);
    let image = render_wms(&geojson, device, queue, config, &mut render_option).await?;
    let image_format = convert_format(format);
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

#[get("/wmts/real-time?<param..>")]
async fn wmts_real_time(
    param: WebMapTileServiceQueryParam,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    texture_vec: &State<Arc<Mutex<Vec<Texture>>>>,
) -> Result<(ContentType, Vec<u8>), String> {
    let WebMapTileServiceQueryParam {
        layers,
        styles,
        x,
        y,
        z,
        format,
    } = param;
    let (geojson, mut render_option) = get_data_from_fs(config, &layers, &styles)?;
    render_option.region = RenderRegion::TileIndex(x, y, z);
    let image =
        render_wmts_tile(&geojson, device, queue, config, texture_vec, &render_option).await?;
    let image_format = convert_format(format);
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

#[get("/wmts/cache?<param..>")]
async fn wmts_cache(
    param: WebMapTileServiceQueryParam,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    texture_vec: &State<Arc<Mutex<Vec<Texture>>>>,
) -> Result<(ContentType, NamedFile), String> {
    let WebMapTileServiceQueryParam {
        layers,
        styles,
        x,
        y,
        z,
        format: _,
    } = param;
    let (dir, file_name) = get_image_path(&layers, &styles, x, y, z, config);
    let path = PathBuf::from(format!("{dir}/{file_name}"));
    if !path.exists() {
        let (geojson, mut render_option) = get_data_from_fs(config, &layers, &styles)?;
        render_option.region = RenderRegion::TileIndex(x, y, z);
        let image =
            render_wmts_tile(&geojson, device, queue, config, texture_vec, &render_option).await?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("create dir failed: {}", e.to_string()))?;
        image
            .save(path.as_path())
            .map_err(|e| format!("sava image failed: {}", e.to_string()))?;
    }
    let f = NamedFile::open(path)
        .await
        .map_err(|e| format!("open image cache failed: {}", e.to_string()))?;
    return Ok((ContentType::PNG, f));
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

#[derive(Debug, FromForm)]
struct WebMapServiceQueryParam {
    #[field(name = uncase("layers"))]
    #[field(name = uncase("layer"))]
    layers: String,
    #[field(name = uncase("styles"))]
    styles: String,
    #[field(name = uncase("width"))]
    width: u32,
    #[field(name = uncase("height"))]
    height: u32,
    #[field(name = uncase("format"))]
    format: Option<String>,
    #[field(name = uncase("bbox"))]
    bbox: Option<String>,
}

#[derive(Debug, FromForm)]
struct WebMapTileServiceQueryParam {
    #[field(name = uncase("layers"))]
    #[field(name = uncase("layer"))]
    layers: String,
    #[field(name = uncase("styles"))]
    styles: String,
    #[field(name = uncase("x"))]
    x: u32,
    #[field(name = uncase("y"))]
    y: u32,
    #[field(name = uncase("z"))]
    z: u32,
    #[field(name = uncase("format"))]
    format: Option<String>,
}

fn convert_format(format_str: Option<String>) -> ImageFormat {
    match format_str {
        Some(format_str) => match ImageFormat::from_mime_type(&format_str) {
            Some(format) => format,
            None => match ImageFormat::from_extension(&format_str) {
                Some(format) => format,
                None => ImageFormat::Png,
            },
        },
        None => ImageFormat::Png,
    }
}

fn convert_bbox(bbox_str: Option<String>, need_proj: bool) -> RenderRegion {
    if bbox_str.is_none() {
        return RenderRegion::All;
    }
    let bbox_str = bbox_str.unwrap();
    let parts: Vec<&str> = bbox_str.split(',').collect();
    if parts.len() != 4 {
        return RenderRegion::All;
    }
    if let (Ok(min_x), Ok(min_y), Ok(max_x), Ok(max_y)) = (
        parts[0].parse::<f64>(),
        parts[1].parse::<f64>(),
        parts[2].parse::<f64>(),
        parts[3].parse::<f64>(),
    ) {
        if need_proj {
            let (min_x, max_y) = transform_4326_to_3857_point(min_x, max_y);
            let (max_x, min_y) = transform_4326_to_3857_point(max_x, min_y);
            RenderRegion::Rect(geo::Rect::new((min_x, max_y), (max_x, min_y)))
        } else {
            RenderRegion::Rect(geo::Rect::new((min_x, max_y), (max_x, min_y)))
        }
    } else {
        RenderRegion::All
    }
}

fn get_image_path(
    data_path: &str,
    style_path: &str,
    x: u32,
    y: u32,
    z: u32,
    config: &State<Config>,
) -> (String, String) {
    let dir = config.data_path.join(&config.cache_path);
    let dir = dir.to_str().unwrap_or("cache");
    (
        format!("{dir}/{data_path}/{style_path}/{z}/{x}"),
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

async fn render_wms(
    geojson: &GeoJson,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    render_option: &mut RenderOption,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, String> {
    let rect = match render_option.region {
        RenderRegion::All => match &geojson {
            GeoJson::Geometry(geometry) => {
                let geom = geo_types::Geometry::<f64>::try_from(geometry)
                    .map_err(|e| format!("convert geojson failed: {}", e.to_string()))?;
                geom.bounding_rect()
            }
            GeoJson::Feature(feature) => match feature.geometry.as_ref() {
                Some(geom) => {
                    let geom = geo_types::Geometry::<f64>::try_from(geom)
                        .map_err(|e| format!("convert geojson failed: {}", e.to_string()))?;
                    geom.bounding_rect()
                }
                None => None,
            },
            GeoJson::FeatureCollection(feature_collection) => {
                let mut has_rect = false;
                let mut b_x_min = f64::MAX;
                let mut b_x_max = f64::MIN;
                let mut b_y_min = f64::MAX;
                let mut b_y_max = f64::MIN;
                for feature in feature_collection.features.iter() {
                    if let Some(geom) = &feature.geometry {
                        let geom = geo_types::Geometry::<f64>::try_from(geom)
                            .map_err(|e| format!("convert geojson failed: {}", e.to_string()))?;
                        if let Some(rect) = geom.bounding_rect() {
                            let rect_min = rect.min();
                            let rect_max = rect.max();
                            (b_x_min, b_y_max, b_x_max, b_y_min) = (
                                b_x_min.min(rect_min.x),
                                b_y_max.max(rect_max.y),
                                b_x_max.max(rect_max.x),
                                b_y_min.min(rect_min.y),
                            );
                            has_rect = true;
                        }
                    }
                }
                if has_rect {
                    Some(geo::Rect::new((b_x_min, b_y_max), (b_x_max, b_y_min)))
                } else {
                    None
                }
            }
        },
        _ => None,
    };

    if rect.is_some() {
        render_option.region = RenderRegion::Rect(rect.unwrap());
    }
    let mut renderer = vello::Renderer::new(
        &device,
        vello::RendererOptions {
            num_init_threads: config.shader_init_threads,
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .expect("Got non-Send/Sync error from creating renderer");
    let buffer = render_geojson_to_buffer_with_new_texture(
        geojson,
        device,
        queue,
        &mut renderer,
        Affine::IDENTITY,
        render_option,
    )
    .await
    .map_err(|e| e.to_string())?;
    let (width, height) = render_option.get_pixel_size();
    image::RgbaImage::from_raw(width, height, buffer).ok_or(String::from("render image error"))
}

fn get_one_texture(texture_vec: &Arc<Mutex<Vec<Texture>>>) -> Texture {
    loop {
        let mut v = texture_vec.lock().unwrap();
        let x = v.pop();
        if x.is_some() {
            drop(v);
            return x.unwrap();
        }
        drop(v);
        let time = tokio::time::Duration::from_millis(100);
        sleep(time);
    }
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
    let texture = get_one_texture(&texture_vec);
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

    let buffer = render_geojson_to_buffer(
        geojson,
        device,
        queue,
        &mut renderer,
        &texture,
        Affine::IDENTITY,
        &render_option,
    )
    .await
    .map_err(|e| e.to_string())?;
    let mut v = texture_vec.lock().unwrap();
    v.push(texture);
    drop(v);
    image::RgbaImage::from_raw(256, 256, buffer).ok_or(String::from("render image error"))
}

async fn render_geojson_to_buffer_with_new_texture(
    geojson: &GeoJson,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    transform: Affine,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let mut geom_to_render_vec = get_geom_from_geojson(geojson)?;
    if option.need_proj_geom {
        geom_to_render_vec.iter_mut().for_each(|mut geom| {
            utils::transform(&mut geom, &option.tile_proj);
        });
    }
    let mut geom_s: Vec<RenderedGeometry> =
        geom_to_render_vec.iter().map(|geom| geom.into()).collect();
    geello::render_to_buffer_with_new_texture(
        &mut geom_s,
        device,
        queue,
        renderer,
        transform,
        option,
    )
}

async fn render_geojson_to_buffer(
    geojson: &GeoJson,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    transform: Affine,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let mut geom_to_render_vec = get_geom_from_geojson(geojson)?;
    if option.need_proj_geom {
        geom_to_render_vec.iter_mut().for_each(|mut geom| {
            geello::utils::transform(&mut geom, &option.tile_proj);
        });
    }
    let mut geom_s: Vec<RenderedGeometry> =
        geom_to_render_vec.iter().map(|geom| geom.into()).collect();
    geello::render_to_buffer(
        &mut geom_s,
        device,
        queue,
        renderer,
        texture,
        transform,
        option,
    )
}

fn get_geom_from_geojson(geojson: &GeoJson) -> anyhow::Result<Vec<geo_types::Geometry>> {
    let mut geom_to_render_vec = Vec::new();
    match geojson {
        GeoJson::Geometry(geometry) => {
            let geom = geo_types::Geometry::<f64>::try_from(geometry)?;
            geom_to_render_vec.push(geom);
        }
        GeoJson::Feature(feature) => {
            let geom = feature
                .geometry
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("only feature has no geometry"))?;
            let geom = geo_types::Geometry::<f64>::try_from(geom)?;
            geom_to_render_vec.push(geom);
        }
        GeoJson::FeatureCollection(feature_collection) => {
            for (index, feature) in feature_collection.features.iter().enumerate() {
                let geom = feature
                    .geometry
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("feature (index:{index}) has no geometry"))?;
                let geom = geo_types::Geometry::<f64>::try_from(geom)?;
                geom_to_render_vec.push(geom);
            }
        }
    };
    Ok(geom_to_render_vec)
}

// async fn render_geojson_file_to_buffer<P: AsRef<std::path::Path>>(
//     path: P,
//     device: &Device,
//     queue: &Queue,
//     renderer: &mut Renderer,
//     texture: &Texture,
//     option: &RenderOption,
// ) -> anyhow::Result<Vec<u8>> {
//     let mut file = File::open(path)?;
//     let mut geojson_str = String::new();
//     let _ = file.read_to_string(&mut geojson_str);
//     let geojson = geojson_str.parse::<GeoJson>().unwrap();
//     render_geojson_to_buffer(
//         &geojson,
//         device,
//         queue,
//         renderer,
//         texture,
//         Affine::IDENTITY,
//         option,
//     )
//     .await
// }

// fn render_to_image<P: AsRef<std::path::Path>>(
//     geoms: &mut Vec<RenderedGeometry>,
//     device: &Device,
//     queue: &Queue,
//     renderer: &mut Renderer,
//     option: &RenderOption,
//     path: P,
// ) -> anyhow::Result<()> {
//     let data = geello::render_to_buffer_with_new_texture(
//         geoms,
//         device,
//         queue,
//         renderer,
//         Affine::IDENTITY,
//         option,
//     )?;
//     let (width, height) = option.get_pixel_size();
//     let image = image::RgbaImage::from_raw(width, height, data)
//         .ok_or_else(|| anyhow::anyhow!("create image error."))?;
//     image.save(path)?;
//     Ok(())
// }
