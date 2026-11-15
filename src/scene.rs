//! Preparation freezes pixel-space commands and owns only drawing resources.
// Backend-free builds retain command packets for inspection and later ownership transfer.
#![cfg_attr(
    not(any(feature = "render-cpu", feature = "render-gpu")),
    allow(dead_code)
)]
use crate::*;
use geo_types::{Coord, Geometry, LineString, Polygon};
use std::sync::Arc;

/// Bounded diagnostics identify skipped incompatible components without source retention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub item: usize,
    pub source_ordinal: u64,
    pub message: &'static str,
}
/// Stable inspection surface; command encodings remain private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneSummary {
    pub width: u32,
    pub height: u32,
    pub command_count: usize,
    pub coordinate_count: usize,
    pub incompatible_components: usize,
    pub estimated_accounted_bytes: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub omitted_diagnostics: usize,
}
/// Immutable drawing packet; cloning shares resources and their accounting reservation.
#[derive(Debug, Clone)]
pub struct PreparedScene {
    pub(crate) inner: Arc<SceneData>,
}
#[derive(Debug)]
pub(crate) struct SceneData {
    pub view: View,
    pub commands: Vec<Command>,
    pub summary: SceneSummary,
    pub _reservations: Vec<crate::operation::Reservation>,
}
impl PreparedScene {
    #[cfg(feature = "server")]
    pub(crate) fn add_missing_geometry_targets(
        &mut self,
        count: usize,
        ctx: &OperationContext,
    ) -> Result<(), GeelloError> {
        let data = Arc::get_mut(&mut self.inner).expect("new scene is not shared");
        let summary = &mut data.summary;
        let retained = count.min(
            ctx.limits()
                .max_diagnostics
                .saturating_sub(summary.diagnostics.len()),
        );
        let bytes = retained
            .checked_mul(std::mem::size_of::<Diagnostic>())
            .ok_or(GeelloError::SizeOverflow)?;
        data._reservations.push(ctx.reserve(bytes)?);
        summary.estimated_accounted_bytes = summary
            .estimated_accounted_bytes
            .checked_add(bytes)
            .ok_or(GeelloError::SizeOverflow)?;
        summary.incompatible_components = summary
            .incompatible_components
            .checked_add(count)
            .ok_or(GeelloError::SizeOverflow)?;
        summary.omitted_diagnostics = summary
            .omitted_diagnostics
            .checked_add(count - retained)
            .ok_or(GeelloError::SizeOverflow)?;
        for _ in 0..retained {
            summary.diagnostics.push(Diagnostic {
                item: usize::MAX,
                source_ordinal: u64::MAX,
                message: "geometry animation stable target is missing; skipped",
            });
        }
        Ok(())
    }
    pub fn summary(&self) -> &SceneSummary {
        &self.inner.summary
    }
    pub fn view(&self) -> &View {
        &self.inner.view
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum PathElement {
    Move([f64; 2]),
    Line([f64; 2]),
    Cubic([f64; 2], [f64; 2], [f64; 2]),
    Close,
}
#[derive(Debug, Clone)]
pub(crate) struct Stroke {
    pub width: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f64,
    pub dashes: Vec<f64>,
    pub dash_offset: f64,
}
#[derive(Debug)]
pub(crate) enum Command {
    Path {
        path: Arc<[PathElement]>,
        color: Color,
        opacity: f64,
        stroke: Option<Stroke>,
    },
    Image {
        image: Arc<RgbaImage>,
        rect: [f64; 4],
        opacity: f64,
    },
}
struct Builder<'a> {
    view: &'a View,
    ctx: &'a OperationContext,
    commands: Vec<Command>,
    reservations: Vec<crate::operation::Reservation>,
    summary: SceneSummary,
    #[cfg(feature = "text")]
    obstacles: Vec<crate::text::MarkerObstacle>,
    #[cfg(feature = "text")]
    current_feature: Option<(usize, u64)>,
}
impl Builder<'_> {
    fn reserve(&mut self, n: usize) -> Result<(), GeelloError> {
        self.reservations.push(self.ctx.reserve(n)?);
        self.summary.estimated_accounted_bytes = self
            .summary
            .estimated_accounted_bytes
            .checked_add(n)
            .ok_or(GeelloError::SizeOverflow)?;
        Ok(())
    }
    fn command(&mut self, c: Command) -> Result<(), GeelloError> {
        self.ctx.checkpoint(1)?;
        if self.commands.len() >= self.ctx.limits().max_commands {
            return Err(GeelloError::BudgetExceeded("commands"));
        }
        let extra = match &c {
            Command::Path {
                stroke: Some(stroke),
                ..
            } => stroke
                .dashes
                .len()
                .checked_mul(8)
                .ok_or(GeelloError::SizeOverflow)?,
            _ => 0,
        };
        self.reserve(
            std::mem::size_of::<Command>()
                .checked_add(extra)
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        self.commands
            .try_reserve(1)
            .map_err(|_| GeelloError::AllocationFailed)?;
        self.commands.push(c);
        Ok(())
    }
    fn diagnostic(&mut self, item: usize, ordinal: u64) -> Result<(), GeelloError> {
        self.summary.incompatible_components += 1;
        if self.summary.diagnostics.len() < self.ctx.limits().max_diagnostics {
            self.reserve(std::mem::size_of::<Diagnostic>())?;
            self.summary.diagnostics.push(Diagnostic {
                item,
                source_ordinal: ordinal,
                message: "symbol incompatible with geometry component",
            })
        } else {
            self.summary.omitted_diagnostics += 1
        }
        Ok(())
    }
    fn point(&mut self, c: Coord) -> Result<[f64; 2], GeelloError> {
        self.ctx.checkpoint(1)?;
        if !c.x.is_finite() || !c.y.is_finite() {
            return Err(GeelloError::InvalidGeometry("non-finite coordinate".into()));
        }
        if self.view.crs.space() == CoordinateSpace::GeographicXy
            && (!(-180.0..=180.0).contains(&c.x) || !(-90.0..=90.0).contains(&c.y))
        {
            return Err(GeelloError::InvalidGeometry(
                "outside geographic domain".into(),
            ));
        }
        self.summary.coordinate_count += 1;
        self.view.pixel(c.x, c.y)
    }
    fn path(
        &mut self,
        rings: &[&LineString],
        close: bool,
    ) -> Result<Arc<[PathElement]>, GeelloError> {
        let count = rings.iter().try_fold(0usize, |n, r| {
            n.checked_add(r.0.len())
                .and_then(|v| v.checked_add(usize::from(close)))
                .ok_or(GeelloError::SizeOverflow)
        })?;
        self.reserve(
            count
                .checked_mul(std::mem::size_of::<PathElement>())
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let mut p = Vec::new();
        p.try_reserve_exact(count)
            .map_err(|_| GeelloError::AllocationFailed)?;
        for ring in rings {
            for (i, c) in ring.0.iter().enumerate() {
                let xy = self.point(*c)?;
                p.push(if i == 0 {
                    PathElement::Move(xy)
                } else {
                    PathElement::Line(xy)
                })
            }
            if close && !ring.0.is_empty() {
                p.push(PathElement::Close)
            }
        }
        Ok(p.into())
    }
    fn stroke(&self, s: &StrokeStyle) -> Result<Stroke, GeelloError> {
        crate::style::opacity(s.opacity)?;
        let (sx, sy, _, _) = self.view.scales();
        let scale = (sx.sqrt() * sy.sqrt()).abs();
        let width = crate::style::length(s.width, scale)?;
        if !s.miter_limit.is_finite()
            || s.miter_limit < 1.0
            || !s.dash_offset.is_finite()
            || !s.dashes.len().is_multiple_of(2)
            || s.dashes.iter().any(|n| !n.is_finite() || *n <= 0.0)
        {
            return Err(GeelloError::InvalidStyle(
                "invalid stroke/dash parameters".into(),
            ));
        }
        let units = if matches!(s.width, Length::MapUnits(_)) {
            scale
        } else {
            1.0
        };
        Ok(Stroke {
            width,
            cap: s.cap,
            join: s.join,
            miter_limit: s.miter_limit,
            dashes: s.dashes.iter().map(|n| n * units).collect(),
            dash_offset: s.dash_offset * units,
        })
    }
    fn line(&mut self, l: &LineString, s: &StrokeStyle, close: bool) -> Result<(), GeelloError> {
        let stroke = self.stroke(s)?;
        if l.0.len() < 2 || l.0.windows(2).all(|w| w[0] == w[1]) {
            for c in &l.0 {
                self.point(*c)?;
            }
            return Ok(());
        }
        let path = self.path(&[l], close)?;
        if stroke.width > 0.0 {
            self.command(Command::Path {
                path,
                color: s.color,
                opacity: s.opacity,
                stroke: Some(stroke),
            })?
        }
        Ok(())
    }
    fn polygon(&mut self, p: &Polygon, s: &Symbol) -> Result<(), GeelloError> {
        match s {
            Symbol::PolygonFill { color, opacity } => {
                crate::style::opacity(*opacity)?;
                let rings: Vec<_> = std::iter::once(p.exterior()).chain(p.interiors()).collect();
                let path = self.path(&rings, true)?;
                if !path.is_empty() {
                    self.command(Command::Path {
                        path,
                        color: *color,
                        opacity: *opacity,
                        stroke: None,
                    })?
                }
            }
            Symbol::PolygonOutline(stroke) => {
                for ring in std::iter::once(p.exterior()).chain(p.interiors()) {
                    self.line(ring, stroke, true)?
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    fn marker(&mut self, c: Coord, s: &PointStyle) -> Result<(), GeelloError> {
        crate::style::opacity(s.opacity)?;
        if !s.angle_degrees.is_finite() {
            return Err(GeelloError::InvalidStyle("invalid marker angle".into()));
        }
        let center = self.point(c)?;
        let (sx, sy, _, _) = self.view.scales();
        let size = crate::style::length(s.size, sx.sqrt() * sy.sqrt())?;
        if size == 0.0 {
            return Ok(());
        }
        let r = size / 2.0;
        #[cfg(feature = "text")]
        if s.opacity > 0. && (s.fill.is_some() || s.stroke.is_some()) {
            let stroke = s
                .stroke
                .as_ref()
                .map(|stroke| self.stroke(stroke).map(|s| s.width / 2.))
                .transpose()?
                .unwrap_or(0.);
            let extent = r * 2.0f64.sqrt() + stroke;
            self.reserve(std::mem::size_of::<crate::text::MarkerObstacle>())?;
            self.obstacles.push(crate::text::MarkerObstacle {
                bounds: [
                    center[0] - extent,
                    center[1] - extent,
                    center[0] + extent,
                    center[1] + extent,
                ],
                source: self.current_feature,
            });
        }
        let angle = s.angle_degrees.to_radians();
        let (sin, cos) = angle.sin_cos();
        let tx = |x: f64, y: f64| [center[0] + x * cos - y * sin, center[1] + x * sin + y * cos];
        self.reserve(6 * std::mem::size_of::<PathElement>())?;
        let p = match s.shape {
            MarkerShape::Circle => {
                let k = r * 0.5522847498307936;
                vec![
                    PathElement::Move(tx(r, 0.0)),
                    PathElement::Cubic(tx(r, k), tx(k, r), tx(0.0, r)),
                    PathElement::Cubic(tx(-k, r), tx(-r, k), tx(-r, 0.0)),
                    PathElement::Cubic(tx(-r, -k), tx(-k, -r), tx(0.0, -r)),
                    PathElement::Cubic(tx(k, -r), tx(r, -k), tx(r, 0.0)),
                    PathElement::Close,
                ]
            }
            MarkerShape::Square => vec![
                PathElement::Move(tx(-r, -r)),
                PathElement::Line(tx(r, -r)),
                PathElement::Line(tx(r, r)),
                PathElement::Line(tx(-r, r)),
                PathElement::Close,
            ],
            MarkerShape::Triangle => {
                let h = size * 3.0f64.sqrt() / 2.0;
                vec![
                    PathElement::Move(tx(0.0, -h / 2.0)),
                    PathElement::Line(tx(r, h / 2.0)),
                    PathElement::Line(tx(-r, h / 2.0)),
                    PathElement::Close,
                ]
            }
        };
        let path: Arc<[PathElement]> = p.into();
        if let Some(color) = s.fill {
            self.command(Command::Path {
                path: path.clone(),
                color,
                opacity: s.opacity,
                stroke: None,
            })?
        }
        if let Some(st) = &s.stroke {
            let stroke = self.stroke(st)?;
            if stroke.width > 0.0 {
                self.command(Command::Path {
                    path,
                    color: st.color,
                    opacity: s.opacity * st.opacity,
                    stroke: Some(stroke),
                })?
            }
        }
        Ok(())
    }
    fn geometry(
        &mut self,
        g: &Geometry,
        s: &Symbol,
        item: usize,
        ordinal: u64,
        depth: usize,
    ) -> Result<(), GeelloError> {
        self.ctx.checkpoint(1)?;
        if depth > 64 {
            return Err(GeelloError::BudgetExceeded("geometry depth"));
        }
        match (g, s) {
            (Geometry::GeometryCollection(c), _) => {
                for child in &c.0 {
                    self.geometry(child, s, item, ordinal, depth + 1)?
                }
            }
            (Geometry::MultiPoint(p), _) => {
                for p in &p.0 {
                    self.geometry(&Geometry::Point(*p), s, item, ordinal, depth + 1)?
                }
            }
            (Geometry::MultiLineString(lines), _) => {
                for l in &lines.0 {
                    self.ctx.checkpoint(1)?;
                    match s {
                        Symbol::Line(stroke) => self.line(l, stroke, false)?,
                        _ => self.diagnostic(item, ordinal)?,
                    }
                }
            }
            (Geometry::MultiPolygon(polys), _) => {
                for p in &polys.0 {
                    self.ctx.checkpoint(1)?;
                    match s {
                        Symbol::PolygonFill { .. } | Symbol::PolygonOutline(_) => {
                            self.polygon(p, s)?
                        }
                        _ => self.diagnostic(item, ordinal)?,
                    }
                }
            }
            (Geometry::Point(p), Symbol::Point(style)) => self.marker(p.0, style)?,
            (Geometry::LineString(l), Symbol::Line(st)) => self.line(l, st, false)?,
            (Geometry::Line(l), Symbol::Line(st)) => {
                self.line(&LineString(vec![l.start, l.end]), st, false)?
            }
            (Geometry::Polygon(p), Symbol::PolygonFill { .. } | Symbol::PolygonOutline(_)) => {
                self.polygon(p, s)?
            }
            (Geometry::Rect(r), Symbol::PolygonFill { .. } | Symbol::PolygonOutline(_)) => {
                self.polygon(&r.to_polygon(), s)?
            }
            (Geometry::Triangle(t), Symbol::PolygonFill { .. } | Symbol::PolygonOutline(_)) => {
                self.polygon(&t.to_polygon(), s)?
            }
            _ => self.diagnostic(item, ordinal)?,
        }
        Ok(())
    }
}
/// Prepare already-resolved, same-CRS memory items. No source, property or style survives as deferred work.
/// Projection, raster processing and dynamic rule evaluation are explicit M2 operations.
pub fn prepare_scene(
    composition: &MapComposition,
    view: &View,
    context: &OperationContext,
) -> Result<PreparedScene, GeelloError> {
    view.validate()?;
    context.check()?;
    let mut b = Builder {
        view,
        ctx: context,
        commands: vec![],
        reservations: vec![],
        #[cfg(feature = "text")]
        obstacles: vec![],
        #[cfg(feature = "text")]
        current_feature: None,
        summary: SceneSummary {
            width: view.width,
            height: view.height,
            command_count: 0,
            coordinate_count: 0,
            incompatible_components: 0,
            estimated_accounted_bytes: 0,
            diagnostics: vec![],
            omitted_diagnostics: 0,
        },
    };
    if let Some(color) = composition.background {
        let path: Arc<[PathElement]> = vec![
            PathElement::Move([0.0, 0.0]),
            PathElement::Line([view.width as f64, 0.0]),
            PathElement::Line([view.width as f64, view.height as f64]),
            PathElement::Line([0.0, view.height as f64]),
            PathElement::Close,
        ]
        .into();
        b.reserve(5 * std::mem::size_of::<PathElement>())?;
        b.command(Command::Path {
            path,
            color,
            opacity: 1.0,
            stroke: None,
        })?
    }
    for (item_index, item) in composition.items.iter().enumerate() {
        context.checkpoint(1)?;
        match item {
            DrawItem::Vector(v) => {
                for batch in &v.batches {
                    if batch.crs != view.crs {
                        return Err(GeelloError::ProjectionRequired);
                    }
                }
                for symbol in &v.symbols {
                    for batch in &v.batches {
                        for feature in &batch.features {
                            context.checkpoint(1)?;
                            if let Some(g) = &feature.geometry {
                                #[cfg(feature = "text")]
                                {
                                    b.current_feature = Some((item_index, feature.source_ordinal));
                                }
                                b.geometry(g, symbol, item_index, feature.source_ordinal, 0)?
                            }
                        }
                    }
                }
            }
            DrawItem::Image(i) => {
                if i.crs != view.crs {
                    return Err(GeelloError::ProjectionRequired);
                }
                i.bounds.validate()?;
                crate::style::opacity(i.opacity)?;
                let a = view.pixel(i.bounds.min_x, i.bounds.max_y)?;
                let z = view.pixel(i.bounds.max_x, i.bounds.min_y)?;
                if a[0] >= z[0] || a[1] >= z[1] {
                    return Err(GeelloError::InvalidRaster(
                        "positive image placement required".into(),
                    ));
                }
                if i.image.width() > u16::MAX as u32 || i.image.height() > u16::MAX as u32 {
                    return Err(GeelloError::DimensionOverflow);
                }
                b.reserve(i.image.pixels().len())?;
                b.command(Command::Image {
                    image: i.image.clone(),
                    rect: [a[0], a[1], z[0], z[1]],
                    opacity: i.opacity,
                })?
            }
            #[cfg(feature = "text")]
            DrawItem::Label(_) => {}
        }
    }
    #[cfg(feature = "text")]
    for label in crate::text::layout(composition, view, &b.obstacles, context)? {
        for hold in label.holds {
            b.summary.estimated_accounted_bytes = b
                .summary
                .estimated_accounted_bytes
                .checked_add(hold.bytes() as usize)
                .ok_or(GeelloError::SizeOverflow)?;
            b.reservations.push(hold);
        }
        b.command(Command::Path {
            path: label.path,
            color: label.color,
            opacity: 1.,
            stroke: None,
        })?;
    }
    b.summary.command_count = b.commands.len();
    context.check()?;
    Ok(PreparedScene {
        inner: Arc::new(SceneData {
            view: view.clone(),
            commands: b.commands,
            summary: b.summary,
            _reservations: b.reservations,
        }),
    })
}
