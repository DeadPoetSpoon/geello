//! Framed SHP/DBF preparation with strict text decoding and preflight bounds.
use super::*;
use crate::{CoordinateSpace, Properties, SourceOrdinate};
use shapefile::{PolygonRing, ReadableShape, Shape, dbase::encoding_rs};
use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
};

#[derive(Debug, Clone, Default)]
pub struct ShapefileOptions {
    pub encoding: Option<String>,
}
pub fn open_shapefile(
    path: impl AsRef<Path>,
    options: &VectorReadOptions,
    context: &OperationContext,
) -> Result<VectorSnapshot, GeelloError> {
    open_shapefile_with(path, options, &ShapefileOptions::default(), context)
}
pub fn open_shapefile_with(
    path: impl AsRef<Path>,
    options: &VectorReadOptions,
    shape_options: &ShapefileOptions,
    context: &OperationContext,
) -> Result<VectorSnapshot, GeelloError> {
    options.limits.validate()?;
    context.check()?;
    let path = path.as_ref();
    let crs = if let Some(crs) = &options.crs_override {
        crs.clone()
    } else {
        let text = String::from_utf8(read_small(
            &path.with_extension("prj"),
            options.limits.max_record_bytes,
        )?)
        .map_err(source)?;
        let space = if text.trim_start().starts_with("GEOGCS[")
            || text.trim_start().starts_with("GEOGCRS[")
        {
            CoordinateSpace::GeographicXy
        } else {
            CoordinateSpace::Planar
        };
        Crs::new(text, space)?
    };
    let mut shp = File::open(path).map_err(io)?;
    let mut shx = if path.with_extension("shx").exists() {
        Some(File::open(path.with_extension("shx")).map_err(io)?)
    } else {
        None
    };
    let mut dbf = File::open(path.with_extension("dbf")).map_err(io)?;
    let total = shp
        .metadata()
        .map_err(io)?
        .len()
        .checked_add(dbf.metadata().map_err(io)?.len())
        .ok_or(GeelloError::SizeOverflow)?;
    if total > options.limits.max_source_bytes {
        return Err(GeelloError::BudgetExceeded("source bytes"));
    }
    let mut header = [0u8; 100];
    shp.read_exact(&mut header).map_err(io)?;
    if i32::from_be_bytes(header[0..4].try_into().unwrap()) != 9994
        || i32::from_le_bytes(header[28..32].try_into().unwrap()) != 1000
    {
        return Err(source("invalid SHP header"));
    }
    let declared = i32::from_be_bytes(header[24..28].try_into().unwrap());
    if declared < 50 || declared as u64 * 2 != shp.metadata().map_err(io)?.len() {
        return Err(source("SHP length mismatch"));
    }
    if let Some(index) = &mut shx {
        let mut h = [0; 100];
        index.read_exact(&mut h).map_err(io)?;
        let len = i32::from_be_bytes(h[24..28].try_into().unwrap());
        if h[..24] != header[..24]
            || h[28..] != header[28..]
            || len < 50
            || len as u64 * 2 != index.metadata().map_err(io)?.len()
            || !(len as usize * 2 - 100).is_multiple_of(8)
        {
            return Err(source("invalid/mismatched SHX header"));
        }
    }
    let mut dbh = [0u8; 32];
    dbf.read_exact(&mut dbh).map_err(io)?;
    let count = u32::from_le_bytes(dbh[4..8].try_into().unwrap()) as usize;
    if count > options.limits.max_features {
        return Err(GeelloError::BudgetExceeded("features"));
    }
    let header_len = u16::from_le_bytes(dbh[8..10].try_into().unwrap()) as usize;
    let record_len = u16::from_le_bytes(dbh[10..12].try_into().unwrap()) as usize;
    if header_len < 33
        || !(header_len - 33).is_multiple_of(32)
        || record_len == 0
        || record_len > options.limits.max_record_bytes
    {
        return Err(source("invalid DBF layout"));
    }
    let expected = (header_len as u64)
        .checked_add(
            (count as u64)
                .checked_mul(record_len as u64)
                .ok_or(GeelloError::SizeOverflow)?,
        )
        .ok_or(GeelloError::SizeOverflow)?;
    if dbf.metadata().map_err(io)?.len() < expected {
        return Err(source("truncated DBF records"));
    }
    let cpg = if path.with_extension("cpg").exists() {
        Some(String::from_utf8(read_small(&path.with_extension("cpg"), 256)?).map_err(source)?)
    } else {
        None
    };
    let enc = encoding(
        shape_options
            .encoding
            .as_deref()
            .or(cpg.as_deref())
            .map(str::trim),
        dbh[29],
    )?;
    let mut fields = vec![];
    let mut names = HashSet::new();
    let mut field_bytes = 1usize;
    for _ in 0..(header_len - 33) / 32 {
        let mut f = [0u8; 32];
        dbf.read_exact(&mut f).map_err(io)?;
        let name = std::str::from_utf8(f[..11].split(|b| *b == 0).next().unwrap())
            .map_err(source)?
            .to_string();
        if !names.insert(name.clone()) {
            return Err(source("duplicate DBF name"));
        }
        let len = f[16] as usize;
        field_bytes += len;
        fields.push((name, f[11], len, f[17]));
    }
    let mut marker = [0];
    dbf.read_exact(&mut marker).map_err(io)?;
    if marker[0] != 13 || field_bytes != record_len {
        return Err(source("DBF field layout mismatch"));
    }
    let mut builder = SnapshotBuilder::new(options, crs, context)?;
    for ordinal in 0..count {
        context.checkpoint(1)?;
        let offset = shp.stream_position().map_err(io)?;
        let mut rh = [0; 8];
        shp.read_exact(&mut rh).map_err(io)?;
        if i32::from_be_bytes(rh[..4].try_into().unwrap()) != ordinal as i32 + 1 {
            return Err(source("SHP record number mismatch"));
        }
        let words = i32::from_be_bytes(rh[4..8].try_into().unwrap());
        if let Some(index) = &mut shx {
            let mut entry = [0; 8];
            index.read_exact(&mut entry).map_err(io)?;
            if i32::from_be_bytes(entry[..4].try_into().unwrap()) as i64 * 2 != offset as i64
                || entry[4..] != rh[4..]
            {
                return Err(source("SHX record offset/length mismatch"));
            }
        }
        if words < 2 {
            return Err(source("invalid SHP record length"));
        }
        let bytes = (words as usize)
            .checked_mul(2)
            .ok_or(GeelloError::SizeOverflow)?;
        if bytes > options.limits.max_record_bytes || bytes > i32::MAX as usize {
            return Err(GeelloError::BudgetExceeded("record bytes"));
        }
        let _scratch = context.reserve(
            bytes
                .checked_mul(32)
                .and_then(|n| n.checked_add(record_len))
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        let mut raw = vec![0; bytes];
        shp.read_exact(&mut raw).map_err(io)?;
        let mut dbraw = vec![0; record_len];
        dbf.read_exact(&mut dbraw).map_err(io)?;
        if dbraw[0] == b'*' {
            continue;
        }
        if dbraw[0] != b' ' {
            return Err(source("invalid DBF deletion flag"));
        }
        let result = (|| {
            preflight_shape(&raw, options.limits.max_coordinates_per_feature)?;
            let shape =
                Shape::read_from(&mut std::io::Cursor::new(&raw), bytes as i32).map_err(source)?;
            let (geometry, mut source_ordinates) = match &shape {
                Shape::Polygon(p) => polygon(p.rings(), |_| vec![])?,
                Shape::PolygonM(p) => polygon(p.rings(), |p| vec![p.m])?,
                Shape::PolygonZ(p) => polygon(p.rings(), |p| vec![p.z, p.m])?,
                _ => {
                    let source_ordinates = ordinates(&shape)?;
                    let geometry = if matches!(shape, Shape::NullShape) {
                        None
                    } else {
                        Some(geo_types::Geometry::<f64>::try_from(shape).map_err(source)?)
                    };
                    (geometry, source_ordinates)
                }
            };
            if !has_measure(&raw)? {
                for ordinate in &mut source_ordinates {
                    ordinate.trailing.pop();
                }
                source_ordinates.retain(|v| !v.trailing.is_empty());
            }
            let mut properties = Properties::new();
            let mut offset = 1;
            for (name, kind, len, decimals) in &fields {
                properties.insert(
                    name.clone(),
                    dbf_value(*kind, *decimals, &dbraw[offset..offset + len], enc)?,
                );
                offset += len;
            }
            Ok(Feature {
                source_id: None,
                id: None,
                geometry,
                properties,
                source_ordinal: ordinal as u64,
                source_ordinates,
            })
        })();
        builder.push(result, bytes + record_len)?;
    }
    if shp.stream_position().map_err(io)? != shp.metadata().map_err(io)?.len() {
        return Err(source("SHP/DBF record count mismatch"));
    }
    if let Some(index) = &mut shx
        && index.stream_position().map_err(io)? != index.metadata().map_err(io)?.len()
    {
        return Err(source("SHX record count mismatch"));
    }
    builder.finish()
}
fn read_small(path: &Path, max: usize) -> Result<Vec<u8>, GeelloError> {
    let mut f = File::open(path).map_err(io)?;
    if f.metadata().map_err(io)?.len() > max as u64 {
        return Err(GeelloError::BudgetExceeded("sidecar bytes"));
    }
    let mut b = vec![];
    f.read_to_end(&mut b).map_err(io)?;
    Ok(b)
}
/// SHP rings need not group holes immediately after their exterior. Associate
/// by strict containment and preserve outer/hole source order in the resulting
/// MultiPolygon and matching Z/M paths. Never repair/reverse/close source rings.
fn polygon<P: Copy>(
    rings: &[PolygonRing<P>],
    trailing: impl Fn(&P) -> Vec<f64>,
) -> Result<(Option<geo_types::Geometry>, Vec<SourceOrdinate>), GeelloError>
where
    geo_types::Coord: From<P>,
{
    use geo::{Area, Contains};
    let lines: Vec<geo_types::LineString> = rings
        .iter()
        .map(|r| {
            geo_types::LineString(
                r.points()
                    .iter()
                    .copied()
                    .map(geo_types::Coord::from)
                    .collect(),
            )
        })
        .collect();
    for line in &lines {
        if line.0.len() < 4 || line.0.first() != line.0.last() {
            return Err(source("unclosed/short SHP polygon ring"));
        }
    }
    let mut polygons: Vec<geo_types::Polygon> = vec![];
    let mut assignments = vec![None; rings.len()];
    for (i, ring) in rings.iter().enumerate() {
        if matches!(ring, PolygonRing::Outer(_)) {
            assignments[i] = Some((polygons.len(), 0));
            polygons.push(geo_types::Polygon::new(lines[i].clone(), vec![]));
        }
    }
    for (i, ring) in rings.iter().enumerate() {
        if matches!(ring, PolygonRing::Inner(_)) {
            let hole = geo_types::Polygon::new(lines[i].clone(), vec![]);
            let owner = polygons
                .iter()
                .enumerate()
                .filter(|(_, p)| p.contains(&hole))
                .min_by(|(_, a), (_, b)| a.unsigned_area().total_cmp(&b.unsigned_area()))
                .map(|(index, _)| index)
                .ok_or_else(|| source("orphan/crossing SHP inner ring"))?;
            let index = polygons[owner].interiors().len() + 1;
            polygons[owner].interiors_push(lines[i].clone());
            assignments[i] = Some((owner, index));
        }
    }
    let mut ordinates = vec![];
    for (i, ring) in rings.iter().enumerate() {
        let (poly, part) = assignments[i].unwrap();
        for (j, p) in ring.points().iter().enumerate() {
            let trailing = trailing(p);
            if !trailing.is_empty() {
                ordinates.push(SourceOrdinate {
                    coordinate_path: vec![poly, part, j],
                    trailing,
                });
            }
        }
    }
    Ok((Some(geo_types::MultiPolygon(polygons).into()), ordinates))
}
fn encoding(
    label: Option<&str>,
    marker: u8,
) -> Result<&'static encoding_rs::Encoding, GeelloError> {
    let label = label
        .map(|s| s.trim_start_matches('\u{feff}').to_ascii_uppercase())
        .unwrap_or_else(|| {
            match marker {
                0xf0 => "UTF-8",
                0x7a => "GBK",
                0x03 | 0x57 => "WINDOWS-1252",
                0x7b => "SHIFT_JIS",
                _ => "",
            }
            .into()
        });
    let label = match label.as_str() {
        "65001" => "UTF-8",
        "936" | "CP936" => "GBK",
        s => s,
    };
    encoding_rs::Encoding::for_label(label.as_bytes())
        .ok_or_else(|| source("explicit or reliable DBF encoding required"))
}
fn dbf_value(
    kind: u8,
    decimals: u8,
    bytes: &[u8],
    enc: &'static encoding_rs::Encoding,
) -> Result<PropertyValue, GeelloError> {
    use PropertyValue::*;
    if kind == b'I' {
        return if bytes.len() == 4 {
            Ok(I64(i32::from_le_bytes(bytes.try_into().unwrap()) as i64))
        } else {
            Err(source("invalid DBF integer width"))
        };
    }
    let text = enc
        .decode_without_bom_handling_and_without_replacement(bytes)
        .ok_or_else(|| source("undecodable DBF bytes"))?;
    let text = text.trim_matches([' ', '\0']);
    if text.is_empty() {
        return Ok(Null);
    }
    Ok(match kind {
        b'C' => String(text.to_owned()),
        b'N' if decimals == 0 => {
            if let Ok(n) = text.parse::<i64>() {
                I64(n)
            } else {
                U64(text.parse().map_err(source)?)
            }
        }
        b'N' | b'F' => {
            let n: f64 = text.parse().map_err(source)?;
            if !n.is_finite() {
                return Err(source("nonfinite DBF number"));
            }
            F64(n)
        }
        b'L' => match text {
            "T" | "t" | "Y" | "y" | "1" => Bool(true),
            "F" | "f" | "N" | "n" | "0" => Bool(false),
            "?" => Null,
            _ => return Err(source("invalid logical DBF value")),
        },
        b'D' => String(text.to_owned()),
        _ => {
            return Err(GeelloError::Unsupported(format!(
                "DBF field type {}",
                kind as char
            )));
        }
    })
}
fn preflight_shape(raw: &[u8], max: usize) -> Result<(), GeelloError> {
    let read = |off: usize| -> Result<i32, GeelloError> {
        Ok(i32::from_le_bytes(
            raw.get(off..off + 4)
                .ok_or_else(|| source("short SHP record"))?
                .try_into()
                .unwrap(),
        ))
    };
    let points = match read(0)? {
        0 | 1 | 11 | 21 => return Ok(()),
        8 | 18 | 28 => read(36)?,
        3 | 5 | 13 | 15 | 23 | 25 => {
            let parts = read(36)?;
            if parts < 0 || parts as usize > raw.len() / 4 {
                return Err(source("invalid SHP part count"));
            }
            let points = read(40)?;
            if points < 0 || (parts == 0) != (points == 0) {
                return Err(source("invalid SHP parts/points"));
            }
            let mut previous = -1;
            for part in 0..parts as usize {
                let start = read(44 + part * 4)?;
                if start < 0 || start >= points || start <= previous || (part == 0 && start != 0) {
                    return Err(source("invalid SHP part offset"));
                }
                previous = start;
            }
            let required = 44usize
                .checked_add(parts as usize * 4)
                .and_then(|n| n.checked_add(points as usize * 16))
                .ok_or(GeelloError::SizeOverflow)?;
            if required > raw.len() {
                return Err(source("truncated SHP coordinates"));
            }
            points
        }
        31 => return Err(GeelloError::Unsupported("Shapefile MultiPatch".into())),
        _ => return Err(source("unknown SHP record type")),
    };
    if points < 0 || points as usize > raw.len() / 16 {
        return Err(source("invalid SHP point count"));
    }
    if points as usize > max {
        return Err(GeelloError::BudgetExceeded("coordinates"));
    }
    Ok(())
}
/// M arrays are optional for Z records. Preserve absence rather than accepting
/// the decoder's synthesized NO_DATA ordinate as an original source value.
fn has_measure(raw: &[u8]) -> Result<bool, GeelloError> {
    let kind = i32::from_le_bytes(raw[..4].try_into().unwrap());
    let count = |offset| -> usize {
        i32::from_le_bytes(raw[offset..offset + 4].try_into().unwrap()) as usize
    };
    let without = match kind {
        11 => 28,
        18 => 40 + count(36) * 16 + 16 + count(36) * 8,
        13 | 15 => 44 + count(36) * 4 + count(40) * 16 + 16 + count(40) * 8,
        _ => return Ok(true),
    };
    if raw.len() < without {
        return Err(source("truncated SHP Z array"));
    }
    Ok(raw.len() > without)
}
fn ordinates(shape: &Shape) -> Result<Vec<SourceOrdinate>, GeelloError> {
    let mut out = vec![];
    macro_rules! point {
        ($p:expr,$path:expr,z) => {
            out.push(SourceOrdinate {
                coordinate_path: $path,
                trailing: vec![$p.z, $p.m],
            })
        };
        ($p:expr,$path:expr,m) => {
            out.push(SourceOrdinate {
                coordinate_path: $path,
                trailing: vec![$p.m],
            })
        };
    }
    macro_rules! parts {
        ($s:expr,$kind:ident) => {
            for (i, part) in $s.parts().iter().enumerate() {
                for (j, p) in part.iter().enumerate() {
                    point!(p, vec![i, j], $kind)
                }
            }
        };
    }
    macro_rules! rings {
        ($s:expr,$kind:ident) => {{
            let mut poly = 0;
            let mut ring = 0;
            let mut started = false;
            for part in $s.rings() {
                match part {
                    PolygonRing::Outer(_) => {
                        if started {
                            poly += 1;
                        }
                        ring = 0;
                        started = true
                    }
                    PolygonRing::Inner(_) => ring += 1,
                };
                let pts = part.points();
                if !pts.is_empty() && (pts.len() < 4 || pts.first() != pts.last()) {
                    return Err(source("unclosed SHP polygon ring"));
                }
                for (j, p) in pts.iter().enumerate() {
                    point!(p, vec![poly, ring, j], $kind)
                }
            }
        }};
    }
    match shape {
        Shape::PointZ(p) => point!(p, vec![], z),
        Shape::PointM(p) => point!(p, vec![], m),
        Shape::MultipointZ(s) => {
            for (i, p) in s.points().iter().enumerate() {
                point!(p, vec![i], z)
            }
        }
        Shape::MultipointM(s) => {
            for (i, p) in s.points().iter().enumerate() {
                point!(p, vec![i], m)
            }
        }
        Shape::PolylineZ(s) => parts!(s, z),
        Shape::PolylineM(s) => parts!(s, m),
        Shape::PolygonZ(s) => rings!(s, z),
        Shape::PolygonM(s) => rings!(s, m),
        Shape::Polygon(s) => {
            for part in s.rings() {
                let pts = part.points();
                if !pts.is_empty() && (pts.len() < 4 || pts.first() != pts.last()) {
                    return Err(source("unclosed SHP polygon ring"));
                }
            }
        }
        _ => {}
    }
    Ok(out)
}
