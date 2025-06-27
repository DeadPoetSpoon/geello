pub mod point_renderer;
pub use point_renderer::*;
pub mod line_renderer;
pub use line_renderer::*;
pub mod area_renderer;
pub use area_renderer::*;

use geo::Rect;
use vello::{Scene, kurbo::Affine};

use crate::{MagicConverter, MagicFetcher, MagicValue, rendered_geometry::RenderedGeometry};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GeometryRenderer {
    Point(
        #[serde(default)] MagicValue<RenderedGeometryFilter>,
        #[serde(default)] MagicValue<PointRenderer>,
    ),
    Line(
        #[serde(default)] MagicValue<RenderedGeometryFilter>,
        #[serde(default)] MagicValue<LineRenderer>,
    ),
    Area(
        #[serde(default)] MagicValue<RenderedGeometryFilter>,
        #[serde(default)] MagicValue<AreaRenderer>,
    ),
}

impl Default for GeometryRenderer {
    fn default() -> Self {
        GeometryRenderer::Point(
            RenderedGeometryFilter::None.into(),
            PointRenderer::default().into(),
        )
    }
}

impl MagicFetcher for GeometryRenderer {
    fn fetch(&mut self) -> Result<(), String> {
        match self {
            GeometryRenderer::Point(filter, renderer) => {
                filter.fetch()?;
                renderer.fetch()?;
            }
            GeometryRenderer::Line(filter, renderer) => {
                filter.fetch()?;
                renderer.fetch()?;
            }
            GeometryRenderer::Area(filter, renderer) => {
                filter.fetch()?;
                renderer.fetch()?;
            }
        };
        Ok(())
    }
}

impl MagicConverter for GeometryRenderer {
    fn convert(
        &mut self,
        props: &std::collections::HashMap<String, crate::PropValue>,
    ) -> Result<(), String> {
        match self {
            GeometryRenderer::Point(filter, renderer) => {
                filter.convert(props)?;
                renderer.convert(props)?;
            }
            GeometryRenderer::Line(filter, renderer) => {
                filter.convert(props)?;
                renderer.convert(props)?;
            }
            GeometryRenderer::Area(filter, renderer) => {
                filter.convert(props)?;
                renderer.convert(props)?;
            }
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
            GeometryRenderer::Point(filter, renderer) => {
                let filter = filter.as_ref();
                for rendered_geometry in rendered_geometrys {
                    if rendered_geometry.fit_filter(filter) {
                        let props = rendered_geometry.props();
                        renderer.convert(props)?;
                        let renderer = renderer.as_ref();
                        if let Some(point) = rendered_geometry.center_point(render_rect) {
                            renderer.draw(scene, transform, point)?;
                        }
                    }
                }
            }
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
            }
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
            }
        };
        Ok(())
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum RenderedGeometryFilter {
    #[default]
    None,
    Layer(String),
}

impl MagicFetcher for RenderedGeometryFilter {
    fn fetch(&mut self) -> Result<(), String> {
        Ok(())
    }
}

impl MagicConverter for RenderedGeometryFilter {
    fn convert(
        &mut self,
        _: &std::collections::HashMap<String, crate::PropValue>,
    ) -> Result<(), String> {
        Ok(())
    }
}
