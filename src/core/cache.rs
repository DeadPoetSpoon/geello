use foyer::CacheBuilder;

use crate::core::error::GeelloResult;
use crate::core::layer::{LayerId, LayerSource, RenderedLayer, WorkspaceRegistry};

/// In-memory cache for parsed [`RenderedLayer`] instances.
///
/// Backed by foyer, which manages eviction automatically via LRU.
/// This is a core facility — every layer load goes through the cache,
/// whether in library or server mode.
///
/// # Examples
///
/// ```no_run
/// use geello::core::{LayerCache, LayerCacheBuilder, WorkspaceRegistry};
///
/// let cache = LayerCacheBuilder::new().capacity(256).build();
/// let registry = WorkspaceRegistry::new();
///
/// let layer = cache.get_or_load(&/* layer_id */, &registry).unwrap();
/// ```
pub struct LayerCache {
    inner: foyer::Cache<LayerId, RenderedLayer>,
}

/// Builder for [`LayerCache`] with configurable capacity.
///
/// Default capacity is **512** entries.
pub struct LayerCacheBuilder {
    capacity: usize,
}

impl LayerCacheBuilder {
    /// Creates a builder with the default capacity of 512.
    pub fn new() -> Self {
        Self { capacity: 512 }
    }

    /// Sets the maximum number of cached entries.
    pub fn capacity(mut self, n: usize) -> Self {
        self.capacity = n;
        self
    }

    /// Consumes the builder and returns a ready-to-use [`LayerCache`].
    pub fn build(self) -> LayerCache {
        let inner = CacheBuilder::new(self.capacity).build();
        LayerCache { inner }
    }
}

impl Default for LayerCacheBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LayerCache {
    /// Check the cache for `id`. On miss, resolve the [`LayerSource`]
    /// from `registry`, load the data, parse it, insert into the cache,
    /// and return the result.
    ///
    /// # Errors
    ///
    /// Returns [`GeelloError::InvalidArgument`] if the layer is not
    /// registered, or a load/parse error from the data source.
    pub fn get_or_load(
        &self,
        id: &LayerId,
        registry: &WorkspaceRegistry,
    ) -> GeelloResult<RenderedLayer> {
        if let Some(entry) = self.inner.get(id) {
            return Ok(entry.value().clone());
        }

        let source = registry.get(id).ok_or_else(|| {
            crate::core::error::GeelloError::InvalidArgument(format!(
                "Layer '{id:?}' not found in registry"
            ))
        })?;

        let layer = load_from_source(id, source)?;
        self.inner.insert(id.clone(), layer.clone());
        Ok(layer)
    }
}

/// Load and parse a [`RenderedLayer`] from its configured [`LayerSource`].
pub(crate) fn load_from_source(id: &LayerId, source: &LayerSource) -> GeelloResult<RenderedLayer> {
    match source {
        LayerSource::FileSystem { path, projection } => {
            let raw = std::fs::read_to_string(path)?;
            let src = LayerSource::FileSystem {
                path: path.clone(),
                projection: *projection,
            };
            super::geojson::parse_geojson_str(&raw, id.clone(), src)
        }
        #[cfg(feature = "from_http")]
        LayerSource::Http { .. } => Err(crate::core::error::GeelloError::GeoJson(
            "HTTP source loading not yet implemented (see 1.3.4)".into(),
        )),
        #[cfg(feature = "from_samba")]
        LayerSource::Samba { .. } => Err(crate::core::error::GeelloError::GeoJson(
            "Samba source loading not yet implemented".into(),
        )),
    }
}
