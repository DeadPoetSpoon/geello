use serde::{Deserialize, Serialize};

use super::area::AreaConfig;
use super::line::LineConfig;
use super::point::PointConfig;
use super::raster::RasterConfig;
use super::text::TextConfig;

/// A single renderer definition inside a [`LayerStyle`].
///
/// Serialises as a TOML table with at most one renderer-kind key
/// (e.g. `point = { ... }`) and an optional `geometry_filter`.
///
/// # Example TOML
///
/// ```toml
/// [[layers.renderers]]
/// point = {}
/// geometry_filter = { prop = { name = "capital", value = true } }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RendererDef {
    /// Point renderer configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<PointConfig>,
    /// Line renderer configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<LineConfig>,
    /// Area (polygon) renderer configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<AreaConfig>,
    /// Text label renderer configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextConfig>,
    /// Raster image renderer configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raster: Option<RasterConfig>,

    /// Per-geometry filter applied before rendering this renderer's output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_filter: Option<GeometryFilter>,
}

/// Per-geometry filter supporting property and bounding-box conditions.
///
/// Both fields are optional — if both are `None`, all geometries pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeometryFilter {
    /// Filter by a feature property key-value pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop: Option<PropertyCondition>,

    /// Filter by geometry bounding-box intersection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BboxFilter>,
}

/// A property-based condition that a feature must satisfy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCondition {
    /// Property key (e.g. `"name"`, `"population"`).
    pub name: String,
    /// Comparison operator.
    #[serde(default)]
    pub op: PropertyOp,
    /// Expected value.
    pub value: PropertyValue,
}

/// Comparison operators for [`PropertyCondition`].
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum PropertyOp {
    /// Equal (`==`).
    #[default]
    Eq,
}

/// Serializable property value used in filter conditions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Bool(bool),
}

/// Axis-aligned bounding-box filter.
///
/// A geometry is included only if its bounding box intersects this rectangle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BboxFilter {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
