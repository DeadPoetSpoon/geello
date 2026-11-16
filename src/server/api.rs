//! Basic HTTP and protected administration adapters. Deployment must isolate
//! /admin/v1 behind its reverse proxy; no in-process user system is implied.
use super::http_extract::{CheckedPath as Path, CheckedQuery as Query};
use super::*;
use crate::{AspectPolicy, Bounds, View};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::{fs::File, io::Write, sync::Arc};
pub fn router(state: Arc<ServerState>) -> Router {
    let admission_state = state.clone();
    Router::new()
        .route("/health/ready", get(ready))
        .route("/health/live", get(live))
        .route("/api", get(super::http_docs::openapi))
        .route("/api.html", get(super::http_docs::html))
        .route("/admin/v1/resources", get(list))
        .route(
            "/admin/v1/resources/{kind}/{id}",
            get(resource).delete(remove).post(unpublish),
        )
        .route(
            "/admin/v1/resources/{kind}/{id}/content",
            put(upload).delete(delete_content),
        )
        .route("/admin/v1/resources/{kind}/{id}/source", put(register))
        .route("/admin/v1/operations/{id}", get(operation))
        .route("/maps/{id}/image", get(image))
        .route("/tiles/{id}/{z}/{x}/{y}", get(tile))
        .merge(super::ogc::routes())
        .merge(super::legacy::routes())
        .merge(super::features::routes())
        .merge(super::websocket::routes())
        .fallback(|| async { ServerError::new(404, "route-not-found", "Route not found") })
        .method_not_allowed_fallback(|| async {
            ServerError::new(405, "method-not-allowed", "Method not allowed")
        })
        .layer(DefaultBodyLimit::disable())
        .layer(axum::middleware::from_fn_with_state(
            admission_state,
            admit_request,
        ))
        .layer(axum::middleware::from_fn(request_id))
        .with_state(state)
}

async fn request_id(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    // Multiple field values are ambiguous, even if each value is valid alone.
    let mut supplied = request.headers().get_all("x-request-id").iter();
    let id = supplied
        .next()
        .filter(|_| supplied.next().is_none())
        .and_then(|value| value.to_str().ok())
        .filter(|value| valid_request_id(value))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("req-{}", super::catalog::operation_id()));
    let mut response = next.run(request).await;
    if let Ok(value) = axum::http::HeaderValue::from_str(&id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_graphic() && character != '"' && character != ',')
}
async fn admit_request(
    State(s): State<Arc<ServerState>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<Response> {
    let _activity = s.activities.enter()?;
    let _request_slot = s.requests.clone().try_acquire_owned().map_err(|_| {
        ServerError::new(
            503,
            "request-capacity",
            "HTTP request capacity is exhausted",
        )
    })?;
    tokio::select! {
        response = next.run(request) => Ok(response),
        _ = s.activities.closed() => Err(ServerError::new(503,"shutdown","Server is shutting down")),
    }
}
async fn ready(State(s): State<Arc<ServerState>>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"status":"ready","backend":s.backend_name,"backend_diagnostic":s.backend_diagnostic,"catalog":s.catalog.stats(),"live_accounted_bytes":s.scheduler.live_bytes(),"cache_failures":s.cache_failures(),"response_cache":s.response_cache_stats()}),
    )
}

/// Liveness intentionally excludes catalog/backend readiness. It answers whether
/// the process can still serve a shutdown-aware health request.
async fn live(State(s): State<Arc<ServerState>>) -> Result<Json<serde_json::Value>> {
    if s.activities.is_closed() {
        return Err(ServerError::new(503, "shutdown", "Server is shutting down"));
    }
    Ok(Json(serde_json::json!({"status":"alive"})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}
async fn list(State(s): State<Arc<ServerState>>, Query(q): Query<ListQuery>) -> Result<Response> {
    let (generation, records) = s.catalog.list_accounted(
        q.offset.unwrap_or(0),
        q.limit.unwrap_or(s.config.profile.max_list_items),
    )?;
    #[derive(serde::Serialize)]
    struct Page<'a> {
        generation: u64,
        resources: &'a [ResourceRecord],
    }
    json_response(
        &s,
        &Page {
            generation,
            resources: &records,
        },
    )
}
fn key(kind: &str, id: &str) -> Result<ResourceKey> {
    ResourceKey::new(ResourceKind::parse(kind)?, id)
}
async fn resource(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Response> {
    let record = s.catalog.get_accounted(&key(&kind, &id)?)?;
    let etag = record.etag();
    let mut response = json_response(&s, &*record)?;
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().map_err(ServerError::io)?);
    Ok(response)
}
async fn operation(State(s): State<Arc<ServerState>>, Path(id): Path<String>) -> Result<Response> {
    json_response(&s, &*s.operation_accounted(&id)?)
}
async fn register(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response> {
    let key = key(&kind, &id)?;
    let precondition = Precondition::from_headers(&headers)?;
    s.catalog.check(&key, &precondition)?;
    let ctx = s.scheduler.context();
    let _body_memory = ctx.reserve(memory::envelope(s.config.profile.max_registry_bytes)?)?;
    let bytes = tokio::time::timeout(
        std::time::Duration::from_millis(s.config.profile.operation_timeout_ms),
        to_bytes(body, s.config.profile.max_registry_bytes),
    )
    .await
    .map_err(|_| ServerError::new(408, "request-timeout", "Source registration timed out"))?
    .map_err(|_| ServerError::new(413, "body-limit", "Source registration exceeds profile"))?;
    let source: SourceSpec = serde_json::from_slice(&bytes)
        .map_err(|_| ServerError::invalid("Invalid source registration JSON"))?;
    let op = s.new_operation(key)?;
    s.start_prepare(op.clone(), source, Ownership::External, precondition, None)?;
    let mut response = json_response(&s, &op)?;
    *response.status_mut() = StatusCode::ACCEPTED;
    Ok(response)
}
async fn upload(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response> {
    let _upload_slot =
        s.uploads.clone().try_acquire_owned().map_err(|_| {
            ServerError::new(503, "upload-capacity", "Upload capacity is exhausted")
        })?;
    let key = key(&kind, &id)?;
    let precondition = Precondition::from_headers(&headers)?;
    s.catalog.check(&key, &precondition)?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .ok_or_else(|| ServerError::invalid("Content-Type required"))?;
    let format = match (key.kind, content_type) {
        (ResourceKind::Dataset, "application/geo+json" | "application/geojson") => "geojson",
        (ResourceKind::Dataset, "image/tiff" | "image/geotiff") => "geotiff",
        (ResourceKind::Dataset, "application/zip") => "shapefile-zip",
        (
            ResourceKind::Dataset
            | ResourceKind::Style
            | ResourceKind::Map
            | ResourceKind::Animation,
            "application/toml" | "text/toml",
        ) => "toml",
        (
            ResourceKind::Font,
            "font/ttf" | "font/otf" | "font/collection" | "application/octet-stream",
        ) => "font",
        _ => {
            return Err(ServerError::new(
                415,
                "content-type",
                "Unsupported resource Content-Type",
            ));
        }
    }
    .to_owned();
    if let Some(n) = headers.get(header::CONTENT_LENGTH)
        && n.to_str()
            .ok()
            .and_then(|n| n.parse::<u64>().ok())
            .is_none_or(|n| n > s.config.profile.max_upload_bytes as u64)
    {
        return Err(ServerError::new(
            413,
            "upload-limit",
            "Upload exceeds profile",
        ));
    }
    let op = s.new_operation(key)?;
    let mut upload_guard = UploadGuard {
        state: s.clone(),
        operation: Some(op.clone()),
    };
    let stage = Arc::new(
        tempfile::Builder::new()
            .prefix("upload-")
            .tempdir_in(s.catalog.root().join("staging"))
            .map_err(ServerError::io)?,
    );
    let path = stage.path().join(match format.as_str() {
        "geojson" => "source.geojson",
        "geotiff" => "source.tif",
        "shapefile-zip" => "source.zip",
        "toml" => "definition.toml",
        _ => "font.bin",
    });
    let mut file = File::options()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(ServerError::io)?;
    let mut stream = body.into_data_stream();
    let mut received = 0usize;
    let max = s.config.profile.max_upload_bytes;
    let ctx = s.scheduler.context();
    let result: Result<()> = async {
        loop {
            ctx.check()?;
            let chunk = tokio::time::timeout(
                std::time::Duration::from_millis(s.config.profile.operation_timeout_ms),
                stream.next(),
            )
            .await
            .map_err(|_| ServerError::new(408, "upload-timeout", "Upload timed out"))?;
            let Some(chunk) = chunk else {
                break;
            };
            let chunk = chunk.map_err(|_| ServerError::invalid("Upload stream failed"))?;
            received = received
                .checked_add(chunk.len())
                .ok_or_else(|| ServerError::invalid("Upload length overflow"))?;
            if received > max {
                return Err(ServerError::new(
                    413,
                    "upload-limit",
                    "Upload exceeds profile",
                ));
            }
            let hold = ctx.reserve(chunk.len())?;
            let stage_owner = stage.clone();
            file = s
                .scheduler
                .blocking(
                    WorkClass::Background,
                    ctx.clone(),
                    move |_| -> Result<File> {
                        let _hold = hold;
                        let _stage = stage_owner;
                        file.write_all(&chunk).map_err(ServerError::io)?;
                        Ok(file)
                    },
                )
                .await?;
        }
        let stage_owner = stage.clone();
        s.scheduler
            .blocking(WorkClass::Background, ctx.clone(), move |_| {
                let _stage = stage_owner;
                file.sync_all().map_err(ServerError::io)
            })
            .await?;
        if received == 0 {
            return Err(ServerError::invalid("Empty upload"));
        }
        Ok(())
    }
    .await;
    if let Err(e) = result {
        s.fail_upload(op, e.clone());
        return Err(e);
    }
    let text_header = |name: &str| -> Result<Option<String>> {
        headers
            .get(name)
            .map(|v| {
                v.to_str()
                    .map(str::to_owned)
                    .map_err(|_| ServerError::invalid("Invalid interpretation header"))
            })
            .transpose()
    };
    let source = SourceSpec {
        path,
        format,
        encoding: text_header("x-geello-encoding")?,
        crs: text_header("x-geello-crs")?,
        sample_kind: text_header("x-geello-sample-kind")?,
    };
    s.start_prepare(
        op.clone(),
        source,
        Ownership::Managed,
        precondition,
        Some(stage),
    )?;
    upload_guard.operation = None;
    let mut response = json_response(&s, &op)?;
    *response.status_mut() = StatusCode::ACCEPTED;
    Ok(response)
}
/// Every post-creation exit, including dropping an incomplete HTTP body,
/// leaves a terminal ledger record unless ownership reached preparation.
struct UploadGuard {
    state: Arc<ServerState>,
    operation: Option<ResourceOperation>,
}
impl Drop for UploadGuard {
    fn drop(&mut self) {
        if let Some(op) = self.operation.take() {
            // An explicit failure has already recorded its more useful reason.
            if self
                .state
                .operation(&op.operation_id)
                .is_ok_and(|current| current.status == OperationStatus::Staging)
            {
                self.state.fail_upload(
                    op,
                    ServerError::new(
                        400,
                        "upload-interrupted",
                        "Upload ended before preparation started",
                    ),
                );
            }
        }
    }
}
async fn unpublish(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response> {
    let id = id
        .strip_suffix(":unpublish")
        .ok_or_else(|| ServerError::invalid("Expected :unpublish action"))?;
    let key = key(&kind, id)?;
    let condition = Precondition::from_headers(&headers)?;
    let scheduler = s.scheduler.clone();
    let catalog = s.catalog.clone();
    let record = scheduler
        .blocking(WorkClass::Http, scheduler.context(), move |_| {
            catalog.unpublish(&key, &condition)
        })
        .await?;
    let mut response = json_response(&s, &record)?;
    response.headers_mut().insert(
        header::ETAG,
        record.etag().parse().map_err(ServerError::io)?,
    );
    Ok(response)
}
async fn remove(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    let key = key(&kind, &id)?;
    let condition = Precondition::from_headers(&headers)?;
    let scheduler = s.scheduler.clone();
    scheduler
        .blocking(WorkClass::Http, scheduler.context(), move |_| {
            s.catalog.remove(&key, &condition, false)
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn delete_content(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    let key = key(&kind, &id)?;
    let condition = Precondition::from_headers(&headers)?;
    let scheduler = s.scheduler.clone();
    scheduler
        .blocking(
            WorkClass::Http,
            scheduler.context(),
            move |_| -> Result<()> {
                s.catalog.remove(&key, &condition, true)?;
                Ok(())
            },
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ImageQuery {
    bbox: String,
    crs: String,
    width: u32,
    height: u32,
    geello_time: Option<f64>,
}
async fn image(
    State(s): State<Arc<ServerState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<ImageQuery>,
) -> Result<Response> {
    let mut parts = q.bbox.split(',');
    let mut numbers = [0.; 4];
    for number in &mut numbers {
        *number = parts
            .next()
            .and_then(|part| part.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .ok_or_else(|| ServerError::invalid("bbox requires four finite XY coordinates"))?;
    }
    if parts.next().is_some() {
        return Err(ServerError::invalid("bbox requires four coordinates"));
    }
    let [a, b, c, d] = numbers;
    let view = View {
        crs: super::runtime::parse_crs(&q.crs)?,
        bounds: Bounds {
            min_x: a,
            min_y: b,
            max_x: c,
            max_y: d,
        },
        width: q.width,
        height: q.height,
        aspect: AspectPolicy::Stretch,
    };
    let image = s
        .render_map_at(&id, view, q.geello_time.unwrap_or(0.))
        .await?;
    response(&s, &image, &headers)
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct TileQuery {
    pub(super) geello_time: Option<f64>,
}
async fn tile(
    State(s): State<Arc<ServerState>>,
    Path((id, z, x, y)): Path<(String, u32, u32, u32)>,
    headers: HeaderMap,
    Query(q): Query<TileQuery>,
) -> Result<Response> {
    let image = s
        .render_tile_at(&id, z, x, y, q.geello_time.unwrap_or(0.))
        .await?;
    response(&s, &image, &headers)
}
fn accounted_body(bytes: crate::Accounted<Vec<u8>>) -> Body {
    let (bytes, holds) = bytes.into_parts();
    Body::from_stream(futures_util::stream::unfold(
        Some((Some(bytes), holds)),
        |state| async move {
            let (bytes, holds) = state?;
            let bytes = bytes?;
            Some((
                Ok::<_, std::io::Error>(axum::body::Bytes::from(bytes)),
                Some((None, holds)),
            ))
        },
    ))
}
pub(super) fn json_response(s: &ServerState, value: &impl serde::Serialize) -> Result<Response> {
    let bytes = crate::derived::bounded_json(
        value,
        s.config.profile.max_registry_bytes,
        &s.scheduler.memory_context(),
    )?;
    Ok((
        [(header::CONTENT_TYPE, "application/json")],
        accounted_body(bytes),
    )
        .into_response())
}
pub(super) fn bytes_response(
    s: &ServerState,
    bytes: Vec<u8>,
    content_type: &'static str,
) -> Result<Response> {
    let hold = s.scheduler.memory_context().reserve(bytes.len())?;
    Ok((
        [(header::CONTENT_TYPE, content_type)],
        accounted_body(crate::Accounted::new(bytes, vec![hold])),
    )
        .into_response())
}
pub(super) fn response(
    s: &ServerState,
    image: &EncodedResponse,
    headers: &HeaderMap,
) -> Result<Response> {
    let not_modified = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|v| v.trim() == image.etag || v.trim() == "*")
        });
    let mut response = if not_modified {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let hold = s.scheduler.memory_context().reserve(image.bytes.len())?;
        accounted_body(crate::Accounted::new(image.bytes.clone(), vec![hold])).into_response()
    };
    let h = response.headers_mut();
    for (name, value) in [
        ("etag", image.etag.as_str()),
        ("cache-control", "no-cache"),
        ("content-type", &image.content_type),
        ("x-geello-backend", &image.backend),
        ("x-geello-render-revision", &image.render_revision),
        (
            "x-geello-incomplete",
            if image.incomplete { "true" } else { "false" },
        ),
    ] {
        h.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).map_err(ServerError::io)?,
            value.parse().map_err(ServerError::io)?,
        );
    }
    Ok(response)
}
