pub mod cache;
pub mod error;
pub mod geojson;
pub mod layer;
pub mod lazy;
pub mod style;

pub use cache::{LayerCache, LayerCacheBuilder};
pub use error::{GeelloError, GeelloResult};
pub use layer::{
    Crs, LayerId, LayerSource, ProjectionSpec, PropertyValue, RenderedGeometry, RenderedLayer,
    WorkspaceRegistry, load_layer, load_layers,
};
pub use lazy::{Encoding, FromPropValue, LazyValue, ResolveContext, opt_resolve};
