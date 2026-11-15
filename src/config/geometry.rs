//! TOML adapter for stable-ID geometry tracks. No IO or registry state is held.
use super::*;
use crate::animation::{
    BindingClock, GeometryAction, GeometryBinding, Interpolation, Repeat, Track, Trajectory, Value,
    ValueKind,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryBindingDefinition {
    pub id: String,
    pub animation: String,
    pub track: String,
    pub feature_ids: Vec<FeatureId>,
    pub max_features: usize,
    pub start: f64,
    pub speed: f64,
    pub repeat: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryTrackDefinition {
    pub id: String,
    pub source_crs: Crs,
    pub interpolation: String,
    pub trajectory: Option<TrajectoryDefinition>,
    #[serde(default)]
    pub transforms: Vec<TransformDefinition>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrajectoryDefinition {
    pub anchor: [f64; 2],
    pub points: Vec<[f64; 2]>,
    pub timing: String,
    pub times: Option<Vec<f64>>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum TransformDefinition {
    Translate {
        keyframes: Vec<Keyframe>,
    },
    Rotate {
        anchor: [f64; 2],
        keyframes: Vec<Keyframe>,
    },
    Scale {
        anchor: [f64; 2],
        keyframes: Vec<Keyframe>,
    },
}
fn interpolation(value: &str) -> Result<Interpolation, GeelloError> {
    match value {
        "step" => Ok(Interpolation::Step),
        "linear" => Ok(Interpolation::Linear),
        _ => Err(config_error("invalid geometry interpolation")),
    }
}
fn vector(value: &PropertyValue) -> Result<[f64; 2], GeelloError> {
    let PropertyValue::Array(values) = value else {
        return Err(config_error("XY numeric pair required"));
    };
    if values.len() != 2 {
        return Err(config_error("XY numeric pair required"));
    }
    Ok([number(&values[0])?, number(&values[1])?])
}
fn number(value: &PropertyValue) -> Result<f64, GeelloError> {
    let n = match value {
        PropertyValue::I64(n) => *n as f64,
        PropertyValue::U64(n) => *n as f64,
        PropertyValue::F64(n) => *n,
        _ => return Err(config_error("finite geometry number required")),
    };
    if !n.is_finite() {
        return Err(config_error("nonfinite geometry value"));
    }
    Ok(n)
}
fn finite(point: [f64; 2]) -> Result<(), GeelloError> {
    if point.iter().any(|n| !n.is_finite()) {
        return Err(config_error("nonfinite geometry point"));
    }
    Ok(())
}
pub(super) fn validate_geometry_bindings(item: &MapItem) -> Result<(), GeelloError> {
    unique(
        item.animation_bindings
            .iter()
            .map(|b| b.id.as_str())
            .chain(item.geometry_bindings.iter().map(|b| b.id.as_str())),
    )?;
    let mut targets = HashSet::new();
    for binding in &item.geometry_bindings {
        if !valid_resource_id(&binding.animation)
            || binding.track.is_empty()
            || binding.feature_ids.is_empty()
            || binding.feature_ids.len() > binding.max_features
        {
            return Err(config_error(
                "invalid geometry binding identity or feature limit",
            ));
        }
        clock(binding)?;
        for id in &binding.feature_ids {
            if matches!(id, FeatureId::String(s) if s.starts_with("generated:"))
                || !targets.insert(id)
            {
                return Err(config_error(
                    "geometry targets require unique stable IDs and one ordered track per feature",
                ));
            }
        }
    }
    Ok(())
}
fn clock(binding: &GeometryBindingDefinition) -> Result<BindingClock, GeelloError> {
    let repeat = match binding.repeat.as_str() {
        "once" => Repeat::Once,
        "loop" => Repeat::Loop,
        _ => return Err(config_error("invalid geometry repeat mode")),
    };
    BindingClock::new(binding.start, binding.speed, repeat)
}
pub(super) fn validate_geometry_track(
    track: &GeometryTrackDefinition,
    duration: f64,
) -> Result<(), GeelloError> {
    Crs::new(track.source_crs.definition(), track.source_crs.space())?;
    interpolation(&track.interpolation)?;
    if track.trajectory.is_none() && track.transforms.is_empty() {
        return Err(config_error("empty geometry track"));
    }
    if let Some(path) = &track.trajectory {
        finite(path.anchor)?;
        if path.points.len() < 2 {
            return Err(config_error("trajectory requires at least two points"));
        }
        for point in &path.points {
            finite(*point)?;
        }
        match (path.timing.as_str(), &path.times) {
            ("distance", None) => {
                let length = path.points.windows(2).try_fold(0., |sum, p| {
                    let next = sum + (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]);
                    if !next.is_finite() {
                        return Err(config_error("trajectory length overflow"));
                    }
                    Ok(next)
                })?;
                if length == 0. {
                    return Err(config_error("distance trajectory has zero length"));
                }
            }
            ("timestamps", Some(times)) if times.len() == path.points.len() => {
                validate_times(times.iter().copied(), duration)?;
            }
            _ => {
                return Err(config_error(
                    "trajectory timing must be distance without times or timestamps with matching times",
                ));
            }
        }
    }
    for transform in &track.transforms {
        let (frames, rotation, scale) = match transform {
            TransformDefinition::Translate { keyframes } => (keyframes, false, false),
            TransformDefinition::Rotate { anchor, keyframes } => {
                finite(*anchor)?;
                (keyframes, true, false)
            }
            TransformDefinition::Scale { anchor, keyframes } => {
                finite(*anchor)?;
                (keyframes, false, true)
            }
        };
        validate_times(frames.iter().map(|f| f.time), duration)?;
        for frame in frames {
            if rotation {
                number(&frame.value)?;
            } else {
                let v = vector(&frame.value)?;
                if scale && v.iter().any(|n| *n < 0.) {
                    return Err(config_error("scale must be nonnegative"));
                }
            }
        }
    }
    Ok(())
}
fn validate_times(times: impl Iterator<Item = f64>, duration: f64) -> Result<(), GeelloError> {
    let mut last = None;
    for time in times {
        if !time.is_finite() || time < 0. || time > duration || last.is_some_and(|t| time <= t) {
            return Err(config_error(
                "geometry times must strictly increase within duration",
            ));
        }
        last = Some(time);
    }
    if last.is_none() {
        return Err(config_error("geometry keyframes required"));
    }
    Ok(())
}

/// Prepare against the actual source CRS, so incompatible dataset refreshes
/// fail before publication. Missing stable IDs are diagnosed during rendering.
pub fn prepare_geometry_animations(
    item: &MapItem,
    animations: &BTreeMap<String, AnimationDefinition>,
    source_crs: &Crs,
    ctx: &OperationContext,
) -> Result<Accounted<Vec<Accounted<GeometryBinding>>>, GeelloError> {
    let hold = ctx.reserve(item.geometry_bindings.iter().try_fold(0usize, |n, b| {
        b.feature_ids.iter().try_fold(
            n.checked_add(512).ok_or(GeelloError::SizeOverflow)?,
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
        )
    })?)?;
    validate_geometry_bindings(item)?;
    let mut output = Vec::with_capacity(item.geometry_bindings.len());
    for binding in &item.geometry_bindings {
        ctx.checkpoint(1)?;
        let definition = animations
            .get(&binding.animation)
            .filter(|a| a.id == binding.animation)
            .ok_or_else(|| config_error("missing geometry animation"))?;
        let track = definition
            .geometry_tracks
            .iter()
            .find(|t| t.id == binding.track)
            .ok_or_else(|| config_error("missing geometry track"))?;
        validate_geometry_track(track, definition.duration)?;
        if &track.source_crs != source_crs {
            return Err(config_error("geometry track and dataset source CRS differ"));
        }
        let interpolation = interpolation(&track.interpolation)?;
        let trajectory = track
            .trajectory
            .as_ref()
            .map(|path| {
                Trajectory::new(
                    path.anchor,
                    &path.points,
                    path.times.as_deref(),
                    definition.duration,
                    interpolation,
                    ctx,
                )
            })
            .transpose()?;
        let _temporary = ctx.reserve(
            track
                .transforms
                .len()
                .checked_mul(256)
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let mut actions = Vec::with_capacity(track.transforms.len());
        for transform in &track.transforms {
            ctx.checkpoint(1)?;
            let (frames, kind) = match transform {
                TransformDefinition::Translate { keyframes } => (keyframes, ValueKind::Vector),
                TransformDefinition::Rotate { keyframes, .. } => (keyframes, ValueKind::Number),
                TransformDefinition::Scale { keyframes, .. } => {
                    (keyframes, ValueKind::NonnegativeVector)
                }
            };
            let _frames_hold = ctx.reserve(
                frames
                    .len()
                    .checked_mul(64)
                    .ok_or(GeelloError::SizeOverflow)?,
            )?;
            let frames = frames
                .iter()
                .map(|frame| {
                    ctx.checkpoint(1)?;
                    Ok(crate::animation::Keyframe {
                        time: frame.time,
                        value: if kind == ValueKind::Number {
                            Value::Number(number(&frame.value)?)
                        } else {
                            Value::Vector(vector(&frame.value)?)
                        },
                    })
                })
                .collect::<Result<Vec<_>, GeelloError>>()?;
            let prepared = Track::new(definition.duration, interpolation, kind, &frames, ctx)?;
            actions.push(match transform {
                TransformDefinition::Translate { .. } => GeometryAction::Translate(prepared),
                TransformDefinition::Rotate { anchor, .. } => GeometryAction::Rotate {
                    anchor: *anchor,
                    track: prepared,
                },
                TransformDefinition::Scale { anchor, .. } => GeometryAction::Scale {
                    anchor: *anchor,
                    track: prepared,
                },
            });
        }
        output.push(GeometryBinding::new(
            source_crs.clone(),
            &binding.feature_ids,
            clock(binding)?,
            trajectory,
            actions,
            binding.max_features,
            ctx,
        )?);
    }
    Ok(Accounted::new(output, vec![hold]))
}
