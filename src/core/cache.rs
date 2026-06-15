use foyer::CacheBuilder;

use crate::core::error::GeelloResult;
use crate::core::layer::{LayerId, LayerSource, RenderedLayer, WorkspaceRegistry};

/// In-memory cache for parsed [`RenderedLayer`] instances.
///
/// Backed by foyer, which manages eviction automatically via LRU.
/// This is a core facility — every layer load goes through the cache,
/// whether in library or server mode.
///
/// When `from_http` is enabled, the cache can be configured with a
/// [`reqwest::blocking::Client`] to fetch layers from HTTP sources.
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
    #[cfg(feature = "from_http")]
    http_client: Option<reqwest::blocking::Client>,
}

/// Builder for [`LayerCache`] with configurable capacity and optional
/// HTTP client.
///
/// Default capacity is **512** entries.
pub struct LayerCacheBuilder {
    capacity: usize,
    #[cfg(feature = "from_http")]
    http_client: Option<reqwest::blocking::Client>,
}

impl LayerCacheBuilder {
    /// Creates a builder with the default capacity of 512.
    pub fn new() -> Self {
        Self {
            capacity: 512,
            #[cfg(feature = "from_http")]
            http_client: None,
        }
    }

    /// Sets the maximum number of cached entries.
    pub fn capacity(mut self, n: usize) -> Self {
        self.capacity = n;
        self
    }

    /// Configures a [`reqwest::blocking::Client`] for loading layers
    /// from HTTP sources.
    ///
    /// Required only when using [`LayerSource::Http`] variants; ignored
    /// when `from_http` is disabled.
    #[cfg(feature = "from_http")]
    pub fn http_client(mut self, client: reqwest::blocking::Client) -> Self {
        self.http_client = Some(client);
        self
    }

    /// Consumes the builder and returns a ready-to-use [`LayerCache`].
    pub fn build(self) -> LayerCache {
        let inner = CacheBuilder::new(self.capacity).build();
        LayerCache {
            inner,
            #[cfg(feature = "from_http")]
            http_client: self.http_client,
        }
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

        let layer = self.load_from_source(id, source)?;
        self.inner.insert(id.clone(), layer.clone());
        Ok(layer)
    }

    /// Load and parse a [`RenderedLayer`] from its configured [`LayerSource`].
    fn load_from_source(&self, id: &LayerId, source: &LayerSource) -> GeelloResult<RenderedLayer> {
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
            LayerSource::Http { url, projection } => {
                let client = self.http_client.as_ref().ok_or_else(|| {
                    crate::core::error::GeelloError::GeoJson(
                        "HTTP client not configured. Use LayerCacheBuilder::http_client()".into(),
                    )
                })?;
                let response = client.get(url).send()?;
                let raw = response.text()?;
                let src = LayerSource::Http {
                    url: url.clone(),
                    projection: *projection,
                };
                super::geojson::parse_geojson_str(&raw, id.clone(), src)
            }
            #[cfg(feature = "from_samba")]
            LayerSource::Samba { .. } => Err(crate::core::error::GeelloError::GeoJson(
                "Samba source loading not yet implemented".into(),
            )),
        }
    }
}
