use serde::{Deserialize, Serialize};

use crate::core::error::GeelloResult;
use crate::core::lazy::{LazyValue, ResolveContext};
use crate::from_prop_never;

use super::renderer::{RendererDef, ResolvedRendererDef};

from_prop_never!(LayerFilter);
from_prop_never!(LayerStyle);
from_prop_never!(RendererDef);
from_prop_never!(Vec<LayerStyle>);
from_prop_never!(Vec<LazyValue<LayerStyle>>);
from_prop_never!(Vec<LazyValue<RendererDef>>);

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

/// Top-level style definition — the unit of serialisation (TOML file).
///
/// The `layers` field supports all [`LazyValue`] sources — inline `[...]`,
/// `{ file = "layers.toml" }`, `{ url = "https://..." }`, etc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Style {
    #[serde(default)]
    pub layers: LazyValue<Vec<LazyValue<LayerStyle>>>,
}

/// Fully-resolved [`Style`] ready for the render pipeline.
#[derive(Debug, Clone, Default)]
pub struct ResolvedStyle {
    pub layers: Vec<ResolvedLayerStyle>,
}

impl Style {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedStyle> {
        let raw_layers = self.layers.resolve(ctx)?;
        let layers = raw_layers
            .into_iter()
            .map(|lv| lv.resolve(ctx)?.resolve(ctx))
            .collect::<GeelloResult<Vec<_>>>()?;
        Ok(ResolvedStyle { layers })
    }
}

// ---------------------------------------------------------------------------
// LayerStyle
// ---------------------------------------------------------------------------

/// Rendering configuration that targets one logical layer.
///
/// Both `filter` and `renderers` support [`LazyValue`] sources.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayerStyle {
    #[serde(default)]
    pub filter: LazyValue<LayerFilter>,

    #[serde(default)]
    pub renderers: LazyValue<Vec<LazyValue<RendererDef>>>,
}

/// Fully-resolved [`LayerStyle`].
#[derive(Debug, Clone)]
pub struct ResolvedLayerStyle {
    pub filter: ResolvedLayerFilter,
    pub renderers: Vec<ResolvedRendererDef>,
}

impl LayerStyle {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedLayerStyle> {
        let filter = self.filter.resolve(ctx)?;
        let filter = filter.resolve(ctx)?;
        let raw = self.renderers.resolve(ctx)?;
        let renderers = raw
            .into_iter()
            .map(|rv| rv.resolve(ctx)?.resolve(ctx))
            .collect::<GeelloResult<Vec<_>>>()?;
        Ok(ResolvedLayerStyle { filter, renderers })
    }
}

// ---------------------------------------------------------------------------
// LayerFilter
// ---------------------------------------------------------------------------

/// Filter that selects layers by workspace and / or name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayerFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<LazyValue<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<LazyValue<String>>,
}

/// Fully-resolved [`LayerFilter`].
#[derive(Debug, Clone)]
pub struct ResolvedLayerFilter {
    pub workspace: Option<String>,
    pub name: Option<String>,
}

impl LayerFilter {
    pub fn resolve(self, ctx: &ResolveContext) -> GeelloResult<ResolvedLayerFilter> {
        Ok(ResolvedLayerFilter {
            workspace: opt_resolve_lazy(self.workspace, ctx)?,
            name: opt_resolve_lazy(self.name, ctx)?,
        })
    }
}

impl ResolvedLayerFilter {
    pub fn matches(&self, workspace: &str, name: &str) -> bool {
        self.workspace.as_deref().map_or(true, |w| w == workspace)
            && self.name.as_deref().map_or(true, |n| n == name)
    }
}

fn opt_resolve_lazy(
    opt: Option<LazyValue<String>>,
    ctx: &ResolveContext,
) -> GeelloResult<Option<String>> {
    opt.map(|v| v.resolve(ctx)).transpose()
}
