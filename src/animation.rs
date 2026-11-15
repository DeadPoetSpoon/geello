//! Stateless, memory-only keyframe evaluation. Times are finite seconds; colors
//! use linear-light premultiplied interpolation, independently of symbol opacity.
use crate::{Accounted, Color, GeelloError, Length, OperationContext, Symbol};
mod geometry;
pub use geometry::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Step,
    Linear,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Once,
    Loop,
}
/// Scalar domains are validated at preparation, including step-only tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Number,
    Nonnegative,
    Opacity,
    Color,
    Vector,
    NonnegativeVector,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Number(f64),
    Color(Color),
    Vector([f64; 2]),
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    pub time: f64,
    pub value: Value,
}
/// Immutable validated track, with its own finite positive duration. Endpoint
/// values hold outside the track's keyframe range within that duration.
#[derive(Debug)]
pub struct Track {
    duration: f64,
    kind: ValueKind,
    interpolation: Interpolation,
    frames: Vec<Keyframe>,
}
impl Track {
    /// Copies caller-owned frames only after admitting the retained allocation.
    /// Keep the returned accounting wrapper alive while using the track.
    pub fn new(
        duration: f64,
        interpolation: Interpolation,
        kind: ValueKind,
        frames: &[Keyframe],
        ctx: &OperationContext,
    ) -> Result<Accounted<Self>, GeelloError> {
        if !duration.is_finite() || duration <= 0. || frames.is_empty() {
            return Err(invalid(
                "positive duration and at least one keyframe required",
            ));
        }
        let hold = ctx.reserve(
            frames
                .len()
                .checked_mul(std::mem::size_of::<Keyframe>())
                .and_then(|n| n.checked_add(std::mem::size_of::<Self>()))
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let mut last = None;
        for frame in frames {
            ctx.checkpoint(1)?;
            if !frame.time.is_finite()
                || frame.time < 0.
                || frame.time > duration
                || last.is_some_and(|t| frame.time <= t)
            {
                return Err(invalid(
                    "keyframe times must strictly increase within duration",
                ));
            }
            match (kind, frame.value) {
                (ValueKind::Color, Value::Color(_)) => {}
                (ValueKind::Vector | ValueKind::NonnegativeVector, Value::Vector(v))
                    if v.iter().all(|n| {
                        n.is_finite() && (kind != ValueKind::NonnegativeVector || *n >= 0.)
                    }) => {}
                (
                    ValueKind::Number | ValueKind::Nonnegative | ValueKind::Opacity,
                    Value::Number(n),
                ) if n.is_finite()
                    && (kind != ValueKind::Nonnegative || n >= 0.)
                    && (kind != ValueKind::Opacity || (0.0..=1.0).contains(&n)) => {}
                _ => return Err(invalid("keyframe value has incompatible type or range")),
            }
            last = Some(frame.time);
        }
        Ok(Accounted::new(
            Self {
                duration,
                kind,
                interpolation,
                frames: frames.to_vec(),
            },
            vec![hold],
        ))
    }

    /// None means the binding has not started: preserve the evaluated base
    /// property. Calling in any order never mutates the track or source data.
    pub fn evaluate(
        &self,
        binding: BindingClock,
        map_time: f64,
        ctx: &OperationContext,
    ) -> Result<Option<Value>, GeelloError> {
        ctx.check()?;
        let Some(time) = binding.local_time(map_time, self.duration)? else {
            return Ok(None);
        };
        // Bounded binary search keeps evaluation independent of frame history.
        ctx.checkpoint(u64::from(self.frames.len().ilog2()) + 1)?;
        let right = self.frames.partition_point(|frame| frame.time <= time);
        let value = if right == 0 {
            self.frames[0].value
        } else if right == self.frames.len() || self.interpolation == Interpolation::Step {
            self.frames[right - 1].value
        } else {
            let a = self.frames[right - 1];
            let b = self.frames[right];
            let t = (time - a.time) / (b.time - a.time);
            interpolate(a.value, b.value, t)?
        };
        Ok(Some(canonical(value)))
    }
}

/// Map binding clock; it has no playback state and never resets the map clock.
#[derive(Debug, Clone, Copy)]
pub struct BindingClock {
    start: f64,
    speed: f64,
    repeat: Repeat,
}

/// Only value targets are animated: font selection and text layout stay static.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolParameter {
    Size,
    Angle,
    Width,
    FillColor,
    StrokeColor,
    Opacity,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    Pixels,
    MapUnits,
}
/// A prepared style override; evaluate base property expressions and filters
/// first, then apply this value. No source property or geometry is modified.
#[derive(Debug)]
pub struct SymbolTrack {
    parameter: SymbolParameter,
    unit: Option<LengthUnit>,
    clock: BindingClock,
    track: Accounted<Track>,
}
impl SymbolTrack {
    pub fn new(
        parameter: SymbolParameter,
        unit: Option<LengthUnit>,
        clock: BindingClock,
        duration: f64,
        interpolation: Interpolation,
        frames: &[Keyframe],
        ctx: &OperationContext,
    ) -> Result<Self, GeelloError> {
        if matches!(parameter, SymbolParameter::Size | SymbolParameter::Width) != unit.is_some() {
            return Err(invalid(
                "length tracks require a unit; other targets must omit length units",
            ));
        }
        let kind = match parameter {
            SymbolParameter::Size | SymbolParameter::Width => ValueKind::Nonnegative,
            SymbolParameter::Angle => ValueKind::Number,
            SymbolParameter::FillColor | SymbolParameter::StrokeColor => ValueKind::Color,
            SymbolParameter::Opacity => ValueKind::Opacity,
        };
        Ok(Self {
            parameter,
            unit,
            clock,
            track: Track::new(duration, interpolation, kind, frames, ctx)?,
        })
    }
    pub fn parameter(&self) -> SymbolParameter {
        self.parameter
    }

    /// Validate even before the binding starts; incompatible targets are not
    /// hidden until a later frame. Applying the same frame twice replaces values.
    pub fn apply(
        &self,
        symbol: &mut Symbol,
        time: f64,
        ctx: &OperationContext,
    ) -> Result<(), GeelloError> {
        self.validate_target(symbol)?;
        let Some(value) = self.track.evaluate(self.clock, time, ctx)? else {
            return Ok(());
        };
        let length = |n| match self.unit {
            Some(LengthUnit::Pixels) => Length::Pixels(n),
            Some(LengthUnit::MapUnits) => Length::MapUnits(n),
            None => unreachable!("validated length unit"),
        };
        match (self.parameter, value, symbol) {
            (SymbolParameter::Size, Value::Number(n), Symbol::Point(p)) => p.size = length(n),
            (SymbolParameter::Angle, Value::Number(n), Symbol::Point(p)) => p.angle_degrees = n,
            (SymbolParameter::Width, Value::Number(n), Symbol::Point(p)) => {
                p.stroke.as_mut().unwrap().width = length(n)
            }
            (
                SymbolParameter::Width,
                Value::Number(n),
                Symbol::Line(s) | Symbol::PolygonOutline(s),
            ) => s.width = length(n),
            (SymbolParameter::FillColor, Value::Color(c), Symbol::Point(p)) => p.fill = Some(c),
            (SymbolParameter::FillColor, Value::Color(c), Symbol::PolygonFill { color, .. }) => {
                *color = c
            }
            (SymbolParameter::StrokeColor, Value::Color(c), Symbol::Point(p)) => {
                p.stroke.as_mut().unwrap().color = c
            }
            (
                SymbolParameter::StrokeColor,
                Value::Color(c),
                Symbol::Line(s) | Symbol::PolygonOutline(s),
            ) => s.color = c,
            (SymbolParameter::Opacity, Value::Number(n), Symbol::Point(p)) => p.opacity = n,
            (
                SymbolParameter::Opacity,
                Value::Number(n),
                Symbol::Line(s) | Symbol::PolygonOutline(s),
            ) => s.opacity = n,
            (SymbolParameter::Opacity, Value::Number(n), Symbol::PolygonFill { opacity, .. }) => {
                *opacity = n
            }
            _ => unreachable!("validated track domain and symbol target"),
        }
        Ok(())
    }
    pub fn validate_target(&self, symbol: &Symbol) -> Result<(), GeelloError> {
        let valid = match (self.parameter, symbol) {
            (SymbolParameter::Opacity, _) => true,
            (SymbolParameter::Size | SymbolParameter::Angle, Symbol::Point(_)) => true,
            (SymbolParameter::FillColor, Symbol::Point(p)) => p.fill.is_some(),
            (SymbolParameter::FillColor, Symbol::PolygonFill { .. }) => true,
            (SymbolParameter::Width | SymbolParameter::StrokeColor, Symbol::Point(p)) => {
                p.stroke.is_some()
            }
            (
                SymbolParameter::Width | SymbolParameter::StrokeColor,
                Symbol::Line(_) | Symbol::PolygonOutline(_),
            ) => true,
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(invalid("animation target is absent or incompatible"))
        }
    }
    /// Raster animation is restricted to opacity and remains separate from
    /// numeric resampling/colorization. The base value survives before start.
    pub fn raster_opacity(
        &self,
        base: f64,
        time: f64,
        ctx: &OperationContext,
    ) -> Result<f64, GeelloError> {
        crate::style::opacity(base)?;
        if self.parameter != SymbolParameter::Opacity {
            return Err(invalid("raster animation only supports opacity"));
        }
        match self.track.evaluate(self.clock, time, ctx)? {
            Some(Value::Number(n)) => Ok(n),
            None => Ok(base),
            _ => unreachable!("validated opacity domain"),
        }
    }
}
impl BindingClock {
    pub fn new(start: f64, speed: f64, repeat: Repeat) -> Result<Self, GeelloError> {
        if !start.is_finite() || start < 0. || !speed.is_finite() || speed <= 0. {
            return Err(invalid(
                "nonnegative start and positive finite speed required",
            ));
        }
        Ok(Self {
            start,
            speed,
            repeat,
        })
    }
    fn local_time(self, time: f64, duration: f64) -> Result<Option<f64>, GeelloError> {
        validate_time(time)?;
        if time < self.start {
            return Ok(None);
        }
        let elapsed = (time - self.start) * self.speed;
        let local = match self.repeat {
            Repeat::Once => elapsed.min(duration),
            Repeat::Loop if elapsed.is_finite() => elapsed.rem_euclid(duration),
            Repeat::Loop => return Err(invalid("loop clock overflow")),
        };
        Ok(Some(local))
    }
}

pub fn validate_time(time: f64) -> Result<(), GeelloError> {
    if !time.is_finite() || time < 0. {
        return Err(invalid("animation time must be finite nonnegative seconds"));
    }
    Ok(())
}
fn canonical(value: Value) -> Value {
    match value {
        Value::Color(Color([_, _, _, 0])) => Value::Color(Color::TRANSPARENT),
        _ => value,
    }
}
fn interpolate(a: Value, b: Value, t: f64) -> Result<Value, GeelloError> {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            // Avoid overflowing b-a for opposite, individually finite endpoints.
            let n = a * (1. - t) + b * t;
            if !n.is_finite() {
                return Err(invalid("interpolated number is not finite"));
            }
            Ok(Value::Number(n))
        }
        (Value::Vector(a), Value::Vector(b)) => {
            let out = [a[0] * (1. - t) + b[0] * t, a[1] * (1. - t) + b[1] * t];
            if out.iter().any(|n| !n.is_finite()) {
                return Err(invalid("interpolated vector is not finite"));
            }
            Ok(Value::Vector(out))
        }
        (Value::Color(Color(a)), Value::Color(Color(b))) => {
            let aa = a[3] as f64 / 255.;
            let ba = b[3] as f64 / 255.;
            let alpha = aa * (1. - t) + ba * t;
            let mut out = [0; 4];
            out[3] = (alpha * 255.).round() as u8;
            if out[3] != 0 {
                for i in 0..3 {
                    let linear = (decode(a[i]) * aa * (1. - t) + decode(b[i]) * ba * t) / alpha;
                    out[i] = encode(linear);
                }
            }
            Ok(Value::Color(Color(out)))
        }
        _ => Err(invalid("incompatible interpolation endpoints")),
    }
}
fn decode(channel: u8) -> f64 {
    let s = channel as f64 / 255.;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}
fn encode(linear: f64) -> u8 {
    let s = if linear <= 0.0031308 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1. / 2.4) - 0.055
    };
    (s.clamp(0., 1.) * 255.).round() as u8
}
fn invalid(message: &str) -> GeelloError {
    GeelloError::InvalidConfiguration(message.into())
}
