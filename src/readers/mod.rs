//! Layer-two bounded input preparation and independent native query cursors.
//! Snapshots own immutable records; preparation may fail the memory budget rather
//! than spill implicitly. Derived disk-backed snapshots belong to M3.
#[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
use crate::PropertyValue;
use crate::operation::Reservation;
use crate::query::Filter;
use crate::{Bounds, Crs, Feature, FeatureId, GeelloError, OperationContext, VectorBatch};
#[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
use geo::CoordsIter;
use geo::{BoundingRect, Intersects};
use geo_types::{Coord, Rect};
#[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
use std::collections::HashSet;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Deref,
    sync::Arc,
};

#[cfg(feature = "read-geojson")]
mod json;
#[cfg(feature = "read-geojson")]
pub use json::*;
#[cfg(feature = "read-shapefile")]
mod shape;
#[cfg(feature = "read-shapefile")]
pub use shape::*;
#[cfg(feature = "read-geotiff")]
mod tiff;
#[cfg(feature = "read-geotiff")]
pub use tiff::*;

/// All limits are caller-selected, not production defaults. Every feature is atomic.
#[derive(Debug, Clone)]
pub struct ReaderLimits {
    pub max_source_bytes: u64,
    pub max_record_bytes: usize,
    pub max_features: usize,
    pub max_coordinates_per_feature: usize,
    pub max_depth: usize,
    pub batch_features: usize,
    pub batch_bytes: usize,
}
impl ReaderLimits {
    pub fn validate(&self) -> Result<(), GeelloError> {
        if self.max_source_bytes == 0
            || self.max_record_bytes == 0
            || self.max_features == 0
            || self.max_depth == 0
            || self.max_depth > 64
            || self.batch_features == 0
            || self.batch_bytes == 0
        {
            return Err(GeelloError::InvalidSource(
                "positive reader limits and depth <=64 required".into(),
            ));
        }
        Ok(())
    }
}
/// Tolerant recovery is restricted to structurally framed records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPolicy {
    Strict,
    SkipInvalidRecords,
}
#[derive(Debug, Clone)]
pub enum IdentityPolicy {
    /// Source IDs remain typed; absent IDs use a disjoint version-local namespace.
    OriginalOrOrdinal,
    Attribute(String),
}
#[derive(Debug, Clone)]
pub struct VectorReadOptions {
    /// Supplied immutable source identity; changes when bytes/interpretation change.
    pub data_version: String,
    pub crs_override: Option<Crs>,
    pub limits: ReaderLimits,
    pub policy: ReadPolicy,
    pub identity: IdentityPolicy,
    pub queryables: Vec<Queryable>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    String,
    Boolean,
    Integer,
    Number,
}
#[derive(Debug, Clone)]
pub struct Queryable {
    pub name: String,
    pub field: String,
    pub kind: ScalarType,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertyKind {
    Null,
    Boolean,
    String,
    Integer,
    Number,
    Array,
    Object,
}
#[derive(Debug, Clone)]
pub struct PropertySchema {
    pub kinds: BTreeSet<PropertyKind>,
    pub optional: bool,
}
#[derive(Debug, Clone)]
pub struct VectorMetadata {
    pub data_version: String,
    /// Generated effective IDs are distinct from all IDs supplied by the source.
    pub generated_ids: Vec<FeatureId>,
    pub crs: Crs,
    pub feature_count: usize,
    pub null_geometries: usize,
    pub bounds: Option<Bounds>,
    pub schema: BTreeMap<String, PropertySchema>,
    pub skipped_records: u64,
    pub diagnostics: Vec<String>,
}
#[derive(Debug)]
struct Records {
    features: Vec<Feature>,
    sizes: Vec<usize>,
    _reservations: Vec<Reservation>,
}
#[derive(Debug, Clone)]
pub struct VectorSnapshot {
    metadata: Arc<VectorMetadata>,
    records: Arc<Records>,
    limits: ReaderLimits,
}
impl VectorSnapshot {
    pub fn metadata(&self) -> &VectorMetadata {
        &self.metadata
    }
    pub fn query(&self, query: VectorQuery) -> Result<VectorCursor, GeelloError> {
        if let Some(filter) = &query.filter {
            filter.validate()?;
        }
        if let Some((bounds, _)) = &query.spatial {
            bounds.validate()?;
        }
        Ok(VectorCursor {
            snapshot: self.clone(),
            query,
            next: 0,
            terminal: false,
            failure: None,
        })
    }
}
#[derive(Debug, Clone, Copy)]
pub enum SpatialMatch {
    BoundingBoxCandidates,
    ExactIntersection,
}
#[derive(Debug, Clone, Default)]
pub struct VectorQuery {
    pub spatial: Option<(Bounds, SpatialMatch)>,
    pub ids: Vec<FeatureId>,
    pub filter: Option<Filter>,
}
/// A yielded batch keeps its allocation accounting alive independently of the cursor.
#[derive(Debug)]
pub struct ReadBatch {
    pub(crate) batch: VectorBatch,
    _reservation: Reservation,
}
impl Deref for ReadBatch {
    type Target = VectorBatch;
    fn deref(&self) -> &VectorBatch {
        &self.batch
    }
}
#[derive(Debug)]
pub struct VectorCursor {
    snapshot: VectorSnapshot,
    query: VectorQuery,
    next: usize,
    terminal: bool,
    failure: Option<GeelloError>,
}
impl VectorCursor {
    /// Errors are terminal. A subsequent call repeats no partial successful batch.
    pub fn next_batch(
        &mut self,
        context: &OperationContext,
    ) -> Result<Option<ReadBatch>, GeelloError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if self.terminal {
            return Ok(None);
        }
        let result = self.read(context);
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        if result.is_err() || matches!(result, Ok(None)) {
            self.terminal = true;
        }
        result
    }
    fn read(&mut self, context: &OperationContext) -> Result<Option<ReadBatch>, GeelloError> {
        context.check()?;
        let mut selected = vec![];
        let mut bytes = 0usize;
        while self.next < self.snapshot.records.features.len()
            && selected.len() < self.snapshot.limits.batch_features
        {
            context.checkpoint(1)?;
            let n = self.next;
            let f = &self.snapshot.records.features[n];
            if !matches_query(f, &self.query)? {
                self.next += 1;
                continue;
            }
            let size = self.snapshot.records.sizes[n];
            if size > self.snapshot.limits.batch_bytes {
                return Err(GeelloError::BudgetExceeded("batch bytes"));
            }
            if bytes.checked_add(size).ok_or(GeelloError::SizeOverflow)?
                > self.snapshot.limits.batch_bytes
            {
                break;
            }
            selected.push(n);
            bytes += size;
            self.next += 1;
        }
        if selected.is_empty() {
            return Ok(None);
        }
        let reservation = context.reserve(bytes)?;
        let features = selected
            .into_iter()
            .map(|n| self.snapshot.records.features[n].clone())
            .collect();
        Ok(Some(ReadBatch {
            batch: VectorBatch {
                crs: self.snapshot.metadata.crs.clone(),
                features,
            },
            _reservation: reservation,
        }))
    }
}
fn matches_query(f: &Feature, q: &VectorQuery) -> Result<bool, GeelloError> {
    if !q.ids.is_empty() && !f.id.as_ref().is_some_and(|id| q.ids.contains(id)) {
        return Ok(false);
    }
    if let Some(filter) = &q.filter
        && !filter.evaluate(&f.properties)?.matches()
    {
        return Ok(false);
    }
    if let Some((b, mode)) = q.spatial {
        let Some(g) = &f.geometry else {
            return Ok(false);
        };
        let r = Rect::new(
            Coord {
                x: b.min_x,
                y: b.min_y,
            },
            Coord {
                x: b.max_x,
                y: b.max_y,
            },
        );
        return Ok(match mode {
            SpatialMatch::BoundingBoxCandidates => {
                g.bounding_rect().is_some_and(|b| b.intersects(&r))
            }
            SpatialMatch::ExactIntersection => g.intersects(&r),
        });
    }
    Ok(true)
}

#[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
pub(crate) struct SnapshotBuilder<'a> {
    options: &'a VectorReadOptions,
    context: &'a OperationContext,
    crs: Crs,
    features: Vec<Feature>,
    sizes: Vec<usize>,
    reservations: Vec<Reservation>,
    ids: HashSet<FeatureId>,
    skipped: u64,
    diagnostics: Vec<String>,
    generated_ordinals: Vec<usize>,
}
#[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
impl<'a> SnapshotBuilder<'a> {
    pub(crate) fn new(
        options: &'a VectorReadOptions,
        crs: Crs,
        context: &'a OperationContext,
    ) -> Result<Self, GeelloError> {
        options.limits.validate()?;
        if options.data_version.is_empty() {
            return Err(source("nonempty DataVersion required"));
        }
        context.check()?;
        let mut aliases = HashSet::new();
        for q in &options.queryables {
            if q.name.is_empty() || !aliases.insert(&q.name) {
                return Err(GeelloError::InvalidSource(
                    "duplicate/empty queryable alias".into(),
                ));
            }
        }
        Ok(Self {
            options,
            context,
            crs,
            features: vec![],
            sizes: vec![],
            reservations: vec![],
            ids: HashSet::new(),
            skipped: 0,
            diagnostics: vec![],
            generated_ordinals: vec![],
        })
    }
    pub(crate) fn push(
        &mut self,
        feature: Result<Feature, GeelloError>,
        encoded_bytes: usize,
    ) -> Result<(), GeelloError> {
        self.context.checkpoint(1)?;
        let result = feature.and_then(|mut f| {
            if self.features.len() >= self.options.limits.max_features {
                return Err(GeelloError::BudgetExceeded("features"));
            }
            if encoded_bytes > self.options.limits.max_record_bytes {
                return Err(GeelloError::BudgetExceeded("record bytes"));
            }
            if let Some(g) = &f.geometry {
                let mut count = 0;
                for c in g.coords_iter() {
                    self.context.checkpoint(1)?;
                    count += 1;
                    if !c.x.is_finite() || !c.y.is_finite() {
                        return Err(GeelloError::InvalidGeometry("nonfinite XY".into()));
                    }
                }
                if count > self.options.limits.max_coordinates_per_feature {
                    return Err(GeelloError::BudgetExceeded("coordinates"));
                }
            }
            let generated = matches!(self.options.identity, IdentityPolicy::OriginalOrOrdinal)
                && f.id.is_none();
            f.id = Some(match &self.options.identity {
                IdentityPolicy::OriginalOrOrdinal => f.id.clone().unwrap_or_else(|| {
                    FeatureId::String(format!(
                        "generated:{}:{}",
                        self.options.data_version, f.source_ordinal
                    ))
                }),
                IdentityPolicy::Attribute(key) => match f.properties.get(key) {
                    Some(PropertyValue::String(v)) => FeatureId::String(v.clone()),
                    Some(PropertyValue::I64(v)) => FeatureId::I64(*v),
                    Some(PropertyValue::U64(v)) => FeatureId::U64(*v),
                    _ => {
                        return Err(GeelloError::InvalidSource(
                            "identity attribute must be string/integer".into(),
                        ));
                    }
                },
            });
            for q in &self.options.queryables {
                if let Some(v) = f.properties.get(&q.field)
                    && !scalar_matches(v, q.kind)
                {
                    return Err(GeelloError::InvalidSource(format!(
                        "queryable {} type mismatch",
                        q.name
                    )));
                }
            }
            if !generated && self.ids.contains(f.id.as_ref().unwrap()) {
                return Err(GeelloError::InvalidSource(
                    "duplicate effective feature identity".into(),
                ));
            }
            // A conservative accounting envelope covers property nodes, geometry,
            // indexes, IDs and source-ordinate paths, not merely UTF-8 source bytes.
            let size = encoded_bytes
                .checked_mul(32)
                .and_then(|n| n.checked_add(1024))
                .ok_or(GeelloError::SizeOverflow)?;
            let guard = self.context.reserve(size)?;
            if generated {
                self.generated_ordinals.push(self.features.len());
            } else {
                self.ids.insert(f.id.as_ref().unwrap().clone());
            }
            self.features.push(f);
            self.sizes.push(size);
            self.reservations.push(guard);
            Ok(())
        });
        match result {
            Err(e @ (GeelloError::InvalidSource(_) | GeelloError::InvalidGeometry(_)))
                if self.options.policy == ReadPolicy::SkipInvalidRecords =>
            {
                self.skipped += 1;
                if self.diagnostics.len() < self.context.limits().max_diagnostics {
                    self.diagnostics.push(e.to_string())
                }
                Ok(())
            }
            other => other,
        }
    }
    pub(crate) fn finish(mut self) -> Result<VectorSnapshot, GeelloError> {
        // A source may itself contain our human-readable generated prefix.
        // Deterministically extend that prefix until generated and original ID
        // namespaces are disjoint for this complete source version.
        let mut generated_ids = HashSet::new();
        for &index in &self.generated_ordinals {
            self.context.checkpoint(1)?;
            let f = &mut self.features[index];
            let mut id = f.id.clone().unwrap();
            while self.ids.contains(&id) || generated_ids.contains(&id) {
                self.context.checkpoint(1)?;
                if let FeatureId::String(s) = &mut id {
                    if s.len() >= self.options.limits.max_record_bytes {
                        return Err(GeelloError::BudgetExceeded("generated identity bytes"));
                    }
                    s.insert(0, '_');
                }
            }
            generated_ids.insert(id.clone());
            f.id = Some(id);
        }
        let mut schema: BTreeMap<String, PropertySchema> = BTreeMap::new();
        let mut present: BTreeMap<String, usize> = BTreeMap::new();
        let mut bounds: Option<Bounds> = None;
        let mut nulls = 0;
        for f in &self.features {
            self.context.checkpoint(1)?;
            for (key, v) in &f.properties {
                self.context.checkpoint(1)?;
                *present.entry(key.clone()).or_default() += 1;
                schema
                    .entry(key.clone())
                    .or_insert_with(|| PropertySchema {
                        kinds: BTreeSet::new(),
                        optional: false,
                    })
                    .kinds
                    .insert(kind(v));
            }
            if let Some(g) = &f.geometry {
                if let Some(r) = g.bounding_rect() {
                    let b = Bounds {
                        min_x: r.min().x,
                        min_y: r.min().y,
                        max_x: r.max().x,
                        max_y: r.max().y,
                    };
                    bounds = Some(match bounds {
                        None => b,
                        Some(a) => Bounds {
                            min_x: a.min_x.min(b.min_x),
                            min_y: a.min_y.min(b.min_y),
                            max_x: a.max_x.max(b.max_x),
                            max_y: a.max_y.max(b.max_y),
                        },
                    })
                }
            } else {
                nulls += 1
            }
        }
        for (key, s) in &mut schema {
            s.optional = present[key] < self.features.len()
        }
        Ok(VectorSnapshot {
            metadata: Arc::new(VectorMetadata {
                data_version: self.options.data_version.clone(),
                generated_ids: self
                    .generated_ordinals
                    .iter()
                    .map(|i| self.features[*i].id.clone().unwrap())
                    .collect(),
                crs: self.crs,
                feature_count: self.features.len(),
                null_geometries: nulls,
                bounds,
                schema,
                skipped_records: self.skipped,
                diagnostics: self.diagnostics,
            }),
            records: Arc::new(Records {
                features: self.features,
                sizes: self.sizes,
                _reservations: self.reservations,
            }),
            limits: self.options.limits.clone(),
        })
    }
}
#[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
fn scalar_matches(v: &PropertyValue, t: ScalarType) -> bool {
    matches!(
        (v, t),
        (PropertyValue::Null, _)
            | (PropertyValue::String(_), ScalarType::String)
            | (PropertyValue::Bool(_), ScalarType::Boolean)
            | (
                PropertyValue::I64(_) | PropertyValue::U64(_),
                ScalarType::Integer | ScalarType::Number
            )
            | (PropertyValue::F64(_), ScalarType::Number)
    )
}
#[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
fn kind(v: &PropertyValue) -> PropertyKind {
    match v {
        PropertyValue::Null => PropertyKind::Null,
        PropertyValue::Bool(_) => PropertyKind::Boolean,
        PropertyValue::String(_) => PropertyKind::String,
        PropertyValue::I64(_) | PropertyValue::U64(_) => PropertyKind::Integer,
        PropertyValue::F64(_) => PropertyKind::Number,
        PropertyValue::Array(_) => PropertyKind::Array,
        PropertyValue::Object(_) => PropertyKind::Object,
    }
}
pub(crate) fn io(e: impl std::fmt::Display) -> GeelloError {
    GeelloError::Io(e.to_string())
}
pub(crate) fn source(e: impl std::fmt::Display) -> GeelloError {
    GeelloError::InvalidSource(e.to_string())
}

/// Prepare a vector snapshot through the same immutable scene path used by
/// callers that already own `VectorBatch` values. This convenience function
/// does not perform projection or retain a reader handle; callers must provide
/// a batch whose CRS equals the view CRS or use `RustProjection` first.
#[cfg(any(feature = "render-cpu", feature = "render-gpu"))]
pub fn prepare_vector_snapshot(
    snapshot: &VectorSnapshot,
    view: &crate::View,
    symbols: Vec<crate::Symbol>,
    context: &OperationContext,
) -> Result<crate::PreparedScene, GeelloError> {
    let _scratch =
        context.reserve(snapshot.records.sizes.iter().try_fold(0usize, |sum, n| {
            sum.checked_add(*n).ok_or(GeelloError::SizeOverflow)
        })?)?;
    let batch = Arc::new(VectorBatch {
        crs: snapshot.metadata.crs.clone(),
        features: snapshot.records.features.clone(),
    });
    let composition = crate::MapComposition {
        items: vec![crate::DrawItem::Vector(crate::VectorItem {
            batches: vec![batch],
            symbols,
        })],
        background: None,
    };
    crate::prepare_scene(&composition, view, context)
}

/// Evaluate source-property rules before explicit projection, then freeze a
/// memory scene. Full native geometries are retained until final scene clipping.
#[cfg(all(feature = "config-toml", feature = "rust-proj"))]
pub fn prepare_styled_vector_snapshot(
    snapshot: &VectorSnapshot,
    query: VectorQuery,
    style: &crate::config::StyleDefinition,
    view: &crate::View,
    context: &OperationContext,
) -> Result<crate::PreparedScene, GeelloError> {
    let mut cursor = snapshot.query(query)?;
    let mut retained = vec![];
    while let Some(batch) = cursor.next_batch(context)? {
        retained.push(batch);
    }
    let mut clone_guards = vec![];
    let mut batches = vec![];
    for batch in &retained {
        let bytes = batch.features.iter().try_fold(256usize, |n, f| {
            n.checked_add(crate::core::feature_clone_bytes(f)?)
                .ok_or(GeelloError::SizeOverflow)
        })?;
        clone_guards.push(context.reserve(bytes)?);
        batches.push(Arc::new((**batch).clone()));
    }
    let mut composition = crate::config::resolve_vector_style(style, &batches, context)?;
    let mut projected = vec![];
    if snapshot.metadata.crs != view.crs {
        let operation = crate::projection::RustProjection::new(
            snapshot.metadata.crs.clone(),
            view.crs.clone(),
        )?;
        for item in &mut composition.value.items {
            if let crate::DrawItem::Vector(vector) = item {
                for batch in &mut vector.batches {
                    let output = operation.project_batch(batch, context)?;
                    clone_guards.push(context.reserve(output.features.iter().try_fold(
                        256usize,
                        |n, f| {
                            n.checked_add(crate::core::feature_clone_bytes(f)?)
                                .ok_or(GeelloError::SizeOverflow)
                        },
                    )?)?);
                    *batch = Arc::new((*output).clone());
                    projected.push(output);
                }
            }
        }
    }
    crate::prepare_scene(&composition, view, context)
}

/// One-shot local registered vector definition to immutable scene. HTTP path
/// validation/publication remains server-owned; this explicit layer-two API
/// accepts a prepared dataset definition, never an untrusted request path.
#[cfg(all(
    feature = "config-toml",
    feature = "rust-proj",
    any(feature = "read-geojson", feature = "read-shapefile")
))]
pub fn prepare_vector_dataset(
    dataset: &crate::config::DatasetDefinition,
    options: &VectorReadOptions,
    query: VectorQuery,
    style: &crate::config::StyleDefinition,
    view: &crate::View,
    context: &OperationContext,
) -> Result<crate::PreparedScene, GeelloError> {
    let mut options = options.clone();
    if let Some(crs) = &dataset.source.crs {
        let space = if crs == "OGC:CRS84" || crs == "EPSG:4326" {
            crate::CoordinateSpace::GeographicXy
        } else {
            options
                .crs_override
                .as_ref()
                .filter(|c| c.definition() == crs)
                .ok_or_else(|| source("explicit CRS coordinate space required"))?
                .space()
        };
        options.crs_override = Some(Crs::new(crs, space)?);
    }
    if let Some(identity) = &dataset.identity {
        options.identity = match identity.kind.as_str() {
            "attribute" => IdentityPolicy::Attribute(
                identity
                    .field
                    .clone()
                    .ok_or_else(|| source("identity attribute missing"))?,
            ),
            "source" | "original-or-ordinal" => IdentityPolicy::OriginalOrOrdinal,
            _ => return Err(source("invalid dataset identity policy")),
        };
    }
    options.queryables = dataset
        .queryables
        .iter()
        .map(|q| {
            Ok(Queryable {
                name: q.name.clone(),
                field: q.field.clone(),
                kind: match q.kind.as_str() {
                    "string" => ScalarType::String,
                    "boolean" => ScalarType::Boolean,
                    "integer" => ScalarType::Integer,
                    "number" => ScalarType::Number,
                    _ => return Err(source("invalid dataset queryable type")),
                },
            })
        })
        .collect::<Result<_, _>>()?;
    let snapshot = match dataset.source.kind.as_str() {
        #[cfg(feature = "read-geojson")]
        "geojson" => open_geojson(&dataset.source.path, &options, context)?,
        #[cfg(feature = "read-shapefile")]
        "shapefile" => open_shapefile_with(
            &dataset.source.path,
            &options,
            &ShapefileOptions {
                encoding: dataset.source.encoding.clone(),
            },
            context,
        )?,
        _ => {
            return Err(GeelloError::Unsupported(
                "dataset vector reader feature/format".into(),
            ));
        }
    };
    prepare_styled_vector_snapshot(&snapshot, query, style, view, context)
}
