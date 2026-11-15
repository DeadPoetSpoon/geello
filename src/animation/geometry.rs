//! Source-plane whole-feature transforms, independent of projection and IO.
use super::*;
use crate::{Crs, FeatureId, VectorBatch};
use geo::MapCoords;
use geo_types::{Coord, Geometry, GeometryCollection};

/// Every anchor is explicit in original source coordinates. It follows all
/// earlier operations, including trajectory positioning and repeated transforms.
#[derive(Debug)]
pub enum GeometryAction {
    Translate(Accounted<Track>),
    Rotate {
        anchor: [f64; 2],
        track: Accounted<Track>,
    },
    Scale {
        anchor: [f64; 2],
        track: Accounted<Track>,
    },
}

/// Position an explicit original anchor along a source-plane polyline. Distance
/// mode uses cumulative Euclidean distance; timestamp mode uses caller times.
#[derive(Debug)]
pub struct Trajectory {
    anchor: [f64; 2],
    track: Accounted<Track>,
}
impl Trajectory {
    pub fn new(
        anchor: [f64; 2],
        points: &[[f64; 2]],
        times: Option<&[f64]>,
        duration: f64,
        interpolation: Interpolation,
        ctx: &OperationContext,
    ) -> Result<Self, GeelloError> {
        finite(anchor)?;
        if points.len() < 2 || times.is_some_and(|t| t.len() != points.len()) {
            return Err(invalid(
                "trajectory requires at least two points and matching times",
            ));
        }
        let _hold = ctx.reserve(
            points
                .len()
                .checked_mul(64)
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let mut frames = Vec::with_capacity(points.len());
        let mut distance = 0.;
        for (i, &point) in points.iter().enumerate() {
            ctx.checkpoint(1)?;
            finite(point)?;
            if i > 0 && times.is_none() {
                let length = (point[0] - points[i - 1][0]).hypot(point[1] - points[i - 1][1]);
                if !length.is_finite() {
                    return Err(invalid("trajectory length overflow"));
                }
                if length == 0. {
                    continue;
                }
                let next = distance + length;
                if !next.is_finite() || next <= distance {
                    return Err(invalid("trajectory distance cannot be represented"));
                }
                distance = next;
            }
            frames.push(Keyframe {
                time: times.map_or(distance, |t| t[i]),
                value: Value::Vector(point),
            });
        }
        if times.is_none() {
            if distance == 0. {
                return Err(invalid("distance trajectory has zero length"));
            }
            for frame in &mut frames {
                frame.time = frame.time / distance * duration;
            }
        }
        Ok(Self {
            anchor,
            track: Track::new(duration, interpolation, ValueKind::Vector, &frames, ctx)?,
        })
    }
}

/// Immutable ordered operations. Selection is typed, stable effective identity;
/// generated ordinal identities are deliberately rejected for refresh safety.
#[derive(Debug)]
pub struct GeometryBinding {
    crs: Crs,
    ids: Vec<FeatureId>,
    clock: BindingClock,
    trajectory: Option<Trajectory>,
    actions: Vec<GeometryAction>,
}
impl GeometryBinding {
    pub fn new(
        crs: Crs,
        ids: &[FeatureId],
        clock: BindingClock,
        trajectory: Option<Trajectory>,
        actions: Vec<GeometryAction>,
        max_features: usize,
        ctx: &OperationContext,
    ) -> Result<Accounted<Self>, GeelloError> {
        if ids.is_empty() || ids.len() > max_features {
            return Err(invalid(
                "geometry selection exceeds its nonzero feature limit",
            ));
        }
        if trajectory.is_none() && actions.is_empty() {
            return Err(invalid("empty geometry track"));
        }
        let bytes = ids.iter().try_fold(
            actions
                .len()
                .checked_mul(256)
                .and_then(|n| n.checked_add(crs.definition().len()))
                .and_then(|n| n.checked_add(std::mem::size_of::<Self>()))
                .ok_or(GeelloError::SizeOverflow)?,
            |n, id| {
                n.checked_add(128)
                    .and_then(|n| {
                        n.checked_add(match id {
                            FeatureId::String(s) => s.len(),
                            _ => 0,
                        })
                    })
                    .ok_or(GeelloError::SizeOverflow)
            },
        )?;
        let hold = ctx.reserve(bytes)?;
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            ctx.checkpoint(1)?;
            if matches!(id, FeatureId::String(s) if s.starts_with("generated:")) || !seen.insert(id)
            {
                return Err(invalid(
                    "geometry targets require unique stable effective IDs",
                ));
            }
        }
        for action in &actions {
            let (track, expected) = match action {
                GeometryAction::Translate(t) => (t, ValueKind::Vector),
                GeometryAction::Rotate { track, .. } => (track, ValueKind::Number),
                GeometryAction::Scale { track, .. } => (track, ValueKind::NonnegativeVector),
            };
            if track.kind != expected {
                return Err(invalid("incompatible geometry action track domain"));
            }
            if let GeometryAction::Rotate { anchor, .. } | GeometryAction::Scale { anchor, .. } =
                action
            {
                finite(*anchor)?;
            }
        }
        Ok(Accounted::new(
            Self {
                crs,
                ids: ids.to_vec(),
                clock,
                trajectory,
                actions,
            },
            vec![hold],
        ))
    }
    pub fn ids(&self) -> &[FeatureId] {
        &self.ids
    }
    pub fn crs(&self) -> &Crs {
        &self.crs
    }

    /// Always derive from original geometry. Properties, source ordinates and
    /// source identities are copied unchanged; no source or track is mutated.
    pub fn evaluate_batch(
        &self,
        batch: &VectorBatch,
        time: f64,
        ctx: &OperationContext,
    ) -> Result<Accounted<VectorBatch>, GeelloError> {
        validate_time(time)?;
        if batch.crs != self.crs {
            return Err(invalid("geometry binding source CRS differs from dataset"));
        }
        let mut matrix = Affine::IDENTITY;
        if let Some(path) = &self.trajectory
            && let Some(Value::Vector(point)) = path.track.evaluate(self.clock, time, ctx)?
        {
            matrix.then(Affine::translation([
                point[0] - path.anchor[0],
                point[1] - path.anchor[1],
            ]))?;
        }
        for action in &self.actions {
            ctx.checkpoint(1)?;
            match action {
                GeometryAction::Translate(track) => {
                    if let Some(value) = track.evaluate(self.clock, time, ctx)? {
                        let Value::Vector(v) = value else {
                            return Err(invalid("translation requires vector track"));
                        };
                        matrix.then(Affine::translation(v))?;
                    }
                }
                GeometryAction::Rotate { anchor, track } => {
                    if let Some(value) = track.evaluate(self.clock, time, ctx)? {
                        let Value::Number(angle) = value else {
                            return Err(invalid("rotation requires numeric track"));
                        };
                        let (s, c) = angle.rem_euclid(360.).to_radians().sin_cos();
                        let pivot = matrix.point(*anchor)?;
                        matrix.then(Affine::around(pivot, [c, -s, s, c]))?;
                    }
                }
                GeometryAction::Scale { anchor, track } => {
                    if let Some(value) = track.evaluate(self.clock, time, ctx)? {
                        let Value::Vector(v) = value else {
                            return Err(invalid("scale requires vector track"));
                        };
                        if v.iter().any(|n| *n < 0.) {
                            return Err(invalid("scale must be nonnegative"));
                        }
                        let pivot = matrix.point(*anchor)?;
                        matrix.then(Affine::around(pivot, [v[0], 0., 0., v[1]]))?;
                    }
                }
            }
        }
        let bytes = batch.features.iter().try_fold(256usize, |n, f| {
            n.checked_add(crate::core::feature_clone_bytes(f)?)
                .ok_or(GeelloError::SizeOverflow)
        })?;
        let hold = ctx.reserve(bytes.checked_mul(2).ok_or(GeelloError::SizeOverflow)?)?;
        let mut features = Vec::with_capacity(batch.features.len());
        for feature in &batch.features {
            ctx.checkpoint(self.ids.len() as u64 + 1)?;
            let mut out = feature.clone();
            if time >= self.clock.start
                && feature.id.as_ref().is_some_and(|id| self.ids.contains(id))
            {
                out.geometry = feature
                    .geometry
                    .as_ref()
                    .map(|g| transform_geometry(g, matrix, ctx))
                    .transpose()?;
            }
            features.push(out);
        }
        Ok(Accounted::new(
            VectorBatch {
                crs: batch.crs.clone(),
                features,
            },
            vec![hold],
        ))
    }
}

fn transform_geometry(
    g: &Geometry,
    matrix: Affine,
    ctx: &OperationContext,
) -> Result<Geometry, GeelloError> {
    ctx.checkpoint(1)?;
    match g {
        Geometry::Polygon(p) => ctx.checkpoint(p.interiors().len() as u64)?,
        Geometry::MultiLineString(lines) => ctx.checkpoint(lines.0.len() as u64)?,
        Geometry::MultiPolygon(polygons) => {
            ctx.checkpoint(polygons.0.len() as u64)?;
            for polygon in &polygons.0 {
                ctx.checkpoint(polygon.interiors().len() as u64)?;
            }
        }
        _ => {}
    }
    // A rotated rectangle is a polygon, never an axis-aligned rectangle made
    // from its transformed minimum and maximum corners.
    match g {
        Geometry::Rect(rect) => {
            transform_geometry(&Geometry::Polygon(rect.to_polygon()), matrix, ctx)
        }
        Geometry::GeometryCollection(collection) => {
            Ok(Geometry::GeometryCollection(GeometryCollection(
                collection
                    .iter()
                    .map(|g| transform_geometry(g, matrix, ctx))
                    .collect::<Result<Vec<_>, _>>()?,
            )))
        }
        _ => g.try_map_coords(|c: Coord<f64>| {
            ctx.checkpoint(1)?;
            let [x, y] = matrix.point([c.x, c.y])?;
            Ok::<_, GeelloError>(Coord { x, y })
        }),
    }
}

fn finite(v: [f64; 2]) -> Result<(), GeelloError> {
    if v.iter().any(|n| !n.is_finite()) {
        Err(invalid("nonfinite geometry coordinate or anchor"))
    } else {
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Affine([f64; 6]);
impl Affine {
    const IDENTITY: Self = Self([1., 0., 0., 1., 0., 0.]);
    fn translation(v: [f64; 2]) -> Self {
        Self([1., 0., 0., 1., v[0], v[1]])
    }
    fn around(p: [f64; 2], m: [f64; 4]) -> Self {
        Self([
            m[0],
            m[1],
            m[2],
            m[3],
            p[0] - m[0] * p[0] - m[1] * p[1],
            p[1] - m[2] * p[0] - m[3] * p[1],
        ])
    }
    fn point(self, p: [f64; 2]) -> Result<[f64; 2], GeelloError> {
        finite(p)?;
        let [a, b, c, d, x, y] = self.0;
        let out = [a * p[0] + b * p[1] + x, c * p[0] + d * p[1] + y];
        finite(out)?;
        Ok(out)
    }
    fn then(&mut self, next: Self) -> Result<(), GeelloError> {
        let [a, b, c, d, x, y] = self.0;
        let [e, f, g, h, u, v] = next.0;
        self.0 = [
            e * a + f * c,
            e * b + f * d,
            g * a + h * c,
            g * b + h * d,
            e * x + f * y + u,
            g * x + h * y + v,
        ];
        if self.0.iter().any(|n| !n.is_finite()) {
            return Err(invalid("geometry transform overflow"));
        }
        Ok(())
    }
}
