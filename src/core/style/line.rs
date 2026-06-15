use serde::{Deserialize, Serialize};

use crate::core::error::GeelloResult;
use crate::core::lazy::{opt_resolve, FromPropValue, LazyValue, ResolveContext};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<LazyValue<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<LazyValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<LazyValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<LazyValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miter_limit: Option<LazyValue<f64>>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedLineConfig {
    pub width: Option<f64>,
    pub color: Option<String>,
    pub join: Option<String>,
    pub cap: Option<String>,
    pub miter_limit: Option<f64>,
}

impl LineConfig {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedLineConfig>
    where
        f64: FromPropValue,
        String: FromPropValue,
    {
        Ok(ResolvedLineConfig {
            width: opt_resolve(self.width, ctx)?,
            color: opt_resolve(self.color, ctx)?,
            join: opt_resolve(self.join, ctx)?,
            cap: opt_resolve(self.cap, ctx)?,
            miter_limit: opt_resolve(self.miter_limit, ctx)?,
        })
    }
}
