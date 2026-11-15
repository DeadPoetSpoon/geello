//! Resolved memory styles. Optional TOML rules and memory-only animation are
//! evaluated before immutable scene preparation and backend execution.
use crate::{GeelloError, ImagePlacement, VectorBatch};
use std::sync::Arc;

/// Straight sRGB color; symbol opacity multiplies the alpha channel once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub [u8; 4]);
impl Color {
    pub const BLACK: Self = Self([0, 0, 0, 255]);
    pub const BLUE: Self = Self([0, 0, 255, 255]);
    pub const GRAY: Self = Self([128, 128, 128, 255]);
    pub const TRANSPARENT: Self = Self([0, 0, 0, 0]);
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Pixels(f64),
    MapUnits(f64),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerShape {
    Circle,
    Square,
    Triangle,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}
/// Complete stroke state, fixed before backend execution. Dashes use stroke units.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeStyle {
    pub color: Color,
    pub width: Length,
    pub opacity: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f64,
    pub dashes: Vec<f64>,
    pub dash_offset: f64,
}
impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            width: Length::Pixels(1.0),
            opacity: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            dashes: vec![],
            dash_offset: 0.0,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct PointStyle {
    pub shape: MarkerShape,
    pub size: Length,
    pub angle_degrees: f64,
    pub fill: Option<Color>,
    pub stroke: Option<StrokeStyle>,
    pub opacity: f64,
}
impl Default for PointStyle {
    fn default() -> Self {
        Self {
            shape: MarkerShape::Circle,
            size: Length::Pixels(6.0),
            angle_degrees: 0.0,
            fill: Some(Color::BLUE),
            stroke: None,
            opacity: 1.0,
        }
    }
}
/// Symbols are applied in order across all batches, preserving casing/interior order.
#[derive(Debug, Clone, PartialEq)]
pub enum Symbol {
    Point(PointStyle),
    Line(StrokeStyle),
    PolygonFill { color: Color, opacity: f64 },
    PolygonOutline(StrokeStyle),
}
/// One resolved vector draw item in map order; no reader or resource lookup is retained.
#[derive(Debug, Clone)]
pub struct VectorItem {
    pub batches: Vec<Arc<VectorBatch>>,
    pub symbols: Vec<Symbol>,
}
#[derive(Debug, Clone)]
pub enum DrawItem {
    Vector(VectorItem),
    Image(ImagePlacement),
}
#[derive(Debug, Clone, Default)]
pub struct MapComposition {
    pub items: Vec<DrawItem>,
    pub background: Option<Color>,
}
/// Explicit font resource definitions remain available in backend-free builds.
#[derive(Debug, Clone)]
pub struct FontResource {
    pub identity: String,
    pub bytes: Arc<[u8]>,
    pub face_index: u32,
}
#[derive(Debug, Clone)]
pub struct TextStyle {
    pub fonts: Vec<Arc<FontResource>>,
    pub size: Length,
    pub color: Color,
    pub language: String,
}

pub(crate) fn opacity(v: f64) -> Result<(), GeelloError> {
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        Err(GeelloError::InvalidStyle(
            "opacity must be within 0..1".into(),
        ))
    } else {
        Ok(())
    }
}
pub(crate) fn length(v: Length, scale: f64) -> Result<f64, GeelloError> {
    let n = match v {
        Length::Pixels(n) => n,
        Length::MapUnits(n) => n * scale,
    };
    if !n.is_finite() || n < 0.0 || n > f32::MAX as f64 {
        return Err(GeelloError::InvalidStyle("invalid symbol length".into()));
    }
    Ok(n)
}
