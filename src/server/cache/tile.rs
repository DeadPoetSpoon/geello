//! Tile cache — hybrid memory + disk cache for rendered tile images.
//!
//! Uses Foyer's `HybridCache` for automatic memory/disk tiering.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Unique key identifying a rendered tile.
///
/// Combines content hashes of layer data and style configuration
/// with the tile's z/x/y coordinates.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TileKey {
    /// Hash of the combined layer data
    pub layer_hash: u64,
    /// Hash of the style configuration
    pub style_hash: u64,
    /// Zoom level
    pub z: u32,
    /// Tile column
    pub x: u32,
    /// Tile row
    pub y: u32,
}

impl TileKey {
    /// Create a new tile key.
    pub fn new(layer_hash: u64, style_hash: u64, z: u32, x: u32, y: u32) -> Self {
        Self {
            layer_hash,
            style_hash,
            z,
            x,
            y,
        }
    }

    /// Compute a hash from layer names concatenation.
    pub fn hash_layers(layers: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        layers.hash(&mut hasher);
        hasher.finish()
    }

    /// Compute a hash from style content string.
    pub fn hash_style(style_content: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        style_content.hash(&mut hasher);
        hasher.finish()
    }
}

impl Hash for TileKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layer_hash.hash(state);
        self.style_hash.hash(state);
        self.z.hash(state);
        self.x.hash(state);
        self.y.hash(state);
    }
}

impl PartialEq for TileKey {
    fn eq(&self, other: &Self) -> bool {
        self.layer_hash == other.layer_hash
            && self.style_hash == other.style_hash
            && self.z == other.z
            && self.x == other.x
            && self.y == other.y
    }
}

impl Eq for TileKey {}

/// Convenience alias for the tile hybrid cache type.
pub type TileCache = foyer::HybridCache<TileKey, Vec<u8>>;
