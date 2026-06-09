use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::{Router, routing::get};
use serde::Deserialize;

use crate::core::option::{RenderOption, RenderRegion};
use crate::server::error::ServerError;
use crate::server::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/wmts/real-time", get(real_time))
        .route("/wmts/cache", get(cache))
}

#[derive(Debug, Deserialize)]
pub struct WmtsParams {
    pub layers: String,
    pub styles: String,
    pub x: u32,
    pub y: u32,
    pub z: u32,
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String {
    "png".to_string()
}

/// GET /wmts/real-time
pub async fn real_time(
    State(state): State<AppState>,
    Query(params): Query<WmtsParams>,
) -> Result<impl IntoResponse, ServerError> {
    let geojson_data = load_layers(&state, &params.layers).await?;
    let mut option = load_style(&state, &params.styles).await?;
    option.region = RenderRegion::TileIndex(params.x, params.y, params.z);

    let image_bytes = render_tile(&state, &geojson_data, &mut option, &params.format).await?;
    let content_type = mime_type(&params.format);
    Ok(([(header::CONTENT_TYPE, content_type)], image_bytes))
}

/// GET /wmts/cache
pub async fn cache(
    State(state): State<AppState>,
    Query(params): Query<WmtsParams>,
) -> Result<impl IntoResponse, ServerError> {
    let cache_path = build_cache_path(&state, &params).await;
    if cache_path.exists() {
        let bytes = tokio::fs::read(&cache_path)
            .await
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        let content_type = mime_type(&params.format);
        return Ok(([(header::CONTENT_TYPE, content_type)], bytes));
    }

    let geojson_data = load_layers(&state, &params.layers).await?;
    let mut option = load_style(&state, &params.styles).await?;
    option.region = RenderRegion::TileIndex(params.x, params.y, params.z);

    let image_bytes = render_tile(&state, &geojson_data, &mut option, &params.format).await?;

    // Save to disk cache
    if let Some(parent) = cache_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&cache_path, &image_bytes).await;

    let content_type = mime_type(&params.format);
    Ok(([(header::CONTENT_TYPE, content_type)], image_bytes))
}

fn mime_type(format: &str) -> &'static str {
    match format.to_lowercase().as_str() {
        "png" => "image/png",
        "jpeg" | "jpg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

async fn build_cache_path(state: &AppState, params: &WmtsParams) -> std::path::PathBuf {
    let config = state.config.read().await;
    let layers_safe = params
        .layers
        .replace(['\\', ',', ':', '*', '<', '>', '?', '|', '"'], "_");
    let styles_safe = params
        .styles
        .replace(['\\', ',', ':', '*', '<', '>', '?', '|', '"'], "_");
    config
        .cache_path
        .join(&layers_safe)
        .join(&styles_safe)
        .join(params.z.to_string())
        .join(params.x.to_string())
        .join(format!("{}.png", params.y))
}

async fn load_layers(
    state: &AppState,
    layers: &str,
) -> Result<Vec<(String, geojson::GeoJson)>, ServerError> {
    let mut result = Vec::new();
    let config = state.config.read().await;
    for layer_path in layers.split(',') {
        let layer_path = layer_path.trim();
        if layer_path.is_empty() {
            continue;
        }
        let is_url = layer_path.to_lowercase().starts_with("http");
        let geojson: geojson::GeoJson = if is_url {
            let resp = state
                .http_client
                .get(layer_path)
                .send()
                .await
                .map_err(|e| ServerError::BadRequest(format!("HTTP error: {}", e)))?;
            resp.json()
                .await
                .map_err(|e| ServerError::BadRequest(format!("JSON parse error: {}", e)))?
        } else {
            let path = config.data_path.join(layer_path);
            let content = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| ServerError::NotFound(format!("File not found: {}", e)))?;
            content
                .parse()
                .map_err(|e| ServerError::BadRequest(format!("GeoJSON parse error: {}", e)))?
        };
        result.push((layer_path.to_string(), geojson));
    }
    Ok(result)
}

async fn load_style(state: &AppState, style: &str) -> Result<RenderOption, ServerError> {
    let config = state.config.read().await;
    let style_path = if style.ends_with(".toml") {
        style.to_string()
    } else {
        format!("{}.toml", style)
    };
    let full_path = config.data_path.join(&style_path);
    let content = tokio::fs::read_to_string(&full_path)
        .await
        .map_err(|e| ServerError::NotFound(format!("Style not found: {}", e)))?;
    let option: RenderOption = toml::from_str(&content)
        .map_err(|e| ServerError::BadRequest(format!("TOML parse error: {}", e)))?;
    Ok(option)
}

async fn render_tile(
    state: &AppState,
    geojson_data: &[(String, geojson::GeoJson)],
    option: &mut RenderOption,
    format: &str,
) -> Result<Vec<u8>, ServerError> {
    let geom_vec = crate::server::routes::wms::geojson_to_geom_vec(geojson_data)?;
    let mut rendered = get_rendered_geometry(&geom_vec, option);
    let mut vello_renderer = vello::Renderer::new(
        &state.device,
        vello::RendererOptions {
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .map_err(|e| ServerError::RenderError(e.to_string()))?;

    let texture = state.texture_pool.acquire().await;
    let raw_bytes = crate::render_to_buffer_async(
        &mut rendered,
        &state.device,
        &state.queue,
        &mut vello_renderer,
        &texture,
        vello::kurbo::Affine::IDENTITY,
        option,
    )
    .await?;
    state.texture_pool.release(texture);

    let (w, h) = option.get_pixel_size();
    encode_image(&raw_bytes, w, h, format)
}

/// Encode raw RGBA bytes into PNG/JPEG/WebP using the `image` crate.
fn encode_image(raw: &[u8], width: u32, height: u32, format: &str) -> Result<Vec<u8>, ServerError> {
    use image::{ImageEncoder, codecs::png::PngEncoder};
    let mut buf = Vec::new();
    match format.to_lowercase().as_str() {
        "png" => {
            let encoder = PngEncoder::new(&mut buf);
            encoder
                .write_image(raw, width, height, image::ExtendedColorType::Rgba8)
                .map_err(|e| ServerError::RenderError(format!("PNG encode: {}", e)))?;
        },
        "jpeg" | "jpg" => {
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90);
            encoder
                .write_image(raw, width, height, image::ExtendedColorType::Rgba8)
                .map_err(|e| ServerError::RenderError(format!("JPEG encode: {}", e)))?;
        },
        _ => {
            // Default to PNG
            let encoder = PngEncoder::new(&mut buf);
            encoder
                .write_image(raw, width, height, image::ExtendedColorType::Rgba8)
                .map_err(|e| ServerError::RenderError(format!("PNG encode: {}", e)))?;
        },
    }
    Ok(buf)
}

fn get_rendered_geometry(
    geom_vec: &[(String, Vec<geo_types::Geometry>)],
    option: &RenderOption,
) -> Vec<crate::core::geometry::RenderedGeometry> {
    let proj = if option.need_proj_geom {
        Some(option.tile_proj)
    } else {
        None
    };
    let mut rendered = Vec::new();
    for (layer, geoms) in geom_vec {
        for g in geoms {
            rendered.push(crate::core::geometry::RenderedGeometry::new(
                Some(layer.clone()),
                Default::default(),
                g.clone(),
                &proj,
            ));
        }
    }
    rendered
}

// ── Public _impl wrappers for cross-module use ──

pub async fn load_layers_impl(
    state: &AppState,
    layers: &str,
) -> Result<Vec<(String, geojson::GeoJson)>, ServerError> {
    load_layers(state, layers).await
}

pub async fn load_style_impl(state: &AppState, style: &str) -> Result<RenderOption, ServerError> {
    load_style(state, style).await
}

pub fn mime_type_impl(format: &str) -> &'static str {
    mime_type(format)
}

pub fn get_rendered_geometry_impl(
    geom_vec: &[(String, Vec<geo_types::Geometry>)],
    option: &RenderOption,
) -> Vec<crate::core::geometry::RenderedGeometry> {
    get_rendered_geometry(geom_vec, option)
}

pub fn encode_image_impl(raw: &[u8], w: u32, h: u32, f: &str) -> Result<Vec<u8>, ServerError> {
    encode_image(raw, w, h, f)
}
