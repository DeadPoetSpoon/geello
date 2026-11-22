//! OGC API Features core-shaped discovery and GeoJSON output subset.
use super::http_extract::CheckedQuery as Query;
use super::{api, *};
use crate::{Feature, FeatureId};
use axum::{Router, extract::State, response::Response, routing::get};
use geo::Intersects;
use geo_types::Geometry;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::time::{Duration, Instant};

pub(crate) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/ogc/features", get(landing))
        .route("/ogc/features/", get(landing))
        .route("/ogc/features/conformance", get(conformance))
        .route("/ogc/features/collections", get(collections))
        .route("/ogc/features/collections/{id}", get(collection))
        .route("/ogc/features/collections/{id}/items", get(items))
        .route(
            "/ogc/features/collections/{id}/items/{feature_id}",
            get(item),
        )
}
fn json(s: &ServerState, value: &impl Serialize) -> Result<Response> {
    api::json_response(s, value)
}
async fn landing(State(s): State<Arc<ServerState>>) -> Result<Response> {
    json(
        &s,
        &serde_json::json!({"title":"Geello Features tested subset","description":"Published vector datasets as GeoJSON; fixed CRS84 XY, bounded offset/limit and bbox.","links":[{"href":format!("{}/ogc/features",s.config.public_base_url.trim_end_matches('/')),"rel":"self"},{"href":format!("{}/ogc/features/conformance",s.config.public_base_url.trim_end_matches('/')),"rel":"conformance"},{"href":format!("{}/ogc/features/collections",s.config.public_base_url.trim_end_matches('/')),"rel":"data"}]}),
    )
}
async fn conformance(State(s): State<Arc<ServerState>>) -> Result<Response> {
    json(
        &s,
        &serde_json::json!({"conformsTo":[],"x-geello-tested-subset":"features","x-geello-status":"tested subset; not an official conformance certification"}),
    )
}
fn base(s: &ServerState) -> String {
    s.config.public_base_url.trim_end_matches('/').to_owned()
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionsQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}
async fn collections(
    State(s): State<Arc<ServerState>>,
    Query(query): Query<CollectionsQuery>,
) -> Result<Response> {
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(s.config.profile.max_list_items);
    let (generation, records, more) = s.catalog.public_datasets(offset, limit)?;
    let values = records.iter().map(|r| serde_json::json!({"id":r.key.id,"title":r.key.id,"itemType":"feature","crs":["http://www.opengis.net/def/crs/OGC/1.3/CRS84"],"links":[{"href":format!("{}/ogc/features/collections/{}",base(&s),r.key.id),"rel":"self"},{"href":format!("{}/ogc/features/collections/{}/items",base(&s),r.key.id),"rel":"items"}]})).collect::<Vec<_>>();
    json(
        &s,
        &serde_json::json!({"collections":values,"links":collection_links(&s, offset, limit, records.len(), more)?,"x-geello-generation":generation}),
    )
}
fn collection_links(
    state: &ServerState,
    offset: usize,
    limit: usize,
    count: usize,
    more: bool,
) -> Result<Vec<serde_json::Value>> {
    let root = format!("{}/ogc/features/collections", base(state));
    let mut links = vec![
        serde_json::json!({"href":format!("{root}?offset={offset}&limit={limit}"),"rel":"self"}),
    ];
    if more {
        let next = offset
            .checked_add(count)
            .ok_or(crate::GeelloError::SizeOverflow)?;
        links.push(
            serde_json::json!({"href":format!("{root}?offset={next}&limit={limit}"),"rel":"next"}),
        );
    }
    Ok(links)
}
fn dataset(s: &ServerState, id: &str) -> Result<crate::Accounted<ResourceRecord>> {
    let r = s
        .catalog
        .get_accounted(&ResourceKey::new(ResourceKind::Dataset, id.to_owned())?)?;
    if !r.published || r.class.as_deref() != Some("vector") {
        return Err(ServerError::new(
            404,
            "resource-unpublished",
            "Feature collection is not published",
        ));
    }
    Ok(r)
}
async fn collection(
    State(s): State<Arc<ServerState>>,
    axum::extract::Path((id,)): axum::extract::Path<(String,)>,
) -> Result<Response> {
    let r = dataset(&s, &id)?;
    json(
        &s,
        &serde_json::json!({"id":id,"title":id,"itemType":"feature","crs":["http://www.opengis.net/def/crs/OGC/1.3/CRS84"],"links":[{"href":format!("{}/ogc/features/collections/{id}",base(&s)),"rel":"self"},{"href":format!("{}/ogc/features/collections/{id}/items",base(&s)),"rel":"items"}],"x-geello-data-version":r.data_version}),
    )
}
#[derive(Debug, Clone, PartialEq)]
struct FeatureQuery {
    bbox: Option<[f64; 4]>,
    offset: usize,
    limit: usize,
    cursor: Option<String>,
    datetime: Option<String>,
    equalities: Vec<(String, String)>,
    ids: Vec<FeatureId>,
    hits: bool,
    include_null_bbox: bool,
    fields: Vec<(String, crate::PropertyValue)>,
}
struct Snapshot {
    pin: ResourcePin,
    key: ResourceKey,
    query: FeatureQuery,
    expires: Instant,
    token: String,
    proof_key: std::hash::RandomState,
    _memory: crate::operation::Reservation,
}
/// Server-owned fixed snapshots. Access never extends the absolute deadline.
pub(crate) struct CursorStore {
    entries: Mutex<BTreeMap<String, Arc<Snapshot>>>,
    max: usize,
    ttl: Duration,
    memory: crate::OperationContext,
    changed: Arc<tokio::sync::Notify>,
}
impl CursorStore {
    pub(crate) fn new(
        profile: &ServerProfile,
        memory: crate::OperationContext,
        activities: Arc<super::activity::Activities>,
    ) -> Arc<Self> {
        let store = Arc::new(Self {
            entries: Mutex::new(BTreeMap::new()),
            max: profile.max_feature_cursors,
            ttl: Duration::from_millis(profile.feature_cursor_ttl_ms),
            memory,
            changed: Arc::new(tokio::sync::Notify::new()),
        });
        let weak = Arc::downgrade(&store);
        let activity = activities
            .enter()
            .expect("cursor maintenance starts before shutdown");
        tokio::spawn(async move {
            let _activity = activity;
            loop {
                let Some(store) = weak.upgrade() else { break };
                let changed = store.changed.clone().notified_owned();
                tokio::pin!(changed);
                changed.as_mut().enable();
                let next = {
                    let mut entries = store.entries.lock().unwrap();
                    entries.retain(|_, value| value.expires > Instant::now());
                    entries.values().map(|v| v.expires).min()
                };
                drop(store);
                tokio::select! {
                    _ = activities.closed() => {
                        if let Some(store) = weak.upgrade() { store.entries.lock().unwrap().clear(); }
                        break;
                    }
                    _ = &mut changed => {},
                    _ = async { if let Some(next) = next { tokio::time::sleep_until(next).await } else { std::future::pending::<()>().await } } => {},
                }
            }
        });
        store
    }
    fn retain(&self, snapshot: Arc<Snapshot>) -> Result<()> {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, value| value.expires > Instant::now());
        if snapshot.expires <= Instant::now() {
            return Err(expired());
        }
        if !entries.contains_key(&snapshot.token) && entries.len() >= self.max {
            return Err(ServerError::new(
                503,
                "feature-cursor-capacity",
                "Feature snapshot capacity exhausted",
            ));
        }
        entries.insert(snapshot.token.clone(), snapshot);
        self.changed.notify_one();
        Ok(())
    }
    fn resolve(&self, token: &str, id: &str) -> Result<(Arc<Snapshot>, usize)> {
        let mut parts = token.split('.');
        if parts.next() != Some("v2") {
            return Err(expired());
        }
        let key = parts.next().ok_or_else(expired)?;
        let offset = parts
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or_else(expired)?;
        let proof = parts.next().ok_or_else(expired)?;
        if parts.next().is_some() {
            return Err(expired());
        }
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, value| value.expires > Instant::now());
        let snapshot = entries.get(key).ok_or_else(expired)?;
        if snapshot.key.id != id || proof != cursor_proof(snapshot, offset) {
            return Err(expired());
        }
        Ok((snapshot.clone(), offset))
    }
}
fn expired() -> ServerError {
    ServerError::new(
        410,
        "feature-cursor-expired",
        "Feature snapshot expired or unavailable; restart the query",
    )
}
fn cursor_proof(snapshot: &Snapshot, offset: usize) -> String {
    use std::hash::BuildHasher;
    format!(
        "{:016x}",
        snapshot.proof_key.hash_one((&snapshot.key.id, offset))
    )
}
fn encode_cursor(snapshot: &Snapshot, offset: usize) -> String {
    format!(
        "v2.{}.{}.{}",
        snapshot.token,
        offset,
        cursor_proof(snapshot, offset)
    )
}
fn query(uri: &axum::http::Uri, legacy: bool) -> Result<FeatureQuery> {
    let mut query = FeatureQuery {
        bbox: None,
        offset: 0,
        limit: 100,
        cursor: None,
        datetime: None,
        equalities: vec![],
        ids: vec![],
        hits: false,
        include_null_bbox: !legacy,
        fields: vec![],
    };
    let mut seen = BTreeMap::<String, ()>::new();
    for pair in uri
        .query()
        .unwrap_or("")
        .split('&')
        .filter(|x| !x.is_empty())
    {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| ServerError::invalid("Malformed feature query"))?;
        if seen.insert(k.to_owned(), ()).is_some() {
            return Err(ServerError::invalid("Duplicate feature query"));
        }
        let decoded = decode_query_value(v)?;
        let v = decoded.as_str();
        match k {
            "bbox" => {
                let n = v
                    .split(',')
                    .map(|x| x.parse::<f64>().ok().filter(|x| x.is_finite()))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| ServerError::invalid("Invalid bbox"))?;
                if n.len() != 4 || n[0] >= n[2] || n[1] >= n[3] {
                    return Err(ServerError::invalid("Invalid bbox"));
                }
                query.bbox = Some([n[0], n[1], n[2], n[3]]);
            }
            "offset" => {
                query.offset = v
                    .parse()
                    .map_err(|_| ServerError::invalid("Invalid offset"))?
            }
            "limit" => {
                query.limit = v
                    .parse()
                    .map_err(|_| ServerError::invalid("Invalid limit"))?
            }
            "cursor" if !v.is_empty() && v.len() <= 256 => query.cursor = Some(v.to_owned()),
            "datetime" => {
                let value = v.to_owned();
                validate_datetime(&value)?;
                query.datetime = Some(value);
            }
            "ids" => query.ids = v.split(',').map(decode_feature_id).collect::<Result<_>>()?,
            "resultType" if legacy && matches!(v, "hits" | "results") => query.hits = v == "hits",
            _ if !k.is_empty() && !matches!(k, "cursor" | "resultType") => query
                .equalities
                .push((decode_query_value(k)?, v.to_owned())),
            _ => return Err(ServerError::invalid("Unknown or invalid feature query")),
        }
    }
    if query.limit == 0 || query.limit > 10_000 {
        return Err(ServerError::invalid("Invalid limit"));
    }
    if query.cursor.is_some() && seen.len() != 1 {
        return Err(ServerError::invalid("Cursor fixes all query parameters"));
    }
    Ok(query)
}
pub(crate) fn decode_query_value(value: &str) -> Result<String> {
    let mut out = Vec::with_capacity(value.len());
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        out.push(if byte == b'%' {
            let hi = input.next().and_then(|v| (v as char).to_digit(16));
            let lo = input.next().and_then(|v| (v as char).to_digit(16));
            match (hi, lo) {
                (Some(hi), Some(lo)) => (hi * 16 + lo) as u8,
                _ => return Err(ServerError::invalid("Invalid query escape")),
            }
        } else {
            byte
        });
    }
    String::from_utf8(out).map_err(|_| ServerError::invalid("Invalid query UTF-8"))
}
/// Phase-one data is timeless. Validate the RFC3339 selection without
/// inferring temporal fields or connecting it to animation time.
fn validate_datetime(value: &str) -> Result<()> {
    fn instant(value: &str) -> Option<(i64, String)> {
        if !value.is_ascii() || value.len() < 20 || value.len() > 128 {
            return None;
        }
        let n = |range: std::ops::Range<usize>| value.get(range)?.parse::<i64>().ok();
        if ![
            &value[..4],
            &value[5..7],
            &value[8..10],
            &value[11..13],
            &value[14..16],
            &value[17..19],
        ]
        .iter()
        .all(|v| v.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        if &value[4..5] != "-"
            || &value[7..8] != "-"
            || !matches!(&value[10..11], "T" | "t")
            || &value[13..14] != ":"
            || &value[16..17] != ":"
        {
            return None;
        }
        let (year, month, day, hour, minute, second) = (
            n(0..4)?,
            n(5..7)?,
            n(8..10)?,
            n(11..13)?,
            n(14..16)?,
            n(17..19)?,
        );
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            _ => return None,
        };
        if day < 1 || day > days || hour > 23 || minute > 59 || second > 60 {
            return None;
        }
        let mut tail = &value[19..];
        let mut fraction = String::new();
        if let Some(decimal) = tail.strip_prefix('.') {
            let length = decimal.bytes().take_while(u8::is_ascii_digit).count();
            if length == 0 {
                return None;
            }
            fraction = decimal[..length].trim_end_matches('0').to_owned();
            tail = &decimal[length..];
        }
        let offset = if matches!(tail, "Z" | "z") {
            0
        } else {
            if tail.len() != 6 || !matches!(&tail[..1], "+" | "-") || &tail[3..4] != ":" {
                return None;
            }
            if !tail[1..3]
                .bytes()
                .chain(tail[4..6].bytes())
                .all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let hours = tail[1..3].parse::<i64>().ok()?;
            let minutes = tail[4..6].parse::<i64>().ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            (hours * 60 + minutes) * 60 * if tail.starts_with('-') { -1 } else { 1 }
        };
        let mut days_since = 0i64;
        for y in 0..year {
            days_since += if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
                366
            } else {
                365
            };
        }
        for m in 1..month {
            days_since += match m {
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                4 | 6 | 9 | 11 => 30,
                _ => {
                    if leap {
                        29
                    } else {
                        28
                    }
                }
            };
        }
        Some((((days_since + day - 1) * 24 + hour) * 60 + minute) * 60 + second - offset)
            .map(|seconds| (seconds, fraction))
    }
    let invalid = || ServerError::invalid("Invalid RFC3339 datetime selection");
    if let Some((start, end)) = value.split_once('/') {
        if end.contains('/') || (start == ".." && end == "..") {
            return Err(invalid());
        }
        let start = if start == ".." {
            None
        } else {
            Some(instant(start).ok_or_else(invalid)?)
        };
        let end = if end == ".." {
            None
        } else {
            Some(instant(end).ok_or_else(invalid)?)
        };
        if start.zip(end).is_some_and(|(start, end)| start > end) {
            return Err(invalid());
        }
    } else {
        instant(value).ok_or_else(invalid)?;
    }
    Ok(())
}
fn feature_value(f: &Feature) -> serde_json::Value {
    let id = f.id.as_ref().map(|v| match v {
        FeatureId::String(x) => serde_json::Value::String(x.clone()),
        FeatureId::I64(x) => serde_json::json!(x),
        FeatureId::U64(x) => serde_json::json!(x),
    });
    let geometry = f
        .geometry
        .as_ref()
        .map(geometry_value)
        .unwrap_or(serde_json::Value::Null);
    let properties = serde_json::to_value(&f.properties)
        .unwrap_or(serde_json::Value::Object(Default::default()));
    serde_json::json!({"type":"Feature","id":id,"geometry":geometry,"properties":properties})
}
fn coord(c: geo_types::Coord<f64>) -> serde_json::Value {
    serde_json::json!([c.x, c.y])
}
fn line(value: &geo_types::LineString<f64>) -> serde_json::Value {
    serde_json::Value::Array(value.0.iter().copied().map(coord).collect())
}
fn polygon(value: &geo_types::Polygon<f64>) -> serde_json::Value {
    let mut rings = vec![line(value.exterior())];
    rings.extend(value.interiors().iter().map(line));
    serde_json::Value::Array(rings)
}
fn geometry_value(g: &Geometry<f64>) -> serde_json::Value {
    match g {
        Geometry::Point(v) => serde_json::json!({"type":"Point","coordinates":coord(v.0)}),
        Geometry::MultiPoint(v) => {
            serde_json::json!({"type":"MultiPoint","coordinates":v.0.iter().map(|p|coord(p.0)).collect::<Vec<_>>() })
        }
        Geometry::LineString(v) => serde_json::json!({"type":"LineString","coordinates":line(v)}),
        Geometry::MultiLineString(v) => {
            serde_json::json!({"type":"MultiLineString","coordinates":v.0.iter().map(line).collect::<Vec<_>>() })
        }
        Geometry::Polygon(v) => serde_json::json!({"type":"Polygon","coordinates":polygon(v)}),
        Geometry::MultiPolygon(v) => {
            serde_json::json!({"type":"MultiPolygon","coordinates":v.0.iter().map(polygon).collect::<Vec<_>>() })
        }
        Geometry::GeometryCollection(v) => {
            serde_json::json!({"type":"GeometryCollection","geometries":v.0.iter().map(geometry_value).collect::<Vec<_>>() })
        }
        Geometry::Rect(v) => geometry_value(&Geometry::Polygon(v.to_polygon())),
        Geometry::Triangle(v) => geometry_value(&Geometry::Polygon(v.to_polygon())),
        Geometry::Line(v) => geometry_value(&Geometry::LineString((*v).into())),
    }
}
pub(crate) async fn items(
    State(s): State<Arc<ServerState>>,
    axum::extract::Path((id,)): axum::extract::Path<(String,)>,
    uri: axum::http::Uri,
) -> Result<Response> {
    execute_items(s, id, uri, false).await
}
pub(crate) async fn execute_items(
    s: Arc<ServerState>,
    id: String,
    uri: axum::http::Uri,
    legacy: bool,
) -> Result<Response> {
    let mut query = query(&uri, legacy)?;
    let continuation = query.cursor.is_some();
    let (snapshot, offset) =
        if let Some(token) = &query.cursor {
            s.feature_cursors.resolve(token, &id)?
        } else {
            let key = ResourceKey::new(ResourceKind::Dataset, id.clone())?;
            let pin = s.catalog.pin(&key)?;
            if pin.records[&key].class.as_deref() != Some("vector") {
                return Err(ServerError::new(
                    404,
                    "resource-unpublished",
                    "Feature collection is not published",
                ));
            }
            let definition = pin.records[&key]
                .definition
                .as_deref()
                .map(crate::config::parse::<crate::config::DatasetDefinition>)
                .transpose()?;
            for (alias, value) in &query.equalities {
                let field = definition
                    .as_ref()
                    .and_then(|d| d.queryables.iter().find(|q| &q.name == alias))
                    .ok_or_else(|| ServerError::invalid("Unknown feature queryable"))?;
                let value =
                    match field.kind.as_str() {
                        "string" => crate::PropertyValue::String(value.clone()),
                        "boolean" => match value.as_str() {
                            "true" => crate::PropertyValue::Bool(true),
                            "false" => crate::PropertyValue::Bool(false),
                            _ => return Err(ServerError::invalid("Invalid boolean equality")),
                        },
                        "integer" => {
                            if value.starts_with('-') {
                                crate::PropertyValue::I64(value.parse().map_err(|_| {
                                    ServerError::invalid("Invalid integer equality")
                                })?)
                            } else {
                                crate::PropertyValue::U64(value.parse().map_err(|_| {
                                    ServerError::invalid("Invalid integer equality")
                                })?)
                            }
                        }
                        "number" => crate::PropertyValue::F64(
                            value
                                .parse::<f64>()
                                .ok()
                                .filter(|v| v.is_finite())
                                .ok_or_else(|| ServerError::invalid("Invalid number equality"))?,
                        ),
                        _ => return Err(ServerError::invalid("Invalid queryable type")),
                    };
                query.fields.push((
                    if field.field.starts_with('/') {
                        field.field.clone()
                    } else {
                        format!("/{}", field.field.replace('~', "~0").replace('/', "~1"))
                    },
                    value,
                ));
            }
            let hold = s
                .feature_cursors
                .memory
                .reserve(2048 + id.len() * 4 + memory::model_bytes(&query.equalities)?)?;
            let offset = query.offset;
            (
                Arc::new(Snapshot {
                    pin,
                    key,
                    query,
                    expires: Instant::now() + s.feature_cursors.ttl,
                    token: super::catalog::identity(&[super::catalog::operation_id().as_bytes()]),
                    proof_key: std::hash::RandomState::new(),
                    _memory: hold,
                }),
                offset,
            )
        };
    let worker = s.clone();
    let fixed = snapshot.clone();
    let ctx = s.scheduler.context();
    let (page, has_next, number_matched, _page_holds) = s
        .scheduler
        .blocking(WorkClass::Http, ctx, move |ctx| {
            let record = &fixed.pin.records[&fixed.key];
            let path = record.derived_path.as_ref().ok_or_else(|| {
                ServerError::new(503, "preparing", "Dataset has no complete derived version")
            })?;
            let store = crate::derived::DerivedVector::open_with_context(
                path,
                worker.config.profile.store_limits(),
                &ctx,
            )?;
            let target = crate::Crs::new("OGC:CRS84", crate::CoordinateSpace::GeographicXy)?;
            let projection = if store.crs() != &target {
                Some(crate::projection::RustProjection::new(
                    store.crs().clone(),
                    target,
                )?)
            } else {
                None
            };
            let mut cursor = store.query(crate::readers::VectorQuery::default(), &ctx)?;
            let mut page = Vec::new();
            let mut holds = Vec::new();
            let mut matched = 0usize;
            while let Some(batch) = cursor.next_batch(
                worker.config.profile.shard_features,
                worker.config.profile.max_live_bytes.min(usize::MAX as u64) as usize / 8,
                &ctx,
            )? {
                let projected = projection
                    .as_ref()
                    .map(|p| p.project_batch(&batch, &ctx))
                    .transpose()?;
                let features = projected
                    .as_ref()
                    .map(|b| &b.features)
                    .unwrap_or(&batch.features);
                for feature in features {
                    ctx.checkpoint(1)?;
                    if !fixed.query.ids.is_empty()
                        && !feature
                            .id
                            .as_ref()
                            .is_some_and(|id| fixed.query.ids.contains(id))
                    {
                        continue;
                    }
                    let mut matches = true;
                    for (field, value) in &fixed.query.fields {
                        if !(crate::query::Filter::Eq {
                            field: field.clone(),
                            value: value.clone(),
                        })
                        .evaluate(&feature.properties)?
                        .matches()
                        {
                            matches = false;
                            break;
                        }
                    }
                    if !matches {
                        continue;
                    }
                    if let Some(bbox) = fixed.query.bbox {
                        // Features Core includes null geometry in bbox selections;
                        // explicitly empty geometry is excluded.
                        if let Some(geometry) = &feature.geometry {
                            let selection = geo_types::Rect::new(
                                geo_types::Coord {
                                    x: bbox[0],
                                    y: bbox[1],
                                },
                                geo_types::Coord {
                                    x: bbox[2],
                                    y: bbox[3],
                                },
                            );
                            if !geometry.intersects(&selection) {
                                continue;
                            }
                        } else if !fixed.query.include_null_bbox {
                            continue;
                        }
                    }
                    matched = matched
                        .checked_add(1)
                        .ok_or(crate::GeelloError::SizeOverflow)?;
                    if matched <= offset || fixed.query.hits || page.len() == fixed.query.limit {
                        continue;
                    }
                    holds.push(ctx.reserve(memory::envelope(
                        crate::core::feature_clone_bytes(feature)?,
                    )?)?);
                    page.push(feature_value(feature));
                }
            }
            let has_next = !fixed.query.hits && matched > offset.saturating_add(page.len());
            Ok((page, has_next, matched, holds))
        })
        .await?;
    let mut links = vec![
        serde_json::json!({"href": format!("{}/ogc/features/collections/{id}/items",base(&s)), "rel":"self"}),
    ];
    if has_next {
        let next = offset
            .checked_add(page.len())
            .ok_or_else(|| ServerError::invalid("Offset overflow"))?;
        links.push(serde_json::json!({"href":format!("{}/ogc/features/collections/{id}/items?cursor={}",base(&s),encode_cursor(&snapshot,next)), "rel":"next"}));
    }
    let record = &snapshot.pin.records[&snapshot.key];
    let mut response = json(
        &s,
        &serde_json::json!({"type":"FeatureCollection", "features":page,
        "numberMatched":number_matched,"numberReturned":page.len(),"links":links,
        "x-geello-data-version":record.data_version}),
    )?;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/geo+json"),
    );
    if has_next && (!continuation || snapshot.expires > Instant::now()) {
        s.feature_cursors.retain(snapshot)?;
    }
    Ok(response)
}
pub(crate) fn decode_feature_id(value: &str) -> Result<FeatureId> {
    if let Some(hex) = value.strip_prefix('s') {
        if !hex.len().is_multiple_of(2) {
            return Err(ServerError::invalid("Invalid typed feature ID"));
        }
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|c| {
                let high = (c[0] as char).to_digit(16)?;
                let low = (c[1] as char).to_digit(16)?;
                Some((high * 16 + low) as u8)
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| ServerError::invalid("Invalid typed feature ID"))?;
        return String::from_utf8(bytes)
            .map(FeatureId::String)
            .map_err(|_| ServerError::invalid("Invalid typed feature ID"));
    }
    if let Some(number) = value.strip_prefix('n') {
        if number.starts_with('-') {
            return number
                .parse()
                .map(FeatureId::I64)
                .map_err(|_| ServerError::invalid("Invalid typed feature ID"));
        }
        return number
            .parse()
            .map(FeatureId::U64)
            .map_err(|_| ServerError::invalid("Invalid typed feature ID"));
    }
    Err(ServerError::invalid(
        "Expected s<UTF-8 hex> or n<integer> feature ID",
    ))
}
async fn item(
    State(s): State<Arc<ServerState>>,
    axum::extract::Path((id, feature_id)): axum::extract::Path<(String, String)>,
) -> Result<Response> {
    let feature_id = decode_feature_id(&feature_id)?;
    let key = ResourceKey::new(ResourceKind::Dataset, id)?;
    let pin = s.catalog.pin(&key)?;
    if pin.records[&key].class.as_deref() != Some("vector") {
        return Err(ServerError::new(
            404,
            "resource-unpublished",
            "Feature collection is not published",
        ));
    }
    let worker = s.clone();
    let value = s
        .scheduler
        .blocking(WorkClass::Http, s.scheduler.context(), move |ctx| {
            let record = &pin.records[&key];
            let path = record.derived_path.as_ref().ok_or_else(|| {
                ServerError::new(503, "preparing", "Dataset has no complete derived version")
            })?;
            let store = crate::derived::DerivedVector::open_with_context(
                path,
                worker.config.profile.store_limits(),
                &ctx,
            )?;
            let mut cursor = store.query(
                crate::readers::VectorQuery {
                    ids: vec![feature_id],
                    ..Default::default()
                },
                &ctx,
            )?;
            let target = crate::Crs::new("OGC:CRS84", crate::CoordinateSpace::GeographicXy)?;
            while let Some(batch) = cursor.next_batch(
                worker.config.profile.shard_features,
                worker.config.profile.max_live_bytes.min(usize::MAX as u64) as usize / 8,
                &ctx,
            )? {
                let projected = if store.crs() != &target {
                    Some(
                        crate::projection::RustProjection::new(
                            store.crs().clone(),
                            target.clone(),
                        )?
                        .project_batch(&batch, &ctx)?,
                    )
                } else {
                    None
                };
                let features = projected
                    .as_ref()
                    .map(|b| &b.features)
                    .unwrap_or(&batch.features);
                if let Some(feature) = features.first() {
                    let hold = ctx.reserve(memory::envelope(crate::core::feature_clone_bytes(
                        feature,
                    )?)?)?;
                    return Ok(crate::Accounted::new(feature_value(feature), vec![hold]));
                }
            }
            Err(ServerError::new(
                404,
                "feature-not-found",
                "Feature is not present",
            ))
        })
        .await?;
    let mut response = json(&s, &*value)?;
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/geo+json"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeless_datetime_validates_calendar_offsets_order_and_percent_encoding() {
        for value in [
            "2024-02-29T12:34:56Z",
            "2024-01-01T00:00:00.1+08:00",
            "../2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00Z/..",
            "2024-01-01T08:00:00+08:00/2024-01-01T00:00:00Z",
        ] {
            assert!(validate_datetime(value).is_ok(), "{value}");
        }
        for value in [
            "2023-02-29T00:00:00Z",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:00:00.Z",
            "2024-01-01T00:00:00+-1:00",
            "../..",
            "2025-01-01T00:00:00Z/2024-01-01T00:00:00Z",
            "not-a-date",
        ] {
            assert!(validate_datetime(value).is_err(), "{value}");
        }
        let uri = "/items?datetime=2024-01-01T00%3A00%3A00%2B08%3A00"
            .parse()
            .unwrap();
        assert_eq!(
            query(&uri, false).unwrap().datetime.as_deref(),
            Some("2024-01-01T00:00:00+08:00")
        );
    }
}
