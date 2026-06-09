//! Geello Axum server — router assembly and startup.
//!
//! Replaces the old Rocket-based server. Provides WMTS, WMS,
//! WebSocket animation, and static web-map endpoints.

pub mod cache;
pub mod config;
pub mod error;
pub mod hot_reload;
pub mod routes;
pub mod state;

use std::sync::Arc;
use tokio::sync::RwLock;

use axum::Router;
use tower_http::services::ServeDir;

use self::cache::CacheManager;
use self::config::ServerConfig;
use self::state::AppState;
use crate::device::TexturePool;

/// Build the full Axum application router.
pub fn build_router(state: AppState) -> Router {
    let public = Router::new()
        .merge(routes::wmts::routes())
        .merge(routes::wms::routes())
        .merge(routes::websocket::routes())
        .nest_service("/map", ServeDir::new("assets/web-map"));

    let admin = Router::new().merge(routes::admin::routes());

    Router::new().merge(public).merge(admin).with_state(state)
}

/// Initialize GPU context, load config, and start the Axum server.
pub async fn run() {
    // Init tracing
    tracing_subscriber::fmt::init();

    // Load config
    let config = ServerConfig::load().expect("Failed to load server config");
    tracing::info!("Config loaded: {:?}", config);

    // Init GPU
    let mut render_context = vello::util::RenderContext::new();
    let device_id = render_context
        .device(None)
        .await
        .expect("No compatible GPU device found");

    let device_handle = &render_context.devices[device_id];
    let device = device_handle.device.clone();
    let queue = device_handle.queue.clone();

    // Create texture pool
    let texture_pool = Arc::new(TexturePool::new(
        &device,
        config.wmts_texture_count,
        256,
        256,
    ));

    // Init cache (Foyer hybrid: memory + disk)
    let cache_path = config.cache_path.clone();
    std::fs::create_dir_all(&cache_path).expect("Failed to create cache directory");
    let cache = Arc::new(
        CacheManager::new(
            cache_path,
            64 * 1024 * 1024,  // 64 MB memory for tile cache
            256 * 1024 * 1024, // 256 MB disk for tile cache
        )
        .await
        .expect("Failed to initialize cache"),
    );

    // Init HTTP client
    let http_client = reqwest::Client::new();

    let app_state = AppState {
        device,
        queue,
        texture_pool: texture_pool.clone(),
        cache: cache.clone(),
        config: Arc::new(RwLock::new(config.clone())),
        http_client,
    };

    // Start hot reload watcher
    let watch_paths = vec![config.data_path.clone()];
    let _hot_reload = hot_reload::HotReloader::start(watch_paths, cache);

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
