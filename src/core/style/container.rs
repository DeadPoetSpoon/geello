use serde::{Deserialize, Serialize};

use super::renderer::RendererDef;

/// Top-level style definition — the unit of serialisation (TOML file).
///
/// A [`Style`] contains zero or more [`LayerStyle`] entries, each of which
/// targets a set of layers (by workspace / name filter) and specifies which
/// renderers to apply.
///
/// All fields use `#[serde(default)]` so that missing keys in the file
/// fall back to sensible defaults.  When the file is absent altogether,
/// [`Style::default()`] produces a valid empty style.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Style {
    /// Per-layer rendering configurations, matched top-to-bottom.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerStyle>,
}

/// Rendering configuration that targets one logical layer.
///
/// A [`LayerStyle`] is applied when [`Self::filter`] matches the layer's
/// [`LayerId`](crate::core::LayerId).  Multiple [`LayerStyle`] entries may
/// match the same layer — they are all applied, in order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayerStyle {
    /// Layer selection filter.  An empty/default filter matches everything.
    #[serde(default)]
    pub filter: LayerFilter,

    /// Ordered list of renderers to apply to the matched layer(s).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renderers: Vec<RendererDef>,
}

/// Filter that selects layers by workspace and / or name.
///
/// A filter with both fields set to `None` matches *all* layers.
/// Setting one or both restricts the match.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayerFilter {
    /// If set, only layers in this workspace are matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,

    /// If set, only layers with this exact name are matched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl LayerFilter {
    /// Returns `true` when this filter matches the given workspace and name.
    pub fn matches(&self, workspace: &str, name: &str) -> bool {
        self.workspace.as_deref().map_or(true, |w| w == workspace)
            && self.name.as_deref().map_or(true, |n| n == name)
    }
}
