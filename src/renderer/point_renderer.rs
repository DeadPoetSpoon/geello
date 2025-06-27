use std::collections::HashMap;

use geo::Point;
use vello::{
    kurbo::{Affine, Circle},
    peniko::{Brush, color::palette},
};

use crate::{MagicConverter, MagicFetcher, MagicValue, PropValue};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PointRenderer {
    #[serde(default)]
    pub radius: MagicValue<PropValue>,
    #[serde(default)]
    pub brush: MagicValue<Brush>,
}

impl std::default::Default for PointRenderer {
    fn default() -> Self {
        PointRenderer {
            radius: MagicValue::wrap(0.1f64),
            brush: Brush::Solid(palette::css::LIGHT_YELLOW).into(),
        }
    }
}

impl MagicFetcher for PointRenderer {
    fn fetch(&mut self) -> Result<(), String> {
        self.radius.fetch()?;
        self.brush.fetch()?;
        Ok(())
    }
}

impl MagicConverter for PointRenderer {
    fn convert(&mut self, props: &HashMap<String, PropValue>) -> Result<(), String> {
        self.brush.convert(props)?;
        self.radius.convert(props)?;
        Ok(())
    }
}

impl PointRenderer {
    pub fn draw(
        &self,
        scene: &mut vello::Scene,
        transform: Affine,
        point: &Point,
    ) -> Result<(), String> {
        let brush = self.brush.as_ref();
        let radius = self.radius.to_f64()?;
        scene.fill(
            vello::peniko::Fill::NonZero,
            transform,
            brush,
            None,
            &Circle::new((point.x(), point.y()), radius),
        );
        Ok(())
    }
}
