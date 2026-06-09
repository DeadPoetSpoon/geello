//! Foyer-based hybrid cache system.
//!
//! Provides three cache layers:
//! - **Layer cache**: in-memory cache for GeoJSON data
//! - **Style cache**: in-memory cache for RenderOption styles
//! - **Tile cache**: hybrid (memory + disk) cache for rendered tile images

pub mod layer;
pub mod style;
pub mod tile;

use std::path::PathBuf;
use std::sync::Arc;

use foyer::{Cache, CacheBuilder, DeviceBuilder, HybridCache};

pub use layer::CachedLayer;
pub use style::CachedStyle;
pub use tile::{TileCache, TileKey};

/// Central cache manager coordinating all cache layers.
///
/// # Cache Layers
///
/// | Cache | Backend | Key Type | Value Type |
/// |-------|---------|----------|------------|
/// | layer_cache | Foyer in-memory | `String` | `Arc<CachedLayer>` |
/// | style_cache | Foyer in-memory | `String` | `Arc<CachedStyle>` |
/// | tile_cache | Foyer hybrid | `TileKey` | `Vec<u8>` |
pub struct CacheManager {
    pub layer_cache: Cache<String, Arc<CachedLayer>>,
    pub style_cache: Cache<String, Arc<CachedStyle>>,
    pub tile_cache: HybridCache<TileKey, Vec<u8>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DataSource {
    File(PathBuf),
    Url(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CacheStats {
    pub layer_count: usize,
    pub style_count: usize,
    pub tile_count: usize,
}

impl CacheManager {
    /// Create a new CacheManager.
    ///
    /// `disk_path` specifies the directory for the tile hybrid cache disk storage.
    /// `memory_capacity` is the in-memory capacity in bytes for the tile cache.
    /// `disk_capacity` is the disk capacity in bytes for the tile cache.
    pub async fn new(
        disk_path: PathBuf,
        memory_capacity: usize,
        disk_capacity: usize,
    ) -> foyer::Result<Self> {
        let layer_cache = CacheBuilder::new(64).build();
        let style_cache = CacheBuilder::new(64).build();

        let device = foyer::FsDeviceBuilder::new(disk_path)
            .with_capacity(disk_capacity)
            .build()
            .map_err(|e| foyer::Error::from(e))?;

        let tile_cache = foyer::HybridCacheBuilder::new()
            .memory(memory_capacity)
            .storage()
            .with_engine_config(foyer::BlockEngineConfig::new(device))
            .build()
            .await?;

        Ok(Self {
            layer_cache,
            style_cache,
            tile_cache,
        })
    }

    /// Insert a layer into cache
    pub fn insert_layer(&self, key: String, layer: CachedLayer) {
        self.layer_cache.insert(key, Arc::new(layer));
    }

    /// Get a cached layer
    pub fn get_layer(&self, key: &str) -> Option<Arc<CachedLayer>> {
        self.layer_cache.get(key).map(|e| Arc::clone(e.value()))
    }

    /// Invalidate a cached layer
    pub fn remove_layer(&self, key: &str) {
        self.layer_cache.remove(key);
    }

    /// Insert a style into cache
    pub fn insert_style(&self, key: String, style: CachedStyle) {
        self.style_cache.insert(key, Arc::new(style));
    }

    /// Get a cached style
    pub fn get_style(&self, key: &str) -> Option<Arc<CachedStyle>> {
        self.style_cache.get(key).map(|e| Arc::clone(e.value()))
    }

    /// Invalidate a cached style
    pub fn remove_style(&self, key: &str) {
        self.style_cache.remove(key);
    }

    /// Invalidate a cached layer (hot-reload support)
    pub async fn invalidate_layer(&self, key: &str) {
        self.layer_cache.remove(key);
    }

    /// Invalidate a cached style (hot-reload support)
    pub async fn invalidate_style(&self, key: &str) {
        self.style_cache.remove(key);
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            layer_count: self.layer_cache.usage(),
            style_count: self.style_cache.usage(),
            tile_count: 0, // HybridCache stats tracked separately
        }
    }
}
