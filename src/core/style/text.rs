use serde::{Deserialize, Serialize};

use crate::core::error::GeelloResult;
use crate::core::lazy::{FromPropValue, LazyValue, ResolveContext, opt_resolve};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<LazyValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<LazyValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<LazyValue<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<LazyValue<String>>,
    /// Horizontal offset from anchor in map units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_x: Option<LazyValue<f64>>,
    /// Vertical offset from anchor in map units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_y: Option<LazyValue<f64>>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedTextConfig {
    pub field: Option<String>,
    pub font: Option<String>,
    pub size: Option<f64>,
    pub color: Option<String>,
    pub offset_x: Option<f64>,
    pub offset_y: Option<f64>,
}

impl TextConfig {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedTextConfig>
    where
        f64: FromPropValue,
        String: FromPropValue,
    {
        Ok(ResolvedTextConfig {
            field: opt_resolve(self.field, ctx)?,
            font: opt_resolve(self.font, ctx)?,
            size: opt_resolve(self.size, ctx)?,
            color: opt_resolve(self.color, ctx)?,
            offset_x: opt_resolve(self.offset_x, ctx)?,
            offset_y: opt_resolve(self.offset_y, ctx)?,
        })
    }
}
