use serde::{Deserialize, Serialize};

use crate::core::error::GeelloResult;
use crate::core::lazy::{LazyValue, ResolveContext};
use crate::from_prop_never;

use super::area::AreaConfig;
use super::line::LineConfig;
use super::point::PointConfig;
use super::raster::RasterConfig;
use super::text::TextConfig;

from_prop_never!(PointConfig);
from_prop_never!(LineConfig);
from_prop_never!(AreaConfig);
from_prop_never!(TextConfig);
from_prop_never!(RasterConfig);
from_prop_never!(GeometryFilter);
from_prop_never!(PropertyCondition);
from_prop_never!(BboxFilter);

/// A single renderer definition inside a [`LayerStyle`].
///
/// Serialises as a TOML table with at most one renderer-kind key
/// (e.g. `point = { radius = 0.1 }`) and an optional `geometry_filter`.
/// All configs support [`LazyValue`] sources.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RendererDef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<LazyValue<PointConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<LazyValue<LineConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<LazyValue<AreaConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<LazyValue<TextConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raster: Option<LazyValue<RasterConfig>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry_filter: Option<LazyValue<GeometryFilter>>,
}

/// Fully-resolved [`RendererDef`].
#[derive(Debug, Clone)]
pub struct ResolvedRendererDef {
    pub point: Option<PointConfig>,
    pub line: Option<LineConfig>,
    pub area: Option<AreaConfig>,
    pub text: Option<TextConfig>,
    pub raster: Option<RasterConfig>,
    pub geometry_filter: Option<ResolvedGeometryFilter>,
}

impl RendererDef {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedRendererDef> {
        Ok(ResolvedRendererDef {
            point: self.point.map(|v| v.resolve(ctx)).transpose()?,
            line: self.line.map(|v| v.resolve(ctx)).transpose()?,
            area: self.area.map(|v| v.resolve(ctx)).transpose()?,
            text: self.text.map(|v| v.resolve(ctx)).transpose()?,
            raster: self.raster.map(|v| v.resolve(ctx)).transpose()?,
            geometry_filter: self
                .geometry_filter
                .map(|v| v.resolve(ctx)?.resolve(ctx))
                .transpose()?,
        })
    }
}

// ---------------------------------------------------------------------------
// GeometryFilter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeometryFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop: Option<LazyValue<PropertyCondition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<LazyValue<BboxFilter>>,
}

#[derive(Debug, Clone)]
pub struct ResolvedGeometryFilter {
    pub prop: Option<PropertyCondition>,
    pub bbox: Option<BboxFilter>,
}

impl GeometryFilter {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedGeometryFilter> {
        Ok(ResolvedGeometryFilter {
            prop: self.prop.map(|v| v.resolve(ctx)).transpose()?,
            bbox: self.bbox.map(|v| v.resolve(ctx)).transpose()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCondition {
    pub name: String,
    #[serde(default)]
    pub op: PropertyOp,
    pub value: PropertyValue,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum PropertyOp {
    #[default]
    Eq,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Bool(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BboxFilter {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
