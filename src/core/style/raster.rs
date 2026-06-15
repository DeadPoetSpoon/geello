use serde::{Deserialize, Serialize};

use crate::core::error::GeelloResult;
use crate::core::lazy::{opt_resolve, FromPropValue, LazyValue, ResolveContext};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RasterConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<LazyValue<f64>>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedRasterConfig {
    pub opacity: Option<f64>,
}

impl RasterConfig {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedRasterConfig>
    where
        f64: FromPropValue,
    {
        Ok(ResolvedRasterConfig {
            opacity: opt_resolve(self.opacity, ctx)?,
        })
    }
}
