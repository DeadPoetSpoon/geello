//! Typed, lossless memory values. Coordinates used for spatial work are XY.
use geo_types::Geometry;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};
use thiserror::Error;

/// An original integer or string identity. Nonnegative integers compare canonically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FeatureId {
    String(String),
    I64(i64),
    U64(u64),
}
impl PartialEq for FeatureId {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a == b,
            (Self::I64(a), Self::I64(b)) => a == b,
            (Self::U64(a), Self::U64(b)) => a == b,
            (Self::I64(a), Self::U64(b)) | (Self::U64(b), Self::I64(a)) => {
                *a >= 0 && *a as u64 == *b
            }
            _ => false,
        }
    }
}
impl Eq for FeatureId {}
impl Hash for FeatureId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::String(s) => {
                0u8.hash(state);
                s.hash(state);
            }
            Self::I64(n) if *n < 0 => {
                1u8.hash(state);
                n.hash(state);
            }
            Self::I64(n) => {
                2u8.hash(state);
                (*n as u64).hash(state);
            }
            Self::U64(n) => {
                2u8.hash(state);
                n.hash(state);
            }
        }
    }
}

/// A recursive property tree; absent keys and explicit nulls remain distinct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    Null,
    Bool(bool),
    String(String),
    I64(i64),
    U64(u64),
    F64(f64),
    Array(Vec<PropertyValue>),
    Object(BTreeMap<String, PropertyValue>),
}
/// Deterministic property ordering independent of hash seeds.
pub type Properties = BTreeMap<String, PropertyValue>;

/// Original ordinates indexed by structural coordinate paths, never synthesized.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceOrdinate {
    pub coordinate_path: Vec<usize>,
    pub trailing: Vec<f64>,
}
/// A native feature. None is null geometry; empty geo-types values remain empty.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// Original source identity before an identity policy supplies an effective ID.
    pub source_id: Option<PropertyValue>,
    pub id: Option<FeatureId>,
    pub geometry: Option<Geometry<f64>>,
    pub properties: Properties,
    pub source_ordinal: u64,
    pub source_ordinates: Vec<SourceOrdinate>,
}
/// A batch in one explicitly declared source CRS, in stable source order.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorBatch {
    pub crs: Crs,
    pub features: Vec<Feature>,
}

/// Conservative clone envelope, including nested containers and empty geometry
/// components. Allocator bookkeeping/RSS is distinct from operation accounting.
pub(crate) fn feature_clone_bytes(feature: &Feature) -> Result<usize, GeelloError> {
    use geo::CoordsIter;
    fn add(a: usize, b: usize) -> Result<usize, GeelloError> {
        a.checked_add(b).ok_or(GeelloError::SizeOverflow)
    }
    fn property(v: &PropertyValue, depth: usize) -> Result<usize, GeelloError> {
        if depth > 64 {
            return Err(GeelloError::BudgetExceeded("property depth"));
        }
        let mut n = 128;
        match v {
            PropertyValue::String(s) => n = add(n, s.len())?,
            PropertyValue::Array(a) => {
                for v in a {
                    n = add(n, property(v, depth + 1)?)?;
                }
            }
            PropertyValue::Object(o) => {
                for (k, v) in o {
                    n = add(add(n, k.len())?, property(v, depth + 1)?)?;
                }
            }
            _ => {}
        }
        Ok(n)
    }
    fn geometry(g: &Geometry, depth: usize) -> Result<usize, GeelloError> {
        if depth > 64 {
            return Err(GeelloError::BudgetExceeded("geometry depth"));
        }
        let containers = match g {
            Geometry::GeometryCollection(c) => {
                return c
                    .0
                    .iter()
                    .try_fold(128usize, |n, g| add(n, geometry(g, depth + 1)?));
            }
            Geometry::MultiLineString(l) => l.0.len(),
            Geometry::MultiPolygon(p) => {
                p.0.iter()
                    .try_fold(p.0.len(), |n, p| add(n, p.interiors().len()))?
            }
            Geometry::Polygon(p) => p.interiors().len(),
            _ => 0,
        };
        add(
            add(
                128,
                containers
                    .checked_mul(128)
                    .ok_or(GeelloError::SizeOverflow)?,
            )?,
            g.coords_count()
                .checked_mul(32)
                .ok_or(GeelloError::SizeOverflow)?,
        )
    }
    let mut n = 512;
    if let Some(FeatureId::String(s)) = &feature.id {
        n = add(n, s.len())?;
    }
    if let Some(id) = &feature.source_id {
        n = add(n, property(id, 0)?)?;
    }
    if let Some(g) = &feature.geometry {
        n = add(n, geometry(g, 0)?)?;
    }
    for (k, v) in &feature.properties {
        n = add(add(n, k.len())?, property(v, 0)?)?;
    }
    for o in &feature.source_ordinates {
        n = add(
            add(n, 128)?,
            add(
                o.coordinate_path
                    .len()
                    .checked_mul(8)
                    .ok_or(GeelloError::SizeOverflow)?,
                o.trailing
                    .len()
                    .checked_mul(8)
                    .ok_or(GeelloError::SizeOverflow)?,
            )?,
        )?;
    }
    Ok(n)
}

/// Coordinate interpretation. Layer one does not guess EPSG axis order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinateSpace {
    Planar,
    GeographicXy,
}
/// A declared CRS identity; construction validates identity syntax, not a projection database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crs {
    definition: String,
    space: CoordinateSpace,
}
impl Crs {
    pub fn new(definition: impl Into<String>, space: CoordinateSpace) -> Result<Self, GeelloError> {
        let definition = definition.into();
        if definition.trim().is_empty() || definition.chars().any(char::is_control) {
            return Err(GeelloError::InvalidCrs);
        }
        Ok(Self { definition, space })
    }
    pub fn definition(&self) -> &str {
        &self.definition
    }
    pub fn space(&self) -> CoordinateSpace {
        self.space
    }
}
/// Finite, non-wrapped XY bounds. Degenerate views require explicit expansion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}
impl Bounds {
    pub fn validate(&self) -> Result<(), GeelloError> {
        if [self.min_x, self.min_y, self.max_x, self.max_y]
            .iter()
            .any(|v| !v.is_finite())
            || self.max_x < self.min_x
            || self.max_y < self.min_y
        {
            return Err(GeelloError::InvalidView(
                "finite ordered bounds required".into(),
            ));
        }
        Ok(())
    }
    /// Expand each zero span using a caller-supplied world-unit minimum.
    pub fn expanded(self, minimum_span: f64) -> Result<Self, GeelloError> {
        self.validate()?;
        if !minimum_span.is_finite() || minimum_span <= 0.0 {
            return Err(GeelloError::InvalidView(
                "positive finite minimum span required".into(),
            ));
        }
        let mut b = self;
        if b.min_x == b.max_x {
            b.min_x -= minimum_span / 2.0;
            b.max_x += minimum_span / 2.0;
        }
        if b.min_y == b.max_y {
            b.min_y -= minimum_span / 2.0;
            b.max_y += minimum_span / 2.0;
        }
        b.validate()?;
        Ok(b)
    }
}
/// Library fit preserves aspect ratio; service adapters may explicitly stretch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AspectPolicy {
    #[default]
    Fit,
    Stretch,
}
/// A validated view is frozen into a prepared scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub crs: Crs,
    pub bounds: Bounds,
    pub width: u32,
    pub height: u32,
    pub aspect: AspectPolicy,
}
impl View {
    pub fn validate(&self) -> Result<(), GeelloError> {
        self.bounds.validate()?;
        Crs::new(self.crs.definition.clone(), self.crs.space)?;
        if self.width == 0
            || self.height == 0
            || self.bounds.min_x == self.bounds.max_x
            || self.bounds.min_y == self.bounds.max_y
        {
            return Err(GeelloError::InvalidView(
                "non-zero dimensions and positive spans required".into(),
            ));
        }
        if self.width > u16::MAX as u32 || self.height > u16::MAX as u32 {
            return Err(GeelloError::DimensionOverflow);
        }
        let b = self.bounds;
        if !(b.max_x - b.min_x).is_finite() || !(b.max_y - b.min_y).is_finite() {
            return Err(GeelloError::InvalidView("span overflow".into()));
        }
        if self.crs.space == CoordinateSpace::GeographicXy
            && (b.min_x < -180.0 || b.max_x > 180.0 || b.min_y < -90.0 || b.max_y > 90.0)
        {
            return Err(GeelloError::InvalidView(
                "geographic domain exceeded; wrapped views must be split".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn scales(&self) -> (f64, f64, f64, f64) {
        let sx = self.width as f64 / (self.bounds.max_x - self.bounds.min_x);
        let sy = self.height as f64 / (self.bounds.max_y - self.bounds.min_y);
        match self.aspect {
            AspectPolicy::Stretch => (sx, sy, 0.0, 0.0),
            AspectPolicy::Fit => {
                let s = sx.min(sy);
                (
                    s,
                    s,
                    (self.width as f64 - s * (self.bounds.max_x - self.bounds.min_x)) / 2.0,
                    (self.height as f64 - s * (self.bounds.max_y - self.bounds.min_y)) / 2.0,
                )
            }
        }
    }
    pub(crate) fn pixel(&self, x: f64, y: f64) -> Result<[f64; 2], GeelloError> {
        let (sx, sy, ox, oy) = self.scales();
        let p = [
            (x - self.bounds.min_x) * sx + ox,
            (self.bounds.max_y - y) * sy + oy,
        ];
        if p.iter()
            .any(|v| !v.is_finite() || v.abs() > f32::MAX as f64)
        {
            return Err(GeelloError::InvalidGeometry(
                "pixel coordinate overflow".into(),
            ));
        }
        Ok(p)
    }
}

/// Errors carry the operation boundary; post-submit GPU errors invalidate output.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum GeelloError {
    #[error("operation cancelled")]
    Cancelled,
    #[error("operation deadline exceeded")]
    DeadlineExceeded,
    #[error("{0} budget exceeded")]
    BudgetExceeded(&'static str),
    #[error("size exceeds supported range")]
    SizeOverflow,
    #[error("allocation failed")]
    AllocationFailed,
    #[error("render dimensions exceed backend limit")]
    DimensionOverflow,
    #[error("invalid CRS")]
    InvalidCrs,
    #[error("projection required; source and view CRS differ")]
    ProjectionRequired,
    #[error("invalid view: {0}")]
    InvalidView(String),
    #[error("invalid raster: {0}")]
    InvalidRaster(String),
    #[error("invalid geometry: {0}")]
    InvalidGeometry(String),
    #[error("invalid style: {0}")]
    InvalidStyle(String),
    #[error("source I/O error: {0}")]
    Io(String),
    #[error("invalid source: {0}")]
    InvalidSource(String),
    #[error("source changed since snapshot preparation")]
    SourceChanged,
    #[error("unsupported capability: {0}")]
    Unsupported(String),
    #[error("invalid configuration: {0}")]
    InvalidConfiguration(String),
    #[error("projection error: {0}")]
    Projection(String),
    #[error("projection error: {0}")]
    ProjectionDomain(String),
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    #[error("backend error: {0}")]
    Backend(String),
    #[error("GPU submission completed unsuccessfully; target invalid: {0}")]
    TargetInvalid(String),
}

/// Tight top-left straight RGBA8 sRGB output; transparent RGB is canonical zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl RgbaImage {
    pub fn from_rgba(width: u32, height: u32, mut pixels: Vec<u8>) -> Result<Self, GeelloError> {
        if width == 0 || height == 0 || pixels.len() != pixel_bytes(width, height)? {
            return Err(GeelloError::InvalidRaster(
                "RGBA dimensions/length mismatch".into(),
            ));
        }
        for p in pixels.chunks_exact_mut(4) {
            if p[3] == 0 {
                p[..3].fill(0);
            }
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}
pub(crate) fn pixel_bytes(w: u32, h: u32) -> Result<usize, GeelloError> {
    (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(GeelloError::SizeOverflow)
}
