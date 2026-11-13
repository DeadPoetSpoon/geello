//! Native numeric raster bands and explicit georeferencing.
use crate::{Crs, GeelloError, RgbaImage};
use std::sync::Arc;

/// Numeric values retain their native type, including full-range integers.
#[derive(Debug, Clone, PartialEq)]
pub enum RasterSamples {
    U8(Arc<[u8]>),
    U16(Arc<[u16]>),
    U32(Arc<[u32]>),
    U64(Arc<[u64]>),
    I8(Arc<[i8]>),
    I16(Arc<[i16]>),
    I32(Arc<[i32]>),
    I64(Arc<[i64]>),
    F32(Arc<[f32]>),
    F64(Arc<[f64]>),
}
impl RasterSamples {
    pub fn len(&self) -> usize {
        match self {
            Self::U8(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::U32(v) => v.len(),
            Self::U64(v) => v.len(),
            Self::I8(v) => v.len(),
            Self::I16(v) => v.len(),
            Self::I32(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::F32(v) => v.len(),
            Self::F64(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
/// A band has separate validity and calibration. Raw reads do not apply either.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterBand {
    pub samples: RasterSamples,
    pub validity: Option<Arc<[bool]>>,
    pub no_data: Option<PropertySample>,
    pub scale: f64,
    pub offset: f64,
}
/// Typed NoData avoids rounding integer sentinels through floating point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PropertySample {
    I64(i64),
    U64(u64),
    F64(f64),
}
/// Pixel interpretation affects georeferencing, never numeric sample ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelInterpretation {
    Area,
    Point,
}
/// Affine coefficients: x=a+b*column+c*row, y=d+e*column+f*row.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterGrid {
    pub crs: Crs,
    pub affine: [f64; 6],
    pub interpretation: PixelInterpretation,
}
impl RasterGrid {
    pub fn validate(&self) -> Result<(), GeelloError> {
        let [_, b, c, _, e, f] = self.affine;
        let det = b * f - c * e;
        if self.affine.iter().any(|v| !v.is_finite()) || !det.is_finite() || det == 0.0 {
            return Err(GeelloError::InvalidRaster(
                "singular or non-finite grid".into(),
            ));
        }
        Ok(())
    }
}
/// Contiguous native bands share one grid; alpha is separate from validity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaAssociation {
    #[default]
    Straight,
    Premultiplied,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RasterBlock {
    pub width: u32,
    pub height: u32,
    pub grid: RasterGrid,
    pub bands: Vec<RasterBand>,
    pub alpha: Option<Arc<[f32]>>,
    pub alpha_association: AlphaAssociation,
}
/// Borrowed access does not copy the native bands or their masks.
#[derive(Debug, Clone, Copy)]
pub struct RasterView<'a> {
    pub block: &'a RasterBlock,
}
impl RasterBlock {
    pub fn validate(&self) -> Result<(), GeelloError> {
        self.grid.validate()?;
        let n = (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or(GeelloError::SizeOverflow)?;
        if n == 0 || self.bands.is_empty() {
            return Err(GeelloError::InvalidRaster(
                "nonempty dimensions and bands required".into(),
            ));
        }
        if self.alpha_association == AlphaAssociation::Premultiplied && self.alpha.is_none() {
            return Err(GeelloError::InvalidRaster(
                "premultiplied raster requires alpha".into(),
            ));
        }
        for b in &self.bands {
            let nodata_ok = matches!(
                (&b.samples, b.no_data),
                (_, None)
                    | (
                        RasterSamples::U8(_)
                            | RasterSamples::U16(_)
                            | RasterSamples::U32(_)
                            | RasterSamples::U64(_),
                        Some(PropertySample::U64(_))
                    )
                    | (
                        RasterSamples::I8(_)
                            | RasterSamples::I16(_)
                            | RasterSamples::I32(_)
                            | RasterSamples::I64(_),
                        Some(PropertySample::I64(_))
                    )
                    | (
                        RasterSamples::F32(_) | RasterSamples::F64(_),
                        Some(PropertySample::F64(_))
                    )
            );
            if !nodata_ok {
                return Err(GeelloError::InvalidRaster(
                    "NoData type does not match native samples".into(),
                ));
            }
            if b.samples.len() != n
                || b.validity.as_ref().is_some_and(|m| m.len() != n)
                || !b.scale.is_finite()
                || !b.offset.is_finite()
            {
                return Err(GeelloError::InvalidRaster(
                    "band shape or calibration invalid".into(),
                ));
            }
        }
        if self.alpha.as_ref().is_some_and(|a| {
            a.len() != n || a.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        }) {
            return Err(GeelloError::InvalidRaster("invalid alpha".into()));
        }
        Ok(())
    }
    pub fn view(&self) -> RasterView<'_> {
        RasterView { block: self }
    }
}
/// Display-ready image placement. Raw raster-to-color processing follows in M2.
#[derive(Debug, Clone)]
pub struct ImagePlacement {
    pub image: Arc<RgbaImage>,
    pub crs: Crs,
    pub bounds: crate::Bounds,
    pub opacity: f64,
}
