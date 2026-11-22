//! Indexed XY shards and typed companions. Physical Hilbert order never leaks
//! into query order; null/empty geometries live only in the companion stream.
use super::*;
use crate::readers::{VectorQuery, VectorSnapshot};
use crate::{
    Accounted, Crs, Feature, FeatureId, OperationContext, Properties, PropertyValue,
    SourceOrdinate, VectorBatch,
};
use flatgeobuf::{FallibleStreamingIterator, FeatureProperties, FgbReader};
use geo::MapCoords;
use geo_types::Geometry;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{BufReader, Cursor, Read, Seek, SeekFrom},
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
    geometry: Option<Geometry<u64>>,
    properties: BTreeMap<String, Value>,
    ordinal: u64,
    ordinates: Vec<(Vec<usize>, Vec<u64>)>,
}
impl From<&Feature> for Record {
    fn from(f: &Feature) -> Self {
        Self {
            source_id: f.source_id.as_ref().map(Value::from),
            id: f.id.clone(),
            geometry: f.geometry.as_ref().map(|g| {
                g.map_coords(|c| geo_types::Coord {
                    x: c.x.to_bits(),
                    y: c.y.to_bits(),
                })
            }),
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
            geometry: r.geometry.map(|g| {
                g.map_coords(|c| geo_types::Coord {
                    x: f64::from_bits(c.x),
                    y: f64::from_bits(c.y),
                })
            }),
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
    #[serde(skip)]
    _metadata: Option<std::sync::Arc<crate::operation::Reservation>>,
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
    manifest: std::sync::Arc<Manifest>,
    _metadata: Option<std::sync::Arc<crate::operation::Reservation>>,
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
            schema: 2,
            data_version: source.metadata().data_version.clone(),
            crs: source.metadata().crs.clone(),
            shards,
        };
        let version = manifest(staging.path(), &manifest_value, &mut used, limits, ctx)?;
        let destination = parent.join(&version);
        verify_reused_directory(staging.path(), &destination, ctx)?;
        if !destination.exists() {
            std::fs::rename(staging.path(), &destination).map_err(error)?;
        }
        Self::open_with_context(&destination, limits, ctx)
    }
    /// Admit metadata before deserialization and retain it for every cursor.
    pub fn open_with_context(
        root: &Path,
        limits: StoreLimits,
        ctx: &OperationContext,
    ) -> Result<Self, GeelloError> {
        let hold = metadata_memory(root, limits, ctx)?;
        let mut store = Self::open(root, limits)?;
        store._metadata = Some(std::sync::Arc::new(hold));
        Ok(store)
    }
    /// Caller-owned metadata, bounded by max_manifest_bytes. Server operations
    /// must use open_with_context to retain hierarchical memory admission.
    pub fn open(root: &Path, limits: StoreLimits) -> Result<Self, GeelloError> {
        limits.validate()?;
        let manifest: Manifest = load_manifest(root, limits.max_manifest_bytes)?;
        if manifest.schema != 2
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
            manifest: std::sync::Arc::new(manifest),
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
    pub fn crs(&self) -> &Crs {
        &self.manifest.crs
    }
    pub fn query(
        &self,
        query: VectorQuery,
        ctx: &OperationContext,
    ) -> Result<DerivedVectorCursor, GeelloError> {
        self.query_with_spatial_exemptions(query, &[], ctx)
    }
    pub fn query_with_spatial_exemptions(
        &self,
        query: VectorQuery,
        spatial_exemptions: &[FeatureId],
        ctx: &OperationContext,
    ) -> Result<DerivedVectorCursor, GeelloError> {
        if let Some(f) = &query.filter {
            f.validate()?;
        }
        if let Some((b, _)) = query.spatial {
            b.validate()?;
        }
        let mut holds = vec![ctx.reserve(64 * 1024)?];
        holds.push(
            ctx.reserve(
                spatial_exemptions
                    .len()
                    .checked_mul(128)
                    .and_then(|bytes| {
                        spatial_exemptions
                            .iter()
                            .try_fold(bytes, |total, identity| {
                                total.checked_add(match identity {
                                    FeatureId::String(value) => value.len(),
                                    _ => 0,
                                })
                            })
                    })
                    .ok_or(GeelloError::SizeOverflow)?,
            )?,
        );
        let spatial_exemptions: HashSet<_> = spatial_exemptions.iter().cloned().collect();
        let mut selected = vec![];
        for (n, shard) in self.manifest.shards.iter().enumerate() {
            ctx.checkpoint(1)?;
            // Checksum the companion before any successful page is returned.
            let _verified =
                read_file(&self.root, &shard.records, self.limits.max_block_bytes, ctx)?;
            let _index_memory = if query.spatial.is_some() {
                Some(
                    ctx.reserve(
                        shard
                            .locations
                            .len()
                            .checked_mul(64)
                            .ok_or(GeelloError::SizeOverflow)?,
                    )?,
                )
            } else {
                None
            };
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
                if candidates.as_ref().is_some_and(|v| !v.contains(&l.ordinal))
                    && !l
                        .id
                        .as_ref()
                        .is_some_and(|identity| spatial_exemptions.contains(identity))
                {
                    continue;
                }
                if !query.ids.is_empty() && !l.id.as_ref().is_some_and(|id| query.ids.contains(id))
                {
                    continue;
                }
                if selected.len() == selected.capacity() {
                    let additional = selected.capacity().max(64);
                    holds.push(
                        ctx.reserve(
                            additional
                                .checked_mul(std::mem::size_of::<(u64, usize, usize)>())
                                .ok_or(GeelloError::SizeOverflow)?,
                        )?,
                    );
                    selected.reserve_exact(additional);
                }
                selected.push((l.ordinal, n, j));
            }
        }
        selected.sort_unstable();
        let mut unbounded_query = query.clone();
        unbounded_query.spatial = None;
        Ok(DerivedVectorCursor {
            store: self.clone(),
            query,
            unbounded_query,
            spatial_exemptions,
            selected,
            next: 0,
            failed: false,
            file: None,
            _holds: holds,
        })
    }
}
pub struct DerivedVectorCursor {
    store: DerivedVector,
    query: VectorQuery,
    unbounded_query: VectorQuery,
    spatial_exemptions: HashSet<FeatureId>,
    selected: Vec<(u64, usize, usize)>,
    next: usize,
    failed: bool,
    file: Option<(usize, BufReader<File>, u64)>,
    _holds: Vec<crate::operation::Reservation>,
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
            if self.file.as_ref().is_none_or(|(index, _, _)| *index != s) {
                let file = File::open(self.store.root.join(&shard.records.name)).map_err(error)?;
                self.file = Some((s, BufReader::with_capacity(64 * 1024, file), 0));
            }
            let (_, file, position) = self.file.as_mut().unwrap();
            if *position != loc.offset {
                file.seek(SeekFrom::Start(loc.offset)).map_err(error)?;
            }
            let mut bytes = vec![0; loc.length];
            file.read_exact(&mut bytes).map_err(error)?;
            *position = loc.offset + loc.length as u64;
            let record: Record = serde_json::from_slice(&bytes).map_err(error)?;
            let feature = Feature::from(record);
            self.next += 1;
            if feature.source_ordinal != loc.ordinal {
                return Err(error("companion ordinal mismatch"));
            }
            let query = if feature
                .id
                .as_ref()
                .is_some_and(|identity| self.spatial_exemptions.contains(identity))
            {
                &self.unbounded_query
            } else {
                &self.query
            };
            if crate::readers::matches_query(&feature, query)? {
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
    // All serialized shard bytes live in admitted memory until exact disk
    // admission in write_file. No files are written outside this staging root.
    let envelope = features.iter().try_fold(4096usize, |n, f| {
        n.checked_add(super::fgb::envelope(f)?)
            .ok_or(GeelloError::SizeOverflow)
    })?;
    let _scratch = ctx.reserve(envelope.checked_mul(4).ok_or(GeelloError::SizeOverflow)?)?;
    let index_bytes = super::fgb::encode(features, limits.max_block_bytes, ctx)?;
    let mut bytes = vec![];
    let mut locations = vec![];
    for f in features {
        ctx.checkpoint(1)?;
        let record = bounded_json(&Record::from(f), limits.max_record_bytes, ctx)?;
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
        bytes.extend_from_slice(&record);
    }
    let records = write_file(
        root,
        &format!("{n}.records"),
        &bytes,
        used,
        limits.max_disk_bytes,
    )?;
    let index = if let Some(buffer) = index_bytes {
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
    let metadata_bytes = features.iter().try_fold(1024usize, |n, f| {
        let id_bytes = match &f.id {
            Some(FeatureId::String(s)) => s.len(),
            _ => 0,
        };
        n.checked_add(512)
            .and_then(|n| n.checked_add(id_bytes.checked_mul(4)?))
            .ok_or(GeelloError::SizeOverflow)
    })?;
    Ok(Shard {
        _metadata: Some(std::sync::Arc::new(ctx.reserve(metadata_bytes)?)),
        index,
        records,
        locations,
    })
}
