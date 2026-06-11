use super::config::ServerConfig;

/// Application state shared across all Axum routes.
#[derive(Clone)]
pub struct AppState {
    pub config: ServerConfig,
    pub http_client: reqwest::Client,
}
