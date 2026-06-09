use std::collections::HashMap;

use geo::Point;
use vello::{
    kurbo::{Affine, Circle},
    peniko::{Brush, color::palette},
};

use crate::core::value::{LazyValue, MagicConverter, MagicFetcher, PropValue};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PointRenderer {
    #[serde(default)]
    pub radius: LazyValue<PropValue>,
    #[serde(default)]
    pub brush: LazyValue<Brush>,
    #[serde(default)]
    pub must_show: LazyValue<PropValue>,
}

impl std::default::Default for PointRenderer {
    fn default() -> Self {
        PointRenderer {
            radius: LazyValue::wrap(0.1f64),
            brush: Brush::Solid(palette::css::LIGHT_YELLOW).into(),
            must_show: LazyValue::wrap(false),
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
        let radius = self.radius.inner_try_into()?;
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
