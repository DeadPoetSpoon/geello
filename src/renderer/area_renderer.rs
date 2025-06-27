use std::collections::HashMap;

use geo::{Geometry, MultiPolygon, Polygon};
use vello::{
    kurbo::{Affine, BezPath},
    peniko::{Brush, color::palette},
};

use crate::{MagicConverter, MagicFetcher, MagicValue, PropValue, RenderedGeometry};

use super::{GeometryRenderer, LineRenderer};

#[derive(Clone, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub enum LineKind {
    All,
    Exterior,
    Interior,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AreaRenderer {
    pub brush: MagicValue<Brush>,
    pub line_renderers: MagicValue<HashMap<LineKind, Vec<MagicValue<GeometryRenderer>>>>,
}

impl std::default::Default for AreaRenderer {
    fn default() -> Self {
        Self {
            brush: Brush::Solid(palette::css::SEA_GREEN).into(),
            line_renderers: HashMap::default().into(),
        }
    }
}

impl MagicFetcher for AreaRenderer {
    fn fetch(&mut self) -> Result<(), String> {
        self.brush.fetch()?;
        self.line_renderers.fetch()?;
        Ok(())
    }
}

impl MagicConverter for AreaRenderer {
    fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String> {
        self.brush.convert(props)?;
        self.line_renderers.convert(props)?;
        Ok(())
    }
}

impl MagicFetcher for HashMap<LineKind, Vec<MagicValue<GeometryRenderer>>> {
    fn fetch(&mut self) -> Result<(), String> {
        for (_, renderers) in self.iter_mut() {
            for renderer in renderers {
                renderer.fetch()?;
            }
        }
        Ok(())
    }
}

impl MagicConverter for HashMap<LineKind, Vec<MagicValue<GeometryRenderer>>> {
    fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String> {
        for (_, renderers) in self.iter_mut() {
            for renderer in renderers {
                renderer.convert(props)?;
            }
        }
        Ok(())
    }
}

impl AreaRenderer {
    pub fn draw_multi(
        &mut self,
        scene: &mut vello::Scene,
        transform: Affine,
        polygons: &MultiPolygon,
    ) -> Result<(), String> {
        for polygon in polygons {
            self.draw(scene, transform, polygon)?;
        }
        Ok(())
    }
    pub fn draw(
        &mut self,
        scene: &mut vello::Scene,
        transform: Affine,
        polygon: &Polygon,
    ) -> Result<(), String> {
        let brush = self.brush.as_ref();
        let line_renderers = self.line_renderers.as_mut();
        let exterior = polygon.exterior();
        let interiors = polygon.interiors();
        let exterior_path = AreaRenderer::to_shape(polygon);
        scene.fill(
            vello::peniko::Fill::NonZero,
            transform,
            brush,
            None,
            &exterior_path,
        );
        let exterior_geom: Geometry = exterior.clone().into();
        let mut exterior_geom = vec![RenderedGeometry::new_temp(
            Default::default(),
            exterior_geom,
        )];
        let interior_geoms: Vec<Geometry> = interiors
            .iter()
            .map(|interior| interior.clone().into())
            .collect();
        let mut interior_geoms: Vec<RenderedGeometry> = interior_geoms
            .iter()
            .map(|interior| RenderedGeometry::new_temp(Default::default(), interior.clone()))
            .collect();
        for (kind, renderers) in line_renderers.iter_mut() {
            match kind {
                LineKind::All => {
                    for renderer in renderers.iter_mut().map(|x| x.as_mut()) {
                        renderer.draw(scene, transform, &mut exterior_geom, None)?;
                        renderer.draw(scene, transform, &mut interior_geoms, None)?;
                    }
                }
                LineKind::Exterior => {
                    for renderer in renderers.iter_mut().map(|x| x.as_mut()) {
                        renderer.draw(scene, transform, &mut exterior_geom, None)?;
                    }
                }
                LineKind::Interior => {
                    for renderer in renderers.iter_mut().map(|x| x.as_mut()) {
                        renderer.draw(scene, transform, &mut interior_geoms, None)?;
                    }
                }
            }
        }
        Ok(())
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
