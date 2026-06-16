use serde::{Deserialize, Serialize};

use crate::core::error::GeelloResult;
use crate::core::lazy::{FromPropValue, LazyValue, ResolveContext, opt_resolve};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AreaConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<LazyValue<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedAreaConfig {
    pub fill_color: Option<String>,
}

impl AreaConfig {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedAreaConfig>
    where
        String: FromPropValue,
    {
        Ok(ResolvedAreaConfig {
            fill_color: opt_resolve(self.fill_color, ctx)?,
        })
    }
}
