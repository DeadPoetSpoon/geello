use vello::{Scene, kurbo::Affine};

use crate::rendered_geometry::RenderedGeometry;

use super::{AreaRenderer, LineRenderer, PointRenderer};

pub enum GeometryRenderer {
    Point(PointRenderer),
    Line(LineRenderer),
    Area(AreaRenderer),
}

impl GeometryRenderer {
    pub fn draw(
        &self,
        scene: &mut Scene,
        transform: Affine,
        rendered_geometry: &mut RenderedGeometry,
    ) {
        match self {
            GeometryRenderer::Point(renderer) => {
                if let Some(point) = rendered_geometry.center_point() {
                    renderer.draw(scene, transform, point);
                }
            }
            GeometryRenderer::Line(renderer) => {
                if let Some(lines) = rendered_geometry.lines() {
                    renderer.draw_multi(scene, transform, lines);
                }
            }
            GeometryRenderer::Area(renderer) => {
                if let Some(areas) = rendered_geometry.areas() {
                    renderer.draw_multi(scene, transform, areas);
                }
            }
        }
    }
}
