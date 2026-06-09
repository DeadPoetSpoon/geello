//! Layer cache — caches parsed GeoJSON data.

use std::path::PathBuf;
use std::time::Instant;

use super::DataSource;

/// Cached GeoJSON layer.
#[derive(Debug, Clone)]
pub struct CachedLayer {
    /// Parsed GeoJSON data
    pub geojson: geojson::GeoJson,
    /// Where the data was loaded from
    pub source: DataSource,
    /// When it was loaded
    pub loaded_at: Instant,
    /// File modification time (for freshness checks)
    pub file_mtime: Option<std::time::SystemTime>,
    /// Time-to-live in seconds (None = no expiry)
    pub ttl: Option<u64>,
}

impl CachedLayer {
    /// Create a new cached layer from a file source.
    pub fn from_file(path: PathBuf, geojson: geojson::GeoJson, ttl: Option<u64>) -> Self {
        let file_mtime = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());
        Self {
            geojson,
            source: DataSource::File(path),
            loaded_at: Instant::now(),
            file_mtime,
            ttl,
        }
    }

    /// Create a new cached layer from a URL source.
    pub fn from_url(url: String, geojson: geojson::GeoJson, ttl: Option<u64>) -> Self {
        Self {
            geojson,
            source: DataSource::Url(url),
            loaded_at: Instant::now(),
            file_mtime: None,
            ttl,
        }
    }

    /// Check if the cached layer is still fresh.
    pub fn is_fresh(&self) -> bool {
        if let Some(ttl) = self.ttl {
            if self.loaded_at.elapsed().as_secs() > ttl {
                return false;
            }
        }
        // Check if file has been modified since loading
        if let DataSource::File(ref path) = self.source {
            if let Some(cached_mtime) = self.file_mtime {
                if let Ok(meta) = std::fs::metadata(path) {
                    if let Ok(current_mtime) = meta.modified() {
                        return current_mtime <= cached_mtime;
                    }
                }
            }
        }
        true
    }
}
