use std::collections::HashMap;

use geo_types::Geometry;
use serde::{Deserialize, Serialize};

use crate::core::cache::LayerCache;
use crate::core::error::GeelloResult;

/// Unique logical identifier for a geographical layer.
///
/// Analogous to GeoServer's `workspace:layer` naming convention,
/// decoupling the logical identity from the physical data source.
///
/// # Examples
///
/// ```
/// use geello::core::LayerId;
///
/// let osm_roads = LayerId {
///     workspace: "osm".into(),
///     name: "roads".into(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LayerId {
    /// Namespace grouping related layers (e.g. "natural_earth", "osm").
    pub workspace: String,
    /// Layer name within the workspace (e.g. "countries", "roads").
    pub name: String,
}

impl LayerId {
    /// Parse a `"workspace:name"` string into a [`LayerId`].
    ///
    /// # Errors
    ///
    /// Returns `None` if the input does not contain exactly one `:` separator
    /// or either side is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use geello::core::LayerId;
    ///
    /// let id = LayerId::parse("osm:roads").unwrap();
    /// assert_eq!(id.workspace, "osm");
    /// assert_eq!(id.name, "roads");
    /// ```
    pub fn parse(input: &str) -> Option<Self> {
        let (workspace, name) = input.split_once(':')?;
        if workspace.is_empty() || name.is_empty() || workspace.contains(':') || name.contains(':')
        {
            return None;
        }
        Some(Self {
            workspace: workspace.to_owned(),
            name: name.to_owned(),
        })
    }
}

/// Physical source location of layer data.
///
/// Variants are feature-gated so that only the source types
/// enabled at compile time are available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerSource {
    /// Local filesystem path pointing to the data file.
    FileSystem {
        /// Absolute or relative path to the layer data file.
        path: String,
        /// Coordinate projection override. Defaults to [`ProjectionSpec::Auto`]
        /// which extracts the CRS from the file itself (GeoJSON `crs` member,
        /// GeoTIFF tags, etc.).
        #[serde(default)]
        projection: ProjectionSpec,
    },
    /// Remote HTTP/HTTPS resource.
    #[cfg(feature = "from_http")]
    Http {
        /// Full URL to the layer data endpoint.
        url: String,
        #[serde(default)]
        projection: ProjectionSpec,
    },
    /// Samba / SMB network share.
    #[cfg(feature = "from_samba")]
    Samba {
        /// UNC or smb:// path to the layer data file.
        path: String,
        #[serde(default)]
        projection: ProjectionSpec,
    },
}

/// Specifies how the coordinate reference system of a layer is determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectionSpec {
    /// Auto-detect from the data source (GeoJSON `crs` member, GeoTIFF tags).
    Auto,
    /// Explicitly set an EPSG code (e.g. `Epsg(4326)` for WGS84).
    Epsg(u16),
}

impl Default for ProjectionSpec {
    fn default() -> Self {
        Self::Auto
    }
}

/// Resolved coordinate reference system identified by its EPSG code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crs {
    pub epsg: u16,
}

/// Central registry mapping every [`LayerId`] to its [`LayerSource`].
///
/// Acts as the single source of truth for what data is available.
/// Designed to align with foyer-based caching: both the registry and
/// the cache use [`LayerId`] as the key, keeping invalidation simple.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceRegistry {
    layers: HashMap<LayerId, LayerSource>,
}

impl WorkspaceRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            layers: HashMap::new(),
        }
    }

    /// Registers a layer with its source. Returns the previous source if
    /// the [`LayerId`] was already registered.
    pub fn insert(&mut self, id: LayerId, source: LayerSource) -> Option<LayerSource> {
        self.layers.insert(id, source)
    }

    /// Removes a layer from the registry. Returns the source if it existed.
    pub fn remove(&mut self, id: &LayerId) -> Option<LayerSource> {
        self.layers.remove(id)
    }

    /// Looks up the source for a given [`LayerId`].
    pub fn get(&self, id: &LayerId) -> Option<&LayerSource> {
        self.layers.get(id)
    }

    /// Returns all [`LayerId`]s belonging to the given workspace.
    pub fn list_in_workspace(&self, workspace: &str) -> Vec<&LayerId> {
        self.layers
            .keys()
            .filter(|id| id.workspace == workspace)
            .collect()
    }

    /// Returns a deduplicated, sorted list of all workspace names.
    pub fn workspaces(&self) -> Vec<&str> {
        let mut set: std::collections::HashSet<&str> =
            self.layers.keys().map(|id| id.workspace.as_str()).collect();
        let mut vec: Vec<&str> = set.drain().collect();
        vec.sort_unstable();
        vec
    }
}

/// Load a single layer through the cache.
///
/// Convenience wrapper around [`LayerCache::get_or_load`]; loads and caches
/// a single layer identified by [`LayerId`].
///
/// # Errors
///
/// Returns [`GeelloError::InvalidArgument`] if the layer is not registered,
/// or a load/parse error from the data source.
pub fn load_layer(
    id: &LayerId,
    registry: &WorkspaceRegistry,
    cache: &LayerCache,
) -> GeelloResult<RenderedLayer> {
    cache.get_or_load(id, registry)
}

/// Load multiple layers by resolving their IDs against a [`WorkspaceRegistry`]
/// and a [`LayerCache`].
///
/// Accepts a comma-separated string of `"workspace:name"` pairs (e.g.
/// `"osm:roads,ne:countries"`) and loads each one in order.
///
/// Each layer ID is resolved via the cache: cache hit returns
/// immediately; cache miss triggers a load from the registered
/// [`LayerSource`], parses the data, stores it in the cache, and returns.
///
/// # Errors
///
/// Returns [`GeelloError::InvalidArgument`] if a layer ID cannot be parsed
/// or is not found in the registry. Propagates I/O and parse errors from
/// the data source on cache miss.
pub fn load_layers(
    layer_list: &str,
    registry: &WorkspaceRegistry,
    cache: &LayerCache,
) -> GeelloResult<Vec<RenderedLayer>> {
    layer_list
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|token| {
            let id = LayerId::parse(token).ok_or_else(|| {
                crate::core::error::GeelloError::InvalidArgument(format!(
                    "Invalid layer identifier: '{token}'. Expected format: 'workspace:name'"
                ))
            })?;
            cache.get_or_load(&id, registry)
        })
        .collect()
}

/// Value type for GeoJSON feature properties.
///
/// Covers the four primitive JSON value kinds.
/// Nested objects and arrays are not supported — properties
/// requiring structured values should use explicit serialisation
/// via [`LazyValue`](crate::core::LazyValue).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyValue {
    /// A UTF-8 string value.
    String(String),
    /// A 64-bit floating point number.
    Number(f64),
    /// A boolean value.
    Bool(bool),
    /// An explicit null value.
    Null,
}

/// A single rendered geometry with associated feature-level metadata.
#[derive(Debug, Clone)]
pub struct RenderedGeometry {
    /// The geometry shape.
    pub geometry: Geometry<f64>,
    /// Feature properties keyed by property name.
    ///
    /// Used by [`LazyValue::Prop`] to resolve property-driven
    /// rendering parameters at style-evaluation time.
    pub properties: HashMap<String, PropertyValue>,
}

/// A logical layer containing a collection of rendered geometries.
///
/// Each layer has a unique [`LayerId`], a known data [`LayerSource`],
/// and zero or more geometries loaded from that source.
///
/// This is the unit of caching: foyer-based caches in Phase 3 will
/// store fully-parsed [`RenderedLayer`] instances keyed by [`LayerId`].
#[derive(Debug, Clone)]
pub struct RenderedLayer {
    /// Layer identity.
    pub id: LayerId,
    /// Physical data source.
    pub source: LayerSource,
    /// Resolved coordinate reference system (EPSG code).
    pub crs: Crs,
    /// Geometries contained in this layer.
    pub geometries: Vec<RenderedGeometry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::LayerCacheBuilder;
    use std::io::Write;

    /// Helper: write a GeoJSON string to a temp file and return the path.
    fn temp_geojson_file(name: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("geello_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_layer_id_parse() {
        let id = LayerId::parse("osm:roads").unwrap();
        assert_eq!(id.workspace, "osm");
        assert_eq!(id.name, "roads");
    }

    #[test]
    fn test_layer_id_parse_invalid() {
        assert!(LayerId::parse("no_colon").is_none());
        assert!(LayerId::parse(":empty_workspace").is_none());
        assert!(LayerId::parse("empty_name:").is_none());
        assert!(LayerId::parse("a:b:c").is_none());
    }

    #[test]
    fn test_load_layers() {
        let content = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [102.0, 0.5] },
                    "properties": { "name": "a" }
                }
            ]
        }"#;
        let path_a = temp_geojson_file("a.geojson", content);
        let path_b = temp_geojson_file("b.geojson", content);

        let mut registry = WorkspaceRegistry::new();
        registry.insert(
            LayerId::parse("test:a").unwrap(),
            LayerSource::FileSystem {
                path: path_a.to_string_lossy().into(),
                projection: ProjectionSpec::Auto,
            },
        );
        registry.insert(
            LayerId::parse("test:b").unwrap(),
            LayerSource::FileSystem {
                path: path_b.to_string_lossy().into(),
                projection: ProjectionSpec::Auto,
            },
        );

        let cache = LayerCacheBuilder::new().capacity(8).build();
        let layers = load_layers("test:a,test:b", &registry, &cache).unwrap();
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].id.workspace, "test");
        assert_eq!(layers[0].id.name, "a");
        assert_eq!(layers[1].id.name, "b");
    }

    #[test]
    fn test_load_layers_not_found() {
        let registry = WorkspaceRegistry::new();
        let cache = LayerCacheBuilder::new().capacity(1).build();
        let err = load_layers("ghost:layer", &registry, &cache).unwrap_err();
        assert!(matches!(
            err,
            crate::core::error::GeelloError::InvalidArgument(_)
        ));
    }

    #[test]
    fn test_load_layers_invalid_id() {
        let registry = WorkspaceRegistry::new();
        let cache = LayerCacheBuilder::new().capacity(1).build();
        let err = load_layers("badformat", &registry, &cache).unwrap_err();
        assert!(matches!(
            err,
            crate::core::error::GeelloError::InvalidArgument(_)
        ));
    }

    #[test]
    fn test_load_layers_empty_token() {
        let content = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [0.0, 0.0] }
                }
            ]
        }"#;
        let path = temp_geojson_file("single.geojson", content);

        let mut registry = WorkspaceRegistry::new();
        registry.insert(
            LayerId::parse("test:single").unwrap(),
            LayerSource::FileSystem {
                path: path.to_string_lossy().into(),
                projection: ProjectionSpec::Auto,
            },
        );

        let cache = LayerCacheBuilder::new().capacity(1).build();
        let layers = load_layers(",,test:single,,", &registry, &cache).unwrap();
        assert_eq!(layers.len(), 1);
    }
}
