use axum::{extract::State, response::Html, routing::get};

use crate::server::state::AppState;

mod config;
mod routes;
mod state;

/// Build the full Axum application router.
pub fn build_router(state: state::AppState) -> axum::Router {
    let public = axum::Router::new()
        .route("/map", get(map))
        .merge(routes::wmts::routes());

    axum::Router::new().merge(public).with_state(state)
}

async fn map(State(state): State<AppState>) -> Html<String> {
    let path = state.config.data_path.join("map").join("index.html");
    if path.exists() {
        let html = std::fs::read_to_string(&path).unwrap_or_default();
        Html(html)
    } else {
        Html("No map data available".to_string())
    }
}

pub async fn run() {
    // initialize tracing
    tracing_subscriber::fmt::init();

    // Load config
    let config = config::ServerConfig::load().expect("Failed to load server config");
    tracing::info!("Config loaded: {:?}", config);

    // Init HTTP client
    let http_client = reqwest::Client::new();

    let app_state = state::AppState {
        config: config.clone(),
        http_client,
    };

    // Build router
    let app = build_router(app_state);

    // Bind and serve
    let addr = std::net::SocketAddr::new(config.address, config.port);
    tracing::info!("Geello server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app).await.expect("Server error");
}
