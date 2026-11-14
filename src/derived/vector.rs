//! Indexed XY shards and typed companions. Physical Hilbert order never leaks
//! into query order; null/empty geometries live only in the companion stream.
use super::*;
use crate::readers::{VectorQuery, VectorSnapshot};
use crate::{
    Accounted, Crs, Feature, FeatureId, OperationContext, Properties, PropertyValue,
    SourceOrdinate, VectorBatch,
};
use flatgeobuf::geozero::{
    ColumnValue, CoordDimensions, GeomProcessor, GeozeroGeometry, PropertyProcessor,
};
use flatgeobuf::{
    ColumnType, FallibleStreamingIterator, FeatureProperties, FgbReader, FgbWriter,
    FgbWriterOptions, GeometryType,
};
use geo::BoundingRect;
use geo_types::Geometry;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Tagged properties preserve integer-versus-float identity and IEEE bit patterns.
#[derive(Serialize, Deserialize)]
enum Value {
    Null,
    Bool(bool),
    String(String),
    I64(i64),
    U64(u64),
    F64(u64),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}
impl From<&PropertyValue> for Value {
    fn from(v: &PropertyValue) -> Self {
        match v {
            PropertyValue::Null => Self::Null,
            PropertyValue::Bool(v) => Self::Bool(*v),
            PropertyValue::String(v) => Self::String(v.clone()),
            PropertyValue::I64(v) => Self::I64(*v),
            PropertyValue::U64(v) => Self::U64(*v),
            PropertyValue::F64(v) => Self::F64(v.to_bits()),
            PropertyValue::Array(v) => Self::Array(v.iter().map(Self::from).collect()),
            PropertyValue::Object(v) => {
                Self::Object(v.iter().map(|(k, v)| (k.clone(), Self::from(v))).collect())
            }
        }
    }
}
impl From<Value> for PropertyValue {
    fn from(v: Value) -> Self {
        match v {
            Value::Null => Self::Null,
            Value::Bool(v) => Self::Bool(v),
            Value::String(v) => Self::String(v),
            Value::I64(v) => Self::I64(v),
            Value::U64(v) => Self::U64(v),
            Value::F64(v) => Self::F64(f64::from_bits(v)),
            Value::Array(v) => Self::Array(v.into_iter().map(Self::from).collect()),
            Value::Object(v) => {
                Self::Object(v.into_iter().map(|(k, v)| (k, Self::from(v))).collect())
            }
        }
    }
}
#[derive(Serialize, Deserialize)]
struct Record {
    source_id: Option<Value>,
    id: Option<FeatureId>,
    geometry: Option<Geometry<f64>>,
    properties: BTreeMap<String, Value>,
    ordinal: u64,
    ordinates: Vec<(Vec<usize>, Vec<u64>)>,
}
impl From<&Feature> for Record {
    fn from(f: &Feature) -> Self {
        Self {
            source_id: f.source_id.as_ref().map(Value::from),
            id: f.id.clone(),
            geometry: f.geometry.clone(),
            properties: f
                .properties
                .iter()
                .map(|(k, v)| (k.clone(), Value::from(v)))
                .collect(),
            ordinal: f.source_ordinal,
            ordinates: f
                .source_ordinates
                .iter()
                .map(|o| {
                    (
                        o.coordinate_path.clone(),
                        o.trailing.iter().map(|v| v.to_bits()).collect(),
                    )
                })
                .collect(),
        }
    }
}
impl From<Record> for Feature {
    fn from(r: Record) -> Self {
        Self {
            source_id: r.source_id.map(PropertyValue::from),
            id: r.id,
            geometry: r.geometry,
            properties: r
                .properties
                .into_iter()
                .map(|(k, v)| (k, PropertyValue::from(v)))
                .collect::<Properties>(),
            source_ordinal: r.ordinal,
            source_ordinates: r
                .ordinates
                .into_iter()
                .map(|(coordinate_path, v)| SourceOrdinate {
                    coordinate_path,
                    trailing: v.into_iter().map(f64::from_bits).collect(),
                })
                .collect(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    offset: u64,
    length: usize,
    ordinal: u64,
    id: Option<FeatureId>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Shard {
    index: Option<StoredFile>,
    records: StoredFile,
    locations: Vec<Locator>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    data_version: String,
    crs: Crs,
    shards: Vec<Shard>,
}

/// The immutable store owns metadata only; cursors decode bounded source batches.
#[derive(Debug, Clone)]
pub struct DerivedVector {
    root: PathBuf,
    manifest: Manifest,
    limits: StoreLimits,
    version: String,
}
impl DerivedVector {
    pub fn build(
        parent: &Path,
        source: &VectorSnapshot,
        limits: StoreLimits,
        ctx: &OperationContext,
    ) -> Result<Self, GeelloError> {
        limits.validate()?;
        let staging = stage(parent)?;
        let mut shards = vec![];
        let mut pending = vec![];
        let mut holds = vec![];
        let mut used = 0;
        let mut cursor = source.query(VectorQuery::default())?;
        while let Some(batch) = cursor.next_batch(ctx)? {
            for f in &batch.features {
                ctx.checkpoint(1)?;
                holds.push(
                    ctx.reserve(
                        crate::core::feature_clone_bytes(f)?
                            .checked_mul(4)
                            .ok_or(GeelloError::SizeOverflow)?,
                    )?,
                );
                pending.push(f.clone());
                if pending.len() == limits.max_shard_features {
                    shards.push(write_shard(
                        staging.path(),
                        shards.len(),
                        &pending,
                        &mut used,
                        limits,
                        ctx,
                    )?);
                    pending.clear();
                    holds.clear();
                }
            }
        }
        if !pending.is_empty() {
            shards.push(write_shard(
                staging.path(),
                shards.len(),
                &pending,
                &mut used,
                limits,
                ctx,
            )?);
        }
        let manifest_value = Manifest {
            schema: 1,
            data_version: source.metadata().data_version.clone(),
            crs: source.metadata().crs.clone(),
            shards,
        };
        let version = manifest(staging.path(), &manifest_value, &mut used, limits)?;
        let destination = parent.join(&version);
        if !destination.exists() {
            std::fs::rename(staging.path(), &destination).map_err(error)?;
        }
        Self::open(&destination, limits)
    }
    pub fn open(root: &Path, limits: StoreLimits) -> Result<Self, GeelloError> {
        limits.validate()?;
        let manifest: Manifest = load_manifest(root, limits.max_manifest_bytes)?;
        if manifest.schema != 1
            || manifest.shards.iter().any(|s| {
                s.locations.len() > limits.max_shard_features
                    || s.locations.iter().any(|l| {
                        l.length > limits.max_record_bytes
                            || l.offset
                                .checked_add(l.length as u64)
                                .is_none_or(|e| e > s.records.bytes)
                    })
            })
        {
            return Err(error("incompatible vector manifest"));
        }
        let bytes = serde_json::to_vec(&manifest).map_err(error)?;
        let version = blake3::hash(&bytes).to_hex().to_string();
        if root.file_name().and_then(|n| n.to_str()) != Some(version.as_str()) {
            return Err(error("vector manifest identity mismatch"));
        }
        Ok(Self {
            root: root.into(),
            manifest,
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
    pub fn crs(&self) -> &Crs {
        &self.manifest.crs
    }
    pub fn query(
        &self,
        query: VectorQuery,
        ctx: &OperationContext,
    ) -> Result<DerivedVectorCursor, GeelloError> {
        if let Some(f) = &query.filter {
            f.validate()?;
        }
        if let Some((b, _)) = query.spatial {
            b.validate()?;
        }
        let count: usize = self.manifest.shards.iter().map(|s| s.locations.len()).sum();
        let hold = ctx.reserve(count.checked_mul(128).ok_or(GeelloError::SizeOverflow)?)?;
        let mut selected = vec![];
        for (n, shard) in self.manifest.shards.iter().enumerate() {
            ctx.checkpoint(1)?;
            // Checksum the companion before any successful page is returned.
            let _verified =
                read_file(&self.root, &shard.records, self.limits.max_block_bytes, ctx)?;
            let candidates = if let Some((b, _)) = query.spatial {
                let mut ids = HashSet::new();
                if let Some(index) = &shard.index {
                    let bytes = read_file(&self.root, index, self.limits.max_block_bytes, ctx)?;
                    let mut input = Cursor::new(bytes.as_slice());
                    let mut reader = FgbReader::open(&mut input)
                        .map_err(error)?
                        .select_bbox(b.min_x, b.min_y, b.max_x, b.max_y)
                        .map_err(error)?;
                    while let Some(f) = reader.next().map_err(error)? {
                        ids.insert(f.property::<u64>("ordinal").map_err(error)?);
                    }
                }
                Some(ids)
            } else {
                None
            };
            for (j, l) in shard.locations.iter().enumerate() {
                if candidates.as_ref().is_some_and(|v| !v.contains(&l.ordinal)) {
                    continue;
                }
                if !query.ids.is_empty() && !l.id.as_ref().is_some_and(|id| query.ids.contains(id))
                {
                    continue;
                }
                selected.push((l.ordinal, n, j));
            }
        }
        selected.sort_unstable();
        Ok(DerivedVectorCursor {
            store: self.clone(),
            query,
            selected,
            next: 0,
            failed: false,
            _hold: hold,
        })
    }
}
pub struct DerivedVectorCursor {
    store: DerivedVector,
    query: VectorQuery,
    selected: Vec<(u64, usize, usize)>,
    next: usize,
    failed: bool,
    _hold: crate::operation::Reservation,
}
impl DerivedVectorCursor {
    /// A failed cursor never returns another successful page. Full geometries
    /// are restored even when the index selected only bbox candidates.
    pub fn next_batch(
        &mut self,
        max_features: usize,
        max_bytes: usize,
        ctx: &OperationContext,
    ) -> Result<Option<Accounted<VectorBatch>>, GeelloError> {
        if self.failed {
            return Err(error("cursor failed"));
        }
        let result = self.read(max_features, max_bytes, ctx);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn read(
        &mut self,
        max_features: usize,
        max_bytes: usize,
        ctx: &OperationContext,
    ) -> Result<Option<Accounted<VectorBatch>>, GeelloError> {
        if max_features == 0 || max_bytes == 0 {
            return Err(error("positive batch limits required"));
        }
        let mut features = vec![];
        let mut reservations = vec![];
        let mut used = 0;
        while self.next < self.selected.len() && features.len() < max_features {
            ctx.checkpoint(1)?;
            let (_, s, l) = self.selected[self.next];
            let shard = &self.store.manifest.shards[s];
            let loc = &shard.locations[l];
            let envelope = loc
                .length
                .checked_mul(16)
                .ok_or(GeelloError::SizeOverflow)?;
            if envelope > max_bytes {
                return Err(GeelloError::BudgetExceeded("derived batch"));
            }
            if used + envelope > max_bytes {
                break;
            }
            let reservation = ctx.reserve(envelope)?;
            let mut file = File::open(self.store.root.join(&shard.records.name)).map_err(error)?;
            file.seek(SeekFrom::Start(loc.offset)).map_err(error)?;
            let mut bytes = vec![0; loc.length];
            file.read_exact(&mut bytes).map_err(error)?;
            let record: Record = serde_json::from_slice(&bytes).map_err(error)?;
            let feature = Feature::from(record);
            self.next += 1;
            if feature.source_ordinal != loc.ordinal {
                return Err(error("companion ordinal mismatch"));
            }
            if crate::readers::matches_query(&feature, &self.query)? {
                features.push(feature);
                reservations.push(reservation);
                used += envelope;
            }
        }
        if features.is_empty() {
            return Ok(None);
        }
        Ok(Some(Accounted::new(
            VectorBatch {
                crs: self.store.manifest.crs.clone(),
                features,
            },
            reservations,
        )))
    }
}
fn write_shard(
    root: &Path,
    n: usize,
    features: &[Feature],
    used: &mut u64,
    limits: StoreLimits,
    ctx: &OperationContext,
) -> Result<Shard, GeelloError> {
    // Upstream FgbWriter spools an unnamed system temporary file. Charge a
    // conservative shard envelope before construction, including coexistence
    // with the final index. No process-wide temporary-directory mutation is
    // used to redirect another crate's files.
    let envelope = features.iter().try_fold(4096u64, |n, f| {
        n.checked_add(
            (crate::core::feature_clone_bytes(f)? as u64)
                .checked_mul(4)
                .ok_or(GeelloError::SizeOverflow)?,
        )
        .and_then(|v| v.checked_add(4096))
        .ok_or(GeelloError::SizeOverflow)
    })?;
    if envelope > limits.max_block_bytes as u64
        || used
            .checked_add(envelope.saturating_mul(3))
            .is_none_or(|v| v > limits.max_disk_bytes)
    {
        return Err(GeelloError::BudgetExceeded("FGB transient disk envelope"));
    }
    let _scratch =
        ctx.reserve(usize::try_from(envelope).map_err(|_| GeelloError::SizeOverflow)?)?;
    let mut writer = FgbWriter::create_with_options(
        "geello-xy",
        GeometryType::Unknown,
        FgbWriterOptions {
            detect_type: false,
            promote_to_multi: false,
            ..Default::default()
        },
    )
    .map_err(error)?;
    writer.add_column("ordinal", ColumnType::ULong, |_, c| c.nullable = false);
    let mut bytes = vec![];
    let mut locations = vec![];
    let mut indexed = 0;
    for f in features {
        ctx.checkpoint(1)?;
        let record = serde_json::to_vec(&Record::from(f)).map_err(error)?;
        if record.len() > limits.max_record_bytes {
            return Err(GeelloError::BudgetExceeded("companion record"));
        }
        if bytes
            .len()
            .checked_add(record.len())
            .is_none_or(|v| v > limits.max_block_bytes)
        {
            return Err(GeelloError::BudgetExceeded("companion shard"));
        }
        locations.push(Locator {
            offset: bytes.len() as u64,
            length: record.len(),
            ordinal: f.source_ordinal,
            id: f.id.clone(),
        });
        bytes.extend(record);
        if let Some(g) = &f.geometry
            && g.bounding_rect().is_some()
        {
            writer
                .add_feature_geom(Xy(g), |p| {
                    p.property(0, "ordinal", &ColumnValue::ULong(f.source_ordinal))
                        .expect("declared ordinal column");
                })
                .map_err(error)?;
            indexed += 1;
        }
    }
    let records = write_file(
        root,
        &format!("{n}.records"),
        &bytes,
        used,
        limits.max_disk_bytes,
    )?;
    let index = if indexed > 0 {
        let mut buffer = vec![];
        writer.write(&mut buffer).map_err(error)?;
        if buffer.len() > limits.max_block_bytes {
            return Err(GeelloError::BudgetExceeded("FGB shard"));
        }
        Some(write_file(
            root,
            &format!("{n}.fgb"),
            &buffer,
            used,
            limits.max_disk_bytes,
        )?)
    } else {
        None
    };
    Ok(Shard {
        index,
        records,
        locations,
    })
}
struct Xy<'a>(&'a Geometry<f64>);
impl GeozeroGeometry for Xy<'_> {
    fn dims(&self) -> CoordDimensions {
        CoordDimensions::xy()
    }
    fn process_geom<P: GeomProcessor>(&self, p: &mut P) -> flatgeobuf::geozero::error::Result<()> {
        process(self.0, p, 0)
    }
}
fn process<P: GeomProcessor>(
    g: &Geometry<f64>,
    p: &mut P,
    i: usize,
) -> flatgeobuf::geozero::error::Result<()> {
    use geo_types::Geometry::*;
    fn line<P: GeomProcessor>(
        l: &geo_types::LineString,
        p: &mut P,
        tagged: bool,
        i: usize,
    ) -> flatgeobuf::geozero::error::Result<()> {
        p.linestring_begin(tagged, l.0.len(), i)?;
        for (n, c) in l.0.iter().enumerate() {
            p.xy(c.x, c.y, n)?;
        }
        p.linestring_end(tagged, i)
    }
    fn polygon<P: GeomProcessor>(
        v: &geo_types::Polygon,
        p: &mut P,
        tagged: bool,
        i: usize,
    ) -> flatgeobuf::geozero::error::Result<()> {
        p.polygon_begin(tagged, v.interiors().len() + 1, i)?;
        line(v.exterior(), p, false, 0)?;
        for (n, l) in v.interiors().iter().enumerate() {
            line(l, p, false, n + 1)?;
        }
        p.polygon_end(tagged, i)
    }
    match g {
        Point(v) => {
            p.point_begin(i)?;
            p.xy(v.x(), v.y(), 0)?;
            p.point_end(i)
        }
        LineString(v) => line(v, p, true, i),
        Polygon(v) => polygon(v, p, true, i),
        MultiPoint(v) => {
            p.multipoint_begin(v.0.len(), i)?;
            for (n, v) in v.0.iter().enumerate() {
                p.xy(v.x(), v.y(), n)?;
            }
            p.multipoint_end(i)
        }
        MultiLineString(v) => {
            p.multilinestring_begin(v.0.len(), i)?;
            for (n, l) in v.0.iter().enumerate() {
                line(l, p, false, n)?;
            }
            p.multilinestring_end(i)
        }
        MultiPolygon(v) => {
            p.multipolygon_begin(v.0.len(), i)?;
            for (n, v) in v.0.iter().enumerate() {
                polygon(v, p, false, n)?;
            }
            p.multipolygon_end(i)
        }
        GeometryCollection(v) => {
            p.geometrycollection_begin(v.0.len(), i)?;
            for (n, g) in v.0.iter().enumerate() {
                process(g, p, n)?;
            }
            p.geometrycollection_end(i)
        }
        Line(v) => line(&geo_types::LineString(vec![v.start, v.end]), p, true, i),
        Rect(v) => polygon(&v.to_polygon(), p, true, i),
        Triangle(v) => polygon(&v.to_polygon(), p, true, i),
    }
}
