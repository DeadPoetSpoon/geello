//! Root-aware JSON framing. A record is decoded only after its complete boundary
//! has been found; malformed structure is never recovered by searching for `}`.
use super::*;
use crate::{CoordinateSpace, Properties, SourceOrdinate};
use geo_types::*;
use serde_json::{Map, Value};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

pub fn open_geojson(
    path: impl AsRef<Path>,
    options: &VectorReadOptions,
    context: &OperationContext,
) -> Result<VectorSnapshot, GeelloError> {
    read_geojson(
        BufReader::new(File::open(path).map_err(io)?),
        options,
        context,
    )
}
/// Accept a FeatureCollection, Feature or bare Geometry with identical CRS override rules.
pub fn read_geojson(
    input: impl Read,
    options: &VectorReadOptions,
    context: &OperationContext,
) -> Result<VectorSnapshot, GeelloError> {
    options.limits.validate()?;
    let crs = options
        .crs_override
        .clone()
        .unwrap_or(Crs::new("OGC:CRS84", CoordinateSpace::GeographicXy)?);
    let mut builder = SnapshotBuilder::new(options, crs, context)?;
    let mut reader = Framer {
        input: BufReader::new(input),
        bytes: 0,
        limits: &options.limits,
        context,
    };
    reader.expect(b'{')?;
    let mut root = Map::new();
    let mut root_guards = Vec::new();
    let mut root_bytes = 0usize;
    let mut keys = HashSet::new();
    let mut collection = false;
    let mut ordinal = 0u64;
    if reader.peek()? != Some(b'}') {
        loop {
            let key: String = serde_json::from_slice(&reader.value()?).map_err(source)?;
            root_guards.push(
                context.reserve(
                    key.len()
                        .checked_add(128)
                        .ok_or(GeelloError::SizeOverflow)?,
                )?,
            );
            if !keys.insert(key.clone()) {
                return Err(source("duplicate root member"));
            }
            reader.expect(b':')?;
            if key == "features" {
                collection = true;
                reader.expect(b'[')?;
                if reader.peek()? != Some(b']') {
                    loop {
                        let raw = reader.value()?;
                        let _scratch = context
                            .reserve(raw.len().checked_mul(32).ok_or(GeelloError::SizeOverflow)?)?;
                        let value: Value = serde_json::from_slice(&raw).map_err(source)?;
                        builder.push(
                            feature(value, ordinal, options.limits.max_coordinates_per_feature),
                            raw.len(),
                        )?;
                        ordinal = ordinal.checked_add(1).ok_or(GeelloError::SizeOverflow)?;
                        match reader.peek()? {
                            Some(b',') => {
                                reader.byte()?;
                            }
                            Some(b']') => break,
                            _ => return Err(source("expected feature separator")),
                        }
                    }
                }
                reader.expect(b']')?;
            } else {
                let raw = reader.value()?;
                // Foreign root members are bounded and validated but never retained.
                if matches!(
                    key.as_str(),
                    "type" | "geometry" | "coordinates" | "geometries" | "properties" | "id"
                ) {
                    root_bytes = root_bytes
                        .checked_add(raw.len())
                        .ok_or(GeelloError::SizeOverflow)?;
                    if root_bytes > options.limits.max_record_bytes {
                        return Err(GeelloError::BudgetExceeded("root record bytes"));
                    }
                    root_guards
                        .push(context.reserve(
                            raw.len().checked_mul(32).ok_or(GeelloError::SizeOverflow)?,
                        )?);
                    root.insert(key, serde_json::from_slice(&raw).map_err(source)?);
                }
            }
            match reader.peek()? {
                Some(b',') => {
                    reader.byte()?;
                }
                Some(b'}') => break,
                _ => return Err(source("expected root member separator")),
            }
        }
    }
    reader.expect(b'}')?;
    if reader.peek()?.is_some() {
        return Err(source("trailing JSON content"));
    }
    if collection {
        if root.get("type").and_then(Value::as_str) != Some("FeatureCollection") {
            return Err(source("features requires FeatureCollection root type"));
        }
    } else {
        let value = Value::Object(root);
        let bytes = serde_json::to_vec(&value).map_err(source)?.len();
        let f = if value.get("type").and_then(Value::as_str) == Some("Feature") {
            feature(value, 0, options.limits.max_coordinates_per_feature)
        } else {
            let mut ordinates = vec![];
            geometry(
                &value,
                &mut vec![],
                &mut ordinates,
                &mut 0,
                options.limits.max_coordinates_per_feature,
            )
            .map(|g| Feature {
                source_id: None,
                id: None,
                geometry: Some(g),
                properties: Properties::new(),
                source_ordinal: 0,
                source_ordinates: ordinates,
            })
        };
        builder.push(f, bytes)?;
    }
    builder.finish()
}
struct Framer<'a, R: BufRead> {
    input: R,
    bytes: u64,
    limits: &'a ReaderLimits,
    context: &'a OperationContext,
}
impl<R: BufRead> Framer<'_, R> {
    fn byte(&mut self) -> Result<Option<u8>, GeelloError> {
        let mut b = [0];
        let n = self.input.read(&mut b).map_err(io)?;
        if n == 0 {
            return Ok(None);
        }
        self.bytes += 1;
        if self.bytes > self.limits.max_source_bytes {
            return Err(GeelloError::BudgetExceeded("source bytes"));
        }
        if self.bytes.is_multiple_of(4096) {
            self.context.checkpoint(4096)?;
        }
        Ok(Some(b[0]))
    }
    fn peek(&mut self) -> Result<Option<u8>, GeelloError> {
        loop {
            let b = self.input.fill_buf().map_err(io)?.first().copied();
            match b {
                Some(b' ' | b'\r' | b'\n' | b'\t') => {
                    self.byte()?;
                }
                _ => return Ok(b),
            }
        }
    }
    fn expect(&mut self, b: u8) -> Result<(), GeelloError> {
        self.peek()?;
        if self.byte()? != Some(b) {
            return Err(source(format!("expected JSON delimiter {}", b as char)));
        }
        Ok(())
    }
    fn value(&mut self) -> Result<Vec<u8>, GeelloError> {
        self.context.check()?;
        let _scratch = self.context.reserve(
            self.limits
                .max_record_bytes
                .checked_mul(32)
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let first = self.peek()?.ok_or_else(|| source("unexpected EOF"))?;
        let mut out = vec![];
        let mut stack = vec![];
        let mut quoted = false;
        let mut escape = false;
        loop {
            let next = self.input.fill_buf().map_err(io)?.first().copied();
            if !quoted
                && stack.is_empty()
                && !out.is_empty()
                && (first != b'"')
                && next
                    .is_none_or(|b| b == b',' || b == b'}' || b == b']' || b.is_ascii_whitespace())
            {
                break;
            }
            let b = self
                .byte()?
                .ok_or_else(|| source("unterminated JSON value"))?;
            if out.len() >= self.limits.max_record_bytes {
                return Err(GeelloError::BudgetExceeded("record bytes"));
            }
            out.push(b);
            if quoted {
                if escape {
                    escape = false
                } else if b == b'\\' {
                    escape = true
                } else if b == b'"' {
                    quoted = false;
                    if stack.is_empty() {
                        break;
                    }
                }
                continue;
            }
            match b {
                b'"' => quoted = true,
                b'{' => stack.push(b'}'),
                b'[' => stack.push(b']'),
                b'}' | b']' => {
                    if stack.pop() != Some(b) {
                        return Err(source("mismatched JSON nesting"));
                    }
                    if stack.is_empty() {
                        break;
                    }
                }
                _ => {}
            }
            if stack.len() > self.limits.max_depth {
                return Err(GeelloError::BudgetExceeded("JSON depth"));
            }
        }
        // Validate scalars and compound values, including strings and escapes.
        serde_json::from_slice::<Value>(&out).map_err(source)?;
        Ok(out)
    }
}
fn feature(value: Value, ordinal: u64, max: usize) -> Result<Feature, GeelloError> {
    // Use the approved GeoJSON grammar in addition to structural framing.
    let _: geojson::Feature = serde_json::from_value(value.clone()).map_err(source)?;
    let id = match value.get("id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(FeatureId::String(s.clone())),
        Some(Value::Number(n)) => n
            .as_i64()
            .map(FeatureId::I64)
            .or_else(|| n.as_u64().map(FeatureId::U64)),
        _ => return Err(source("invalid feature identity")),
    };
    let properties = match value.get("properties") {
        None | Some(Value::Null) => Properties::new(),
        Some(v) => serde_json::from_value(v.clone()).map_err(source)?,
    };
    let mut ordinates = vec![];
    let g = match value.get("geometry") {
        Some(Value::Null) => None,
        Some(v) => Some(geometry(v, &mut vec![], &mut ordinates, &mut 0, max)?),
        None => return Err(source("missing feature geometry")),
    };
    Ok(Feature {
        source_id: value
            .get("id")
            .map(|v| serde_json::from_value(v.clone()).map_err(source))
            .transpose()?,
        id,
        geometry: g,
        properties,
        source_ordinal: ordinal,
        source_ordinates: ordinates,
    })
}
fn array(v: &Value) -> Result<&Vec<Value>, GeelloError> {
    v.as_array()
        .ok_or_else(|| source("coordinate array required"))
}
fn position(
    v: &Value,
    path: &[usize],
    ordinates: &mut Vec<SourceOrdinate>,
    count: &mut usize,
    max: usize,
) -> Result<Coord, GeelloError> {
    *count += 1;
    if *count > max {
        return Err(GeelloError::BudgetExceeded("coordinates"));
    }
    let v = array(v)?;
    if v.len() < 2 {
        return Err(source("position requires XY"));
    }
    let n: Vec<f64> = v
        .iter()
        .map(|v| {
            v.as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| source("finite ordinate required"))
        })
        .collect::<Result<_, _>>()?;
    if n.len() > 2 {
        ordinates.push(SourceOrdinate {
            coordinate_path: path.to_vec(),
            trailing: n[2..].to_vec(),
        });
    }
    Ok(Coord { x: n[0], y: n[1] })
}
fn line(
    v: &Value,
    path: &mut Vec<usize>,
    ordinates: &mut Vec<SourceOrdinate>,
    count: &mut usize,
    max: usize,
) -> Result<LineString, GeelloError> {
    let mut coords = vec![];
    for (i, v) in array(v)?.iter().enumerate() {
        path.push(i);
        coords.push(position(v, path, ordinates, count, max)?);
        path.pop();
    }
    Ok(LineString(coords))
}
fn polygon(
    v: &Value,
    path: &mut Vec<usize>,
    ordinates: &mut Vec<SourceOrdinate>,
    count: &mut usize,
    max: usize,
) -> Result<Polygon, GeelloError> {
    let mut rings = vec![];
    for (i, v) in array(v)?.iter().enumerate() {
        path.push(i);
        let r = line(v, path, ordinates, count, max)?;
        path.pop();
        if !r.0.is_empty() && (r.0.len() < 4 || r.0.first() != r.0.last()) {
            return Err(source("unclosed or short polygon ring"));
        }
        rings.push(r);
    }
    let mut iter = rings.into_iter();
    Ok(Polygon::new(
        iter.next().unwrap_or_else(|| LineString(vec![])),
        iter.collect(),
    ))
}
fn geometry(
    v: &Value,
    path: &mut Vec<usize>,
    ordinates: &mut Vec<SourceOrdinate>,
    count: &mut usize,
    max: usize,
) -> Result<Geometry, GeelloError> {
    let c = v.get("coordinates").unwrap_or(&Value::Null);
    Ok(
        match v
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| source("geometry type required"))?
        {
            "Point" => Geometry::Point(Point(position(c, path, ordinates, count, max)?)),
            "MultiPoint" => Geometry::MultiPoint(MultiPoint(
                line(c, path, ordinates, count, max)?
                    .0
                    .into_iter()
                    .map(Point)
                    .collect(),
            )),
            "LineString" => Geometry::LineString(line(c, path, ordinates, count, max)?),
            "Polygon" => Geometry::Polygon(polygon(c, path, ordinates, count, max)?),
            "MultiLineString" => {
                let mut lines = vec![];
                for (i, v) in array(c)?.iter().enumerate() {
                    path.push(i);
                    lines.push(line(v, path, ordinates, count, max)?);
                    path.pop();
                }
                Geometry::MultiLineString(MultiLineString(lines))
            }
            "MultiPolygon" => {
                let mut polygons = vec![];
                for (i, v) in array(c)?.iter().enumerate() {
                    path.push(i);
                    polygons.push(polygon(v, path, ordinates, count, max)?);
                    path.pop();
                }
                Geometry::MultiPolygon(MultiPolygon(polygons))
            }
            "GeometryCollection" => {
                let mut geometries = vec![];
                for (i, v) in array(
                    v.get("geometries")
                        .ok_or_else(|| source("geometries required"))?,
                )?
                .iter()
                .enumerate()
                {
                    path.push(i);
                    geometries.push(geometry(v, path, ordinates, count, max)?);
                    path.pop();
                }
                Geometry::GeometryCollection(GeometryCollection(geometries))
            }
            _ => return Err(source("unsupported geometry type")),
        },
    )
}
