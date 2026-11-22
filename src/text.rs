//! Explicit memory fonts, Unicode shaping and deterministic pixel-space labels.
//! No constructor in this module enumerates the filesystem or system fonts.
use crate::{scene::PathElement, *};
use cosmic_text::skrifa::{
    FontRef, MetadataProvider,
    outline::{DrawSettings, OutlinePen},
    prelude::*,
};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Wrap, fontdb};
use geo::{Contains, InteriorPoint};
use geo_types::{Geometry, LineString, Point, Polygon};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Placement is evaluated after projection, in the complete map/metatile view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPlacement {
    Point,
    Polygon,
    Line,
    Curved,
}
#[derive(Debug, Clone)]
pub struct LabelItem {
    pub crs: Crs,
    pub geometry: Geometry,
    pub text: String,
    pub style: TextStyle,
    pub placement: LabelPlacement,
    pub priority: f64,
    pub allow_overlap: bool,
    pub wrap_width: Option<f64>,
    pub repeat_distance: f64,
    /// Matching vector draw item and source ordinal exempt only this marker.
    pub marker_source: Option<(usize, u64)>,
}
/// Original cluster ranges are UTF-8 byte ranges within each original line.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphInfo {
    pub font_identity: String,
    pub glyph_id: u16,
    pub cluster: std::ops::Range<usize>,
    pub line: usize,
    pub origin: [f64; 2],
    pub advance: f64,
}
#[derive(Debug)]
pub struct ShapedText {
    pub glyphs: Vec<GlyphInfo>,
    pub width: f64,
    pub height: f64,
    pub missing_glyphs: usize,
    pub(crate) outlines: Vec<Vec<PathElement>>,
    pub(crate) holds: Vec<crate::operation::Reservation>,
}
fn invalid(message: &str) -> GeelloError {
    GeelloError::InvalidStyle(message.into())
}

/// Validate a selected face using the same isolated parser used by shaping.
pub fn validate_font(font: &FontResource) -> Result<(), GeelloError> {
    let face = FontRef::from_index(&font.bytes, font.face_index)
        .map_err(|_| invalid("invalid explicit font face"))?;
    if face.outline_glyphs().format().is_none() {
        return Err(invalid("font has no supported vector outlines"));
    }
    Ok(())
}

/// Shape only the declared stack. Language defaults to `und`; no locale is read.
/// The operation reserves a conservative shaping workspace before invoking cosmic.
pub fn shape_text(
    text: &str,
    style: &TextStyle,
    size: f64,
    wrap: Option<f64>,
    ctx: &OperationContext,
) -> Result<ShapedText, GeelloError> {
    shape_with_engine(text, style, size, wrap, ctx, &mut ShapeEngine::default())
}

struct FontStack {
    system: FontSystem,
    faces: Vec<(fontdb::ID, Arc<FontResource>)>,
    language: String,
    glyphs: BTreeMap<(usize, u16, u32), Arc<CachedOutline>>,
    glyph_bytes: usize,
    work_bytes: usize,
    holds: Vec<crate::operation::Reservation>,
}
struct CachedOutline {
    path: Vec<PathElement>,
    _holds: Vec<crate::operation::Reservation>,
}
#[derive(Default)]
struct ShapeEngine {
    stack: Option<FontStack>,
    shapes: BTreeMap<(String, u64, Option<u64>), Arc<ShapedText>>,
    shape_bytes: usize,
}
impl ShapeEngine {
    fn prepare(&mut self, style: &TextStyle, language: &str) {
        let matches = self.stack.as_ref().is_some_and(|stack| {
            stack.language == language
                && stack.work_bytes < 4 * 1024 * 1024
                && stack.faces.len() == style.fonts.len()
                && stack
                    .faces
                    .iter()
                    .zip(&style.fonts)
                    .all(|((_, previous), current)| {
                        previous.identity == current.identity
                            && previous.face_index == current.face_index
                            && Arc::ptr_eq(&previous.bytes, &current.bytes)
                    })
        });
        if !matches {
            self.shapes.clear();
            self.shape_bytes = 0;
            self.stack = None;
        }
    }
    fn shape(
        &mut self,
        text: &str,
        style: &TextStyle,
        size: f64,
        wrap: Option<f64>,
        ctx: &OperationContext,
    ) -> Result<Arc<ShapedText>, GeelloError> {
        ctx.checkpoint(text.len() as u64)?;
        let _key_memory = ctx.reserve(
            text.len()
                .checked_add(128)
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        self.prepare(
            style,
            if style.language.is_empty() {
                "und"
            } else {
                &style.language
            },
        );
        let key = (text.to_owned(), size.to_bits(), wrap.map(f64::to_bits));
        if let Some(shaped) = self.shapes.get(&key) {
            return Ok(shaped.clone());
        }
        let mut shaped = shape_with_engine(text, style, size, wrap, ctx, self)?;
        let bytes = shaped
            .outlines
            .iter()
            .map(|path| path.len() * std::mem::size_of::<PathElement>() * 2)
            .sum::<usize>()
            .saturating_add(
                shaped
                    .glyphs
                    .iter()
                    .map(|glyph| glyph.font_identity.len() + std::mem::size_of::<GlyphInfo>() + 64)
                    .sum::<usize>(),
            )
            .saturating_add(text.len() + 256);
        let cacheable =
            self.shapes.len() < 128 && self.shape_bytes.saturating_add(bytes) <= 4 * 1024 * 1024;
        if cacheable {
            shaped.holds.push(ctx.reserve(text.len() + 256)?);
        }
        let shaped = Arc::new(shaped);
        if cacheable {
            self.shapes.insert(key, shaped.clone());
            self.shape_bytes += bytes;
        }
        Ok(shaped)
    }
}

fn shape_with_engine(
    text: &str,
    style: &TextStyle,
    size: f64,
    wrap: Option<f64>,
    ctx: &OperationContext,
    engine: &mut ShapeEngine,
) -> Result<ShapedText, GeelloError> {
    ctx.checkpoint(text.len() as u64)?;
    if style.fonts.is_empty()
        || !size.is_finite()
        || size <= 0.
        || size > 16384.
        || wrap.is_some_and(|w| !w.is_finite() || w <= 0. || w > f32::MAX as f64)
    {
        return Err(invalid(
            "explicit fonts, positive bounded size and wrap required",
        ));
    }
    let language = if style.language.is_empty() {
        "und"
    } else {
        &style.language
    };
    if language.len() > 128
        || language
            .split('-')
            .any(|p| p.is_empty() || p.len() > 8 || !p.bytes().all(|b| b.is_ascii_alphanumeric()))
    {
        return Err(invalid("invalid BCP47 language tag"));
    }
    engine.prepare(style, language);
    let font_bytes = style.fonts.iter().try_fold(0usize, |n, f| {
        n.checked_add(f.bytes.len())
            .ok_or(GeelloError::SizeOverflow)
    })?;
    let workspace = text
        .len()
        .checked_mul(4096)
        .and_then(|n| n.checked_add(65536))
        .ok_or(GeelloError::SizeOverflow)?;
    if engine.stack.is_none() {
        let font_hold = ctx.reserve(
            font_bytes
                .checked_mul(4)
                .and_then(|bytes| bytes.checked_add(65536))
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let mut db = fontdb::Database::new();
        let mut faces = vec![];
        for resource in &style.fonts {
            ctx.checkpoint(1)?;
            validate_font(resource)?;
            let mut source_db = fontdb::Database::new();
            let ids = source_db
                .load_font_source(fontdb::Source::Binary(Arc::new(resource.bytes.clone())));
            let selected = ids
                .iter()
                .find(|id| {
                    source_db
                        .face(**id)
                        .is_some_and(|f| f.index == resource.face_index)
                })
                .copied()
                .ok_or_else(|| invalid("explicit font face is not loadable"))?;
            let mut face = source_db.face(selected).unwrap().clone();
            if let Some((id, _)) = faces.first() {
                let first: &fontdb::FaceInfo = db.face(*id).unwrap();
                if face.weight != first.weight
                    || face.style != first.style
                    || face.stretch != first.stretch
                {
                    return Err(invalid(
                        "a font stack must declare matching exact weight/style/stretch faces",
                    ));
                }
            }
            // A private family makes platform fallback lists inapplicable. All faces
            // share it; stable insertion IDs retain the declared fallback ordering.
            face.families
                .iter_mut()
                .for_each(|(name, _)| *name = "Geello isolated stack".into());
            let id = db.push_face_info(face);
            faces.push((id, resource.clone()));
        }
        engine.stack = Some(FontStack {
            system: FontSystem::new_with_locale_and_db(language.into(), db),
            faces,
            language: language.into(),
            glyphs: BTreeMap::new(),
            glyph_bytes: 0,
            work_bytes: 0,
            holds: vec![font_hold],
        });
    }
    let stack = engine.stack.as_mut().unwrap();
    stack.holds.push(ctx.reserve(workspace)?);
    stack.work_bytes = stack.work_bytes.saturating_add(workspace);
    let first = stack.system.db().face(stack.faces[0].0).unwrap();
    let attrs = Attrs::new()
        .family(Family::Name("Geello isolated stack"))
        .weight(first.weight)
        .style(first.style)
        .stretch(first.stretch);
    let mut buffer = Buffer::new(
        &mut stack.system,
        Metrics::new(size as f32, (size * 1.25) as f32),
    );
    buffer.set_wrap(if wrap.is_some() {
        Wrap::WordOrGlyph
    } else {
        Wrap::None
    });
    buffer.set_size(wrap.map(|w| w as f32), None);
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut stack.system, false);
    let mut result = ShapedText {
        glyphs: vec![],
        width: 0.,
        height: 0.,
        missing_glyphs: 0,
        outlines: vec![],
        holds: vec![],
    };
    for run in buffer.layout_runs() {
        result.width = result.width.max(run.line_w as f64);
        result.height = result.height.max((run.line_top + run.line_height) as f64);
        for glyph in run.glyphs {
            ctx.checkpoint(1)?;
            let (font_index, resource) = stack
                .faces
                .iter()
                .enumerate()
                .find(|(_, (identity, _))| *identity == glyph.font_id)
                .map(|(index, (_, resource))| (index, resource))
                .ok_or_else(|| invalid("shaper escaped explicit font stack"))?;
            let origin = [
                (glyph.x + glyph.font_size * glyph.x_offset) as f64,
                (run.line_y + glyph.y - glyph.font_size * glyph.y_offset) as f64,
            ];
            let glyph_key = (font_index, glyph.glyph_id, glyph.font_size.to_bits());
            let outline = if let Some(outline) = stack.glyphs.get(&glyph_key) {
                outline.clone()
            } else {
                let font = FontRef::from_index(&resource.bytes, resource.face_index)
                    .map_err(|_| invalid("invalid font"))?;
                let mut glyph_holds = vec![];
                let mut pen = Pen {
                    path: vec![],
                    position: [0., 0.],
                    origin: [0., 0.],
                    ctx,
                    holds: &mut glyph_holds,
                    error: None,
                };
                if let Some(outline) = font
                    .outline_glyphs()
                    .get(GlyphId::new(glyph.glyph_id as u32))
                {
                    let _scratch = ctx.reserve(
                        outline.draw_memory_size(cosmic_text::skrifa::outline::Hinting::None),
                    )?;
                    outline
                        .draw(
                            DrawSettings::unhinted(
                                Size::new(glyph.font_size),
                                LocationRef::default(),
                            ),
                            &mut pen,
                        )
                        .map_err(|_| invalid("glyph outline decoding failed"))?;
                }
                if let Some(error) = pen.error {
                    return Err(error);
                }
                let path = pen.path;
                let bytes = path
                    .len()
                    .saturating_mul(std::mem::size_of::<PathElement>() * 2)
                    .saturating_add(256);
                glyph_holds.push(ctx.reserve(256)?);
                let outline = Arc::new(CachedOutline {
                    path,
                    _holds: glyph_holds,
                });
                if stack.glyphs.len() < 256
                    && stack.glyph_bytes.saturating_add(bytes) <= 4 * 1024 * 1024
                {
                    stack.glyphs.insert(glyph_key, outline.clone());
                    stack.glyph_bytes += bytes;
                }
                outline
            };
            if glyph.glyph_id == 0 {
                result.missing_glyphs += 1;
            }
            ctx.checkpoint(outline.path.len() as u64)?;
            result.holds.push(
                ctx.reserve(
                    outline
                        .path
                        .len()
                        .checked_mul(2 * std::mem::size_of::<PathElement>())
                        .ok_or(GeelloError::SizeOverflow)?,
                )?,
            );
            let path = outline
                .path
                .iter()
                .map(|element| {
                    transform(*element, |point| {
                        [point[0] + origin[0], point[1] + origin[1]]
                    })
                })
                .collect();
            result.holds.push(
                ctx.reserve(std::mem::size_of::<GlyphInfo>() + resource.identity.len() + 64)?,
            );
            result.outlines.push(path);
            result.glyphs.push(GlyphInfo {
                font_identity: resource.identity.clone(),
                glyph_id: glyph.glyph_id,
                cluster: glyph.start..glyph.end,
                line: run.line_i,
                origin,
                advance: glyph.w as f64,
            });
        }
    }
    ctx.check()?;
    Ok(result)
}
struct Pen<'a> {
    path: Vec<PathElement>,
    position: [f64; 2],
    origin: [f64; 2],
    ctx: &'a OperationContext,
    holds: &'a mut Vec<crate::operation::Reservation>,
    error: Option<GeelloError>,
}
impl Pen<'_> {
    fn point(&self, x: f32, y: f32) -> [f64; 2] {
        [self.origin[0] + x as f64, self.origin[1] - y as f64]
    }
    fn push(&mut self, p: PathElement) {
        if self.error.is_some() {
            return;
        }
        let admitted = self
            .ctx
            .checkpoint(1)
            .and_then(|()| self.ctx.reserve(2 * std::mem::size_of::<PathElement>()));
        match admitted {
            Ok(r) => {
                self.holds.push(r);
                self.path.push(p);
            }
            Err(e) => self.error = Some(e),
        }
    }
}
impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.position = p;
        self.push(PathElement::Move(p));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.position = p;
        self.push(PathElement::Line(p));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let q = self.point(cx, cy);
        let p = self.point(x, y);
        let a = self.position;
        self.push(PathElement::Cubic(
            [
                a[0] + (q[0] - a[0]) * 2. / 3.,
                a[1] + (q[1] - a[1]) * 2. / 3.,
            ],
            [
                p[0] + (q[0] - p[0]) * 2. / 3.,
                p[1] + (q[1] - p[1]) * 2. / 3.,
            ],
            p,
        ));
        self.position = p;
    }
    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        let p = self.point(x, y);
        self.push(PathElement::Cubic(
            self.point(ax, ay),
            self.point(bx, by),
            p,
        ));
        self.position = p;
    }
    fn close(&mut self) {
        self.push(PathElement::Close);
    }
}

pub(crate) struct PlacedLabel {
    pub path: Arc<[PathElement]>,
    pub color: Color,
    pub holds: Vec<crate::operation::Reservation>,
}
struct Candidate {
    path: Vec<PathElement>,
    bounds: [f64; 4],
    priority: f64,
    order: usize,
    group: usize,
    overlap: bool,
    color: Color,
    holds: Vec<crate::operation::Reservation>,
}
pub(crate) struct MarkerObstacle {
    pub bounds: [f64; 4],
    pub source: Option<(usize, u64)>,
}
fn transform(p: PathElement, mut f: impl FnMut([f64; 2]) -> [f64; 2]) -> PathElement {
    match p {
        PathElement::Move(p) => PathElement::Move(f(p)),
        PathElement::Line(p) => PathElement::Line(f(p)),
        PathElement::Cubic(a, b, c) => PathElement::Cubic(f(a), f(b), f(c)),
        PathElement::Close => PathElement::Close,
    }
}
fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}
#[derive(Default)]
struct BoundsIndex {
    entries: Vec<([f64; 4], usize)>,
    cells: BTreeMap<(i64, i64), Vec<usize>>,
    wide: Vec<usize>,
    holds: Vec<crate::operation::Reservation>,
}
impl BoundsIndex {
    fn cells(bounds: [f64; 4]) -> Option<[i64; 4]> {
        if bounds.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let cells = bounds.map(|value| (value / 64.).floor() as i64);
        let width = cells[2] as i128 - cells[0] as i128 + 1;
        let height = cells[3] as i128 - cells[1] as i128 + 1;
        (width > 0 && height > 0 && width <= 64 && height <= 64 && width * height <= 64)
            .then_some(cells)
    }
    fn insert(
        &mut self,
        bounds: [f64; 4],
        payload: usize,
        ctx: &OperationContext,
    ) -> Result<(), GeelloError> {
        self.holds.push(ctx.reserve(128)?);
        let index = self.entries.len();
        self.entries.push((bounds, payload));
        if let Some(cells) = Self::cells(bounds) {
            for row in cells[1]..=cells[3] {
                for column in cells[0]..=cells[2] {
                    ctx.checkpoint(1)?;
                    self.holds.push(ctx.reserve(160)?);
                    self.cells.entry((column, row)).or_default().push(index);
                }
            }
        } else {
            self.wide.push(index);
        }
        Ok(())
    }
    fn any(
        &self,
        bounds: [f64; 4],
        ctx: &OperationContext,
        mut matches: impl FnMut(usize) -> bool,
    ) -> Result<bool, GeelloError> {
        let mut inspect = |index: usize| -> Result<bool, GeelloError> {
            ctx.checkpoint(1)?;
            let (other, payload) = self.entries[index];
            Ok(overlaps(other, bounds) && matches(payload))
        };
        if let Some(cells) = Self::cells(bounds) {
            for &index in &self.wide {
                if inspect(index)? {
                    return Ok(true);
                }
            }
            for row in cells[1]..=cells[3] {
                for column in cells[0]..=cells[2] {
                    ctx.checkpoint(1)?;
                    if let Some(indices) = self.cells.get(&(column, row)) {
                        for &index in indices {
                            if inspect(index)? {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        } else {
            for index in 0..self.entries.len() {
                if inspect(index)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
/// One global pass, descending priority and later-drawn tie breaking. Accepted
/// labels are subsequently painted in original draw order.
pub(crate) fn layout(
    composition: &MapComposition,
    view: &View,
    obstacles: &[MarkerObstacle],
    pixel_ratio: f64,
    ctx: &OperationContext,
) -> Result<Vec<PlacedLabel>, GeelloError> {
    let mut candidates = vec![];
    let mut workspace = vec![];
    let mut shaper = ShapeEngine::default();
    let mut obstacle_index = BoundsIndex::default();
    let mut marker_sizes = BTreeMap::<(usize, u64), [f64; 2]>::new();
    for (index, obstacle) in obstacles.iter().enumerate() {
        obstacle_index.insert(obstacle.bounds, index, ctx)?;
        if let Some(source) = obstacle.source {
            workspace.push(ctx.reserve(128)?);
            let size = marker_sizes.entry(source).or_insert([0., 0.]);
            size[0] = size[0].max((obstacle.bounds[2] - obstacle.bounds[0]) / 2.);
            size[1] = size[1].max((obstacle.bounds[3] - obstacle.bounds[1]) / 2.);
        }
    }
    let mut group = 0usize;
    for (order, item) in composition.items.iter().enumerate() {
        let DrawItem::Label(label) = item else {
            continue;
        };
        if label.crs != view.crs {
            return Err(GeelloError::ProjectionRequired);
        }
        if !label.priority.is_finite()
            || !label.repeat_distance.is_finite()
            || label.repeat_distance <= 0.
        {
            return Err(invalid("invalid label priority/repeat"));
        }
        if label.placement == LabelPlacement::Curved
            && (label.wrap_width.is_some() || label.text.contains(['\n', '\r']))
        {
            return Err(invalid("curved labels cannot wrap or contain newlines"));
        }
        let (sx, sy, _, _) = view.scales();
        let size = crate::style::length(label.style.size, (sx * sy).abs().sqrt())?;
        let shaped = shaper.shape(&label.text, &label.style, size, label.wrap_width, ctx)?;
        let marker_size = label
            .marker_source
            .and_then(|source| marker_sizes.get(&source))
            .copied()
            .unwrap_or([0., 0.]);
        let mut anchors = vec![];
        collect(&label.geometry, label.placement, view, ctx, &mut anchors, 0)?;
        for anchor in anchors {
            let centers = if let Anchor::Line(ref line) = anchor {
                let length = line_length(line);
                if length < shaped.width {
                    continue;
                }
                if length < label.repeat_distance {
                    vec![length / 2.]
                } else {
                    let count = ((length - label.repeat_distance / 2.) / label.repeat_distance)
                        .floor() as usize
                        + 1;
                    workspace
                        .push(ctx.reserve(count.checked_mul(8).ok_or(GeelloError::SizeOverflow)?)?);
                    ctx.checkpoint(count as u64)?;
                    (0..count)
                        .map(|i| label.repeat_distance / 2. + i as f64 * label.repeat_distance)
                        .collect()
                }
            } else {
                vec![0.]
            };
            for center in centers {
                group = group.checked_add(1).ok_or(GeelloError::SizeOverflow)?;
                let offsets: &[[f64; 2]] = if matches!(anchor, Anchor::Point(_)) {
                    &[
                        [1., -1.],
                        [1., 0.],
                        [1., 1.],
                        [0., -1.],
                        [0., 1.],
                        [-1., -1.],
                        [-1., 0.],
                        [-1., 1.],
                    ]
                } else {
                    &[[0., 0.]]
                };
                for offset in offsets {
                    let mut path = vec![];
                    let mut bounds = [
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ];
                    let mut holds = vec![];
                    let mut rejected = false;
                    let mut cluster_positions = std::collections::BTreeMap::new();
                    for glyph in &shaped.glyphs {
                        let range = cluster_positions
                            .entry((glyph.line, glyph.cluster.start, glyph.cluster.end))
                            .or_insert((f64::INFINITY, f64::NEG_INFINITY));
                        range.0 = range.0.min(glyph.origin[0]);
                        range.1 = range.1.max(glyph.origin[0] + glyph.advance);
                    }
                    let mut previous_angle: Option<f64> = None;
                    for (glyph, outline) in shaped.glyphs.iter().zip(&shaped.outlines) {
                        let (origin, angle) = match &anchor {
                            Anchor::Point(p) => (
                                [
                                    p[0] + offset[0]
                                        * (shaped.width / 2. + 2. * pixel_ratio + marker_size[0]),
                                    p[1] + offset[1]
                                        * (shaped.height / 2. + 2. * pixel_ratio + marker_size[1]),
                                ],
                                0.,
                            ),
                            Anchor::Polygon(_, p) => (*p, 0.),
                            Anchor::Line(line) => {
                                let (p, mut angle) = sample(line, center);
                                if angle.cos() < 0. {
                                    angle += std::f64::consts::PI;
                                }
                                if label.placement == LabelPlacement::Curved {
                                    let reverse = sample(line, center).1.cos() < 0.;
                                    let range = cluster_positions
                                        [&(glyph.line, glyph.cluster.start, glyph.cluster.end)];
                                    let cluster_center = (range.0 + range.1) / 2.;
                                    let distance = center
                                        + (cluster_center - shaped.width / 2.)
                                            * if reverse { -1. } else { 1. };
                                    let (p, mut a) = sample(line, distance);
                                    if reverse {
                                        a += std::f64::consts::PI;
                                    }
                                    let delta = (a - angle).sin().atan2((a - angle).cos()).abs();
                                    let adjacent = previous_angle.map_or(0., |previous| {
                                        (a - previous).sin().atan2((a - previous).cos()).abs()
                                    });
                                    previous_angle = Some(a);
                                    if delta > std::f64::consts::FRAC_PI_4
                                        || adjacent > std::f64::consts::PI / 6.
                                    {
                                        rejected = true;
                                        break;
                                    }
                                    (
                                        [
                                            p[0] - (cluster_center - shaped.width / 2.) * a.cos(),
                                            p[1] - (cluster_center - shaped.width / 2.) * a.sin(),
                                        ],
                                        a,
                                    )
                                } else {
                                    (p, angle)
                                }
                            }
                        };
                        holds.push(
                            ctx.reserve(
                                outline
                                    .len()
                                    .checked_mul(2 * std::mem::size_of::<PathElement>())
                                    .ok_or(GeelloError::SizeOverflow)?,
                            )?,
                        );
                        for element in outline {
                            ctx.checkpoint(1)?;
                            path.push(transform(*element, |p| {
                                let x = p[0] - shaped.width / 2.;
                                let y = p[1] - shaped.height / 2.;
                                let p = [
                                    origin[0] + x * angle.cos() - y * angle.sin(),
                                    origin[1] + x * angle.sin() + y * angle.cos(),
                                ];
                                bounds[0] = bounds[0].min(p[0]);
                                bounds[1] = bounds[1].min(p[1]);
                                bounds[2] = bounds[2].max(p[0]);
                                bounds[3] = bounds[3].max(p[1]);
                                p
                            }));
                        }
                    }
                    if rejected || path.is_empty() {
                        continue;
                    }
                    if let Anchor::Polygon(p, _) = &anchor {
                        let rect =
                            geo_types::Rect::new((bounds[0], bounds[1]), (bounds[2], bounds[3]))
                                .to_polygon();
                        if !p.contains(&rect) {
                            continue;
                        }
                    }
                    holds.push(ctx.reserve(256)?);
                    if !label.allow_overlap
                        && obstacle_index.any(bounds, ctx, |index| {
                            let obstacle = &obstacles[index];
                            !(obstacle.source.is_some() && obstacle.source == label.marker_source)
                        })?
                    {
                        continue;
                    }
                    candidates.push(Candidate {
                        path,
                        bounds,
                        priority: label.priority,
                        order,
                        group,
                        overlap: label.allow_overlap,
                        color: label.style.color,
                        holds,
                    });
                }
            }
        }
    }
    candidates.sort_by(|a, b| {
        b.priority
            .total_cmp(&a.priority)
            .then(b.order.cmp(&a.order))
    });
    let mut accepted: Vec<Candidate> = vec![];
    let mut accepted_groups = BTreeSet::new();
    let mut accepted_index = BoundsIndex::default();
    for candidate in candidates {
        ctx.checkpoint(1)?;
        if accepted_groups.contains(&candidate.group) {
            continue;
        }
        if candidate.overlap || !accepted_index.any(candidate.bounds, ctx, |_| true)? {
            workspace.push(ctx.reserve(64)?);
            accepted_groups.insert(candidate.group);
            if !candidate.overlap {
                accepted_index.insert(candidate.bounds, accepted.len(), ctx)?;
            }
            accepted.push(candidate);
        }
    }
    accepted.retain(|candidate| {
        candidate.bounds[2] >= 0.
            && candidate.bounds[3] >= 0.
            && candidate.bounds[0] <= view.width as f64
            && candidate.bounds[1] <= view.height as f64
    });
    accepted.sort_by_key(|c| c.order);
    Ok(accepted
        .into_iter()
        .map(|c| PlacedLabel {
            path: c.path.into(),
            color: c.color,
            holds: c.holds,
        })
        .collect())
}
enum Anchor {
    Point([f64; 2]),
    Polygon(Polygon, [f64; 2]),
    Line(Vec<[f64; 2]>),
}
fn collect(
    g: &Geometry,
    placement: LabelPlacement,
    view: &View,
    ctx: &OperationContext,
    out: &mut Vec<Anchor>,
    depth: usize,
) -> Result<(), GeelloError> {
    if depth > 64 {
        return Err(GeelloError::BudgetExceeded("geometry depth"));
    }
    ctx.checkpoint(1)?;
    let line = |l: &LineString| -> Result<Vec<[f64; 2]>, GeelloError> {
        l.0.iter().map(|p| view.pixel(p.x, p.y)).collect()
    };
    match g {
        Geometry::Point(p) if placement == LabelPlacement::Point => {
            out.push(Anchor::Point(view.pixel(p.x(), p.y())?))
        }
        Geometry::LineString(l)
            if matches!(placement, LabelPlacement::Line | LabelPlacement::Curved) =>
        {
            out.push(Anchor::Line(line(l)?))
        }
        Geometry::Polygon(p) if placement == LabelPlacement::Polygon => {
            let rings = p
                .interiors()
                .iter()
                .map(|l| {
                    line(l).map(|l| {
                        LineString::from(l.into_iter().map(|p| (p[0], p[1])).collect::<Vec<_>>())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let exterior = line(p.exterior())?;
            let p = Polygon::new(
                LineString::from(
                    exterior
                        .into_iter()
                        .map(|p| (p[0], p[1]))
                        .collect::<Vec<_>>(),
                ),
                rings,
            );
            if let Some(Point(c)) = p.interior_point() {
                out.push(Anchor::Polygon(p, [c.x, c.y]));
            }
        }
        Geometry::MultiPoint(m) => {
            for p in &m.0 {
                collect(&Geometry::Point(*p), placement, view, ctx, out, depth + 1)?;
            }
        }
        Geometry::MultiLineString(m) => {
            for l in &m.0 {
                collect(
                    &Geometry::LineString(l.clone()),
                    placement,
                    view,
                    ctx,
                    out,
                    depth + 1,
                )?;
            }
        }
        Geometry::MultiPolygon(m) => {
            for p in &m.0 {
                collect(
                    &Geometry::Polygon(p.clone()),
                    placement,
                    view,
                    ctx,
                    out,
                    depth + 1,
                )?;
            }
        }
        Geometry::GeometryCollection(m) => {
            for g in &m.0 {
                collect(g, placement, view, ctx, out, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn line_length(line: &[[f64; 2]]) -> f64 {
    line.windows(2)
        .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
        .sum()
}
fn sample(line: &[[f64; 2]], mut distance: f64) -> ([f64; 2], f64) {
    for pair in line.windows(2) {
        let dx = pair[1][0] - pair[0][0];
        let dy = pair[1][1] - pair[0][1];
        let length = dx.hypot(dy);
        if length == 0. {
            continue;
        }
        if distance <= length {
            let t = (distance / length).clamp(0., 1.);
            return ([pair[0][0] + dx * t, pair[0][1] + dy * t], dy.atan2(dx));
        }
        distance -= length;
    }
    (line.last().copied().unwrap_or([0., 0.]), 0.)
}

#[cfg(test)]
mod optimization_tests {
    use super::*;

    fn context() -> OperationContext {
        OperationContext::new(
            OperationLimits {
                max_live_bytes: 128 << 20,
                max_work: 10_000_000,
                max_commands: 100_000,
                max_diagnostics: 16,
            },
            None,
        )
    }

    #[test]
    fn request_local_shaping_reuses_text_and_isolates_changed_font_stacks() {
        let ctx = context();
        let baseline = ctx.live_bytes();
        let bytes: Arc<[u8]> =
            Arc::from(include_bytes!("../tests/fixtures/fonts/latin.ttf").as_slice());
        let mut style = TextStyle {
            fonts: vec![Arc::new(FontResource {
                identity: "latin".into(),
                bytes: bytes.clone(),
                face_index: 0,
            })],
            size: Length::Pixels(20.),
            color: Color::BLACK,
            language: "und".into(),
        };
        {
            let mut engine = ShapeEngine::default();
            let first = engine.shape("office ffi", &style, 20., None, &ctx).unwrap();
            let after_first = ctx.live_bytes();
            let second = engine.shape("office ffi", &style, 20., None, &ctx).unwrap();
            assert!(Arc::ptr_eq(&first, &second));
            assert_eq!(ctx.live_bytes(), after_first);
            let other = engine.shape("office", &style, 20., None, &ctx).unwrap();
            assert!(!engine.stack.as_ref().unwrap().glyphs.is_empty());
            assert_eq!(other.glyphs[0].font_identity, "latin");
            style.fonts = vec![Arc::new(FontResource {
                identity: "replacement".into(),
                bytes,
                face_index: 0,
            })];
            let replacement = engine.shape("office ffi", &style, 20., None, &ctx).unwrap();
            assert!(
                replacement
                    .glyphs
                    .iter()
                    .all(|glyph| glyph.font_identity == "replacement")
            );
            assert!(!Arc::ptr_eq(&first, &replacement));
        }
        assert_eq!(ctx.live_bytes(), baseline);
    }

    #[test]
    fn indexed_overlap_matches_linear_detection_including_wide_and_negative_bounds() {
        let ctx = context();
        let mut index = BoundsIndex::default();
        let mut entries = vec![];
        for ordinal in 0..1000 {
            let horizontal = (ordinal % 50) as f64 * 70. - 1500.;
            let vertical = (ordinal / 50) as f64 * 70. - 700.;
            let bounds = [horizontal, vertical, horizontal + 20., vertical + 20.];
            entries.push(bounds);
            index.insert(bounds, ordinal, &ctx).unwrap();
        }
        let wide = [-2000., -1., 2000., 1.];
        index.insert(wide, entries.len(), &ctx).unwrap();
        entries.push(wide);
        for ordinal in 0..100 {
            let coordinate = ordinal as f64 * 33. - 1700.;
            let bounds = [coordinate, -50., coordinate + 25., 50.];
            assert_eq!(
                index.any(bounds, &ctx, |_| true).unwrap(),
                entries.iter().any(|other| overlaps(*other, bounds))
            );
        }
        assert!(
            index
                .any([-5000., -5000., 5000., 5000.], &ctx, |_| true)
                .unwrap()
        );
    }
}
