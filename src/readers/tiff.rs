//! Native chunk-window GeoTIFF access. Source bytes are snapshotted once under
//! budget so independent cursors never reopen a changing external path.
use super::{io, source};
use crate::operation::Reservation;
use crate::{
    CoordinateSpace, Crs, GeelloError, OperationContext, PixelInterpretation, PropertySample,
    RasterBand, RasterBlock, RasterGrid, RasterSamples,
};
use ::tiff::{
    decoder::{Decoder, DecodingResult, Limits},
    tags::Tag,
};
use std::{
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct RasterReadOptions {
    pub crs_override: Option<Crs>,
    pub primary_ifd: Option<usize>,
    pub max_source_bytes: usize,
    pub max_decode_bytes: usize,
    pub max_tag_bytes: usize,
}
#[derive(Debug, Clone)]
pub struct RasterMetadata {
    pub width: u32,
    pub height: u32,
    pub grid: RasterGrid,
    pub bands: usize,
    /// Original file depth, including binary16 promoted to f32 by native reads.
    pub bits_per_sample: u16,
    pub has_subifd: bool,
    pub white_is_zero: bool,
    pub diagnostics: Vec<String>,
}
#[derive(Debug)]
struct RasterSource {
    bytes: Arc<[u8]>,
    raw_view: Arc<RawTiffView>,
    _reservations: Vec<Reservation>,
}
#[derive(Debug, Clone)]
pub struct RasterSnapshot {
    metadata: RasterMetadata,
    source: Arc<RasterSource>,
    options: RasterReadOptions,
    ifd: usize,
    alpha: Option<u16>,
    bits: u16,
    sample_format: u16,
    no_data: Option<String>,
}
#[derive(Debug, Clone, Copy)]
pub struct NativeWindow {
    pub column: u32,
    pub row: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug)]
pub struct RasterReadBlock {
    block: RasterBlock,
    _reservation: Reservation,
}
impl std::ops::Deref for RasterReadBlock {
    type Target = RasterBlock;
    fn deref(&self) -> &RasterBlock {
        &self.block
    }
}
/// The decoder otherwise normalizes WhiteIsZero (including floating samples).
/// Reversing that arithmetic loses raw precision, so expose a read-only overlay
/// of just the photometry tag. Source bytes and all numeric samples stay intact.
#[derive(Debug, Default)]
struct RawTiffView {
    patches: Vec<(u64, u8)>,
    white_ifds: std::collections::HashSet<u64>,
}
struct SourceCursor {
    cursor: Cursor<Arc<[u8]>>,
    raw_view: Arc<RawTiffView>,
}
impl Read for SourceCursor {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let start = self.cursor.position();
        let n = self.cursor.read(buffer)?;
        let first = self.raw_view.patches.partition_point(|(p, _)| *p < start);
        for &(position, value) in &self.raw_view.patches[first..] {
            if position - start >= n as u64 {
                break;
            }
            buffer[(position - start) as usize] = value;
        }
        Ok(n)
    }
}
impl Seek for SourceCursor {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(position)
    }
}
type TiffDecoder = Decoder<SourceCursor>;
/// Bound directory allocation before Decoder::new, whose initial directory is read
/// before caller limits can be attached. Traverse main and SubIFD graphs alike.
fn preflight_directories(
    bytes: &[u8],
    options: &RasterReadOptions,
    context: &OperationContext,
) -> Result<RawTiffView, GeelloError> {
    let mut view = RawTiffView::default();
    if bytes.len() < 8 {
        return Err(source("truncated TIFF header"));
    }
    let le = &bytes[..2] == b"II";
    if !le && &bytes[..2] != b"MM" {
        return Err(source("invalid TIFF byte order"));
    }
    let number = |offset: usize, size: usize| -> Result<u64, GeelloError> {
        let end = offset.checked_add(size).ok_or(GeelloError::SizeOverflow)?;
        let data = bytes
            .get(offset..end)
            .ok_or_else(|| source("TIFF offset outside source"))?;
        let mut out = 0u64;
        if le {
            for (i, b) in data.iter().enumerate() {
                out |= (*b as u64) << (8 * i);
            }
        } else {
            for b in data {
                out = (out << 8) | *b as u64;
            }
        }
        Ok(out)
    };
    let big = match number(2, 2)? {
        42 => false,
        43 => true,
        _ => return Err(source("invalid TIFF magic")),
    };
    if big && (number(4, 2)? != 8 || number(6, 2)? != 0) {
        return Err(source("invalid BigTIFF offset size"));
    }
    let (pointer, count_size, entry_size, inline) = if big {
        (8usize, 8usize, 20usize, 8usize)
    } else {
        (4, 2, 12, 4)
    };
    let mut pending = vec![number(if big { 8 } else { 4 }, pointer)?];
    let mut seen = std::collections::HashSet::new();
    while let Some(offset) = pending.pop() {
        if offset == 0 {
            continue;
        }
        context.checkpoint(1)?;
        if !seen.insert(offset) {
            return Err(source("cyclic/repeated TIFF directory"));
        }
        if seen.len() > options.max_tag_bytes / 16 {
            return Err(GeelloError::BudgetExceeded("IFD count"));
        }
        let offset = usize::try_from(offset).map_err(|_| GeelloError::SizeOverflow)?;
        let count =
            usize::try_from(number(offset, count_size)?).map_err(|_| GeelloError::SizeOverflow)?;
        let start = offset
            .checked_add(count_size)
            .ok_or(GeelloError::SizeOverflow)?;
        let span = count
            .checked_mul(entry_size)
            .ok_or(GeelloError::SizeOverflow)?;
        if span > options.max_tag_bytes {
            return Err(GeelloError::BudgetExceeded("TIFF tags"));
        }
        let end = start.checked_add(span).ok_or(GeelloError::SizeOverflow)?;
        pending.push(number(end, pointer)?);
        let mut total = span;
        for i in 0..count {
            context.checkpoint(1)?;
            let p = start + i * entry_size;
            let tag = number(p, 2)?;
            let kind = number(p + 2, 2)?;
            let elements = usize::try_from(number(p + 4, if big { 8 } else { 4 })?)
                .map_err(|_| GeelloError::SizeOverflow)?;
            let size = match kind {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 | 16 | 17 | 18 => 8,
                _ => return Err(source("invalid TIFF tag type")),
            };
            let len = elements
                .checked_mul(size)
                .ok_or(GeelloError::SizeOverflow)?;
            total = total.checked_add(len).ok_or(GeelloError::SizeOverflow)?;
            if total > options.max_tag_bytes {
                return Err(GeelloError::BudgetExceeded("TIFF tag bytes"));
            }
            let value = p + if big { 12 } else { 8 };
            let data = if len <= inline {
                value
            } else {
                usize::try_from(number(value, pointer)?).map_err(|_| GeelloError::SizeOverflow)?
            };
            let stop = data.checked_add(len).ok_or(GeelloError::SizeOverflow)?;
            if stop > bytes.len() {
                return Err(source("TIFF tag outside source"));
            }
            if (tag == 254 && elements == 1 && number(data, size)? & 4 != 0)
                || (tag == 262 && elements == 1 && number(data, size)? == 4)
            {
                return Err(GeelloError::Unsupported(
                    "internal GeoTIFF validity mask".into(),
                ));
            }
            if tag == 262 && kind == 3 && elements == 1 && number(data, size)? == 0 {
                view.white_ifds.insert(offset as u64);
                view.patches
                    .push(((data + if le { 0 } else { 1 }) as u64, 1));
            }
            if tag == 330 {
                if !matches!(kind, 4 | 13 | 16 | 18) {
                    return Err(source("invalid SubIFD pointer type"));
                }
                if pending
                    .len()
                    .checked_add(elements)
                    .ok_or(GeelloError::SizeOverflow)?
                    > options.max_tag_bytes / 16
                {
                    return Err(GeelloError::BudgetExceeded("SubIFD count"));
                }
                for i in 0..elements {
                    pending.push(number(data + i * size, size)?);
                }
            }
        }
    }
    view.patches.sort_unstable_by_key(|p| p.0);
    Ok(view)
}
fn decoder(
    bytes: Arc<[u8]>,
    raw_view: Arc<RawTiffView>,
    options: &RasterReadOptions,
) -> Result<TiffDecoder, GeelloError> {
    let mut limits = Limits::default();
    limits.decoding_buffer_size = options.max_decode_bytes;
    limits.intermediate_buffer_size = options.max_decode_bytes;
    limits.ifd_value_size = options.max_tag_bytes;
    Ok(Decoder::new(SourceCursor {
        cursor: Cursor::new(bytes),
        raw_view,
    })
    .map_err(source)?
    .with_limits(limits))
}
pub fn open_geotiff(
    path: impl AsRef<Path>,
    options: &RasterReadOptions,
    context: &OperationContext,
) -> Result<RasterSnapshot, GeelloError> {
    read_geotiff(File::open(path).map_err(io)?, options, context)
}
pub fn read_geotiff(
    mut input: impl Read,
    options: &RasterReadOptions,
    context: &OperationContext,
) -> Result<RasterSnapshot, GeelloError> {
    context.check()?;
    if options.max_source_bytes == 0 || options.max_decode_bytes == 0 || options.max_tag_bytes == 0
    {
        return Err(source("positive raster limits required"));
    }
    // Charge the source as it grows. The configured maximum bounds input but is
    // not itself an allocation; large files must not consume the whole budget
    // before any bytes have been read.
    let mut reservations = Vec::new();
    let mut bytes = vec![];
    let mut buffer = [0u8; 64 * 1024];
    loop {
        context.checkpoint(1)?;
        let n = input.read(&mut buffer).map_err(io)?;
        if n == 0 {
            break;
        }
        if bytes
            .len()
            .checked_add(n)
            .ok_or(GeelloError::SizeOverflow)?
            > options.max_source_bytes
        {
            return Err(GeelloError::BudgetExceeded("source bytes"));
        }
        // Reserve before growing the Vec. Its geometric capacity and the final
        // Arc conversion can overlap, so retain a conservative three-copy
        // envelope for actual input, rather than the configured upper bound.
        reservations.push(context.reserve(n.checked_mul(3).ok_or(GeelloError::SizeOverflow)?)?);
        bytes.extend_from_slice(&buffer[..n]);
    }
    let bytes: Arc<[u8]> = Arc::from(bytes);
    let _directory_scratch = context.reserve(
        options
            .max_tag_bytes
            .checked_mul(32)
            .ok_or(GeelloError::SizeOverflow)?,
    )?;
    let raw_view = Arc::new(preflight_directories(&bytes, options, context)?);
    let mut d = decoder(bytes.clone(), raw_view.clone(), options)?;
    let mut primary = vec![];
    let mut index = 0;
    let mut has_subifd = false;
    loop {
        context.checkpoint(1)?;
        let flags = d
            .find_tag_unsigned::<u32>(Tag::NewSubfileType)
            .map_err(source)?
            .unwrap_or(0);
        if flags & 4 != 0
            || d.find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
                .map_err(source)?
                == Some(4)
        {
            return Err(GeelloError::Unsupported(
                "internal GeoTIFF validity mask".into(),
            ));
        }
        if let Some(offsets) = d.find_tag(Tag::SubIfd).map_err(source)? {
            has_subifd = true;
            let mut pending = offsets.into_ifd_vec().map_err(source)?;
            let mut seen = std::collections::HashSet::new();
            while let Some(ptr) = pending.pop() {
                context.checkpoint(1)?;
                if !seen.insert(ptr.0) {
                    return Err(source("cyclic/repeated SubIFD"));
                }
                if seen.len() > options.max_tag_bytes / 16 {
                    return Err(GeelloError::BudgetExceeded("SubIFD count"));
                }
                let dir = d.read_directory(ptr).map_err(source)?;
                let mut tags = d.read_directory_tags(&dir);
                if tags
                    .find_tag_unsigned::<u32>(Tag::NewSubfileType)
                    .map_err(source)?
                    .unwrap_or(0)
                    & 4
                    != 0
                    || tags
                        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
                        .map_err(source)?
                        == Some(4)
                {
                    return Err(GeelloError::Unsupported("SubIFD validity mask".into()));
                }
                if let Some(v) = tags.find_tag(Tag::SubIfd).map_err(source)? {
                    pending.extend(v.into_ifd_vec().map_err(source)?);
                }
            }
        }
        if flags & 1 == 0 {
            primary.push(index);
        }
        if !d.more_images() {
            break;
        }
        d.next_image().map_err(source)?;
        index += 1;
        if index > options.max_tag_bytes / 16 {
            return Err(GeelloError::BudgetExceeded("IFD count"));
        }
    }
    let ifd = match options.primary_ifd {
        Some(n) if primary.contains(&n) => n,
        Some(_) => return Err(source("selected IFD is not a primary image")),
        None if primary.len() == 1 => primary[0],
        _ => {
            return Err(source(
                "multiple/missing primary images require explicit selection",
            ));
        }
    };
    d.seek_to_image(ifd).map_err(source)?;
    let white_is_zero = d
        .ifd_pointer()
        .is_some_and(|p| raw_view.white_ifds.contains(&p.0));
    let (width, height) = d.dimensions().map_err(source)?;
    if d.find_tag_unsigned::<u16>(Tag::Orientation)
        .map_err(source)?
        .unwrap_or(1)
        != 1
    {
        return Err(GeelloError::Unsupported(
            "TIFF orientation other than top-left".into(),
        ));
    }
    let color = d.colortype().map_err(source)?;
    let samples = d
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .map_err(source)?
        .unwrap_or(1) as usize;
    let photometry = d
        .get_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
        .map_err(source)?;
    if !matches!(photometry, 0..=2) {
        return Err(GeelloError::Unsupported(
            "TIFF palette/complex photometry".into(),
        ));
    }
    let bits = d
        .get_tag(Tag::BitsPerSample)
        .map_err(source)?
        .into_u16_vec()
        .map_err(source)?;
    if bits.is_empty() || bits.iter().any(|b| *b != bits[0]) || !matches!(bits[0], 8 | 16 | 32 | 64)
    {
        return Err(GeelloError::Unsupported(
            "packed/mixed TIFF bit depth".into(),
        ));
    }
    let formats = d
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .map_err(source)?
        .unwrap_or(vec![1]);
    if formats.is_empty()
        || formats.iter().any(|f| *f != formats[0])
        || !matches!(formats[0], 1..=3)
    {
        return Err(GeelloError::Unsupported(
            "complex/mixed sample format".into(),
        ));
    }
    let extras = d
        .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
        .map_err(source)?
        .unwrap_or_default();
    if extras.len() > 1 || extras.iter().any(|v| !matches!(v, 1 | 2)) {
        return Err(GeelloError::Unsupported(
            "unknown TIFF extra sample semantics".into(),
        ));
    }
    let alpha = extras.first().copied();
    let bands = samples
        .checked_sub(alpha.is_some() as usize)
        .ok_or_else(|| source("invalid TIFF sample count"))?;
    if (photometry == 2 && bands != 3) || (photometry != 2 && bands != 1) {
        return Err(source("photometry/sample count mismatch"));
    }
    if alpha == Some(1) && !matches!((formats[0], bits[0]), (1, 8 | 16)) {
        return Err(GeelloError::Unsupported(
            "associated alpha requires unsigned 8/16-bit validation".into(),
        ));
    }
    if alpha.is_some() && formats[0] == 2 {
        return Err(GeelloError::Unsupported("signed TIFF alpha".into()));
    }
    let grid = read_grid(&mut d, options)?;
    let no_data = d
        .find_tag(Tag::GdalNodata)
        .map_err(source)?
        .map(|v| v.into_string().map_err(source))
        .transpose()?;
    let metadata = RasterMetadata {
        width,
        height,
        grid,
        bands,
        bits_per_sample: bits[0],
        has_subifd,
        white_is_zero,
        diagnostics: if has_subifd {
            vec!["Source overviews bypassed; reading primary base chunks".into()]
        } else {
            vec![]
        },
    };
    let _ = color;
    Ok(RasterSnapshot {
        metadata,
        source: Arc::new(RasterSource {
            bytes,
            raw_view,
            _reservations: reservations,
        }),
        options: options.clone(),
        ifd,
        alpha,
        bits: bits[0],
        sample_format: formats[0],
        no_data,
    })
}
fn read_grid(d: &mut TiffDecoder, o: &RasterReadOptions) -> Result<RasterGrid, GeelloError> {
    let keys = d
        .find_tag_unsigned_vec::<u16>(Tag::GeoKeyDirectoryTag)
        .map_err(source)?
        .unwrap_or_default();
    let mut geographic_epsg = None;
    let mut projected_epsg = None;
    let mut model_type = None;
    let mut interpretation = PixelInterpretation::Area;
    if !keys.is_empty() {
        if keys.len() < 4 || keys[0] != 1 || keys.len() != 4 + keys[3] as usize * 4 {
            return Err(source("invalid GeoKey directory"));
        }
        let mut seen = std::collections::HashSet::new();
        for key in keys[4..].chunks_exact(4) {
            if !seen.insert(key[0]) {
                return Err(source("duplicate GeoKey"));
            }
            if key[1] == 0 && key[2] == 1 {
                match key[0] {
                    1024 => model_type = Some(key[3]),
                    1025 => {
                        interpretation = match key[3] {
                            1 => PixelInterpretation::Area,
                            2 => PixelInterpretation::Point,
                            _ => return Err(source("invalid raster interpretation")),
                        }
                    }
                    2048 => geographic_epsg = Some(key[3]),
                    3072 => projected_epsg = Some(key[3]),
                    _ => {}
                }
            }
        }
    }
    let crs = match &o.crs_override {
        Some(c) => c.clone(),
        None => {
            // GeographicType describes the base geodetic CRS of a projected
            // model too. A user-defined projection must never fall back to it.
            let (code, geographic) = match model_type {
                Some(1) => (projected_epsg, false),
                Some(2) if projected_epsg.is_none() => (geographic_epsg, true),
                None if projected_epsg.is_some() => (projected_epsg, false),
                None => (geographic_epsg, true),
                _ => {
                    return Err(source(
                        "unsupported/contradictory GeoTIFF model; explicit CRS override required",
                    ));
                }
            };
            let code = code
                .filter(|n| *n != 0 && *n != 32767)
                .ok_or_else(|| source("GeoTIFF CRS metadata or explicit override required"))?;
            Crs::new(
                format!("EPSG:{code}"),
                if geographic {
                    CoordinateSpace::GeographicXy
                } else {
                    CoordinateSpace::Planar
                },
            )?
        }
    };
    let matrix = d
        .find_tag(Tag::ModelTransformationTag)
        .map_err(source)?
        .map(|v| v.into_f64_vec().map_err(source))
        .transpose()?;
    let scale = d
        .find_tag(Tag::ModelPixelScaleTag)
        .map_err(source)?
        .map(|v| v.into_f64_vec().map_err(source))
        .transpose()?;
    let tie = d
        .find_tag(Tag::ModelTiepointTag)
        .map_err(source)?
        .map(|v| v.into_f64_vec().map_err(source))
        .transpose()?;
    let mut affine = if let Some(m) = matrix {
        if m.len() != 16
            || scale.is_some()
            || tie.is_some()
            || m.iter().any(|v| !v.is_finite())
            || m[12..] != [0., 0., 0., 1.]
            || m[2] != 0.
            || m[6] != 0.
        {
            return Err(source("invalid/contradictory GeoTIFF model matrix"));
        }
        [m[3], m[0], m[1], m[7], m[4], m[5]]
    } else {
        let s = scale.ok_or_else(|| source("missing pixel scale/model transform"))?;
        let t = tie.ok_or_else(|| source("missing tiepoints"))?;
        if s.len() != 3
            || s.iter().chain(t.iter()).any(|v| !v.is_finite())
            || s[0] <= 0.
            || s[1] <= 0.
            || t.is_empty()
            || t.len() % 6 != 0
        {
            return Err(source("invalid GeoTIFF scale/tiepoints"));
        }
        let a = [t[3] - t[0] * s[0], s[0], 0., t[4] + t[1] * s[1], 0., -s[1]];
        for p in t.chunks_exact(6) {
            if (a[0] + p[0] * a[1] - p[3]).abs() > 1e-8 || (a[3] + p[1] * a[5] - p[4]).abs() > 1e-8
            {
                return Err(source("contradictory tiepoints"));
            }
        }
        a
    };
    if interpretation == PixelInterpretation::Point {
        affine[0] -= (affine[1] + affine[2]) * 0.5;
        affine[3] -= (affine[4] + affine[5]) * 0.5;
    }
    let grid = RasterGrid {
        crs,
        affine,
        interpretation,
    };
    grid.validate()?;
    if grid.crs.space() == CoordinateSpace::GeographicXy {
        let (width, height) = d.dimensions().map_err(source)?;
        for (x, y) in [
            (0., 0.),
            (width as f64, 0.),
            (0., height as f64),
            (width as f64, height as f64),
        ] {
            let longitude = affine[0] + x * affine[1] + y * affine[2];
            let latitude = affine[3] + x * affine[4] + y * affine[5];
            if longitude.abs() > 180. + 1e-9 || latitude.abs() > 90. + 1e-9 {
                return Err(source(
                    "geographic raster grid outside XY degree domain; verify CRS/georeferencing",
                ));
            }
        }
    }
    Ok(grid)
}
impl RasterSnapshot {
    pub fn metadata(&self) -> &RasterMetadata {
        &self.metadata
    }
    pub fn read_native_window(
        &self,
        w: NativeWindow,
        context: &OperationContext,
    ) -> Result<RasterReadBlock, GeelloError> {
        context.check()?;
        if w.width == 0
            || w.height == 0
            || w.column
                .checked_add(w.width)
                .is_none_or(|n| n > self.metadata.width)
            || w.row
                .checked_add(w.height)
                .is_none_or(|n| n > self.metadata.height)
        {
            return Err(source("native window outside image"));
        }
        let _directory_scratch = context.reserve(
            self.options
                .max_tag_bytes
                .checked_mul(32)
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let mut d = decoder(
            self.source.bytes.clone(),
            self.source.raw_view.clone(),
            &self.options,
        )?;
        d.seek_to_image(self.ifd).map_err(source)?;
        let channels = self.metadata.bands + self.alpha.is_some() as usize;
        let n = (w.width as usize)
            .checked_mul(w.height as usize)
            .ok_or(GeelloError::SizeOverflow)?;
        let reservation = context.reserve(
            n.checked_mul(channels * 16 + 8)
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let mut bands: Vec<RasterSamples> = (0..channels)
            .map(|_| empty_samples(self.sample_format, self.bits, n))
            .collect::<Result<_, _>>()?;
        let planar = d
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
            .map_err(source)?
            .unwrap_or(1)
            == 2;
        let (cw, ch) = d.chunk_dimensions();
        let across = self.metadata.width.div_ceil(cw);
        let down = self.metadata.height.div_ceil(ch);
        let per_plane = across.checked_mul(down).ok_or(GeelloError::SizeOverflow)?;
        for cy in w.row / ch..=(w.row + w.height - 1) / ch {
            for cx in w.column / cw..=(w.column + w.width - 1) / cw {
                for plane in 0..if planar { channels } else { 1 } {
                    context.checkpoint(1)?;
                    let index = cy * across + cx + plane as u32 * per_plane;
                    let (width, height) = d.chunk_data_dimensions(index);
                    let bytes = (width as usize)
                        .checked_mul(height as usize)
                        .and_then(|n| n.checked_mul(if planar { 1 } else { channels }))
                        .and_then(|n| n.checked_mul(self.bits as usize / 8))
                        .ok_or(GeelloError::SizeOverflow)?;
                    if bytes > self.options.max_decode_bytes {
                        return Err(GeelloError::BudgetExceeded("TIFF chunk bytes"));
                    }
                    let _scratch =
                        context.reserve(bytes.checked_mul(4).ok_or(GeelloError::SizeOverflow)?)?;
                    let raw = d.read_chunk(index).map_err(source)?;
                    for y in (cy * ch).max(w.row)..(cy * ch + height).min(w.row + w.height) {
                        context.checkpoint(1)?;
                        for x in (cx * cw).max(w.column)..(cx * cw + width).min(w.column + w.width)
                        {
                            let src = ((y - cy * ch) * width + x - cx * cw) as usize;
                            let dst = ((y - w.row) * w.width + x - w.column) as usize;
                            for channel in 0..if planar { 1 } else { channels } {
                                copy_sample(
                                    &raw,
                                    src * if planar { 1 } else { channels } + channel,
                                    &mut bands[if planar { plane } else { channel }],
                                    dst,
                                )?;
                            }
                        }
                    }
                }
            }
        }
        let nodata = self
            .no_data
            .as_deref()
            .map(|v| parse_nodata(v, self.sample_format))
            .transpose()?;
        // NoData is defined in the original sample domain, before unassociation.
        let original_validity: Vec<Option<Arc<[bool]>>> = bands[..self.metadata.bands]
            .iter()
            .map(|samples| {
                nodata.map(|n| {
                    let b = RasterBand {
                        samples: samples.clone(),
                        validity: None,
                        no_data: Some(n),
                        scale: 1.,
                        offset: 0.,
                    };
                    Arc::from(
                        (0..b.samples.len())
                            .map(|i| crate::raster_processing::valid_sample(&b, i))
                            .collect::<Vec<_>>(),
                    )
                })
            })
            .collect();
        let alpha = if self.alpha.is_some() {
            let raw = bands.pop().unwrap();
            let max = if self.sample_format == 3 {
                1.
            } else {
                (2f64).powi(self.bits as i32) - 1.
            };
            let values: Vec<f32> = (0..n)
                .map(|i| crate::raster_processing::sample(&raw, i) / max)
                .map(|a| {
                    if a.is_finite() && (0. ..=1.).contains(&a) {
                        Ok(a as f32)
                    } else {
                        Err(source("invalid TIFF alpha sample"))
                    }
                })
                .collect::<Result<_, _>>()?;
            Some(Arc::from(values))
        } else {
            None
        };
        let bands = bands
            .into_iter()
            .zip(original_validity)
            .map(|(samples, validity)| RasterBand {
                samples,
                validity,
                no_data: nodata,
                scale: 1.,
                offset: 0.,
            })
            .collect();
        let mut grid = self.metadata.grid.clone();
        grid.affine[0] += grid.affine[1] * w.column as f64 + grid.affine[2] * w.row as f64;
        grid.affine[3] += grid.affine[4] * w.column as f64 + grid.affine[5] * w.row as f64;
        let block = RasterBlock {
            width: w.width,
            height: w.height,
            grid,
            bands,
            alpha,
            alpha_association: if self.alpha == Some(1) {
                crate::AlphaAssociation::Premultiplied
            } else {
                crate::AlphaAssociation::Straight
            },
        };
        block.validate()?;
        Ok(RasterReadBlock {
            block,
            _reservation: reservation,
        })
    }

    /// One-shot primary raster to the exact view grid. Reading remains native;
    /// inverse projection, resampling, colorization and scene freezing are
    /// separate calls. Automatic ranges must be prepared for the DataVersion.
    #[cfg(feature = "rust-proj")]
    pub fn prepare_scene(
        &self,
        view: &crate::View,
        colors: &crate::raster_processing::RasterColorOptions,
        kind: crate::raster_processing::RasterKind,
        method: crate::raster_processing::Resampling,
        context: &OperationContext,
    ) -> Result<crate::PreparedScene, GeelloError> {
        view.validate()?;
        let block = self.read_native_window(
            NativeWindow {
                column: 0,
                row: 0,
                width: self.metadata.width,
                height: self.metadata.height,
            },
            context,
        )?;
        let inverse = if view.crs == block.grid.crs {
            None
        } else {
            Some(crate::projection::RustProjection::new(
                view.crs.clone(),
                block.grid.crs.clone(),
            )?)
        };
        let grid = RasterGrid {
            crs: view.crs.clone(),
            interpretation: PixelInterpretation::Area,
            affine: [
                view.bounds.min_x,
                (view.bounds.max_x - view.bounds.min_x) / view.width as f64,
                0.,
                view.bounds.max_y,
                0.,
                -(view.bounds.max_y - view.bounds.min_y) / view.height as f64,
            ],
        };
        let sampled = crate::raster_processing::resample(
            &block,
            view.width,
            view.height,
            grid,
            kind,
            method,
            |p| {
                inverse
                    .as_ref()
                    .map_or(Ok(p), |operation| operation.transform_xy(p))
            },
            context,
        )?;
        let image = crate::raster_processing::colorize(&sampled, colors, context)?;
        let _copy = context.reserve(image.image().pixels().len())?;
        crate::prepare_scene(
            &crate::MapComposition {
                background: None,
                items: vec![crate::DrawItem::Image(crate::ImagePlacement {
                    image: Arc::new(image.image().clone()),
                    crs: view.crs.clone(),
                    bounds: view.bounds,
                    opacity: 1.,
                })],
            },
            view,
            context,
        )
    }
}
fn empty_samples(format: u16, bits: u16, n: usize) -> Result<RasterSamples, GeelloError> {
    macro_rules! make {
        ($v:ident,$zero:expr) => {
            Ok(RasterSamples::$v(Arc::from(vec![$zero; n])))
        };
    }
    match (format, bits) {
        (1, 8) => make!(U8, 0u8),
        (1, 16) => make!(U16, 0u16),
        (1, 32) => make!(U32, 0u32),
        (1, 64) => make!(U64, 0u64),
        (2, 8) => make!(I8, 0i8),
        (2, 16) => make!(I16, 0i16),
        (2, 32) => make!(I32, 0i32),
        (2, 64) => make!(I64, 0i64),
        // The decoder exposes IEEE half-floats as F16; output promotes them to
        // f32 while the snapshot retains the source bit-depth metadata.
        (3, 16) | (3, 32) => make!(F32, 0f32),
        (3, 64) => make!(F64, 0f64),
        _ => Err(GeelloError::Unsupported("TIFF numeric type".into())),
    }
}
fn copy_sample(
    raw: &DecodingResult,
    src: usize,
    band: &mut RasterSamples,
    dst: usize,
) -> Result<(), GeelloError> {
    macro_rules! cp {
        ($v:ident) => {
            if let (DecodingResult::$v(a), RasterSamples::$v(b)) = (raw, &mut *band) {
                Arc::get_mut(b).ok_or_else(|| source("shared destination band"))?[dst] = *a
                    .get(src)
                    .ok_or_else(|| source("short decoded TIFF chunk"))?;
                return Ok(());
            }
        };
    }
    cp!(U8);
    cp!(U16);
    cp!(U32);
    cp!(U64);
    cp!(I8);
    cp!(I16);
    cp!(I32);
    cp!(I64);
    cp!(F32);
    cp!(F64);
    if let (DecodingResult::F16(a), RasterSamples::F32(b)) = (raw, band) {
        Arc::get_mut(b).unwrap()[dst] = a[src].to_f32();
        return Ok(());
    }
    Err(source("TIFF decoded sample type mismatch"))
}
fn parse_nodata(v: &str, format: u16) -> Result<PropertySample, GeelloError> {
    let v = v.trim_matches(['\0', ' ']);
    Ok(match format {
        1 => PropertySample::U64(v.parse().map_err(source)?),
        2 => PropertySample::I64(v.parse().map_err(source)?),
        _ => PropertySample::F64(v.parse().map_err(source)?),
    })
}
