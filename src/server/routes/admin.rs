use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::{Router, routing::get};

use crate::server::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/cache/stats", get(cache_stats))
        .route("/example/{name}", get(render_option_example))
}

/// GET /health
pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// GET /cache/stats
pub async fn cache_stats(State(state): State<AppState>) -> impl IntoResponse {
    let stats = state.cache.stats();
    Json(stats)
}

/// GET /example/{name}
pub async fn render_option_example(Path(name): Path<String>) -> impl IntoResponse {
    if name == "area-renderer" {
        let area = crate::AreaRenderer::default();
        let renderer =
            crate::GeometryRenderer::Area(crate::RenderedGeometryFilter::None.into(), area.into());
        match toml::to_string_pretty(&renderer) {
            Ok(s) => (StatusCode::OK, s).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        let mut option = crate::RenderOption::default();
        option.renderers.push(
            crate::GeometryRenderer::Area(
                crate::RenderedGeometryFilter::None.into(),
                crate::AreaRenderer::default().into(),
            )
            .into(),
        );
        match toml::to_string_pretty(&option) {
            Ok(s) => (StatusCode::OK, s).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    }
}
