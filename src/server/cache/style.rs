//! Style cache — caches parsed TOML style files.

use std::path::PathBuf;
use std::time::Instant;

use crate::core::option::RenderOption;

use super::DataSource;

/// Cached render style configuration.
#[derive(Debug, Clone)]
pub struct CachedStyle {
    /// Parsed render option
    pub option: RenderOption,
    /// Where the style was loaded from
    pub source: DataSource,
    /// When it was loaded
    pub loaded_at: Instant,
    /// File modification time (for freshness checks)
    pub file_mtime: Option<std::time::SystemTime>,
    /// Time-to-live in seconds (None = no expiry)
    pub ttl: Option<u64>,
}

impl CachedStyle {
    /// Create a new cached style from a file source.
    pub fn from_file(path: PathBuf, option: RenderOption, ttl: Option<u64>) -> Self {
        let file_mtime = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());
        Self {
            option,
            source: DataSource::File(path),
            loaded_at: Instant::now(),
            file_mtime,
            ttl,
        }
    }

    /// Create a new cached style from a URL source.
    pub fn from_url(url: String, option: RenderOption, ttl: Option<u64>) -> Self {
        Self {
            option,
            source: DataSource::Url(url),
            loaded_at: Instant::now(),
            file_mtime: None,
            ttl,
        }
    }

    /// Check if the cached style is still fresh.
    pub fn is_fresh(&self) -> bool {
        if let Some(ttl) = self.ttl {
            if self.loaded_at.elapsed().as_secs() > ttl {
                return false;
            }
        }
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
