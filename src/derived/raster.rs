//! Lossless little-endian native blocks and separately published overview sets.
//! Base source bytes are never rewritten. Numeric masks and alpha stay separate.
use super::*;
use crate::*;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BandHeader {
    kind: String,
    no_data: Option<(String, u64)>,
    scale: u64,
    offset: u64,
    validity: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockHeader {
    width: u32,
    height: u32,
    crs: Crs,
    affine: [u64; 6],
    point: bool,
    bands: Vec<BandHeader>,
    alpha: bool,
    premultiplied: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tile {
    #[serde(skip)]
    _metadata: Option<Arc<crate::operation::Reservation>>,
    column: u32,
    row: u32,
    block: StoredFile,
    weights: Option<StoredFile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Level {
    width: u32,
    height: u32,
    factor: u32,
    tiles: Vec<Tile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    data_version: String,
    block_size: u32,
    crs: Crs,
    affine: [u64; 6],
    point: bool,
    method: String,
    levels: Vec<Level>,
    ranges: Vec<Option<(u64, u64)>>,
}
#[derive(Debug, Clone)]
pub struct DerivedRaster {
    root: PathBuf,
    manifest: Manifest,
    _metadata: Option<Arc<crate::operation::Reservation>>,
    limits: StoreLimits,
    version: String,
}
/// Half-open native pixel window, with no implicit resampling.
#[derive(Debug, Clone, Copy)]
pub struct BlockWindow {
    pub column: u32,
    pub row: u32,
    pub width: u32,
    pub height: u32,
}
/// Optional caller-owned decoded-block cache. Keys identify immutable encoded
/// bytes and their derived version; implementations retain admission with values.
/// Library code has no dependency on a server, registry or cache implementation.
pub trait RasterBlockCache {
    fn get_or_load(
        &self,
        key: String,
        load: &mut dyn FnMut() -> Result<Accounted<RasterBlock>, GeelloError>,
    ) -> Result<Arc<Accounted<RasterBlock>>, GeelloError>;
}
impl DerivedRaster {
    /// Plan against the complete native grid, load only the required window,
    /// then execute the shared layer-one resampler. Projection failures and
    /// excessive footprints fail explicitly; source coverage is never guessed.
    #[allow(clippy::too_many_arguments)]
    pub fn resample_view(
        &self,
        level: usize,
        width: u32,
        height: u32,
        target: RasterGrid,
        kind: raster_processing::RasterKind,
        method: raster_processing::Resampling,
        target_to_source: impl Fn([f64; 2]) -> Result<[f64; 2], GeelloError>,
        ctx: &OperationContext,
    ) -> Result<Option<raster_processing::ProcessedBlock>, GeelloError> {
        self.resample_view_cached(
            level,
            width,
            height,
            target,
            kind,
            method,
            target_to_source,
            ctx,
            None,
        )
    }
    /// As resample_view, with optional immutable decoded-block reuse.
    #[allow(clippy::too_many_arguments)]
    pub fn resample_view_cached(
        &self,
        level: usize,
        width: u32,
        height: u32,
        target: RasterGrid,
        kind: raster_processing::RasterKind,
        method: raster_processing::Resampling,
        target_to_source: impl Fn([f64; 2]) -> Result<[f64; 2], GeelloError>,
        ctx: &OperationContext,
        cache: Option<&dyn RasterBlockCache>,
    ) -> Result<Option<raster_processing::ProcessedBlock>, GeelloError> {
        let (sw, sh, factor) = self
            .level_dimensions(level)
            .ok_or_else(|| error("missing raster level"))?;
        let source = window_grid(&self.grid(), 0, 0, factor);
        let Some(w) = raster_processing::sampling_window(
            &source,
            sw,
            sh,
            &target,
            width,
            height,
            method,
            &target_to_source,
            ctx,
        )?
        else {
            return Ok(None);
        };
        let block = self.read_window_cached(
            level,
            BlockWindow {
                column: w.column,
                row: w.row,
                width: w.width,
                height: w.height,
            },
            ctx,
            cache,
        )?;
        raster_processing::resample(
            &block,
            width,
            height,
            target,
            kind,
            method,
            target_to_source,
            ctx,
        )
        .map(Some)
    }
    /// Prepare base access first. Each loader result may be released after one
    /// block; callers can provide a TIFF window reader or another bounded source.
    #[allow(clippy::too_many_arguments)]
    pub fn build_base(
        parent: &Path,
        data_version: &str,
        width: u32,
        height: u32,
        grid: &RasterGrid,
        block_size: u32,
        limits: StoreLimits,
        ctx: &OperationContext,
        mut load: impl FnMut(BlockWindow, &OperationContext) -> Result<RasterBlock, GeelloError>,
    ) -> Result<Self, GeelloError> {
        limits.validate()?;
        grid.validate()?;
        if width == 0 || height == 0 || block_size == 0 || data_version.is_empty() {
            return Err(error("invalid raster dimensions/version"));
        }
        let stage = stage(parent)?;
        let mut used = 0;
        let mut tiles = vec![];
        let mut ranges: Vec<Option<(f64, f64)>> = vec![];
        for row in (0..height).step_by(block_size as usize) {
            for column in (0..width).step_by(block_size as usize) {
                ctx.checkpoint(1)?;
                let w = BlockWindow {
                    column,
                    row,
                    width: block_size.min(width - column),
                    height: block_size.min(height - row),
                };
                let block = load(w, ctx)?;
                block.validate()?;
                if ranges.is_empty() {
                    ranges.resize(block.bands.len(), None);
                }
                if ranges.len() != block.bands.len() {
                    return Err(error("source band count changed"));
                }
                for (i, r) in ranges.iter_mut().enumerate() {
                    if let Some(v) = crate::raster_processing::block_band_range(&block, i, ctx)? {
                        *r = Some(match *r {
                            Some((min, max)) => (min.min(v.min), max.max(v.max)),
                            None => (v.min, v.max),
                        });
                    }
                }
                if block.width != w.width
                    || block.height != w.height
                    || block.grid != window_grid(grid, column, row, 1)
                {
                    return Err(error("source block grid mismatch"));
                }
                let bytes = encode(&block, limits.max_block_bytes, ctx)?;
                let file = write_file(
                    stage.path(),
                    &format!("base-{column}-{row}.block"),
                    &bytes,
                    &mut used,
                    limits.max_disk_bytes,
                )?;
                tiles.push(Tile {
                    _metadata: Some(Arc::new(ctx.reserve(2048)?)),
                    column,
                    row,
                    block: file,
                    weights: None,
                });
            }
        }
        let value = Manifest {
            schema: 2,
            data_version: data_version.into(),
            block_size,
            crs: grid.crs.clone(),
            affine: grid.affine.map(f64::to_bits),
            point: grid.interpretation == PixelInterpretation::Point,
            method: "base".into(),
            levels: vec![Level {
                width,
                height,
                factor: 1,
                tiles,
            }],
            ranges: ranges
                .into_iter()
                .map(|r| r.map(|(a, b)| (a.to_bits(), b.to_bits())))
                .collect(),
        };
        publish(parent, stage, value, used, limits, ctx)
    }
    /// Admit metadata before deserialization and retain it with this store.
    pub fn open_with_context(
        root: &Path,
        limits: StoreLimits,
        ctx: &OperationContext,
    ) -> Result<Self, GeelloError> {
        let hold = metadata_memory(root, limits, ctx)?;
        let mut store = Self::open(root, limits)?;
        store._metadata = Some(Arc::new(hold));
        Ok(store)
    }
    /// Caller-owned metadata; use open_with_context inside budgeted operations.
    pub fn open(root: &Path, limits: StoreLimits) -> Result<Self, GeelloError> {
        limits.validate()?;
        let manifest: Manifest = load_manifest(root, limits.max_manifest_bytes)?;
        if manifest.schema != 2 || manifest.block_size == 0 || manifest.levels.is_empty() {
            return Err(error("invalid raster manifest"));
        }
        let version = blake3::hash(&serde_json::to_vec(&manifest).map_err(error)?)
            .to_hex()
            .to_string();
        if root.file_name().and_then(|n| n.to_str()) != Some(version.as_str()) {
            return Err(error("raster manifest identity mismatch"));
        }
        let mut expected_factor = 1u32;
        let mut dimensions = (manifest.levels[0].width, manifest.levels[0].height);
        for level in &manifest.levels {
            if dimensions.0 == 0
                || dimensions.1 == 0
                || (level.width, level.height) != dimensions
                || level.factor != expected_factor
            {
                return Err(error("invalid raster level dimensions"));
            }
            let columns = level.width.div_ceil(manifest.block_size);
            let rows = level.height.div_ceil(manifest.block_size);
            if (columns as u64) * (rows as u64) != level.tiles.len() as u64 {
                return Err(error("incomplete raster coverage"));
            }
            for (index, tile) in level.tiles.iter().enumerate() {
                if tile.column != (index as u32 % columns) * manifest.block_size
                    || tile.row != (index as u32 / columns) * manifest.block_size
                {
                    return Err(error("invalid raster tile order or coverage"));
                }
            }
            dimensions = (dimensions.0.div_ceil(2), dimensions.1.div_ceil(2));
            expected_factor = expected_factor
                .checked_mul(2)
                .ok_or(GeelloError::SizeOverflow)?;
        }
        Ok(Self {
            root: root.into(),
            manifest,
            _metadata: None,
            limits,
            version,
        })
    }
    pub fn data_version(&self) -> &str {
        &self.manifest.data_version
    }
    pub fn derived_version(&self) -> &str {
        &self.version
    }
    pub fn dimensions(&self) -> (u32, u32) {
        let l = &self.manifest.levels[0];
        (l.width, l.height)
    }
    pub fn level_dimensions(&self, level: usize) -> Option<(u32, u32, u32)> {
        self.manifest
            .levels
            .get(level)
            .map(|l| (l.width, l.height, l.factor))
    }
    pub fn grid(&self) -> RasterGrid {
        RasterGrid {
            crs: self.manifest.crs.clone(),
            affine: self.manifest.affine.map(f64::from_bits),
            interpretation: if self.manifest.point {
                PixelInterpretation::Point
            } else {
                PixelInterpretation::Area
            },
        }
    }
    pub fn level_count(&self) -> usize {
        self.manifest.levels.len()
    }
    pub fn ranges(&self) -> Vec<Option<crate::raster_processing::SampleRange>> {
        self.manifest
            .ranges
            .iter()
            .map(|v| {
                v.map(|(min, max)| crate::raster_processing::SampleRange {
                    min: f64::from_bits(min),
                    max: f64::from_bits(max),
                })
            })
            .collect()
    }
    /// A conservative level never exceeds either requested axis sampling factor.
    pub fn select_level(&self, max_factor: f64) -> usize {
        self.manifest
            .levels
            .iter()
            .rposition(|l| (l.factor as f64) <= max_factor)
            .unwrap_or(0)
    }
    pub fn read_window(
        &self,
        level: usize,
        w: BlockWindow,
        ctx: &OperationContext,
    ) -> Result<Accounted<RasterBlock>, GeelloError> {
        self.read_window_cached(level, w, ctx, None)
    }
    /// Read a native window using a caller-supplied decoded-block cache.
    pub fn read_window_cached(
        &self,
        level: usize,
        w: BlockWindow,
        ctx: &OperationContext,
        cache: Option<&dyn RasterBlockCache>,
    ) -> Result<Accounted<RasterBlock>, GeelloError> {
        let l = self
            .manifest
            .levels
            .get(level)
            .ok_or_else(|| error("missing raster level"))?;
        if w.width == 0
            || w.height == 0
            || w.column.checked_add(w.width).is_none_or(|x| x > l.width)
            || w.row.checked_add(w.height).is_none_or(|y| y > l.height)
        {
            return Err(error("window outside derived raster"));
        }
        let pixels = (w.width as usize)
            .checked_mul(w.height as usize)
            .ok_or(GeelloError::SizeOverflow)?;
        let mut output: Option<RasterBlock> = None;
        let mut hold = None;
        for tile in &l.tiles {
            let size = self.manifest.block_size;
            if tile.column >= w.column + w.width
                || tile.row >= w.row + w.height
                || tile.column + size <= w.column
                || tile.row + size <= w.row
            {
                continue;
            }
            ctx.check()?;
            let mut load = || {
                let bytes = read_file(&self.root, &tile.block, self.limits.max_block_bytes, ctx)?;
                decode(&bytes, self.limits.max_block_bytes, ctx)
            };
            let block = if let Some(cache) = cache {
                cache.get_or_load(
                    format!(
                        "native:{}:{}:{}",
                        self.version, tile.block.bytes, tile.block.hash
                    ),
                    &mut load,
                )?
            } else {
                Arc::new(load()?)
            };
            if block.width != self.manifest.block_size.min(l.width - tile.column)
                || block.height != self.manifest.block_size.min(l.height - tile.row)
                || block.grid != window_grid(&self.grid(), tile.column, tile.row, l.factor)
            {
                return Err(error("derived block dimensions or grid mismatch"));
            }
            if let Some(out) = &output
                && (out.bands.len() != block.bands.len()
                    || out.alpha.is_some() != block.alpha.is_some()
                    || out.alpha_association != block.alpha_association)
            {
                return Err(error("derived block band layout mismatch"));
            }
            if output.is_none() {
                hold = Some(
                    ctx.reserve(
                        pixels
                            .checked_mul(block.bands.len() * 16 + 8)
                            .ok_or(GeelloError::SizeOverflow)?,
                    )?,
                );
                output = Some(empty_like(
                    &block,
                    w.width,
                    w.height,
                    window_grid(&self.grid(), w.column, w.row, l.factor),
                ));
            }
            let out = output.as_mut().unwrap();
            for y in w.row.max(tile.row)..(w.row + w.height).min(tile.row + block.height) {
                for x in
                    w.column.max(tile.column)..(w.column + w.width).min(tile.column + block.width)
                {
                    ctx.checkpoint(1)?;
                    let src = ((y - tile.row) * block.width + x - tile.column) as usize;
                    let dst = ((y - w.row) * w.width + x - w.column) as usize;
                    for (a, b) in out.bands.iter_mut().zip(&block.bands) {
                        copy_sample(&mut a.samples, dst, &b.samples, src)?;
                        Arc::make_mut(a.validity.as_mut().unwrap())[dst] =
                            b.validity.as_ref().is_none_or(|v| v[src]);
                    }
                    if let (Some(a), Some(b)) = (&mut out.alpha, &block.alpha) {
                        Arc::make_mut(a)[dst] = b[src];
                    }
                }
            }
        }
        let out = output.ok_or_else(|| error("missing raster coverage"))?;
        out.validate()?;
        Ok(Accounted::new(out, hold.into_iter().collect()))
    }
    /// Nearest and weighted mean versions are distinct. Build the complete set
    /// privately, then publish one new DerivedVersion. The base remains usable.
    pub fn build_overviews(
        &self,
        parent: &Path,
        nearest: bool,
        ctx: &OperationContext,
    ) -> Result<Self, GeelloError> {
        let stage = stage(parent)?;
        let mut used = 0;
        let mut manifest = self.manifest.clone();
        manifest.levels.truncate(1);
        manifest.method = if nearest { "nearest" } else { "weighted-mean" }.into();
        for tile in &manifest.levels[0].tiles {
            let bytes = read_file(&self.root, &tile.block, self.limits.max_block_bytes, ctx)?;
            write_file(
                stage.path(),
                &tile.block.name,
                &bytes,
                &mut used,
                self.limits.max_disk_bytes,
            )?;
        }
        while manifest.levels.last().unwrap().width > 1
            || manifest.levels.last().unwrap().height > 1
        {
            let prev = manifest.levels.last().unwrap().clone();
            let factor = prev
                .factor
                .checked_mul(2)
                .ok_or(GeelloError::SizeOverflow)?;
            let mut level = Level {
                width: prev.width.div_ceil(2),
                height: prev.height.div_ceil(2),
                factor,
                tiles: vec![],
            };
            // The staged previous levels are immutable and accessible only here.
            let view = Self {
                root: stage.path().into(),
                manifest: manifest.clone(),
                _metadata: None,
                limits: self.limits,
                version: String::new(),
            };
            let grid = self.grid();
            for row in (0..level.height).step_by(manifest.block_size as usize) {
                for col in (0..level.width).step_by(manifest.block_size as usize) {
                    let width = manifest.block_size.min(level.width - col);
                    let height = manifest.block_size.min(level.height - row);
                    // Retain admission for reduction output and effective
                    // coverage weights until both files have been synced.
                    let _reduction_memory = ctx.reserve(
                        (width as usize)
                            .checked_mul(height as usize)
                            .and_then(|n| {
                                n.checked_mul(
                                    manifest.ranges.len().saturating_mul(40).saturating_add(32),
                                )
                            })
                            .ok_or(GeelloError::SizeOverflow)?,
                    )?;
                    if nearest {
                        let (block, weights) =
                            self.nearest_block(col, row, width, height, factor, ctx)?;
                        let bytes = encode(&block, self.limits.max_block_bytes, ctx)?;
                        let file = write_file(
                            stage.path(),
                            &format!("l{factor}-{col}-{row}.block"),
                            &bytes,
                            &mut used,
                            self.limits.max_disk_bytes,
                        )?;
                        let weight_bytes: Vec<u8> =
                            weights.iter().flat_map(|v| v.to_le_bytes()).collect();
                        if weight_bytes.len() > self.limits.max_block_bytes {
                            return Err(GeelloError::BudgetExceeded("overview weights"));
                        }
                        let weights = write_file(
                            stage.path(),
                            &format!("l{factor}-{col}-{row}.weights"),
                            &weight_bytes,
                            &mut used,
                            self.limits.max_disk_bytes,
                        )?;
                        level.tiles.push(Tile {
                            _metadata: Some(Arc::new(ctx.reserve(2048)?)),
                            column: col,
                            row,
                            block: file,
                            weights: Some(weights),
                        });
                        continue;
                    }
                    let src = view.read_window(
                        manifest.levels.len() - 1,
                        BlockWindow {
                            column: col * 2,
                            row: row * 2,
                            width: (width * 2).min(prev.width - col * 2),
                            height: (height * 2).min(prev.height - row * 2),
                        },
                        ctx,
                    )?;
                    let weights = view.read_weights(
                        manifest.levels.len() - 1,
                        BlockWindow {
                            column: col * 2,
                            row: row * 2,
                            width: src.width,
                            height: src.height,
                        },
                        &src,
                        ctx,
                    )?;
                    let (block, weights) = reduce(
                        &src,
                        &weights,
                        nearest,
                        window_grid(&grid, col, row, factor),
                        ctx,
                    )?;
                    let bytes = encode(&block, self.limits.max_block_bytes, ctx)?;
                    let file = write_file(
                        stage.path(),
                        &format!("l{factor}-{col}-{row}.block"),
                        &bytes,
                        &mut used,
                        self.limits.max_disk_bytes,
                    )?;
                    let weight_bytes: Vec<u8> =
                        weights.iter().flat_map(|v| v.to_le_bytes()).collect();
                    if weight_bytes.len() > self.limits.max_block_bytes {
                        return Err(GeelloError::BudgetExceeded("overview weights"));
                    }
                    let weight_file = write_file(
                        stage.path(),
                        &format!("l{factor}-{col}-{row}.weights"),
                        &weight_bytes,
                        &mut used,
                        self.limits.max_disk_bytes,
                    )?;
                    level.tiles.push(Tile {
                        _metadata: Some(Arc::new(ctx.reserve(2048)?)),
                        column: col,
                        row,
                        block: file,
                        weights: Some(weight_file),
                    });
                }
            }
            manifest.levels.push(level);
        }
        publish(parent, stage, manifest, used, self.limits, ctx)
    }
    /// Categorical levels sample the base directly. Repeatedly selecting a
    /// child overview's center would drift toward its bottom-right descendant
    /// and disagree with nearest sampling at the final level's actual center.
    fn nearest_block(
        &self,
        col: u32,
        row: u32,
        width: u32,
        height: u32,
        factor: u32,
        ctx: &OperationContext,
    ) -> Result<(RasterBlock, Vec<f64>), GeelloError> {
        let (base_width, base_height) = self.dimensions();
        let mut output = None;
        let mut weights = vec![];
        let n = width as usize * height as usize;
        for y in 0..height {
            for x in 0..width {
                let column = (((col + x) as u64 * factor as u64) + factor as u64 / 2)
                    .min(base_width as u64 - 1) as u32;
                let row_source = (((row + y) as u64 * factor as u64) + factor as u64 / 2)
                    .min(base_height as u64 - 1) as u32;
                let pixel = self.read_window(
                    0,
                    BlockWindow {
                        column,
                        row: row_source,
                        width: 1,
                        height: 1,
                    },
                    ctx,
                )?;
                if output.is_none() {
                    output = Some(empty_like(
                        &pixel,
                        width,
                        height,
                        window_grid(&self.grid(), col, row, factor),
                    ));
                    weights.resize(n * (pixel.bands.len() + 1), 0.);
                }
                let out = output.as_mut().unwrap();
                let index = (y * width + x) as usize;
                for (band, source) in pixel.bands.iter().enumerate() {
                    copy_sample(&mut out.bands[band].samples, index, &source.samples, 0)?;
                    let valid = crate::raster_processing::valid_sample(source, 0);
                    Arc::make_mut(out.bands[band].validity.as_mut().unwrap())[index] = valid;
                    weights[band * n + index] = if valid {
                        pixel.alpha.as_ref().map_or(1., |a| a[0] as f64)
                    } else {
                        0.
                    };
                }
                weights[pixel.bands.len() * n + index] = 1.;
                if let (Some(out), Some(alpha)) = (&mut out.alpha, &pixel.alpha) {
                    Arc::make_mut(out)[index] = alpha[0];
                }
            }
        }
        Ok((output.ok_or_else(|| error("empty nearest block"))?, weights))
    }
    fn read_weights(
        &self,
        level: usize,
        w: BlockWindow,
        block: &RasterBlock,
        ctx: &OperationContext,
    ) -> Result<Vec<f64>, GeelloError> {
        let n = (w.width * w.height) as usize;
        let mut weights = vec![0.; n * (block.bands.len() + 1)];
        if level == 0 {
            for (b, band) in block.bands.iter().enumerate() {
                for i in 0..n {
                    weights[b * n + i] = if crate::raster_processing::valid_sample(band, i) {
                        block.alpha.as_ref().map_or(1., |a| a[i] as f64)
                    } else {
                        0.
                    };
                }
            }
            for i in 0..n {
                weights[block.bands.len() * n + i] = 1.;
            }
            return Ok(weights);
        }
        for tile in &self.manifest.levels[level].tiles {
            if tile.column >= w.column + w.width
                || tile.row >= w.row + w.height
                || tile.column + self.manifest.block_size <= w.column
                || tile.row + self.manifest.block_size <= w.row
            {
                continue;
            }
            let bytes = read_file(
                &self.root,
                tile.weights
                    .as_ref()
                    .ok_or_else(|| error("missing weights"))?,
                self.limits.max_block_bytes,
                ctx,
            )?;
            let tw = self
                .manifest
                .block_size
                .min(self.manifest.levels[level].width - tile.column);
            let th = self
                .manifest
                .block_size
                .min(self.manifest.levels[level].height - tile.row);
            if bytes.len() != tw as usize * th as usize * (block.bands.len() + 1) * 8 {
                return Err(error("invalid weights"));
            }
            for y in w.row.max(tile.row)..(w.row + w.height).min(tile.row + th) {
                for x in w.column.max(tile.column)..(w.column + w.width).min(tile.column + tw) {
                    for b in 0..=block.bands.len() {
                        let src = ((b * th as usize + (y - tile.row) as usize) * tw as usize
                            + (x - tile.column) as usize)
                            * 8;
                        weights[b * n + ((y - w.row) * w.width + x - w.column) as usize] =
                            f64::from_le_bytes(bytes[src..src + 8].try_into().unwrap());
                    }
                }
            }
        }
        Ok(weights)
    }
}
fn publish(
    parent: &Path,
    stage: tempfile::TempDir,
    value: Manifest,
    mut used: u64,
    limits: StoreLimits,
    ctx: &OperationContext,
) -> Result<DerivedRaster, GeelloError> {
    let version = manifest(stage.path(), &value, &mut used, limits, ctx)?;
    let destination = parent.join(version);
    verify_reused_directory(stage.path(), &destination, ctx)?;
    if !destination.exists() {
        std::fs::rename(stage.path(), &destination).map_err(error)?;
    }
    DerivedRaster::open_with_context(&destination, limits, ctx)
}
fn window_grid(g: &RasterGrid, col: u32, row: u32, factor: u32) -> RasterGrid {
    let [a, b, c, d, e, f] = g.affine;
    RasterGrid {
        crs: g.crs.clone(),
        interpretation: g.interpretation,
        affine: [
            a + b * (col * factor) as f64 + c * (row * factor) as f64,
            b * factor as f64,
            c * factor as f64,
            d + e * (col * factor) as f64 + f * (row * factor) as f64,
            e * factor as f64,
            f * factor as f64,
        ],
    }
}
fn empty_like(b: &RasterBlock, w: u32, h: u32, grid: RasterGrid) -> RasterBlock {
    let n = (w * h) as usize;
    RasterBlock {
        width: w,
        height: h,
        grid,
        bands: b
            .bands
            .iter()
            .map(|b| RasterBand {
                samples: empty_samples(&b.samples, n),
                validity: Some(vec![false; n].into()),
                no_data: b.no_data,
                scale: b.scale,
                offset: b.offset,
            })
            .collect(),
        alpha: b.alpha.as_ref().map(|_| vec![0.; n].into()),
        alpha_association: b.alpha_association,
    }
}
fn empty_samples(s: &RasterSamples, n: usize) -> RasterSamples {
    macro_rules! kind{($($v:ident),*)=>{match s{$(RasterSamples::$v(_)=>RasterSamples::$v(vec![0 as _;n].into()),)*}}}
    kind!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
}
fn copy_sample(
    a: &mut RasterSamples,
    i: usize,
    b: &RasterSamples,
    j: usize,
) -> Result<(), GeelloError> {
    macro_rules! kind{($($v:ident),*)=>{match(a,b){$((RasterSamples::$v(a),RasterSamples::$v(b))=>{Arc::make_mut(a)[i]=b[j];Ok(())},)*_=>Err(error("inconsistent block sample type"))}}}
    kind!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64)
}
fn reduce(
    src: &RasterBlock,
    weights: &[f64],
    nearest: bool,
    grid: RasterGrid,
    ctx: &OperationContext,
) -> Result<(RasterBlock, Vec<f64>), GeelloError> {
    let w = src.width.div_ceil(2);
    let h = src.height.div_ceil(2);
    let n = (w * h) as usize;
    let sn = (src.width * src.height) as usize;
    let mut out = empty_like(src, w, h, grid);
    let mut ow = vec![0.; n * (src.bands.len() + 1)];
    if !nearest {
        out.alpha_association = AlphaAssociation::Straight;
        for b in &mut out.bands {
            b.samples = RasterSamples::F64(vec![0.; n].into());
            b.no_data = None;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let dst = (y * w + x) as usize;
            // The target center at factor two maps to source pixel (2x+1,
            // 2y+1). Clamp the incomplete right/bottom cell to its last sample.
            let first = ((y * 2 + 1).min(src.height - 1) * src.width
                + (x * 2 + 1).min(src.width - 1)) as usize;
            for (b, band) in src.bands.iter().enumerate() {
                ctx.checkpoint(1)?;
                let mut sum = 0.;
                let mut weight = 0.;
                if nearest {
                    copy_sample(&mut out.bands[b].samples, dst, &band.samples, first)?;
                    weight = weights[b * sn + first];
                } else {
                    for yy in y * 2..(y * 2 + 2).min(src.height) {
                        for xx in x * 2..(x * 2 + 2).min(src.width) {
                            let i = (yy * src.width + xx) as usize;
                            let q = weights[b * sn + i];
                            if q > 0. {
                                let mut value = crate::raster_processing::sample(&band.samples, i);
                                if src.alpha_association == AlphaAssociation::Premultiplied {
                                    let alpha = src.alpha.as_ref().unwrap()[i] as f64;
                                    value = if alpha > 0. { value / alpha } else { 0. };
                                }
                                sum += value * q;
                                weight += q;
                            }
                        }
                    }
                    if let RasterSamples::F64(v) = &mut out.bands[b].samples {
                        Arc::make_mut(v)[dst] = if weight > 0. { sum / weight } else { 0. };
                    }
                }
                ow[b * n + dst] = weight;
                Arc::make_mut(out.bands[b].validity.as_mut().unwrap())[dst] = if nearest {
                    crate::raster_processing::valid_sample(band, first)
                } else {
                    weight > 0.
                };
            }
            let mut coverage = 0.;
            let mut alpha_sum = 0.;
            for yy in y * 2..(y * 2 + 2).min(src.height) {
                for xx in x * 2..(x * 2 + 2).min(src.width) {
                    let i = (yy * src.width + xx) as usize;
                    let q = weights[src.bands.len() * sn + i];
                    coverage += q;
                    alpha_sum += src.alpha.as_ref().map_or(1., |a| a[i] as f64) * q;
                }
            }
            ow[src.bands.len() * n + dst] = if nearest {
                weights[src.bands.len() * sn + first]
            } else {
                coverage
            };
            if let (Some(a), Some(b)) = (&mut out.alpha, &src.alpha) {
                Arc::make_mut(a)[dst] = if nearest {
                    b[first]
                } else {
                    (alpha_sum / coverage.max(1.)) as f32
                };
            }
        }
    }
    Ok((out, ow))
}
fn encode(
    block: &RasterBlock,
    max: usize,
    ctx: &OperationContext,
) -> Result<Accounted<Vec<u8>>, GeelloError> {
    block.validate()?;
    let estimate = (block.width as usize)
        .checked_mul(block.height as usize)
        .and_then(|v| v.checked_mul(block.bands.len() * 9 + 4))
        .and_then(|v| v.checked_add(65536))
        .ok_or(GeelloError::SizeOverflow)?;
    if estimate > max {
        return Err(GeelloError::BudgetExceeded("raster block"));
    }
    let hold = ctx.reserve(estimate * 2)?;
    let header = BlockHeader {
        width: block.width,
        height: block.height,
        crs: block.grid.crs.clone(),
        affine: block.grid.affine.map(f64::to_bits),
        point: block.grid.interpretation == PixelInterpretation::Point,
        bands: block
            .bands
            .iter()
            .map(|b| BandHeader {
                kind: kind(&b.samples).into(),
                no_data: b.no_data.map(|v| match v {
                    PropertySample::I64(v) => ("i".into(), v as u64),
                    PropertySample::U64(v) => ("u".into(), v),
                    PropertySample::F64(v) => ("f".into(), v.to_bits()),
                }),
                scale: b.scale.to_bits(),
                offset: b.offset.to_bits(),
                validity: b.validity.is_some(),
            })
            .collect(),
        alpha: block.alpha.is_some(),
        premultiplied: block.alpha_association == AlphaAssociation::Premultiplied,
    };
    let h = serde_json::to_vec(&header).map_err(error)?;
    let mut bytes = Vec::with_capacity(estimate);
    bytes.extend_from_slice(b"GLRB0002");
    bytes.extend_from_slice(&(h.len() as u32).to_le_bytes());
    bytes.extend(h);
    for b in &block.bands {
        macro_rules! write{($($v:ident),*)=>{match &b.samples{$(RasterSamples::$v(v)=>for x in v.iter(){bytes.extend_from_slice(&x.to_le_bytes());},)*}}}
        write!(U8, U16, U32, U64, I8, I16, I32, I64, F32, F64);
        if let Some(v) = &b.validity {
            bytes.extend(v.iter().map(|v| u8::from(*v)));
        }
    }
    if let Some(v) = &block.alpha {
        for v in v.iter() {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    if bytes.len() > max {
        return Err(GeelloError::BudgetExceeded("raster block"));
    }
    Ok(Accounted::new(bytes, vec![hold]))
}
fn kind(s: &RasterSamples) -> &'static str {
    match s {
        RasterSamples::U8(_) => "U8",
        RasterSamples::U16(_) => "U16",
        RasterSamples::U32(_) => "U32",
        RasterSamples::U64(_) => "U64",
        RasterSamples::I8(_) => "I8",
        RasterSamples::I16(_) => "I16",
        RasterSamples::I32(_) => "I32",
        RasterSamples::I64(_) => "I64",
        RasterSamples::F32(_) => "F32",
        RasterSamples::F64(_) => "F64",
    }
}
fn decode(
    bytes: &[u8],
    max: usize,
    ctx: &OperationContext,
) -> Result<Accounted<RasterBlock>, GeelloError> {
    if bytes.len() < 12 || &bytes[..8] != b"GLRB0002" || bytes.len() > max {
        return Err(error("invalid block header"));
    }
    let hlen = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut pos = 12usize.checked_add(hlen).ok_or(GeelloError::SizeOverflow)?;
    let h: BlockHeader = serde_json::from_slice(
        bytes
            .get(12..pos)
            .ok_or_else(|| error("truncated header"))?,
    )
    .map_err(error)?;
    let n = (h.width as usize)
        .checked_mul(h.height as usize)
        .ok_or(GeelloError::SizeOverflow)?;
    let hold = ctx.reserve(
        n.checked_mul(h.bands.len() * 16 + 8)
            .ok_or(GeelloError::SizeOverflow)?,
    )?;
    let mut take = |size: usize| -> Result<&[u8], GeelloError> {
        let end = pos.checked_add(size).ok_or(GeelloError::SizeOverflow)?;
        let v = bytes
            .get(pos..end)
            .ok_or_else(|| error("truncated samples"))?;
        pos = end;
        Ok(v)
    };
    let mut bands = vec![];
    for b in h.bands {
        macro_rules! read{($($v:ident:$t:ty),*)=>{match b.kind.as_str(){$(stringify!($v)=>{let data=take(n.checked_mul(size_of::<$t>()).ok_or(GeelloError::SizeOverflow)?)?;RasterSamples::$v(data.chunks_exact(size_of::<$t>()).map(|v|<$t>::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>().into())},)*_=>return Err(error("unknown sample kind"))}}}
        let samples =
            read!(U8:u8,U16:u16,U32:u32,U64:u64,I8:i8,I16:i16,I32:i32,I64:i64,F32:f32,F64:f64);
        let validity = if b.validity {
            Some(take(n)?.iter().map(|v| *v != 0).collect::<Vec<_>>().into())
        } else {
            None
        };
        let no_data = b.no_data.map(|(k, v)| match k.as_str() {
            "i" => PropertySample::I64(v as i64),
            "u" => PropertySample::U64(v),
            _ => PropertySample::F64(f64::from_bits(v)),
        });
        bands.push(RasterBand {
            samples,
            validity,
            no_data,
            scale: f64::from_bits(b.scale),
            offset: f64::from_bits(b.offset),
        });
    }
    let alpha = if h.alpha {
        Some(
            take(n * 4)?
                .chunks_exact(4)
                .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
                .collect::<Vec<_>>()
                .into(),
        )
    } else {
        None
    };
    if pos != bytes.len() {
        return Err(error("trailing block bytes"));
    }
    let block = RasterBlock {
        width: h.width,
        height: h.height,
        grid: RasterGrid {
            crs: h.crs,
            affine: h.affine.map(f64::from_bits),
            interpretation: if h.point {
                PixelInterpretation::Point
            } else {
                PixelInterpretation::Area
            },
        },
        bands,
        alpha,
        alpha_association: if h.premultiplied {
            AlphaAssociation::Premultiplied
        } else {
            AlphaAssociation::Straight
        },
    };
    block.validate()?;
    Ok(Accounted::new(block, vec![hold]))
}
