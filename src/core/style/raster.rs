use serde::{Deserialize, Serialize};

/// Raster image renderer configuration.
///
/// TODO: 1.7.6 — add band selection, colour map, opacity, etc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RasterConfig {}
