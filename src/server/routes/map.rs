use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;

/// Serve static files from assets/web-map/
pub async fn serve_map(Path(file_path): Path<String>) -> impl IntoResponse {
    // tower-http ServeDir handles this via nest_service in the router
    // This function is a fallback handler
    let _ = file_path;
    (StatusCode::NOT_FOUND, "File not found")
}
