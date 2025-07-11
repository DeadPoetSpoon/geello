use geello::{
    MagicFetcher, MagicValue, RenderOption, RenderRegion, RenderedGeometry,
    utils::transform_4326_to_3857_point,
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
use std::{collections::HashMap, thread::sleep, time::Duration};
use std::{fs::File, num::NonZero, path::PathBuf, str::FromStr};
use std::{sync::Arc, time::Instant};
use vello::wgpu::Texture;
use vello::wgpu::{self, Extent3d};
use vello::{Renderer, wgpu::Queue};
use vello::{kurbo::Affine, wgpu::Device};

use image::ImageBuffer;
use image::{ImageFormat, Rgba};
use rocket::http::ContentType;
use std::io::{Cursor, Read};
use tokio::sync::{Mutex, RwLock};

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
            let mut texture_vec = Vec::new();
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
                texture_vec.push(texture);
            }
            let texture_vec = Arc::new(Mutex::new(texture_vec));
            let device = context.devices[device_id].device.clone();
            let queen = context.devices[device_id].queue.clone();
            rocket = rocket.manage(device);
            rocket = rocket.manage(queen);
            rocket = rocket.manage(texture_vec);
        }
        None => {
            ("No compatible device found, geello need GPU to render");
            panic!("No compatible device found, geello need GPU to render");
        }
    };
    rocket = rocket.manage(Instant::now());
    let data_cache = Arc::new(RwLock::new(DataCache::default()));
    let data_cache_check = Arc::clone(&data_cache);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            let mut data_cache = data_cache_check.write().await;
            data_cache.check_expired_cache();
            drop(data_cache);
        }
    });
    rocket = rocket.manage(data_cache);
    rocket = rocket.mount(
        "/",
        routes![
            wmts_real_time,
            wmts_cache,
            wms_real_time,
            anim_real_time_websocket,
            web_map,
            get_render_option_example,
            show_data_cache,
        ],
    );
    rocket
}

#[get("/show-data-cache")]
async fn show_data_cache(data_cache: &State<Arc<RwLock<DataCache>>>) -> Result<String, String> {
    let cache = data_cache.read().await;
    let mut cache_str = vec!["layers:".to_string()];
    cache.layer_map.iter().for_each(|(name, layer)| {
        cache_str.push(format!(
            "name:{:?},kind:{:?},expira:{:?}",
            name, layer.kind, layer.expiration
        ));
    });
    cache_str.push("styles:".to_string());
    cache.style_map.iter().for_each(|(name, style)| {
        cache_str.push(format!(
            "name:{:?},kind:{:?},expira:{:?}",
            name, style.kind, style.expiration
        ));
    });
    drop(cache);
    Ok(cache_str.join("\n"))
}

#[get("/example/<name>")]
async fn get_render_option_example(name: String) -> Result<String, String> {
    if name == "area-renderer" {
        let area_renderer = geello::AreaRenderer::default();
        let area_renderer = geello::GeometryRenderer::Area(
            geello::RenderedGeometryFilter::None.into(),
            area_renderer.into(),
        );
        let area_renderer = MagicValue::new(area_renderer);
        ron::ser::to_string_pretty(&area_renderer, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("Ser error: {}", e.to_string()))
    } else {
        let mut option = geello::RenderOption::default();
        option
            .renderers
            .push(MagicValue::new_ron("assets/test/area.ron".to_string()));
        option.renderers.push(
            geello::GeometryRenderer::Line(
                geello::RenderedGeometryFilter::None.into(),
                geello::LineRenderer::default().into(),
            )
            .into(),
        );
        option.renderers.push(
            geello::GeometryRenderer::Point(
                geello::RenderedGeometryFilter::Layer("some_layer".to_string()).into(),
                geello::PointRenderer::default().into(),
            )
            .into(),
        );
        let option: MagicValue<RenderOption> = option.into();
        ron::ser::to_string_pretty(&option, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("Ser error: {}", e.to_string()))
    }
}

#[get("/map/<path>")]
async fn web_map(path: PathBuf, config: &State<Config>) -> Result<NamedFile, String> {
    let mut path = PathBuf::from("assets/web-map").join(path);
    if !path.exists() {
        path = config.data_path.join("web-map").join(path);
    }
    NamedFile::open(path)
        .await
        .map_err(|e| format!("open index.html error: {}", e.to_string()))
}

#[get("/ws/anim?<param..>")]
async fn anim_real_time_websocket<'a>(
    ws: rocket_ws::WebSocket,
    param: WebMapServiceQueryParam,
    device: &'a State<Device>,
    queue: &'a State<Queue>,
    config: &'a State<Config>,
    data_cache: &State<Arc<RwLock<DataCache>>>,
) -> rocket_ws::Stream!['a] {
    let WebMapServiceQueryParam {
        layers,
        styles,
        width,
        height,
        format,
        bbox,
    } = param;
    let geojson = get_data_from_cache(config, &layers, data_cache, None)
        .await
        .expect("read data error.");
    let mut render_option = get_style_from_cache(config, &styles, data_cache, None)
        .await
        .expect("read style error.");
    render_option.pixel_option.width = width;
    render_option.pixel_option.height = height;
    render_option.region = convert_bbox(bbox, render_option.need_proj_geom);
    let image_format = convert_format(format);
    let rect = get_all_render_rect(&geojson, &mut render_option);
    if rect.is_some() {
        render_option.region = RenderRegion::Rect(rect.unwrap());
    };
    let mut renderer = vello::Renderer::new(
        &device,
        vello::RendererOptions {
            num_init_threads: config.shader_init_threads,
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .expect("Got non-Send/Sync error from creating renderer");
    let texture_desc = render_option.get_texture_descriptor();
    let texture = device.create_texture(&texture_desc);
    let size = width * height * 4;
    let mut buffer = Vec::with_capacity(size as usize);
    let time_instant = Instant::now();
    rocket_ws::Stream! { ws =>
        for await message in ws {
            let message = message?;
            if message.to_string() == "exit" {
                break;
            }
            let i = time_instant.elapsed().as_secs() % 10;
            render_option
                .renderers
                .iter_mut()
                .for_each(|type_renderer| {
                    let inner = type_renderer.as_mut();
                    match inner {
                        geello::GeometryRenderer::Point(_,point_renderer) => {
                            let point_renderer = point_renderer.as_mut();
                            if i == 0 {
                                point_renderer.radius = MagicValue::wrap(0.1);
                            }else{
                                let i = i as f64;
                                let old:f64 = point_renderer.radius.inner_try_into().unwrap();
                                point_renderer.radius = MagicValue::wrap(old + 0.001 * i);
                            }

                        }
                        _ => {}
                    }
                });
            let image = render_wms_on_texture(&geojson, device, queue,&mut renderer,&texture,  &mut render_option).await.expect("render errors.");

            let mut cursor = Cursor::new(&mut buffer);
            image
                .write_to(&mut cursor, image_format)
                .map_err(|e| format!("encode image faild: {}", e.to_string())).expect("encode errors.");
            yield cursor.into_inner().clone().into();
        }
    }
}

#[get("/wms?<param..>")]
async fn wms_real_time(
    param: WebMapServiceQueryParam,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    data_cache: &State<Arc<RwLock<DataCache>>>,
) -> Result<(ContentType, Vec<u8>), String> {
    let WebMapServiceQueryParam {
        layers,
        styles,
        width,
        height,
        format,
        bbox,
    } = param;
    let geojson = get_data_from_cache(config, &layers, data_cache, None).await?;
    let mut render_option = get_style_from_cache(config, &styles, data_cache, None).await?;
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
    data_cache: &State<Arc<RwLock<DataCache>>>,
) -> Result<(ContentType, Vec<u8>), String> {
    let WebMapTileServiceQueryParam {
        layers,
        styles,
        x,
        y,
        z,
        format,
    } = param;
    let geojson = get_data_from_cache(config, &layers, data_cache, None).await?;
    let mut render_option = get_style_from_cache(config, &styles, data_cache, None).await?;
    render_option.region = RenderRegion::TileIndex(x, y, z);
    let image = render_wmts_tile(
        &geojson,
        device,
        queue,
        config,
        texture_vec,
        &mut render_option,
    )
    .await?;
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
    data_cache: &State<Arc<RwLock<DataCache>>>,
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
        let geojson = get_data_from_cache(config, &layers, data_cache, None).await?;
        let mut render_option = get_style_from_cache(config, &styles, data_cache, None).await?;
        render_option.region = RenderRegion::TileIndex(x, y, z);
        let image = render_wmts_tile(
            &geojson,
            device,
            queue,
            config,
            texture_vec,
            &mut render_option,
        )
        .await?;
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

#[derive(Debug)]
struct DataCache {
    layer_map: HashMap<String, LayerCache>,
    style_map: HashMap<String, StyleCache>,
}

impl Default for DataCache {
    fn default() -> DataCache {
        DataCache {
            layer_map: HashMap::new(),
            style_map: HashMap::new(),
        }
    }
}

impl DataCache {
    pub fn check_expired_cache(&mut self) {
        let now = Instant::now();
        let layer_name: Vec<String> = self
            .layer_map
            .iter()
            .filter_map(|(name, layer)| {
                if layer.expiration.is_expired(&now) {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        for name in layer_name {
            self.layer_map.remove(&name);
        }
        let style_name: Vec<String> = self
            .style_map
            .iter()
            .filter_map(|(name, style)| {
                if style.expiration.is_expired(&now) {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        for name in style_name {
            self.style_map.remove(&name);
        }
    }
    pub fn layer_cache(&self, name: &str) -> Option<GeoJson> {
        self.layer_map
            .get(name)
            .map_or(None, |cache| Some(cache.inner.clone()))
    }
    pub fn style_cache(&self, name: &str) -> Option<RenderOption> {
        self.style_map
            .get(name)
            .map_or(None, |cache| Some(cache.inner.clone()))
    }
    pub async fn read_geojson_form_link(link: &str) -> Result<GeoJson, String> {
        let geojson_str = reqwest::get(link)
            .await
            .map_err(|e| format!("request data error: {}", e.to_string()))?
            .text()
            .await
            .map_err(|e| format!("recive data read error: {}", e.to_string()))?;
        geojson_str
            .parse::<GeoJson>()
            .map_err(|e| format!("convert {} failed: {}", link, e.to_string()))
    }
    pub fn read_geojson_form_fs(
        config: &State<Config>,
        geojson_path_str: &str,
    ) -> Result<GeoJson, String> {
        let geojson_path = config.data_path.join(geojson_path_str);
        if !geojson_path.exists() {
            return Err(format!("can not find {}", geojson_path_str));
        }
        let mut file = File::open(geojson_path.as_path())
            .map_err(|e| format!("open {} failed: {}", geojson_path.display(), e.to_string()))?;
        let mut geojson_str = String::new();
        let _ = file.read_to_string(&mut geojson_str);
        geojson_str.parse::<GeoJson>().map_err(|e| {
            format!(
                "convert {} failed: {}",
                geojson_path.display(),
                e.to_string()
            )
        })
    }
    pub async fn read_style_form_link(link: &str) -> Result<MagicValue<RenderOption>, String> {
        let option_str = reqwest::get(link)
            .await
            .map_err(|e| format!("request data error: {}", e.to_string()))?
            .text()
            .await
            .map_err(|e| format!("recive data read error: {}", e.to_string()))?;
        ron::de::from_str(&option_str)
            .map_err(|e| format!("convert {} failed: {}", link, e.to_string()))
    }
    pub fn read_style_form_fs(
        config: &State<Config>,
        style_path: &str,
    ) -> Result<MagicValue<RenderOption>, String> {
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
        ron::de::from_str(&option_str).map_err(|e| {
            format!(
                "convert {} failed: {}",
                render_option_path.display(),
                e.to_string()
            )
        })
    }
    pub async fn read_layer(
        &mut self,
        config: &State<Config>,
        geojson_path_str: &str,
        expiration: Expiration,
    ) -> Result<GeoJson, String> {
        let is_link = geojson_path_str.to_lowercase().starts_with("http");
        let geojson = if is_link {
            Self::read_geojson_form_link(geojson_path_str).await?
        } else {
            Self::read_geojson_form_fs(config, geojson_path_str)?
        };
        let cache = LayerCache {
            inner: geojson.clone(),
            kind: if is_link {
                DataKind::Link
            } else {
                DataKind::File
            },
            expiration,
        };
        self.layer_map.insert(geojson_path_str.to_string(), cache);
        Ok(geojson)
    }
    pub async fn read_style(
        &mut self,
        config: &State<Config>,
        style_path: &str,
        expiration: Expiration,
    ) -> Result<RenderOption, String> {
        let is_link = style_path.to_lowercase().starts_with("http");
        let mut style = if is_link {
            Self::read_style_form_link(style_path).await?
        } else {
            Self::read_style_form_fs(config, style_path)?
        };
        style.fetch()?;
        let mut style = style.unwrap();
        style.fetch()?;
        let cache = StyleCache {
            inner: style.clone(),
            kind: if is_link {
                DataKind::Link
            } else {
                DataKind::File
            },
            expiration,
        };
        self.style_map.insert(style_path.to_string(), cache);
        Ok(style)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DataKind {
    File,
    Link,
}
#[derive(Debug, Clone)]
enum Expiration {
    Never,
    At(Instant),
}

impl Expiration {
    pub fn is_expired(&self, instant: &Instant) -> bool {
        match self {
            Expiration::Never => false,
            Expiration::At(expiration) => expiration > instant,
        }
    }
}

impl Default for Expiration {
    fn default() -> Expiration {
        let dur = Duration::from_secs(60 * 60 * 24);
        let expiration = Instant::now() + dur;
        Expiration::At(expiration)
    }
}

#[derive(Debug)]
struct LayerCache {
    inner: GeoJson,
    kind: DataKind,
    expiration: Expiration,
}
#[derive(Debug)]
struct StyleCache {
    inner: RenderOption,
    kind: DataKind,
    expiration: Expiration,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Config {
    data_path: PathBuf,
    wmts_texture_count: u32,
    shader_init_threads: Option<NonZero<usize>>,
    cache_path: PathBuf,
    address: std::net::IpAddr,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            data_path: "assets".into(),
            wmts_texture_count: 100,
            shader_init_threads: None,
            cache_path: "cache".into(),
            address: std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)),
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
            // !WARN: May get invalid BBOX
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

const CHAR_NOT_ALLOWED_IN_PATH: [char; 9] = ['\\', ',', ':', '*', '<', '>', '?', '|', '\"'];

fn get_image_path(
    data_path: &str,
    style_path: &str,
    x: u32,
    y: u32,
    z: u32,
    config: &State<Config>,
) -> (String, String) {
    let dir = config.data_path.join(&config.cache_path);
    let data_path = data_path.replace(CHAR_NOT_ALLOWED_IN_PATH, "/");
    let style_path = style_path.replace(CHAR_NOT_ALLOWED_IN_PATH, "/");
    let dir = dir.to_str().unwrap_or("cache");
    (
        format!("{dir}/{data_path}/{style_path}/{z}/{x}"),
        format!("{y}.png"),
    )
}

async fn get_style_from_cache(
    config: &State<Config>,
    style_path: &str,
    data_cache: &State<Arc<RwLock<DataCache>>>,
    expiration: Option<Expiration>,
) -> Result<RenderOption, String> {
    let data_cache = Arc::clone(data_cache);
    let cache = data_cache.read().await;
    let render_option_cache = cache.style_cache(style_path);
    drop(cache);
    if render_option_cache.is_none() {
        let expiration = expiration.unwrap_or(Expiration::Never);
        let mut cache = data_cache.write().await;
        let render_option = cache.read_style(config, style_path, expiration).await?;
        drop(cache);
        Ok(render_option)
    } else {
        Ok(render_option_cache.unwrap())
    }
}

async fn get_data_from_cache(
    config: &State<Config>,
    data_path: &str,
    data_cache: &State<Arc<RwLock<DataCache>>>,
    expiration: Option<Expiration>,
) -> Result<Vec<(String, GeoJson)>, String> {
    let data_cache = Arc::clone(data_cache);
    let mut geojson_data_vec = Vec::new();
    let geojson_path_vec = data_path.split(',');
    let expiration = expiration.unwrap_or(Expiration::default());
    for geojson_path_str in geojson_path_vec {
        let cache = data_cache.read().await;
        let geojson = cache.layer_cache(geojson_path_str);
        drop(cache);
        if geojson.is_none() {
            let mut cache = data_cache.write().await;
            let geojson = cache
                .read_layer(config, geojson_path_str, expiration.clone())
                .await?;
            drop(cache);
            geojson_data_vec.push((geojson_path_str.to_string(), geojson));
        } else {
            geojson_data_vec.push((geojson_path_str.to_string(), geojson.unwrap()));
        }
    }

    Ok(geojson_data_vec)
}

fn get_all_render_rect(
    geojson: &Vec<(String, GeoJson)>,
    render_option: &RenderOption,
) -> Option<geo::Rect> {
    match render_option.region {
        RenderRegion::All => {
            let mut has_rect = false;
            let mut b_x_min = f64::MAX;
            let mut b_x_max = f64::MIN;
            let mut b_y_min = f64::MAX;
            let mut b_y_max = f64::MIN;
            for geo in geojson {
                if let Some(rect) = get_render_rect(&geo.1, render_option) {
                    has_rect = true;
                    let min = rect.min();
                    let max = rect.max();
                    b_x_min = b_x_min.min(min.x);
                    b_x_max = b_x_max.max(max.x);
                    b_y_min = b_y_min.min(min.y);
                    b_y_max = b_y_max.max(max.y);
                }
            }
            if has_rect {
                Some(geo::Rect::new((b_x_min, b_y_min), (b_x_max, b_y_max)))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn get_render_rect(geojson: &GeoJson, render_option: &RenderOption) -> Option<geo::Rect> {
    match render_option.region {
        RenderRegion::All => match &geojson {
            GeoJson::Geometry(geometry) => {
                let geom = geo_types::Geometry::<f64>::try_from(geometry).expect("msg");
                // .map_err(|e| format!("convert geojson failed: {}", e.to_string()))?;
                geom.bounding_rect()
            }
            GeoJson::Feature(feature) => match feature.geometry.as_ref() {
                Some(geom) => {
                    let geom = geo_types::Geometry::<f64>::try_from(geom).expect("msg");
                    // .map_err(|e| format!("convert geojson failed: {}", e.to_string()))?;
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
                        let geom = geo_types::Geometry::<f64>::try_from(geom).expect("msg");
                        // .map_err(|e| format!("convert geojson failed: {}", e.to_string()))?;
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
    }
}

async fn render_wms_on_texture(
    geojson: &Vec<(String, GeoJson)>,
    device: &State<Device>,
    queue: &State<Queue>,
    renderer: &mut vello::Renderer,
    texture: &Texture,
    render_option: &mut RenderOption,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, String> {
    let buffer = render_geojson_to_buffer(
        geojson,
        device,
        queue,
        renderer,
        texture,
        Affine::IDENTITY,
        render_option,
    )
    .await
    .map_err(|e| e.to_string())?;
    let (width, height) = render_option.get_pixel_size();
    image::RgbaImage::from_raw(width, height, buffer).ok_or(String::from("render image error"))
}

async fn render_wms(
    geojson: &Vec<(String, GeoJson)>,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    render_option: &mut RenderOption,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, String> {
    let rect = get_all_render_rect(geojson, render_option);
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

async fn get_one_texture(texture_vec: &Arc<Mutex<Vec<Texture>>>) -> Texture {
    loop {
        let mut v = texture_vec.lock().await;
        let x = v.pop();
        if x.is_some() {
            drop(v);
            return x.unwrap();
        }
        drop(v);
        let time = tokio::time::Duration::from_millis(10);
        sleep(time);
    }
}

async fn render_wmts_tile(
    geojson: &Vec<(String, GeoJson)>,
    device: &State<Device>,
    queue: &State<Queue>,
    config: &State<Config>,
    texture_vec: &State<Arc<Mutex<Vec<Texture>>>>,
    render_option: &mut RenderOption,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>, String> {
    let texture_vec = Arc::clone(texture_vec);
    let texture = get_one_texture(&texture_vec).await;
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
        render_option,
    )
    .await
    .map_err(|e| e.to_string())?;
    let mut v = texture_vec.lock().await;
    v.push(texture);
    drop(v);
    image::RgbaImage::from_raw(256, 256, buffer).ok_or(String::from("render image error"))
}

async fn render_geojson_to_buffer_with_new_texture(
    geojson: &Vec<(String, GeoJson)>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    transform: Affine,
    option: &mut RenderOption,
) -> Result<Vec<u8>, String> {
    let geom_to_render_vec = get_geom_from_geojson_vec(geojson)
        .map_err(|e| format!("read geojson error: {}", e.to_string()))?;
    let mut geom_s = get_rendered_geometry(geom_to_render_vec, option);
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
    geojson: &Vec<(String, GeoJson)>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    transform: Affine,
    option: &mut RenderOption,
) -> Result<Vec<u8>, String> {
    let geom_to_render_vec = get_geom_from_geojson_vec(geojson)
        .map_err(|e| format!("read geojson error: {}", e.to_string()))?;
    let mut geom_s = get_rendered_geometry(geom_to_render_vec, option);
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

fn get_rendered_geometry(
    geom_vec: Vec<(String, Vec<geo_types::Geometry>)>,
    option: &RenderOption,
) -> Vec<RenderedGeometry> {
    let proj = if option.need_proj_geom {
        Some(option.tile_proj)
    } else {
        None
    };
    let mut rendered_geom = Vec::new();
    for (layer, geom) in geom_vec {
        for g in geom {
            let rg =
                RenderedGeometry::new(Some(layer.clone()), Default::default(), g.clone(), &proj);
            rendered_geom.push(rg);
        }
    }
    rendered_geom
}

fn get_geom_from_geojson_vec(
    geojson: &Vec<(String, GeoJson)>,
) -> Result<Vec<(String, Vec<geo_types::Geometry>)>, String> {
    let mut geom_to_render_vec = Vec::new();
    for (layer, geo) in geojson {
        let gg = get_geom_from_geojson(geo)?;
        geom_to_render_vec.push((layer.clone(), gg));
    }
    Ok(geom_to_render_vec)
}

fn get_geom_from_geojson(geojson: &GeoJson) -> Result<Vec<geo_types::Geometry>, String> {
    let mut geom_to_render_vec = Vec::new();
    match geojson {
        GeoJson::Geometry(geometry) => {
            let geom = geo_types::Geometry::<f64>::try_from(geometry)
                .map_err(|e| format!("convert geometry error: {}", e.to_string()))?;
            geom_to_render_vec.push(geom);
        }
        GeoJson::Feature(feature) => {
            let geom = feature
                .geometry
                .as_ref()
                .ok_or_else(|| format!("only feature has no geometry"))?;
            let geom = geo_types::Geometry::<f64>::try_from(geom)
                .map_err(|e| format!("convert geometry error: {}", e.to_string()))?;
            geom_to_render_vec.push(geom);
        }
        GeoJson::FeatureCollection(feature_collection) => {
            for (index, feature) in feature_collection.features.iter().enumerate() {
                let geom = feature
                    .geometry
                    .as_ref()
                    .ok_or_else(|| format!("feature (index:{index}) has no geometry"))?;
                let geom = geo_types::Geometry::<f64>::try_from(geom)
                    .map_err(|e| format!("convert geometry error: {}", e.to_string()))?;
                geom_to_render_vec.push(geom);
            }
        }
    };
    Ok(geom_to_render_vec)
}
