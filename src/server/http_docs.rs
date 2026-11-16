//! Embedded, offline HTTP contract. No runtime schema generator or CDN assets.
use super::*;
use axum::{
    extract::State,
    http::header,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

pub(super) async fn openapi(State(s): State<Arc<ServerState>>) -> Result<Response> {
    const SOURCE: &str = include_str!("http/openapi.json");
    // Admit the parsed tree before deserializing it. Serialization and response
    // ownership use the same bounded/accounted path as resource metadata.
    let bytes = SOURCE
        .len()
        .checked_add(s.config.public_base_url.len())
        .and_then(|n| n.checked_add(s.backend_name.len()))
        .ok_or(crate::GeelloError::SizeOverflow)?;
    let _tree = s
        .scheduler
        .memory_context()
        .reserve(memory::envelope(bytes)?)?;
    let mut document: serde_json::Value = serde_json::from_str(SOURCE).map_err(ServerError::io)?;
    document["servers"] =
        serde_json::json!([{ "url": s.config.public_base_url.trim_end_matches('/') }]);
    document["x-geello-enabled"] = serde_json::json!({
        "read-geojson": cfg!(feature = "read-geojson"),
        "read-shapefile": cfg!(feature = "read-shapefile"),
        "read-geotiff": cfg!(feature = "read-geotiff"),
        "render-cpu": cfg!(feature = "render-cpu"),
        "render-gpu": cfg!(feature = "render-gpu"),
        "actual-backend": s.backend_name,
        "max-dimension": s.config.profile.max_dimension,
        "max-upload-bytes": s.config.profile.max_upload_bytes,
        "max-list-items": s.config.profile.max_list_items,
        "text": cfg!(feature = "text"),
        "max-animation-fps": s.config.profile.max_animation_fps,
        "max-feature-cursors": s.config.profile.max_feature_cursors,
        "feature-cursor-ttl-ms": s.config.profile.feature_cursor_ttl_ms,
        "metatile-size": s.config.profile.metatile_size,
        "metatile-buffer": s.config.profile.metatile_buffer,
        "wmts-prewarm-size": s.config.profile.wmts_prewarm_size
    });
    super::api::json_response(&s, &document)
}

pub(super) async fn html() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"),
      (header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'"),
      (header::X_CONTENT_TYPE_OPTIONS, "nosniff")],
     include_str!("http/api.html")).into_response()
}
