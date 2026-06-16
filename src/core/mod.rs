pub mod cache;
pub mod error;
pub mod geojson;
pub mod layer;
pub mod lazy;
pub mod projection;
pub mod style;

pub use cache::{LayerCache, LayerCacheBuilder};
pub use error::{GeelloError, GeelloResult};
pub use layer::{
    Crs, LayerId, LayerSource, ProjectionSpec, PropertyValue, RenderedGeometry, RenderedLayer,
    WorkspaceRegistry, load_layer, load_layers,
};
pub use lazy::{Encoding, FromPropValue, LazyValue, ResolveContext, opt_resolve};
pub use projection::{
    TargetCrs, WorldToPixel, tile_to_rect_3857, tile_to_rect_4326, transform_layer,
};
