use std::collections::HashMap;

use geo::{Geometry, LineString, MultiLineString};
use vello::{
    kurbo::{Affine, BezPath, PathEl, Point, Stroke},
    peniko::{Brush, color::palette},
};

use crate::{MagicConverter, MagicFetcher, MagicValue, PropValue, RenderedGeometry};

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
    pub stroke: MagicValue<Stroke>,
    pub brush: MagicValue<Brush>,
    pub node_renderers: MagicValue<HashMap<NodeKind, Vec<MagicValue<GeometryRenderer>>>>,
}

impl std::default::Default for LineRenderer {
    fn default() -> Self {
        Self {
            stroke: Stroke::new(0.1f64).into(),
            brush: Brush::Solid(palette::css::AQUA).into(),
            node_renderers: HashMap::default().into(),
        }
    }
}

impl MagicFetcher for LineRenderer {
    fn fetch(&mut self) -> Result<(), String> {
        self.stroke.fetch()?;
        self.brush.fetch()?;
        self.node_renderers.fetch()?;
        Ok(())
    }
}

impl MagicConverter for LineRenderer {
    fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String> {
        self.stroke.convert(props)?;
        self.brush.convert(props)?;
        self.node_renderers.convert(props)?;
        Ok(())
    }
}

impl MagicFetcher for HashMap<NodeKind, Vec<MagicValue<GeometryRenderer>>> {
    fn fetch(&mut self) -> Result<(), String> {
        for (_, renderers) in self.iter_mut() {
            for renderer in renderers {
                renderer.fetch()?;
            }
        }
        Ok(())
    }
}

impl MagicConverter for HashMap<NodeKind, Vec<MagicValue<GeometryRenderer>>> {
    fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String> {
        for (_, renderers) in self.iter_mut() {
            for renderer in renderers {
                renderer.convert(props)?;
            }
        }
        Ok(())
    }
}

impl LineRenderer {
    pub fn draw(
        &mut self,
        scene: &mut vello::Scene,
        transform: Affine,
        line: &LineString,
    ) -> Result<(), String> {
        let path = LineRenderer::to_shape(line);
        let stroke = self.stroke.as_ref();
        let brush = self.brush.as_ref();
        let node_renderers = self.node_renderers.as_mut();
        scene.stroke(stroke, transform, brush, None, &path);
        if node_renderers.len() > 0 {
            let points = line.points();
            let len = points.len();
            for (index, point) in line.points().enumerate() {
                let geom: Geometry = point.into();
                let mut rendered_geometry =
                    vec![RenderedGeometry::new_temp(Default::default(), geom)];
                let is_start = index == 0;
                let is_end = index == len - 1;
                for (kind, renderers) in node_renderers
                    .iter_mut()
                    .map(|(k, r)| (k, r.iter_mut().map(|x| x.as_mut())))
                {
                    match kind {
                        NodeKind::All => {
                            for renderer in renderers {
                                renderer.draw(scene, transform, &mut rendered_geometry, None)?;
                            }
                        }
                        NodeKind::Mid => {
                            if !is_start && !is_end {
                                for renderer in renderers {
                                    renderer.draw(
                                        scene,
                                        transform,
                                        &mut rendered_geometry,
                                        None,
                                    )?;
                                }
                            }
                        }
                        NodeKind::Start => {
                            if is_start {
                                for renderer in renderers {
                                    renderer.draw(
                                        scene,
                                        transform,
                                        &mut rendered_geometry,
                                        None,
                                    )?;
                                }
                            }
                        }
                        NodeKind::End => {
                            if is_end {
                                for renderer in renderers {
                                    renderer.draw(
                                        scene,
                                        transform,
                                        &mut rendered_geometry,
                                        None,
                                    )?;
                                }
                            }
                        }
                    }
                }
            }
        };
        Ok(())
    }
    pub fn draw_multi(
        &mut self,
        scene: &mut vello::Scene,
        transform: Affine,
        lines: &MultiLineString,
    ) -> Result<(), String> {
        for line in lines {
            self.draw(scene, transform, line)?;
        }
        Ok(())
    }
    pub fn draw_multi_vec(
        &mut self,
        scene: &mut vello::Scene,
        transform: Affine,
        lines: Vec<&LineString>,
    ) -> Result<(), String> {
        for line in lines {
            self.draw(scene, transform, line)?;
        }
        Ok(())
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
