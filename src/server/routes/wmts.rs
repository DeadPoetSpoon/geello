use crate::server::state::AppState;
use axum::{Router, extract::State, response::IntoResponse, routing::get};

pub fn routes() -> Router<AppState> {
    Router::new().route("/wmts/cache", get(cache))
}

pub async fn cache(State(state): State<AppState>) -> Result<impl IntoResponse, ()> {
    Ok(format!("cache: {:?}", state.config))
}
