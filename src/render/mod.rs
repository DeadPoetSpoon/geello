pub mod area;
pub mod line;
pub mod point;
pub mod text;

#[cfg(feature = "raster")]
pub mod raster;

pub use area::*;
pub use line::*;
pub use point::*;
pub use text::*;

#[cfg(feature = "raster")]
pub use raster::*;

use geo::Rect;
use vello::{Scene, kurbo::Affine};

use crate::core::filter::RenderedGeometryFilter;
use crate::core::geometry::RenderedGeometry;
use crate::core::value::{LazyValue, MagicConverter, MagicFetcher, PropValue};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum GeometryRenderer {
    #[default]
    None,
    Point(
        #[serde(default)] LazyValue<RenderedGeometryFilter>,
        #[serde(default)] LazyValue<PointRenderer>,
    ),
    Line(
        #[serde(default)] LazyValue<RenderedGeometryFilter>,
        #[serde(default)] LazyValue<LineRenderer>,
    ),
    Area(
        #[serde(default)] LazyValue<RenderedGeometryFilter>,
        #[serde(default)] LazyValue<AreaRenderer>,
    ),
    /// Text label renderer — renders text at geometry points
    Text(
        #[serde(default)] LazyValue<RenderedGeometryFilter>,
        #[serde(default)] LazyValue<TextRenderer>,
    ),
    /// Raster renderer — renders GeoTIFF imagery (feature = "raster")
    #[cfg(feature = "raster")]
    Raster(#[serde(default)] LazyValue<RasterRenderer>),
}

impl MagicFetcher for GeometryRenderer {
    fn fetch(&mut self) -> Result<(), String> {
        match self {
            GeometryRenderer::None => {},
            GeometryRenderer::Point(filter, renderer) => {
                filter.fetch()?;
                renderer.fetch()?;
            },
            GeometryRenderer::Line(filter, renderer) => {
                filter.fetch()?;
                renderer.fetch()?;
            },
            GeometryRenderer::Area(filter, renderer) => {
                filter.fetch()?;
                renderer.fetch()?;
            },
            GeometryRenderer::Text(filter, renderer) => {
                filter.fetch()?;
                renderer.fetch()?;
            },
            #[cfg(feature = "raster")]
            GeometryRenderer::Raster(renderer) => {
                renderer.fetch()?;
            },
        };
        Ok(())
    }
}

impl MagicConverter for GeometryRenderer {
    fn convert(
        &mut self,
        props: &std::collections::HashMap<String, PropValue>,
    ) -> Result<(), String> {
        match self {
            GeometryRenderer::None => {},
            GeometryRenderer::Point(filter, renderer) => {
                filter.convert(props)?;
                renderer.convert(props)?;
            },
            GeometryRenderer::Line(filter, renderer) => {
                filter.convert(props)?;
                renderer.convert(props)?;
            },
            GeometryRenderer::Area(filter, renderer) => {
                filter.convert(props)?;
                renderer.convert(props)?;
            },
            GeometryRenderer::Text(filter, renderer) => {
                filter.convert(props)?;
                renderer.convert(props)?;
            },
            #[cfg(feature = "raster")]
            GeometryRenderer::Raster(_r) => {
                // Raster has no GeoJSON properties to convert
            },
        };
        Ok(())
    }
}

impl GeometryRenderer {
    pub fn draw(
        &mut self,
        scene: &mut Scene,
        transform: Affine,
        rendered_geometrys: &mut Vec<RenderedGeometry>,
        render_rect: Option<Rect>,
    ) -> Result<(), String> {
        match self {
            GeometryRenderer::None => {},
            GeometryRenderer::Point(filter, renderer) => {
                let filter = filter.as_ref();
                for rendered_geometry in rendered_geometrys {
                    if rendered_geometry.fit_filter(filter) {
                        let props = rendered_geometry.props();
                        renderer.convert(props)?;
                        let renderer = renderer.as_ref();
                        let true_render_rect = if renderer.must_show.inner_try_into()? {
                            render_rect
                        } else {
                            None
                        };
                        if let Some(point) = rendered_geometry.center_point(true_render_rect) {
                            renderer.draw(scene, transform, point)?;
                        }
                    }
                }
            },
            GeometryRenderer::Line(filter, renderer) => {
                let filter = filter.as_ref();
                for rendered_geometry in rendered_geometrys {
                    if rendered_geometry.fit_filter(filter) {
                        let props = rendered_geometry.props();
                        renderer.convert(props)?;
                        let renderer = renderer.as_mut();
                        let lines = rendered_geometry.lines();
                        if lines.is_some() {
                            let lines = lines.unwrap();
                            renderer.draw_multi(scene, transform, lines)?;
                        }
                    }
                }
            },
            GeometryRenderer::Area(filter, renderer) => {
                let filter = filter.as_ref();
                for rendered_geometry in rendered_geometrys {
                    if rendered_geometry.fit_filter(filter) {
                        let props = rendered_geometry.props();
                        renderer.convert(props)?;
                        let renderer = renderer.as_mut();
                        if let Some(areas) = rendered_geometry.areas() {
                            renderer.draw_multi(scene, transform, areas)?;
                        }
                    }
                }
            },
            GeometryRenderer::Text(filter, renderer) => {
                let filter = filter.as_ref();
                for rendered_geometry in rendered_geometrys {
                    if rendered_geometry.fit_filter(filter) {
                        let props = rendered_geometry.props();
                        renderer.convert(props)?;
                        let renderer = renderer.as_ref();
                        let true_render_rect = if render_rect.is_some() {
                            render_rect
                        } else {
                            None
                        };
                        if let Some(point) = rendered_geometry.center_point(true_render_rect) {
                            renderer.draw(scene, transform, point)?;
                        }
                    }
                }
            },
            #[cfg(feature = "raster")]
            GeometryRenderer::Raster(renderer) => {
                let renderer = renderer.as_ref();
                renderer.draw(scene, transform, render_rect)?;
            },
        };
        Ok(())
    }
}
