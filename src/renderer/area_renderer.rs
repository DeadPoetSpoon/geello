use std::collections::HashMap;

use geo::{Geometry, MultiPolygon, Polygon};
use vello::{
    kurbo::{Affine, BezPath},
    peniko::{Brush, color::palette},
};

use crate::RenderedGeometry;

use super::{GeometryRenderer, LineRenderer};

#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum LineKind {
    All,
    Exterior,
    Interior,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AreaRenderer {
    pub brush: Brush,
    pub line_renderers: HashMap<LineKind, Vec<GeometryRenderer>>,
}

impl std::default::Default for AreaRenderer {
    fn default() -> Self {
        Self {
            brush: Brush::Solid(palette::css::SEA_GREEN),
            line_renderers: HashMap::default(),
        }
    }
}

impl AreaRenderer {
    pub fn draw_multi(&self, scene: &mut vello::Scene, transform: Affine, polygons: &MultiPolygon) {
        for polygon in polygons {
            self.draw(scene, transform, polygon);
        }
    }
    pub fn draw(&self, scene: &mut vello::Scene, transform: Affine, polygon: &Polygon) {
        let exterior = polygon.exterior();
        let interiors = polygon.interiors();
        let exterior_path = AreaRenderer::to_shape(polygon);
        scene.fill(
            vello::peniko::Fill::NonZero,
            transform,
            &self.brush,
            None,
            &exterior_path,
        );
        let exterior_geom: Geometry = exterior.clone().into();
        let mut exterior_geom = vec![RenderedGeometry::new_temp(exterior_geom)];
        let interior_geoms: Vec<Geometry> = interiors
            .iter()
            .map(|interior| interior.clone().into())
            .collect();
        let mut interior_geoms: Vec<RenderedGeometry> = interior_geoms
            .iter()
            .map(|interior| RenderedGeometry::new_temp(interior.clone()))
            .collect();
        for (kind, renderers) in self.line_renderers.iter() {
            match kind {
                LineKind::All => {
                    renderers.iter().for_each(|renderer| {
                        renderer.draw(scene, transform, &mut exterior_geom, None);
                        renderer.draw(scene, transform, &mut interior_geoms, None);
                    });
                }
                LineKind::Exterior => {
                    renderers.iter().for_each(|renderer| {
                        renderer.draw(scene, transform, &mut exterior_geom, None);
                    });
                }
                LineKind::Interior => {
                    renderers.iter().for_each(|renderer| {
                        renderer.draw(scene, transform, &mut interior_geoms, None);
                    });
                }
            }
        }
    }
    pub fn to_shape(polygon: &Polygon) -> BezPath {
        let exterior = polygon.exterior();
        let interiors = polygon.interiors();
        let mut exterior_path = LineRenderer::to_shape(exterior);
        interiors.iter().for_each(|x| {
            let p = LineRenderer::to_shape(x);
            p.iter().for_each(|item| {
                exterior_path.push(item);
            });
        });
        exterior_path
    }
}
