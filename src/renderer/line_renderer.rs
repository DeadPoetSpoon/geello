use std::collections::HashMap;

use geo::{Geometry, LineString, MultiLineString};
use vello::{
    kurbo::{Affine, BezPath, PathEl, Point, Stroke},
    peniko::{Brush, color::palette},
};

use crate::RenderedGeometry;

use super::GeometryRenderer;

#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum NodeKind {
    All,
    Mid,
    Start,
    End,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LineRenderer {
    pub stroke: Stroke,
    pub brush: Brush,
    pub node_renderers: HashMap<NodeKind, Vec<GeometryRenderer>>,
}

impl std::default::Default for LineRenderer {
    fn default() -> Self {
        Self {
            stroke: Stroke::new(0.1f64),
            brush: Brush::Solid(palette::css::AQUA),
            node_renderers: HashMap::default(),
        }
    }
}

impl LineRenderer {
    pub fn draw(&self, scene: &mut vello::Scene, transform: Affine, line: &LineString) {
        let path = LineRenderer::to_shape(line);
        scene.stroke(&self.stroke, transform, &self.brush, None, &path);
        if self.node_renderers.len() > 0 {
            let points = line.points();
            let len = points.len();
            for (index, point) in line.points().enumerate() {
                let geom: Geometry = point.into();
                let mut rendered_geometry = vec![RenderedGeometry::new_temp(geom)];
                let is_start = index == 0;
                let is_end = index == len - 1;
                for (kind, renderers) in self.node_renderers.iter() {
                    match kind {
                        NodeKind::All => {
                            for renderer in renderers {
                                renderer.draw(scene, transform, &mut rendered_geometry, None);
                            }
                        }
                        NodeKind::Mid => {
                            if !is_start && !is_end {
                                for renderer in renderers {
                                    renderer.draw(scene, transform, &mut rendered_geometry, None);
                                }
                            }
                        }
                        NodeKind::Start => {
                            if is_start {
                                for renderer in renderers {
                                    renderer.draw(scene, transform, &mut rendered_geometry, None);
                                }
                            }
                        }
                        NodeKind::End => {
                            if is_end {
                                for renderer in renderers {
                                    renderer.draw(scene, transform, &mut rendered_geometry, None);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    pub fn draw_multi(&self, scene: &mut vello::Scene, transform: Affine, lines: &MultiLineString) {
        for line in lines {
            self.draw(scene, transform, line);
        }
    }
    pub fn draw_multi_vec(
        &self,
        scene: &mut vello::Scene,
        transform: Affine,
        lines: Vec<&LineString>,
    ) {
        for line in lines {
            self.draw(scene, transform, line);
        }
    }
    pub fn to_shape(line: &LineString) -> BezPath {
        let mut points = line.points();
        let mut path = BezPath::new();
        let first_point = points.next().unwrap();
        path.push(PathEl::MoveTo(Point::new(first_point.x(), first_point.y())));
        for point in points {
            path.push(PathEl::LineTo(Point::new(point.x(), point.y())));
        }
        path.push(PathEl::ClosePath);
        path
    }
}
