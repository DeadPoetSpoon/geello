use serde::{Deserialize, Serialize};

use crate::core::error::GeelloResult;
use crate::core::lazy::{FromPropValue, LazyValue, ResolveContext, opt_resolve};

/// Point renderer configuration (lazy values).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PointConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<LazyValue<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<LazyValue<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedPointConfig {
    pub radius: Option<f64>,
    pub color: Option<String>,
}

impl PointConfig {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedPointConfig>
    where
        f64: FromPropValue,
        String: FromPropValue,
    {
        Ok(ResolvedPointConfig {
            radius: opt_resolve(self.radius, ctx)?,
            color: opt_resolve(self.color, ctx)?,
        })
    }
}
