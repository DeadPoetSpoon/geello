//! Numeric validity, calibration, resampling and display are independent stages.
use crate::operation::Reservation;
use crate::{
    GeelloError, OperationContext, PropertySample, RasterBand, RasterBlock, RasterGrid,
    RasterSamples, RgbaImage,
};
use std::{ops::Deref, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resampling {
    Nearest,
    Bilinear,
    AreaAverage,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterKind {
    Continuous,
    Categorical,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleRange {
    pub min: f64,
    pub max: f64,
}
impl SampleRange {
    pub fn validate(self) -> Result<(), GeelloError> {
        if !self.min.is_finite()
            || !self.max.is_finite()
            || self.min > self.max
            || !(self.max - self.min).is_finite()
        {
            Err(GeelloError::InvalidRaster(
                "finite ordered display range required".into(),
            ))
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone)]
pub struct RasterColorOptions {
    pub bands: Vec<usize>,
    pub ranges: Vec<SampleRange>,
    pub opacity: f64,
}
#[derive(Debug)]
pub struct ProcessedRaster {
    image: RgbaImage,
    pub invalid_pixels: usize,
    pub precision_warning: bool,
    pub constant_range: bool,
    _reservation: Reservation,
}
impl ProcessedRaster {
    /// Borrowing retains the image reservation; an explicit clone is caller-owned.
    pub fn image(&self) -> &RgbaImage {
        &self.image
    }
}
#[derive(Debug)]
pub struct ProcessedBlock {
    block: RasterBlock,
    _reservation: Reservation,
}
impl Deref for ProcessedBlock {
    type Target = RasterBlock;
    fn deref(&self) -> &RasterBlock {
        &self.block
    }
}

/// Typed NoData comparison happens before calibration; u64 sentinels never pass
/// through f64. Floating NaN/Infinity are invalid for processing only.
pub fn valid_sample(b: &RasterBand, i: usize) -> bool {
    if i >= b.samples.len()
        || b.validity
            .as_ref()
            .is_some_and(|m| !m.get(i).copied().unwrap_or(false))
    {
        return false;
    }
    let missing = match (&b.samples, b.no_data) {
        (RasterSamples::U8(v), Some(PropertySample::U64(n))) => v[i] as u64 == n,
        (RasterSamples::U16(v), Some(PropertySample::U64(n))) => v[i] as u64 == n,
        (RasterSamples::U32(v), Some(PropertySample::U64(n))) => v[i] as u64 == n,
        (RasterSamples::U64(v), Some(PropertySample::U64(n))) => v[i] == n,
        (RasterSamples::I8(v), Some(PropertySample::I64(n))) => v[i] as i64 == n,
        (RasterSamples::I16(v), Some(PropertySample::I64(n))) => v[i] as i64 == n,
        (RasterSamples::I32(v), Some(PropertySample::I64(n))) => v[i] as i64 == n,
        (RasterSamples::I64(v), Some(PropertySample::I64(n))) => v[i] == n,
        (_, Some(PropertySample::F64(n))) => {
            sample(&b.samples, i) == n || n.is_nan() && sample(&b.samples, i).is_nan()
        }
        _ => false,
    };
    !missing && sample(&b.samples, i).is_finite()
}
pub(crate) fn sample(s: &RasterSamples, i: usize) -> f64 {
    match s {
        RasterSamples::U8(v) => v[i] as f64,
        RasterSamples::U16(v) => v[i] as f64,
        RasterSamples::U32(v) => v[i] as f64,
        RasterSamples::U64(v) => v[i] as f64,
        RasterSamples::I8(v) => v[i] as f64,
        RasterSamples::I16(v) => v[i] as f64,
        RasterSamples::I32(v) => v[i] as f64,
        RasterSamples::I64(v) => v[i] as f64,
        RasterSamples::F32(v) => v[i] as f64,
        RasterSamples::F64(v) => v[i],
    }
}
fn calibrated(b: &RasterBand, i: usize) -> Option<f64> {
    if !valid_sample(b, i) {
        return None;
    }
    let v = sample(&b.samples, i) * b.scale + b.offset;
    v.is_finite().then_some(v)
}
fn straight_value(block: &RasterBlock, band: usize, i: usize) -> Option<f64> {
    let b = &block.bands[band];
    if !valid_sample(b, i) {
        return None;
    }
    let mut raw = sample(&b.samples, i);
    if block.alpha_association == crate::AlphaAssociation::Premultiplied {
        let alpha = block.alpha.as_ref()?[i] as f64;
        raw = if alpha > 0. { raw / alpha } else { 0. };
    }
    let v = raw * b.scale + b.offset;
    v.is_finite().then_some(v)
}
/// DataVersion-wide statistics use the same unassociated, calibrated values as
/// colorization. Transparent hidden samples cannot set automatic display ranges.
pub fn block_band_range(
    block: &RasterBlock,
    band: usize,
    context: &OperationContext,
) -> Result<Option<SampleRange>, GeelloError> {
    block.validate()?;
    if band >= block.bands.len() {
        return Err(GeelloError::InvalidRaster("band outside raster".into()));
    }
    let mut range: Option<SampleRange> = None;
    for i in 0..block.bands[band].samples.len() {
        context.checkpoint(1)?;
        if block.alpha.as_ref().is_some_and(|a| a[i] == 0.) {
            continue;
        }
        if let Some(v) = straight_value(block, band, i) {
            range = Some(match range {
                Some(r) => SampleRange {
                    min: r.min.min(v),
                    max: r.max.max(v),
                },
                None => SampleRange { min: v, max: v },
            });
        }
    }
    Ok(range)
}
fn imprecise(s: &RasterSamples) -> bool {
    match s {
        RasterSamples::U64(v) => v.iter().any(|n| *n > 1 << 53),
        RasterSamples::I64(v) => v.iter().any(|n| n.unsigned_abs() > 1 << 53),
        _ => false,
    }
}
/// Compute a version-wide range once, never independently per render viewport.
pub fn band_range(
    b: &RasterBand,
    context: &OperationContext,
) -> Result<Option<SampleRange>, GeelloError> {
    if b.validity
        .as_ref()
        .is_some_and(|m| m.len() != b.samples.len())
        || !b.scale.is_finite()
        || !b.offset.is_finite()
    {
        return Err(GeelloError::InvalidRaster(
            "invalid band mask/calibration".into(),
        ));
    }
    let mut range: Option<SampleRange> = None;
    for i in 0..b.samples.len() {
        context.checkpoint(1)?;
        if let Some(v) = calibrated(b, i) {
            range = Some(match range {
                Some(r) => SampleRange {
                    min: r.min.min(v),
                    max: r.max.max(v),
                },
                None => SampleRange { min: v, max: v },
            })
        }
    }
    Ok(range)
}
pub fn colorize(
    block: &RasterBlock,
    o: &RasterColorOptions,
    context: &OperationContext,
) -> Result<ProcessedRaster, GeelloError> {
    block.validate()?;
    if !matches!(o.bands.len(), 1 | 3)
        || o.bands.len() != o.ranges.len()
        || o.bands.iter().any(|b| *b >= block.bands.len())
        || !o.opacity.is_finite()
        || !(0.0..=1.).contains(&o.opacity)
    {
        return Err(GeelloError::InvalidRaster(
            "display selection/ranges/opacity invalid".into(),
        ));
    }
    for r in &o.ranges {
        r.validate()?;
    }
    let n = block.width as usize * block.height as usize;
    let reservation = context.reserve(n.checked_mul(4).ok_or(GeelloError::SizeOverflow)?)?;
    let mut pixels = vec![0; n * 4];
    let mut invalid = 0;
    for i in 0..n {
        context.checkpoint(1)?;
        let mut colors = [0u8; 3];
        let mut valid = true;
        for (j, (band, range)) in o.bands.iter().zip(&o.ranges).enumerate() {
            if let Some(v) = straight_value(block, *band, i) {
                colors[j] = if range.min == range.max {
                    128
                } else {
                    (((v - range.min) / (range.max - range.min)).clamp(0., 1.) * 255.).round() as u8
                };
            } else {
                valid = false;
                break;
            }
        }
        if !valid {
            invalid += 1;
            continue;
        }
        if o.bands.len() == 1 {
            colors[1] = colors[0];
            colors[2] = colors[0];
        }
        let alpha = block.alpha.as_ref().map_or(1., |a| a[i] as f64) * o.opacity;
        pixels[i * 4..i * 4 + 3].copy_from_slice(&colors);
        pixels[i * 4 + 3] = (alpha * 255.).round() as u8;
    }
    Ok(ProcessedRaster {
        image: RgbaImage::from_rgba(block.width, block.height, pixels)?,
        invalid_pixels: invalid,
        precision_warning: o.bands.iter().any(|b| imprecise(&block.bands[*b].samples)),
        constant_range: o.ranges.iter().any(|r| r.min == r.max),
        _reservation: reservation,
    })
}
/// Resample to an explicit target grid. The mapping closure converts target XY
/// to source XY; callers select identity or an explicit inverse CRS operation.
/// Bilinear/area output is calibrated f64; nearest retains native samples.
#[allow(clippy::too_many_arguments)]
pub fn resample(
    block: &RasterBlock,
    width: u32,
    height: u32,
    grid: RasterGrid,
    kind: RasterKind,
    method: Resampling,
    target_to_source: impl Fn([f64; 2]) -> Result<[f64; 2], GeelloError>,
    context: &OperationContext,
) -> Result<ProcessedBlock, GeelloError> {
    block.validate()?;
    grid.validate()?;
    if width == 0 || height == 0 {
        return Err(GeelloError::InvalidRaster(
            "positive target dimensions required".into(),
        ));
    }
    if kind == RasterKind::Categorical && method != Resampling::Nearest {
        return Err(GeelloError::InvalidRaster(
            "categorical data requires nearest".into(),
        ));
    }
    let n = (width as usize)
        .checked_mul(height as usize)
        .ok_or(GeelloError::SizeOverflow)?;
    let reservation = context.reserve(
        n.checked_mul(
            block
                .bands
                .len()
                .checked_mul(40)
                .and_then(|v| v.checked_add(32))
                .ok_or(GeelloError::SizeOverflow)?,
        )
        .ok_or(GeelloError::SizeOverflow)?,
    )?;
    let mut selections = vec![None; n];
    let mut output: Vec<Vec<f64>> = block.bands.iter().map(|_| vec![0.; n]).collect();
    let mut masks: Vec<Vec<bool>> = block.bands.iter().map(|_| vec![false; n]).collect();
    let mut alpha = vec![0f32; n];
    let _scratch = context.reserve(
        block
            .bands
            .len()
            .checked_mul(32)
            .ok_or(GeelloError::SizeOverflow)?,
    )?;
    let [a, b, c, d, e, f] = block.grid.affine;
    let det = b * f - c * e;
    let source_pixel = |x: f64, y: f64| -> Result<[f64; 2], GeelloError> {
        let g = grid.affine;
        let p = target_to_source([g[0] + g[1] * x + g[2] * y, g[3] + g[4] * x + g[5] * y])?;
        let pixel = [
            ((p[0] - a) * f - (p[1] - d) * c) / det,
            ((p[1] - d) * b - (p[0] - a) * e) / det,
        ];
        if pixel.iter().any(|v| !v.is_finite()) {
            return Err(GeelloError::Projection(
                "nonfinite raster inverse mapping".into(),
            ));
        }
        Ok(pixel)
    };
    for y in 0..height {
        for x in 0..width {
            context.checkpoint(1)?;
            let out = (y * width + x) as usize;
            let p = source_pixel(x as f64 + 0.5, y as f64 + 0.5)?;
            let mut common_weight = 0.;
            let mut alpha_sum = 0.;
            let mut sums = vec![0.; block.bands.len()];
            let mut band_weights = vec![0.; block.bands.len()];
            let mut band_alpha = vec![0.; block.bands.len()];
            // Stream overlaps: a single output cell can cover the entire source,
            // so retaining a list of source weights would be unbounded scratch.
            let mut add = |sx: i64, sy: i64, w: f64| -> Result<(), GeelloError> {
                context.checkpoint(1)?;
                if sx >= 0
                    && sy >= 0
                    && sx < block.width as i64
                    && sy < block.height as i64
                    && w > 0.
                {
                    let i = (sy * block.width as i64 + sx) as usize;
                    if method == Resampling::Nearest {
                        selections[out] = Some(i);
                    }
                    let a = block.alpha.as_ref().map_or(1., |a| a[i] as f64);
                    let mut any = false;
                    for (j, band) in block.bands.iter().enumerate() {
                        let value = if method == Resampling::Nearest {
                            valid_sample(band, i).then_some(0.)
                        } else {
                            straight_value(block, j, i)
                        };
                        if let Some(value) = value {
                            any = true;
                            band_weights[j] += w;
                            band_alpha[j] += w * a;
                            sums[j] += value * w * a;
                        }
                    }
                    if any {
                        common_weight += w;
                        alpha_sum += w * a;
                    }
                }
                Ok(())
            };
            match method {
                Resampling::Nearest => add(p[0].floor() as i64, p[1].floor() as i64, 1.)?,
                Resampling::Bilinear => {
                    let px = p[0] - 0.5;
                    let py = p[1] - 0.5;
                    let ix = px.floor() as i64;
                    let iy = py.floor() as i64;
                    let dx = px - ix as f64;
                    let dy = py - iy as f64;
                    add(ix, iy, (1. - dx) * (1. - dy))?;
                    add(ix + 1, iy, dx * (1. - dy))?;
                    add(ix, iy + 1, (1. - dx) * dy)?;
                    add(ix + 1, iy + 1, dx * dy)?;
                }
                Resampling::AreaAverage => {
                    area_cell(
                        [x as f64, y as f64, x as f64 + 1., y as f64 + 1.],
                        0,
                        &source_pixel,
                        block.width,
                        block.height,
                        &mut add,
                        context,
                    )?;
                }
            }
            if common_weight > 0. {
                alpha[out] = (alpha_sum / common_weight) as f32;
                for j in 0..block.bands.len() {
                    masks[j][out] = band_weights[j] > 0.;
                    output[j][out] = if band_alpha[j] > 0. {
                        sums[j] / band_alpha[j]
                    } else {
                        0.
                    };
                }
            }
        }
    }
    let mut bands = vec![];
    for (j, source) in block.bands.iter().enumerate() {
        let samples = if method == Resampling::Nearest {
            select_native(&source.samples, &selections)
        } else {
            RasterSamples::F64(Arc::from(std::mem::take(&mut output[j])))
        };
        bands.push(RasterBand {
            samples,
            validity: Some(Arc::from(std::mem::take(&mut masks[j]))),
            no_data: if method == Resampling::Nearest {
                source.no_data
            } else {
                None
            },
            scale: if method == Resampling::Nearest {
                source.scale
            } else {
                1.
            },
            offset: if method == Resampling::Nearest {
                source.offset
            } else {
                0.
            },
        });
    }
    Ok(ProcessedBlock {
        block: RasterBlock {
            width,
            height,
            grid,
            bands,
            alpha: Some(Arc::from(alpha)),
            alpha_association: if method == Resampling::Nearest {
                block.alpha_association
            } else {
                crate::AlphaAssociation::Straight
            },
        },
        _reservation: reservation,
    })
}
/// Adaptive inverse footprints use a 1/64 source-pixel curvature tolerance.
/// A discontinuity that cannot meet it within eight subdivisions fails explicitly.
/// Affine footprints, including rotations, use exact polygon/cell intersections.
fn area_cell(
    cell: [f64; 4],
    depth: u8,
    map: &impl Fn(f64, f64) -> Result<[f64; 2], GeelloError>,
    width: u32,
    height: u32,
    add: &mut impl FnMut(i64, i64, f64) -> Result<(), GeelloError>,
    context: &OperationContext,
) -> Result<(), GeelloError> {
    context.checkpoint(1)?;
    let [x0, y0, x1, y1] = cell;
    let (xm, ym) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let p = [map(x0, y0)?, map(x1, y0)?, map(x1, y1)?, map(x0, y1)?];
    let mid = [map(xm, y0)?, map(x1, ym)?, map(xm, y1)?, map(x0, ym)?];
    let center = map(xm, ym)?;
    let curved = (0..4).any(|i| {
        (0..2)
            .any(|axis| (mid[i][axis] - (p[i][axis] + p[(i + 1) % 4][axis]) * 0.5).abs() > 1. / 64.)
    }) || (0..2).any(|axis| {
        (center[axis] - p.iter().map(|p| p[axis]).sum::<f64>() * 0.25).abs() > 1. / 64.
    });
    if curved {
        if depth == 8 {
            return Err(GeelloError::Projection(
                "raster footprint tolerance exceeded".into(),
            ));
        }
        for sub in [
            [x0, y0, xm, ym],
            [xm, y0, x1, ym],
            [xm, ym, x1, y1],
            [x0, ym, xm, y1],
        ] {
            area_cell(sub, depth + 1, map, width, height, add, context)?;
        }
        return Ok(());
    }
    let xmin = p
        .iter()
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.)
        .min(width as f64) as i64;
    let xmax = p
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .max(0.)
        .min(width as f64) as i64;
    let ymin = p
        .iter()
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(0.)
        .min(height as f64) as i64;
    let ymax = p
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .max(0.)
        .min(height as f64) as i64;
    // Triangulation also handles a concave inverse quadrilateral without assuming
    // that the mapped ring is convex. Scratch storage is fixed on the stack.
    for sy in ymin..ymax {
        for sx in xmin..xmax {
            let mut area = 0.;
            for tri in [[p[0], p[1], p[2]], [p[0], p[2], p[3]]] {
                area += clipped_area(tri, sx as f64, sy as f64);
            }
            add(sx, sy, area)?;
        }
    }
    Ok(())
}
fn clipped_area(triangle: [[f64; 2]; 3], x: f64, y: f64) -> f64 {
    let mut points = [[0.; 2]; 12];
    points[..3].copy_from_slice(&triangle);
    let mut n = 3;
    for (axis, bound, lower) in [
        (0, x, true),
        (0, x + 1., false),
        (1, y, true),
        (1, y + 1., false),
    ] {
        if n == 0 {
            return 0.;
        }
        let mut next = [[0.; 2]; 12];
        let mut count = 0;
        let mut a = points[n - 1];
        let inside = |p: [f64; 2]| {
            if lower {
                p[axis] >= bound
            } else {
                p[axis] <= bound
            }
        };
        for b in points[..n].iter().copied() {
            if inside(a) != inside(b) {
                let t = (bound - a[axis]) / (b[axis] - a[axis]);
                next[count] = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                count += 1;
            }
            if inside(b) {
                next[count] = b;
                count += 1;
            }
            a = b;
        }
        points = next;
        n = count;
    }
    // Translate before the cross product to avoid cancellation at large offsets.
    (0..n)
        .map(|i| {
            let a = points[i];
            let b = points[(i + 1) % n];
            (a[0] - x) * (b[1] - y) - (b[0] - x) * (a[1] - y)
        })
        .sum::<f64>()
        .abs()
        * 0.5
}
fn select_native(s: &RasterSamples, indices: &[Option<usize>]) -> RasterSamples {
    macro_rules! select {
        ($v:ident) => {
            if let RasterSamples::$v(v) = s {
                RasterSamples::$v(Arc::from(
                    indices
                        .iter()
                        .map(|i| i.map_or(Default::default(), |i| v[i]))
                        .collect::<Vec<_>>(),
                ))
            } else {
                unreachable!()
            }
        };
    }
    match s {
        RasterSamples::U8(_) => select!(U8),
        RasterSamples::U16(_) => select!(U16),
        RasterSamples::U32(_) => select!(U32),
        RasterSamples::U64(_) => select!(U64),
        RasterSamples::I8(_) => select!(I8),
        RasterSamples::I16(_) => select!(I16),
        RasterSamples::I32(_) => select!(I32),
        RasterSamples::I64(_) => select!(I64),
        RasterSamples::F32(_) => select!(F32),
        RasterSamples::F64(_) => select!(F64),
    }
}
