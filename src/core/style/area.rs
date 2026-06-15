use serde::{Deserialize, Serialize};

use crate::core::error::GeelloResult;
use crate::core::lazy::{opt_resolve, FromPropValue, LazyValue, ResolveContext};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AreaConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<LazyValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<LazyValue<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<LazyValue<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedAreaConfig {
    pub fill_color: Option<String>,
    pub stroke_width: Option<f64>,
    pub stroke_color: Option<String>,
}

impl AreaConfig {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedAreaConfig>
    where
        f64: FromPropValue,
        String: FromPropValue,
    {
        Ok(ResolvedAreaConfig {
            fill_color: opt_resolve(self.fill_color, ctx)?,
            stroke_width: opt_resolve(self.stroke_width, ctx)?,
            stroke_color: opt_resolve(self.stroke_color, ctx)?,
        })
    }
}
