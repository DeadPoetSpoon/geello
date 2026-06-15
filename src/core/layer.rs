use std::collections::HashMap;

use geo_types::Geometry;
use serde::{Deserialize, Serialize};

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

    /// Returns a deduplicated list of all workspace names.
    pub fn workspaces(&self) -> Vec<&str> {
        let mut set: std::collections::HashSet<&str> =
            self.layers.keys().map(|id| id.workspace.as_str()).collect();
        let mut vec: Vec<&str> = set.drain().collect();
        vec.sort_unstable();
        vec
    }

    /// Number of registered layers.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Returns an iterator over all `(LayerId, LayerSource)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&LayerId, &LayerSource)> {
        self.layers.iter()
    }
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
