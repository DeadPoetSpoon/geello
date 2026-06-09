use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::{Router, routing::get};
use serde::Deserialize;

use crate::core::option::{RenderOption, RenderRegion};
use crate::server::error::ServerError;
use crate::server::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/wms", get(real_time))
}

#[derive(Debug, Deserialize)]
pub struct WmsParams {
    pub layers: String,
    pub styles: String,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_format")]
    pub format: String,
    pub bbox: Option<String>,
}

fn default_format() -> String {
    "png".to_string()
}

/// GET /wms
pub async fn real_time(
    State(state): State<AppState>,
    Query(params): Query<WmsParams>,
) -> Result<impl IntoResponse, ServerError> {
    let geojson_data = super::wmts::load_layers_impl(&state, &params.layers).await?;
    let mut option = super::wmts::load_style_impl(&state, &params.styles).await?;
    option.pixel_option.width = params.width;
    option.pixel_option.height = params.height;
    option.region = parse_bbox(&params.bbox);

    let image_bytes = render_wms(&state, &geojson_data, &mut option, &params.format).await?;

    let mime = super::wmts::mime_type_impl(&params.format);
    Ok(([(header::CONTENT_TYPE, mime)], image_bytes))
}

fn parse_bbox(bbox: &Option<String>) -> RenderRegion {
    let Some(bbox_str) = bbox else {
        return RenderRegion::All;
    };
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
        RenderRegion::Rect(geo::Rect::new((min_x, max_y), (max_x, min_y)))
    } else {
        RenderRegion::All
    }
}

pub fn geojson_to_geom_vec(
    geojson_data: &[(String, geojson::GeoJson)],
) -> Result<Vec<(String, Vec<geo_types::Geometry>)>, ServerError> {
    use geojson::GeoJson;
    let mut result = Vec::new();
    for (layer, geo) in geojson_data {
        let geoms = match geo {
            GeoJson::Geometry(g) => {
                vec![
                    geo_types::Geometry::<f64>::try_from(g)
                        .map_err(|e| ServerError::BadRequest(format!("Geometry convert: {}", e)))?,
                ]
            },
            GeoJson::Feature(f) => {
                if let Some(g) = &f.geometry {
                    vec![
                        geo_types::Geometry::<f64>::try_from(g).map_err(|e| {
                            ServerError::BadRequest(format!("Geometry convert: {}", e))
                        })?,
                    ]
                } else {
                    vec![]
                }
            },
            GeoJson::FeatureCollection(fc) => fc
                .features
                .iter()
                .filter_map(|f| {
                    f.geometry.as_ref().map(|g| {
                        geo_types::Geometry::<f64>::try_from(g).map_err(|e| {
                            ServerError::BadRequest(format!("Geometry convert: {}", e))
                        })
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        result.push((layer.clone(), geoms));
    }
    Ok(result)
}

async fn render_wms(
    state: &AppState,
    geojson_data: &[(String, geojson::GeoJson)],
    option: &mut RenderOption,
    format: &str,
) -> Result<Vec<u8>, ServerError> {
    let geom_vec = geojson_to_geom_vec(geojson_data)?;
    let mut rendered = super::wmts::get_rendered_geometry_impl(&geom_vec, option);
    let mut vello_renderer = vello::Renderer::new(
        &state.device,
        vello::RendererOptions {
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .map_err(|e| ServerError::RenderError(e.to_string()))?;

    let raw = crate::render_to_buffer_with_new_texture_async(
        &mut rendered,
        &state.device,
        &state.queue,
        &mut vello_renderer,
        vello::kurbo::Affine::IDENTITY,
        option,
    )
    .await?;

    let (w, h) = option.get_pixel_size();
    super::wmts::encode_image_impl(&raw, w, h, format)
}

// ── Public _impl wrappers for cross-module use ──

pub fn parse_bbox_impl(bbox: &Option<String>) -> RenderRegion {
    parse_bbox(bbox)
}

pub async fn render_wms_impl(
    state: &AppState,
    geojson_data: &[(String, geojson::GeoJson)],
    option: &mut RenderOption,
    format: &str,
) -> Result<Vec<u8>, ServerError> {
    render_wms(state, geojson_data, option, format).await
}
