//! Basic HTTP and protected administration adapters. Deployment must isolate
//! /admin/v1 behind its reverse proxy; no in-process user system is implied.
use super::*;
use crate::{AspectPolicy, Bounds, View};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use futures_util::StreamExt;
use serde::Deserialize;
use std::{fs::File, io::Write, sync::Arc};
pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/health/ready", get(ready))
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
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
}
async fn ready(State(s): State<Arc<ServerState>>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"status":"ready","backend":s.backend_name,"backend_diagnostic":s.backend_diagnostic,"catalog":s.catalog.stats(),"live_accounted_bytes":s.scheduler.live_bytes(),"cache_failures":s.cache_failures()}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}
async fn list(
    State(s): State<Arc<ServerState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({"generation":s.catalog.stats().generation,"resources":s.catalog.list(q.offset.unwrap_or(0),q.limit.unwrap_or(s.config.profile.max_list_items))?}),
    ))
}
fn key(kind: &str, id: &str) -> Result<ResourceKey> {
    ResourceKey::new(ResourceKind::parse(kind)?, id)
}
async fn resource(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Response> {
    let record = s.catalog.get(&key(&kind, &id)?)?;
    let etag = record.etag();
    Ok(([(header::ETAG, etag)], Json(record)).into_response())
}
async fn operation(
    State(s): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<ResourceOperation>> {
    Ok(Json(s.operation(&id)?))
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
    let bytes = to_bytes(body, s.config.profile.max_registry_bytes)
        .await
        .map_err(|_| ServerError::new(413, "body-limit", "Source registration exceeds profile"))?;
    let source: SourceSpec = serde_json::from_slice(&bytes)
        .map_err(|_| ServerError::invalid("Invalid source registration JSON"))?;
    let op = s.new_operation(key)?;
    s.start_prepare(op.clone(), source, Ownership::External, precondition, None);
    Ok((StatusCode::ACCEPTED, Json(op)).into_response())
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
    let stage = tempfile::Builder::new()
        .prefix("upload-")
        .tempdir_in(s.catalog.root().join("staging"))
        .map_err(ServerError::io)?;
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
            let _hold = ctx.reserve(chunk.len())?;
            file = tokio::task::spawn_blocking(move || -> Result<File> {
                file.write_all(&chunk).map_err(ServerError::io)?;
                Ok(file)
            })
            .await
            .map_err(ServerError::io)??;
        }
        tokio::task::spawn_blocking(move || file.sync_all())
            .await
            .map_err(ServerError::io)?
            .map_err(ServerError::io)?;
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
    );
    Ok((StatusCode::ACCEPTED, Json(op)).into_response())
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
    let record = tokio::task::spawn_blocking(move || s.catalog.unpublish(&key, &condition))
        .await
        .map_err(ServerError::io)??;
    Ok(([(header::ETAG, record.etag())], Json(record)).into_response())
}
async fn remove(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    let key = key(&kind, &id)?;
    let condition = Precondition::from_headers(&headers)?;
    tokio::task::spawn_blocking(move || s.catalog.remove(&key, &condition, false))
        .await
        .map_err(ServerError::io)??;
    Ok(StatusCode::NO_CONTENT)
}
async fn delete_content(
    State(s): State<Arc<ServerState>>,
    Path((kind, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    let key = key(&kind, &id)?;
    let condition = Precondition::from_headers(&headers)?;
    tokio::task::spawn_blocking(move || -> Result<()> {
        s.catalog.remove(&key, &condition, true)?;
        Ok(())
    })
    .await
    .map_err(ServerError::io)??;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageQuery {
    bbox: String,
    crs: String,
    width: u32,
    height: u32,
}
async fn image(
    State(s): State<Arc<ServerState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<ImageQuery>,
) -> Result<Response> {
    let numbers = q
        .bbox
        .split(',')
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| ServerError::invalid("bbox requires four finite XY coordinates"))?;
    let [a, b, c, d] = numbers.as_slice() else {
        return Err(ServerError::invalid("bbox requires four coordinates"));
    };
    let view = View {
        crs: super::runtime::parse_crs(&q.crs)?,
        bounds: Bounds {
            min_x: *a,
            min_y: *b,
            max_x: *c,
            max_y: *d,
        },
        width: q.width,
        height: q.height,
        aspect: AspectPolicy::Stretch,
    };
    let image = s.render_map(&id, view).await?;
    response(&image, &headers)
}
async fn tile(
    State(s): State<Arc<ServerState>>,
    Path((id, z, x, y)): Path<(String, u32, u32, u32)>,
    headers: HeaderMap,
) -> Result<Response> {
    let image = s.render_tile(&id, z, x, y).await?;
    response(&image, &headers)
}
fn response(image: &EncodedResponse, headers: &HeaderMap) -> Result<Response> {
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
        Body::from(image.bytes.clone()).into_response()
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
