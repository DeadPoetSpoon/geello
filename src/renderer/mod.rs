pub mod point_renderer;
pub use point_renderer::*;
pub mod line_renderer;
pub use line_renderer::*;
pub mod area_renderer;
pub use area_renderer::*;

use geo::Rect;
use vello::{Scene, kurbo::Affine};

use crate::rendered_geometry::RenderedGeometry;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GeometryRenderer {
    Point(RenderedGeometryFilter, PointRenderer),
    Line(RenderedGeometryFilter, LineRenderer),
    Area(RenderedGeometryFilter, AreaRenderer),
}

impl Default for GeometryRenderer {
    fn default() -> Self {
        GeometryRenderer::Point(RenderedGeometryFilter::None, PointRenderer::default())
    }
}

impl GeometryRenderer {
    pub fn draw(
        &self,
        scene: &mut Scene,
        transform: Affine,
        rendered_geometrys: &mut Vec<RenderedGeometry>,
        render_rect: Option<Rect>,
    ) {
        match self {
            GeometryRenderer::Point(filter, renderer) => {
                for rendered_geometry in rendered_geometrys {
                    if rendered_geometry.fit_filter(filter) {
                        if let Some(point) = rendered_geometry.center_point(render_rect) {
                            renderer.draw(scene, transform, point);
                        }
                    }
                }
            }
            GeometryRenderer::Line(filter, renderer) => {
                for rendered_geometry in rendered_geometrys {
                    if rendered_geometry.fit_filter(filter) {
                        if let Some(lines) = rendered_geometry.lines() {
                            renderer.draw_multi(scene, transform, lines);
                        }
                    }
                }
            }
            GeometryRenderer::Area(filter, renderer) => {
                for rendered_geometry in rendered_geometrys {
                    if rendered_geometry.fit_filter(filter) {
                        if let Some(areas) = rendered_geometry.areas() {
                            renderer.draw_multi(scene, transform, areas);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum RenderedGeometryFilter {
    #[default]
    None,
    Layer(String),
}
