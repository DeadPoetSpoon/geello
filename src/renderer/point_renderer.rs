use geo::Point;
use vello::{
    kurbo::{Affine, Circle},
    peniko::{Brush, color::palette},
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PointRenderer {
    pub radius: f64,
    pub brush: Brush,
}

impl std::default::Default for PointRenderer {
    fn default() -> Self {
        PointRenderer {
            radius: 0.1f64,
            brush: Brush::Solid(palette::css::LIGHT_YELLOW),
        }
    }
}

impl PointRenderer {
    pub fn draw(&self, scene: &mut vello::Scene, transform: Affine, point: &Point) {
        scene.fill(
            vello::peniko::Fill::NonZero,
            transform,
            &self.brush,
            None,
            &Circle::new((point.x(), point.y()), self.radius),
        );
    }
}
