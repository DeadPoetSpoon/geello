use crate::device::TexturePool;
use std::sync::Arc;
use tokio::sync::RwLock;
use vello::wgpu;

use super::cache::CacheManager;
use super::config::ServerConfig;

/// Application state shared across all Axum routes.
///
/// Holds GPU resources, cache layers, configuration, and HTTP client.
/// All fields use `Arc` for cheap cloning across request handlers.
#[derive(Clone)]
pub struct AppState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub texture_pool: Arc<TexturePool>,
    pub cache: Arc<CacheManager>,
    pub config: Arc<RwLock<ServerConfig>>,
    pub http_client: reqwest::Client,
}
