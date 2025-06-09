use std::collections::HashMap;

use geo::{Geometry, MultiPolygon, Polygon};
use vello::{
    kurbo::{Affine, Stroke},
    peniko::{Brush, color::palette},
};

use crate::RenderedGeometry;

use super::{GeometryRenderer, LineRenderer};

pub enum LineKind {
    All,
    Exterior,
    Interior,
}

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
        let exterior_path = LineRenderer::to_shape(exterior);
        scene.fill(
            vello::peniko::Fill::NonZero,
            transform,
            &self.brush,
            None,
            &exterior_path,
        );
        let interior_paths = interiors
            .iter()
            .map(LineRenderer::to_shape)
            .collect::<Vec<_>>();
        interior_paths.iter().for_each(|path| {
            scene.fill(
                vello::peniko::Fill::NonZero,
                transform,
                palette::css::TRANSPARENT,
                None,
                path,
            );
        });
        let exterior_geom: &Geometry = &exterior.clone().into();
        let mut exterior_geom: RenderedGeometry = exterior_geom.into();
        let interior_geoms: Vec<Geometry> = interiors
            .iter()
            .map(|interior| interior.clone().into())
            .collect();
        let mut interior_geoms: Vec<RenderedGeometry> = interior_geoms
            .iter()
            .map(|interior| interior.into())
            .collect();
        for (kind, renderers) in self.line_renderers.iter() {
            match kind {
                LineKind::All => {
                    renderers.iter().for_each(|renderer| {
                        renderer.draw(scene, transform, &mut exterior_geom);
                    });
                    renderers.iter().for_each(|render| {
                        interior_geoms.iter_mut().for_each(|mut interior| {
                            render.draw(scene, transform, &mut interior);
                        });
                    });
                }
                LineKind::Exterior => {
                    renderers.iter().for_each(|renderer| {
                        renderer.draw(scene, transform, &mut exterior_geom);
                    });
                }
                LineKind::Interior => {
                    renderers.iter().for_each(|render| {
                        interior_geoms.iter_mut().for_each(|mut interior| {
                            render.draw(scene, transform, &mut interior);
                        });
                    });
                }
            }
        }
    }
}
