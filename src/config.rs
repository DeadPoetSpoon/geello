//! Strict resource schemas and preparation-time property evaluation.
use crate::query::{Filter, property};
use crate::*;
mod geometry;
pub use geometry::*;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

fn version() -> u32 {
    1
}
fn one() -> f64 {
    1.
}
fn four() -> f64 {
    4.
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDefinition {
    pub kind: String,
    pub path: PathBuf,
    pub encoding: Option<String>,
    pub crs: Option<String>,
    pub sample_kind: Option<String>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryableDefinition {
    pub name: String,
    pub field: String,
    #[serde(rename = "type")]
    pub kind: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityDefinition {
    pub kind: String,
    pub field: Option<String>,
    #[serde(default)]
    pub stable: bool,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetDefinition {
    #[serde(default = "version")]
    pub schema_version: u32,
    pub id: String,
    pub title: Option<String>,
    pub source: SourceDefinition,
    pub identity: Option<IdentityDefinition>,
    #[serde(default)]
    pub queryables: Vec<QueryableDefinition>,
    #[serde(default)]
    pub publish: BTreeMap<String, bool>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StyleDefinition {
    pub schema_version: u32,
    pub id: String,
    pub rules: Vec<StyleRule>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StyleRule {
    pub id: String,
    pub filter: Option<Filter>,
    pub symbols: Vec<SymbolDefinition>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unit {
    Px,
    Map,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericExpression {
    pub value: Option<f64>,
    pub field: Option<String>,
    pub fallback: Option<f64>,
    pub unit: Option<Unit>,
}
impl NumericExpression {
    pub fn evaluate(&self, p: &Properties) -> Result<f64, GeelloError> {
        let value = match (&self.value, &self.field) {
            (Some(v), None) => Some(*v),
            (None, Some(field)) => match property(p, field)? {
                Some(PropertyValue::I64(v)) => Some(*v as f64),
                Some(PropertyValue::U64(v)) => Some(*v as f64),
                Some(PropertyValue::F64(v)) => Some(*v),
                _ => self.fallback,
            },
            _ => {
                return Err(config_error(
                    "numeric expression needs exactly one value or field",
                ));
            }
        };
        value
            .filter(|v| v.is_finite())
            .ok_or_else(|| config_error("numeric expression has no finite result/fallback"))
    }
    pub fn length(&self, p: &Properties) -> Result<Length, GeelloError> {
        let n = self.evaluate(p)?;
        if n < 0. {
            return Err(config_error("negative length"));
        }
        Ok(
            match self
                .unit
                .ok_or_else(|| config_error("length unit required"))?
            {
                Unit::Px => Length::Pixels(n),
                Unit::Map => Length::MapUnits(n),
            },
        )
    }
}
fn default_size() -> NumericExpression {
    NumericExpression {
        value: Some(6.),
        field: None,
        fallback: None,
        unit: Some(Unit::Px),
    }
}
fn default_width() -> NumericExpression {
    NumericExpression {
        value: Some(1.),
        field: None,
        fallback: None,
        unit: Some(Unit::Px),
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeDefinition {
    #[serde(default = "default_width")]
    pub width: NumericExpression,
    pub color: Option<String>,
    #[serde(default = "one")]
    pub opacity: f64,
    pub cap: Option<String>,
    pub join: Option<String>,
    #[serde(default = "four")]
    pub miter_limit: f64,
    #[serde(default)]
    pub dashes: Vec<f64>,
    #[serde(default)]
    pub dash_offset: f64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SymbolDefinition {
    #[cfg(feature = "text")]
    Label {
        id: String,
        field: Option<String>,
        text: Option<String>,
        fonts: Vec<FontDefinition>,
        size: NumericExpression,
        color: Option<String>,
        language: Option<String>,
        placement: String,
        #[serde(default)]
        priority: f64,
        #[serde(default)]
        allow_overlap: bool,
        wrap_width: Option<f64>,
        #[serde(default = "label_repeat")]
        repeat_distance: f64,
    },
    Point {
        id: String,
        shape: Option<String>,
        #[serde(default = "default_size")]
        size: NumericExpression,
        fill: Option<String>,
        stroke: Option<StrokeDefinition>,
        #[serde(default = "one")]
        opacity: f64,
        #[serde(default)]
        angle: f64,
    },
    Line {
        id: String,
        #[serde(default = "default_width")]
        width: NumericExpression,
        color: Option<String>,
        cap: Option<String>,
        join: Option<String>,
        #[serde(default = "four")]
        miter_limit: f64,
        #[serde(default)]
        dashes: Vec<f64>,
        #[serde(default)]
        dash_offset: f64,
        #[serde(default = "one")]
        opacity: f64,
        rings: Option<String>,
    },
    Area {
        id: String,
        fill: Option<String>,
        #[serde(default = "one")]
        opacity: f64,
    },
    Raster {
        id: String,
        mode: String,
        band: Option<usize>,
        bands: Option<Vec<usize>>,
        range: RangeDefinition,
        resampling: ResamplingDefinition,
        #[serde(default = "one")]
        opacity: f64,
    },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RangeDefinition {
    DatasetMinmax,
    Explicit { min: f64, max: f64 },
}
#[cfg(feature = "text")]
fn label_repeat() -> f64 {
    300.
}
#[cfg(feature = "text")]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontDefinition {
    pub resource: String,
    #[serde(default)]
    pub face_index: u32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResamplingDefinition {
    pub enlarge: String,
    pub reduce: String,
}
impl SymbolDefinition {
    pub fn id(&self) -> &str {
        match self {
            #[cfg(feature = "text")]
            Self::Label { id, .. } => id,
            Self::Point { id, .. }
            | Self::Line { id, .. }
            | Self::Area { id, .. }
            | Self::Raster { id, .. } => id,
        }
    }
    /// Translate one-based configuration bands once; caller supplies immutable
    /// version-wide ranges for automatic stretch, never viewport-local ranges.
    pub fn raster_options(
        &self,
        version_ranges: &[Option<crate::raster_processing::SampleRange>],
    ) -> Result<crate::raster_processing::RasterColorOptions, GeelloError> {
        validate_symbol(self)?;
        let Self::Raster {
            band,
            bands,
            range,
            opacity,
            ..
        } = self
        else {
            return Err(config_error("raster symbol required"));
        };
        let bands = band
            .map(|b| vec![b])
            .unwrap_or_else(|| bands.clone().unwrap())
            .into_iter()
            .map(|b| {
                b.checked_sub(1)
                    .ok_or_else(|| config_error("configuration bands are one-based"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ranges = bands
            .iter()
            .map(|b| {
                match range {
            RangeDefinition::Explicit { min, max } => {
                Ok(crate::raster_processing::SampleRange { min: *min, max: *max })
            }
            RangeDefinition::DatasetMinmax => version_ranges.get(*b).copied().flatten()
                .ok_or_else(|| config_error(
                    "version-wide raster range required; all-invalid source needs explicit range",
                )),
        }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(crate::raster_processing::RasterColorOptions {
            bands,
            ranges,
            opacity: *opacity,
        })
    }
    pub fn resolve(&self, p: &Properties) -> Result<Symbol, GeelloError> {
        Ok(match self {
            Self::Point {
                shape,
                size,
                fill,
                stroke,
                opacity: alpha,
                angle,
                ..
            } => {
                opacity(*alpha)?;
                Symbol::Point(PointStyle {
                    shape: match shape.as_deref().unwrap_or("circle") {
                        "circle" => MarkerShape::Circle,
                        "square" => MarkerShape::Square,
                        "triangle" => MarkerShape::Triangle,
                        _ => return Err(config_error("unknown marker shape")),
                    },
                    size: size.length(p)?,
                    angle_degrees: *angle,
                    fill: Some(parse_color(fill.as_deref().unwrap_or("#0000FF"))?),
                    stroke: stroke.as_ref().map(|s| resolve_stroke(s, p)).transpose()?,
                    opacity: *alpha,
                })
            }
            Self::Line {
                width,
                color,
                cap,
                join,
                miter_limit,
                dashes,
                dash_offset,
                opacity: alpha,
                rings,
                ..
            } => {
                let stroke = resolve_stroke(
                    &StrokeDefinition {
                        width: width.clone(),
                        color: color.clone(),
                        cap: cap.clone(),
                        join: join.clone(),
                        miter_limit: *miter_limit,
                        dashes: dashes.clone(),
                        dash_offset: *dash_offset,
                        opacity: *alpha,
                    },
                    p,
                )?;
                if rings.is_some() {
                    if rings.as_deref() != Some("all") {
                        return Err(GeelloError::Unsupported(
                            "only all-ring outline currently validated".into(),
                        ));
                    }
                    Symbol::PolygonOutline(stroke)
                } else {
                    Symbol::Line(stroke)
                }
            }
            Self::Area {
                fill,
                opacity: alpha,
                ..
            } => {
                opacity(*alpha)?;
                Symbol::PolygonFill {
                    color: parse_color(fill.as_deref().unwrap_or("#808080"))?,
                    opacity: *alpha,
                }
            }
            Self::Raster { .. } => {
                return Err(config_error("raster symbol requires raster preparation"));
            }
            #[cfg(feature = "text")]
            Self::Label { .. } => {
                return Err(config_error("label requires explicit font resolution"));
            }
        })
    }
}
fn resolve_stroke(s: &StrokeDefinition, p: &Properties) -> Result<StrokeStyle, GeelloError> {
    opacity(s.opacity)?;
    Ok(StrokeStyle {
        color: parse_color(s.color.as_deref().unwrap_or("#000000"))?,
        width: s.width.length(p)?,
        opacity: s.opacity,
        cap: match s.cap.as_deref().unwrap_or("butt") {
            "butt" => LineCap::Butt,
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => return Err(config_error("unknown cap")),
        },
        join: match s.join.as_deref().unwrap_or("miter") {
            "miter" => LineJoin::Miter,
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => return Err(config_error("unknown join")),
        },
        miter_limit: s.miter_limit,
        dashes: s.dashes.clone(),
        dash_offset: s.dash_offset,
    })
}
pub fn parse_color(text: &str) -> Result<Color, GeelloError> {
    let s = text
        .strip_prefix('#')
        .ok_or_else(|| config_error("hex color required"))?;
    if !matches!(s.len(), 6 | 8) || !s.is_ascii() {
        return Err(config_error("color must be #RRGGBB or #RRGGBBAA"));
    }
    let mut color = [0, 0, 0, 255];
    for (i, v) in s.as_bytes().chunks_exact(2).enumerate() {
        color[i] = u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16)
            .map_err(|_| config_error("invalid hex color"))?;
    }
    Ok(Color(color))
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapDefinition {
    pub schema_version: u32,
    pub id: String,
    pub title: Option<String>,
    pub language: Option<String>,
    pub projection_backend: Option<String>,
    pub output_crs: Vec<String>,
    pub background: Option<String>,
    pub tiles: Option<TileDefinition>,
    pub items: Vec<MapItem>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TileDefinition {
    pub matrix_sets: Vec<String>,
    pub min_zoom: u32,
    pub max_zoom: u32,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapItem {
    pub id: String,
    pub dataset: String,
    pub style: String,
    #[serde(default)]
    pub animation_bindings: Vec<AnimationBinding>,
    #[serde(default)]
    pub geometry_bindings: Vec<GeometryBindingDefinition>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationBinding {
    pub id: String,
    pub animation: String,
    pub track: String,
    pub target: AnimationTarget,
    pub start: f64,
    pub speed: f64,
    pub repeat: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationTarget {
    pub rule: String,
    pub symbol: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationDefinition {
    pub schema_version: u32,
    pub id: String,
    pub duration: f64,
    #[serde(default)]
    pub tracks: Vec<AnimationTrack>,
    #[serde(default)]
    pub geometry_tracks: Vec<GeometryTrackDefinition>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationTrack {
    pub id: String,
    pub parameter: String,
    pub interpolation: String,
    pub unit: Option<Unit>,
    pub keyframes: Vec<Keyframe>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub time: f64,
    pub value: PropertyValue,
}
/// Prepared binding owns its target identity and memory-only evaluator. Keep the
/// accounting returned by `prepare_style_animations` while borrowing bindings.
#[derive(Debug)]
pub struct PreparedStyleBinding {
    pub rule: String,
    pub symbol: String,
    pub track: crate::animation::SymbolTrack,
}
impl AnimationTrack {
    /// Compile typed values and units once, before per-feature style evaluation.
    pub fn prepare(
        &self,
        duration: f64,
        clock: crate::animation::BindingClock,
        ctx: &OperationContext,
    ) -> Result<crate::animation::SymbolTrack, GeelloError> {
        use crate::animation::{Interpolation, LengthUnit, SymbolParameter, SymbolTrack, Value};
        let parameter = match self.parameter.as_str() {
            "size" => SymbolParameter::Size,
            "angle" => SymbolParameter::Angle,
            "width" => SymbolParameter::Width,
            "fill" => SymbolParameter::FillColor,
            "stroke-color" => SymbolParameter::StrokeColor,
            "opacity" => SymbolParameter::Opacity,
            _ => return Err(config_error("unsupported style animation parameter")),
        };
        let interpolation = match self.interpolation.as_str() {
            "step" => Interpolation::Step,
            "linear" => Interpolation::Linear,
            _ => return Err(config_error("unknown animation interpolation")),
        };
        let _temporary = ctx.reserve(
            self.keyframes
                .len()
                .checked_mul(std::mem::size_of::<crate::animation::Keyframe>())
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let frames = self
            .keyframes
            .iter()
            .map(|frame| {
                ctx.checkpoint(1)?;
                let value = match (&frame.value, parameter) {
                    (
                        PropertyValue::String(c),
                        SymbolParameter::FillColor | SymbolParameter::StrokeColor,
                    ) => Value::Color(parse_color(c)?),
                    (PropertyValue::I64(n), _) => Value::Number(*n as f64),
                    (PropertyValue::U64(n), _) => Value::Number(*n as f64),
                    (PropertyValue::F64(n), _) => Value::Number(*n),
                    _ => return Err(config_error("animation value has incompatible type")),
                };
                Ok(crate::animation::Keyframe {
                    time: frame.time,
                    value,
                })
            })
            .collect::<Result<Vec<_>, GeelloError>>()?;
        SymbolTrack::new(
            parameter,
            self.unit.map(|u| match u {
                Unit::Px => LengthUnit::Pixels,
                Unit::Map => LengthUnit::MapUnits,
            }),
            clock,
            duration,
            interpolation,
            &frames,
            ctx,
        )
    }
}

/// Resolve every target, including inactive bindings, and reject same-target
/// writers before rendering. Geometry bindings are a separate M4 contract.
pub fn prepare_style_animations(
    style: &StyleDefinition,
    item: &MapItem,
    animations: &BTreeMap<String, AnimationDefinition>,
    ctx: &OperationContext,
) -> Result<Accounted<Vec<PreparedStyleBinding>>, GeelloError> {
    use crate::animation::{BindingClock, Repeat, SymbolParameter};
    let bytes = item.animation_bindings.iter().try_fold(0usize, |n, b| {
        n.checked_add(512)
            .and_then(|n| n.checked_add(b.target.rule.len()))
            .and_then(|n| n.checked_add(b.target.symbol.len()))
            .and_then(|n| n.checked_add(b.id.len()))
            .ok_or(GeelloError::SizeOverflow)
    })?;
    let hold = ctx.reserve(bytes)?;
    unique(item.animation_bindings.iter().map(|b| b.id.as_str()))?;
    let mut seen = HashSet::new();
    let mut output = Vec::with_capacity(item.animation_bindings.len());
    for binding in &item.animation_bindings {
        ctx.checkpoint(1)?;
        let definition = animations
            .get(&binding.animation)
            .filter(|a| a.id == binding.animation)
            .ok_or_else(|| config_error("missing animation or mismatched resource identity"))?;
        let source_track = definition
            .tracks
            .iter()
            .find(|t| t.id == binding.track)
            .ok_or_else(|| config_error("missing animation track"))?;
        let symbol = style
            .rules
            .iter()
            .find(|r| r.id == binding.target.rule)
            .and_then(|r| r.symbols.iter().find(|s| s.id() == binding.target.symbol))
            .ok_or_else(|| config_error("missing animation target"))?;
        if !seen.insert((
            &binding.target.rule,
            &binding.target.symbol,
            &source_track.parameter,
        )) {
            return Err(config_error("conflicting animation target parameter"));
        }
        let repeat = match binding.repeat.as_str() {
            "once" => Repeat::Once,
            "loop" => Repeat::Loop,
            _ => return Err(config_error("unknown animation repeat mode")),
        };
        let track = source_track.prepare(
            definition.duration,
            BindingClock::new(binding.start, binding.speed, repeat)?,
            ctx,
        )?;
        let target_unit = match (track.parameter(), symbol) {
            (SymbolParameter::Size, SymbolDefinition::Point { size, .. }) => size.unit,
            (
                SymbolParameter::Width,
                SymbolDefinition::Point {
                    stroke: Some(stroke),
                    ..
                },
            ) => stroke.width.unit,
            (SymbolParameter::Width, SymbolDefinition::Line { width, .. }) => width.unit,
            _ => None,
        };
        if source_track.unit != target_unit {
            return Err(config_error("animation and base length units must agree"));
        }
        let valid = match (track.parameter(), symbol) {
            #[cfg(feature = "text")]
            (_, SymbolDefinition::Label { .. }) => false,
            (SymbolParameter::Opacity, _) => true,
            (SymbolParameter::Size | SymbolParameter::Angle, SymbolDefinition::Point { .. }) => {
                true
            }
            (
                SymbolParameter::Width | SymbolParameter::StrokeColor,
                SymbolDefinition::Point { stroke, .. },
            ) => stroke.is_some(),
            (
                SymbolParameter::Width | SymbolParameter::StrokeColor,
                SymbolDefinition::Line { .. },
            ) => true,
            (
                SymbolParameter::FillColor,
                SymbolDefinition::Point { .. } | SymbolDefinition::Area { .. },
            ) => true,
            _ => false,
        };
        if !valid {
            return Err(config_error("incompatible animation symbol target"));
        }
        output.push(PreparedStyleBinding {
            rule: binding.target.rule.clone(),
            symbol: binding.target.symbol.clone(),
            track,
        });
    }
    Ok(Accounted::new(output, vec![hold]))
}
pub fn parse<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, GeelloError> {
    toml::from_str(text).map_err(|e| config_error(e.to_string()))
}
pub fn parse_dataset(text: &str, file: impl AsRef<Path>) -> Result<DatasetDefinition, GeelloError> {
    let mut d: DatasetDefinition = parse(text)?;
    check_header(d.schema_version, &d.id)?;
    if !matches!(d.source.kind.as_str(), "geojson" | "shapefile" | "geotiff") {
        return Err(config_error("unknown source format"));
    }
    if d.source.path.as_os_str().is_empty()
        || d.source
            .sample_kind
            .as_deref()
            .is_some_and(|s| !matches!(s, "continuous" | "categorical"))
    {
        return Err(config_error(
            "nonempty source path and known sample kind required",
        ));
    }
    unique(d.queryables.iter().map(|q| q.name.as_str()))?;
    for q in &mut d.queryables {
        // Queryables identify top-level source property names. Accept explicit
        // one-token pointers as an adapter spelling, then retain the raw name
        // used by reader-wide type validation.
        if q.field.starts_with('/') {
            let tokens = crate::query::pointer_tokens(&q.field)?;
            if tokens.len() != 1 {
                return Err(config_error("queryable must select a top-level property"));
            }
            q.field = tokens[0].clone();
        }
        if q.field.is_empty()
            || [
                "bbox",
                "limit",
                "offset",
                "cursor",
                "datetime",
                "ids",
                "resultType",
                "service",
                "version",
                "request",
                "typenames",
                "outputformat",
                "count",
                "startindex",
                "resourceid",
            ]
            .iter()
            .any(|name| q.name.eq_ignore_ascii_case(name))
            || !matches!(q.kind.as_str(), "string" | "boolean" | "integer" | "number")
        {
            return Err(config_error("invalid scalar queryable"));
        }
    }
    if let Some(identity) = &d.identity
        && (!matches!(
            identity.kind.as_str(),
            "source" | "attribute" | "original-or-ordinal"
        ) || (identity.kind == "attribute"
            && identity.field.as_ref().is_none_or(|s| s.is_empty())))
    {
        return Err(config_error("invalid identity policy"));
    }
    d.source.path = resolve_relative(file, &d.source.path);
    Ok(d)
}
pub fn parse_style(text: &str) -> Result<StyleDefinition, GeelloError> {
    let s: StyleDefinition = parse(text)?;
    check_header(s.schema_version, &s.id)?;
    unique(s.rules.iter().map(|r| r.id.as_str()))?;
    for rule in &s.rules {
        if let Some(filter) = &rule.filter {
            filter.validate()?;
        }
        unique(rule.symbols.iter().map(SymbolDefinition::id))?;
        for symbol in &rule.symbols {
            validate_symbol(symbol)?;
        }
    }
    Ok(s)
}
fn validate_symbol(symbol: &SymbolDefinition) -> Result<(), GeelloError> {
    let check = |e: &NumericExpression| -> Result<(), GeelloError> {
        if e.value.is_some() == e.field.is_some()
            || e.value.is_some_and(|v| !v.is_finite() || v < 0.)
            || e.fallback.is_some_and(|v| !v.is_finite() || v < 0.)
        {
            return Err(config_error("invalid numeric expression"));
        }
        if let Some(field) = &e.field {
            property(&Properties::new(), field)?;
        }
        if e.unit.is_none() {
            return Err(config_error("numeric unit required"));
        }
        Ok(())
    };
    match symbol {
        #[cfg(feature = "text")]
        SymbolDefinition::Label {
            field,
            text,
            fonts,
            size,
            color,
            language,
            placement,
            priority,
            wrap_width,
            repeat_distance,
            ..
        } => {
            if field.is_some() == text.is_some() {
                return Err(config_error("label needs exactly one text or field"));
            }
            if let Some(field) = field {
                crate::query::pointer_tokens(field)?;
            }
            if fonts.is_empty()
                || fonts.iter().any(|f| !valid_resource_id(&f.resource))
                || !matches!(placement.as_str(), "point" | "polygon" | "line" | "curved")
                || !priority.is_finite()
                || !repeat_distance.is_finite()
                || *repeat_distance <= 0.
                || wrap_width.is_some_and(|w| !w.is_finite() || w <= 0.)
                || (placement == "curved" && wrap_width.is_some())
                || size.value.is_some() == size.field.is_some()
                || size.unit.is_none()
                || size.value.is_some_and(|v| !v.is_finite() || v <= 0.)
            {
                return Err(config_error("invalid label configuration"));
            }
            if let Some(field) = &size.field {
                crate::query::pointer_tokens(field)?;
            }
            if let Some(color) = color {
                parse_color(color)?;
            }
            if language.as_ref().is_some_and(|l| {
                l.is_empty()
                    || l.len() > 128
                    || l.split('-').any(|p| {
                        p.is_empty() || p.len() > 8 || !p.bytes().all(|b| b.is_ascii_alphanumeric())
                    })
            }) {
                return Err(config_error("invalid label language"));
            }
        }
        SymbolDefinition::Point {
            shape,
            size,
            opacity,
            angle,
            fill,
            stroke,
            ..
        } => {
            check(size)?;
            if shape
                .as_deref()
                .is_some_and(|s| !matches!(s, "circle" | "square" | "triangle"))
            {
                return Err(config_error("unknown marker shape"));
            }
            crate::style::opacity(*opacity)?;
            if !angle.is_finite() {
                return Err(config_error("nonfinite point angle"));
            }
            if let Some(c) = fill {
                parse_color(c)?;
            }
            if let Some(s) = stroke {
                validate_stroke(s)?;
            }
        }
        SymbolDefinition::Line {
            cap,
            join,
            width,
            color,
            opacity,
            miter_limit,
            dashes,
            dash_offset,
            rings,
            ..
        } => {
            check(width)?;
            validate_cap_join(cap.as_deref(), join.as_deref())?;
            if let Some(c) = color {
                parse_color(c)?;
            }
            crate::style::opacity(*opacity)?;
            if !miter_limit.is_finite()
                || *miter_limit < 1.
                || !dash_offset.is_finite()
                || dashes.len() % 2 != 0
                || dashes.iter().any(|n| !n.is_finite() || *n <= 0.)
                || rings.as_deref().is_some_and(|r| r != "all")
            {
                return Err(config_error("invalid line stroke"));
            }
        }
        SymbolDefinition::Area { fill, opacity, .. } => {
            if let Some(c) = fill {
                parse_color(c)?;
            }
            crate::style::opacity(*opacity)?;
        }
        SymbolDefinition::Raster {
            mode,
            band,
            bands,
            range,
            resampling,
            opacity,
            ..
        } => {
            if !matches!(mode.as_str(), "gray" | "rgb")
                || (mode == "gray" && (band.is_none() || bands.is_some()))
                || (mode == "rgb" && (bands.is_none() || band.is_some()))
                || bands.as_ref().is_some_and(|b| b.len() != 3)
                || band == &Some(0)
                || bands.as_ref().is_some_and(|b| b.contains(&0))
            {
                return Err(config_error("invalid raster symbol bands/mode"));
            }
            match range {
                RangeDefinition::DatasetMinmax => {}
                RangeDefinition::Explicit { min, max } => crate::raster_processing::SampleRange {
                    min: *min,
                    max: *max,
                }
                .validate()?,
            }
            if !matches!(resampling.enlarge.as_str(), "nearest" | "bilinear")
                || !matches!(
                    resampling.reduce.as_str(),
                    "nearest" | "bilinear" | "area" | "area-average"
                )
            {
                return Err(config_error("invalid raster resampling"));
            }
            crate::style::opacity(*opacity)?;
        }
    }
    Ok(())
}
fn validate_stroke(s: &StrokeDefinition) -> Result<(), GeelloError> {
    validate_cap_join(s.cap.as_deref(), s.join.as_deref())?;
    crate::style::opacity(s.opacity)?;
    if let Some(c) = &s.color {
        parse_color(c)?;
    }
    if !s.miter_limit.is_finite()
        || s.miter_limit < 1.
        || !s.dash_offset.is_finite()
        || !s.dashes.len().is_multiple_of(2)
        || s.dashes.iter().any(|n| !n.is_finite() || *n <= 0.)
    {
        return Err(config_error("invalid stroke"));
    }
    if s.width.value.is_some() == s.width.field.is_some()
        || s.width.unit.is_none()
        || s.width.value.is_some_and(|v| !v.is_finite() || v < 0.)
        || s.width.fallback.is_some_and(|v| !v.is_finite() || v < 0.)
    {
        return Err(config_error("invalid stroke width expression"));
    }
    if let Some(field) = &s.width.field {
        crate::query::pointer_tokens(field)?;
    }
    Ok(())
}
fn validate_cap_join(cap: Option<&str>, join: Option<&str>) -> Result<(), GeelloError> {
    if cap.is_some_and(|s| !matches!(s, "butt" | "round" | "square"))
        || join.is_some_and(|s| !matches!(s, "miter" | "round" | "bevel"))
    {
        return Err(config_error("unknown cap/join"));
    }
    Ok(())
}
pub fn parse_map(text: &str) -> Result<MapDefinition, GeelloError> {
    let m: MapDefinition = parse(text)?;
    check_header(m.schema_version, &m.id)?;
    unique(m.items.iter().map(|i| i.id.as_str()))?;
    for item in &m.items {
        validate_geometry_bindings(item)?;
        if !valid_resource_id(&item.dataset) || !valid_resource_id(&item.style) {
            return Err(config_error(
                "map item references must be namespace:local IDs",
            ));
        }
        unique(item.animation_bindings.iter().map(|b| b.id.as_str()))?;
        for binding in &item.animation_bindings {
            if !valid_resource_id(&binding.animation)
                || binding.track.is_empty()
                || binding.target.rule.is_empty()
                || binding.target.symbol.is_empty()
                || !matches!(binding.repeat.as_str(), "once" | "loop")
            {
                return Err(config_error(
                    "invalid animation binding identity or repeat mode",
                ));
            }
            crate::animation::BindingClock::new(
                binding.start,
                binding.speed,
                crate::animation::Repeat::Once,
            )?;
        }
    }
    if m.output_crs.is_empty() {
        return Err(config_error("map CRS allowlist required"));
    }
    if let Some(tiles) = &m.tiles {
        unique(tiles.matrix_sets.iter().map(String::as_str))?;
        if tiles.matrix_sets.is_empty() || tiles.min_zoom > tiles.max_zoom || tiles.max_zoom > 30 {
            return Err(config_error(
                "tile publication requires nonempty grids and ordered zooms within 0..30",
            ));
        }
        for grid in &tiles.matrix_sets {
            let crs = match grid.as_str() {
                "WebMercatorQuad" => "EPSG:3857",
                "WorldCRS84Quad" => "OGC:CRS84",
                _ => return Err(config_error("unsupported tile matrix set")),
            };
            if !m.output_crs.iter().any(|c| c == crs) {
                return Err(config_error("tile matrix CRS is not published by map"));
            }
        }
    }
    if let Some(c) = &m.background {
        parse_color(c)?;
    }
    Ok(m)
}
pub fn parse_animation(text: &str) -> Result<AnimationDefinition, GeelloError> {
    let a: AnimationDefinition = parse(text)?;
    check_header(a.schema_version, &a.id)?;
    if !a.duration.is_finite() || a.duration <= 0. {
        return Err(config_error("positive finite animation duration required"));
    }
    unique(
        a.tracks
            .iter()
            .map(|t| t.id.as_str())
            .chain(a.geometry_tracks.iter().map(|t| t.id.as_str())),
    )?;
    for track in &a.geometry_tracks {
        validate_geometry_track(track, a.duration)?;
    }
    for t in &a.tracks {
        if !matches!(t.interpolation.as_str(), "step" | "linear") || t.keyframes.is_empty() {
            return Err(config_error("invalid animation interpolation/keyframes"));
        }
        if !matches!(
            t.parameter.as_str(),
            "size" | "angle" | "width" | "fill" | "stroke-color" | "opacity"
        ) {
            return Err(config_error("unsupported animation parameter"));
        }
        if matches!(t.parameter.as_str(), "size" | "width") != t.unit.is_some() {
            return Err(config_error(
                "animation length unit is missing or incompatible",
            ));
        }
        let mut last = None;
        for k in &t.keyframes {
            if !k.time.is_finite()
                || k.time < 0.
                || k.time > a.duration
                || last.is_some_and(|n| k.time <= n)
            {
                return Err(config_error("keyframe times must increase within duration"));
            }
            last = Some(k.time);
            if matches!(t.parameter.as_str(), "fill" | "stroke-color") {
                let PropertyValue::String(color) = &k.value else {
                    return Err(config_error("color keyframe must be a hex string"));
                };
                parse_color(color)?;
            } else {
                let value = match &k.value {
                    PropertyValue::I64(n) => *n as f64,
                    PropertyValue::U64(n) => *n as f64,
                    PropertyValue::F64(n) => *n,
                    _ => return Err(config_error("numeric keyframe required")),
                };
                if !value.is_finite()
                    || (matches!(t.parameter.as_str(), "size" | "width") && value < 0.)
                    || (t.parameter == "opacity" && !(0.0..=1.0).contains(&value))
                {
                    return Err(config_error(
                        "animation keyframe is outside its target range",
                    ));
                }
            }
        }
    }
    Ok(a)
}
pub fn resolve_relative(file: impl AsRef<Path>, path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_owned()
    } else {
        file.as_ref().parent().unwrap_or(Path::new(".")).join(path)
    }
}
fn unique<'a>(ids: impl Iterator<Item = &'a str>) -> Result<(), GeelloError> {
    let mut seen = HashSet::new();
    for id in ids {
        if id.is_empty() || !seen.insert(id) {
            return Err(config_error("empty or duplicate local identifier"));
        }
    }
    Ok(())
}
fn check_header(v: u32, id: &str) -> Result<(), GeelloError> {
    if v != 1 || !valid_resource_id(id) {
        return Err(config_error(
            "schema_version 1 and namespace:local resource ID required",
        ));
    }
    Ok(())
}
pub fn valid_resource_id(id: &str) -> bool {
    let mut parts = id.split(':');
    let (Some(a), Some(b)) = (parts.next(), parts.next()) else {
        return false;
    };
    parts.next().is_none()
        && [a, b].iter().all(|s| {
            !s.is_empty()
                && s.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        })
}
fn config_error(message: impl Into<String>) -> GeelloError {
    GeelloError::InvalidConfiguration(message.into())
}

/// Resolve original-property rules before projection. Each symbol contributes
/// across all features before the next symbol; batch partition never changes order.
pub fn resolve_vector_style(
    style: &StyleDefinition,
    batches: &[Arc<VectorBatch>],
    context: &OperationContext,
) -> Result<Accounted<MapComposition>, GeelloError> {
    resolve_vector_style_at(style, batches, &[], 0., context)
}
/// Evaluate filters/base expressions from original attributes, then replace
/// selected symbol values at the requested time, before projection and layout.
pub fn resolve_vector_style_at(
    style: &StyleDefinition,
    batches: &[Arc<VectorBatch>],
    bindings: &[PreparedStyleBinding],
    time: f64,
    context: &OperationContext,
) -> Result<Accounted<MapComposition>, GeelloError> {
    resolve_vector_style_with_fonts(style, batches, bindings, time, &BTreeMap::new(), context)
}
/// Resolve labels using only explicit font resources from the pinned graph.
/// Font keys combine resource ID and selected face index.
pub fn resolve_vector_style_with_fonts(
    style: &StyleDefinition,
    batches: &[Arc<VectorBatch>],
    bindings: &[PreparedStyleBinding],
    time: f64,
    _fonts: &BTreeMap<(String, u32), Arc<FontResource>>,
    context: &OperationContext,
) -> Result<Accounted<MapComposition>, GeelloError> {
    crate::animation::validate_time(time)?;
    let mut items = vec![];
    let mut reservations = vec![];
    for rule in &style.rules {
        if let Some(filter) = &rule.filter {
            filter.validate()?;
        }
        for symbol in &rule.symbols {
            validate_symbol(symbol)?;
            for batch in batches {
                for feature in &batch.features {
                    context.checkpoint(1)?;
                    if rule
                        .filter
                        .as_ref()
                        .map(|f| f.evaluate(&feature.properties))
                        .transpose()?
                        .is_some_and(|v| !v.matches())
                    {
                        continue;
                    }
                    #[cfg(feature = "text")]
                    if let SymbolDefinition::Label {
                        field,
                        text: literal,
                        fonts,
                        size,
                        color,
                        language,
                        placement,
                        priority,
                        allow_overlap,
                        wrap_width,
                        repeat_distance,
                        ..
                    } = symbol
                    {
                        let text = if let Some(text) = literal {
                            text
                        } else {
                            let Some(PropertyValue::String(text)) =
                                property(&feature.properties, field.as_deref().unwrap())?
                            else {
                                continue;
                            };
                            text
                        };
                        let Some(geometry) = &feature.geometry else {
                            continue;
                        };
                        let fonts = fonts
                            .iter()
                            .map(|f| {
                                _fonts
                                    .get(&(f.resource.clone(), f.face_index))
                                    .cloned()
                                    .ok_or_else(|| config_error("missing explicit label font"))
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        reservations.push(
                            context.reserve(
                                crate::core::feature_clone_bytes(feature)?
                                    .checked_mul(2)
                                    .and_then(|n| n.checked_add(512))
                                    .ok_or(GeelloError::SizeOverflow)?,
                            )?,
                        );
                        items.push(DrawItem::Label(crate::text::LabelItem {
                            crs: batch.crs.clone(),
                            geometry: geometry.clone(),
                            text: text.clone(),
                            style: TextStyle {
                                fonts,
                                size: size.length(&feature.properties)?,
                                color: parse_color(color.as_deref().unwrap_or("#000000"))?,
                                language: language.clone().unwrap_or_else(|| "und".into()),
                            },
                            placement: match placement.as_str() {
                                "point" => crate::text::LabelPlacement::Point,
                                "polygon" => crate::text::LabelPlacement::Polygon,
                                "line" => crate::text::LabelPlacement::Line,
                                _ => crate::text::LabelPlacement::Curved,
                            },
                            priority: *priority,
                            allow_overlap: *allow_overlap,
                            wrap_width: *wrap_width,
                            repeat_distance: *repeat_distance,
                            marker_source: items.iter().enumerate().rev().find_map(
                                |(index, item)| {
                                    if let DrawItem::Vector(v) = item
                                        && v.symbols.iter().any(|s| matches!(s, Symbol::Point(_)))
                                        && v.batches.iter().flat_map(|b| &b.features).any(|f| {
                                            f.source_ordinal == feature.source_ordinal
                                                && f.geometry == feature.geometry
                                        })
                                    {
                                        Some((index, feature.source_ordinal))
                                    } else {
                                        None
                                    }
                                },
                            ),
                        }));
                        continue;
                    }
                    let mut resolved = symbol.resolve(&feature.properties)?;
                    for binding in bindings {
                        context.checkpoint(1)?;
                        if binding.rule == rule.id && binding.symbol == symbol.id() {
                            binding.track.apply(&mut resolved, time, context)?;
                        }
                    }
                    reservations.push(
                        context.reserve(
                            crate::core::feature_clone_bytes(feature)?
                                .checked_mul(2)
                                .and_then(|n| n.checked_add(512))
                                .ok_or(GeelloError::SizeOverflow)?,
                        )?,
                    );
                    items.push(DrawItem::Vector(VectorItem {
                        batches: vec![Arc::new(VectorBatch {
                            crs: batch.crs.clone(),
                            features: vec![feature.clone()],
                        })],
                        symbols: vec![resolved],
                    }));
                }
            }
        }
    }
    Ok(Accounted::new(
        MapComposition {
            items,
            background: None,
        },
        reservations,
    ))
}
#[derive(Debug, Default)]
pub struct ResourceGraph {
    pub datasets: BTreeMap<String, DatasetDefinition>,
    pub styles: BTreeMap<String, StyleDefinition>,
    pub maps: BTreeMap<String, MapDefinition>,
    pub animations: BTreeMap<String, AnimationDefinition>,
}
impl ResourceGraph {
    pub fn validate(&self) -> Result<(), GeelloError> {
        for (key, dataset) in &self.datasets {
            if key != &dataset.id {
                return Err(config_error("dataset map key/id mismatch"));
            }
        }
        for (key, style) in &self.styles {
            if key != &style.id {
                return Err(config_error("style map key/id mismatch"));
            }
        }
        for (key, map) in &self.maps {
            if key != &map.id {
                return Err(config_error("map map key/id mismatch"));
            }
        }
        for (key, animation) in &self.animations {
            if key != &animation.id {
                return Err(config_error("animation map key/id mismatch"));
            }
        }
        for map in self.maps.values() {
            for item in &map.items {
                if !self.datasets.contains_key(&item.dataset) {
                    return Err(config_error("map references missing dataset"));
                }
                let style = self
                    .styles
                    .get(&item.style)
                    .ok_or_else(|| config_error("map references missing style"))?;
                let raster = self.datasets[&item.dataset].source.kind == "geotiff";
                validate_geometry_bindings(item)?;
                for binding in &item.geometry_bindings {
                    if raster
                        || !self.animations.get(&binding.animation).is_some_and(|a| {
                            a.geometry_tracks.iter().any(|t| t.id == binding.track)
                        })
                    {
                        return Err(config_error(
                            "missing or incompatible geometry animation track",
                        ));
                    }
                }
                if style
                    .rules
                    .iter()
                    .flat_map(|r| &r.symbols)
                    .any(|symbol| matches!(symbol, SymbolDefinition::Raster { .. }) != raster)
                {
                    return Err(config_error(
                        "dataset format and style symbol are incompatible",
                    ));
                }
                for binding in &item.animation_bindings {
                    let a = self
                        .animations
                        .get(&binding.animation)
                        .ok_or_else(|| config_error("missing animation"))?;
                    if !a.tracks.iter().any(|t| t.id == binding.track)
                        || !style.rules.iter().any(|r| {
                            r.id == binding.target.rule
                                && r.symbols.iter().any(|s| s.id() == binding.target.symbol)
                        })
                    {
                        return Err(config_error("broken animation target/track"));
                    }
                    if !binding.start.is_finite()
                        || !binding.speed.is_finite()
                        || binding.speed <= 0.
                        || !matches!(binding.repeat.as_str(), "once" | "loop")
                    {
                        return Err(config_error("invalid animation binding"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Local external registration validates regular files and retains references
/// only. Durable generations/ETags/HTTP routes are owned by the M3 server.
#[derive(Debug, Default)]
pub struct ExternalRegistry {
    records: BTreeMap<String, ExternalRegistration>,
}
#[derive(Debug, Clone)]
pub struct ExternalRegistration {
    pub id: String,
    pub kind: String,
    pub path: PathBuf,
}
impl ExternalRegistry {
    pub fn register(
        &mut self,
        kind: &str,
        id: &str,
        path: impl AsRef<Path>,
    ) -> Result<&ExternalRegistration, GeelloError> {
        if !valid_resource_id(id)
            || !matches!(kind, "dataset" | "style" | "map" | "animation" | "font")
        {
            return Err(config_error("invalid registration kind/identity"));
        }
        let path = std::fs::canonicalize(path).map_err(|e| GeelloError::Io(e.to_string()))?;
        if !path.is_file() {
            return Err(config_error("registered source must be a regular file"));
        }
        let key = format!("{kind}/{id}");
        if self.records.contains_key(&key) {
            return Err(config_error("duplicate registration"));
        }
        self.records.insert(
            key.clone(),
            ExternalRegistration {
                id: id.into(),
                kind: kind.into(),
                path,
            },
        );
        Ok(&self.records[&key])
    }
    pub fn get(&self, kind: &str, id: &str) -> Option<&ExternalRegistration> {
        self.records.get(&format!("{kind}/{id}"))
    }
    pub fn remove(&mut self, kind: &str, id: &str) -> Option<ExternalRegistration> {
        self.records.remove(&format!("{kind}/{id}"))
    }
}
