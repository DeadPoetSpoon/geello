use super::http_extract::{CheckedPath as Path, CheckedQuery as Query};
use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;

pub(super) fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/admin", get(|| async { Redirect::temporary("./admin/") }))
        .route("/admin/", get(index))
        .route("/admin/assets/{*file}", get(asset))
        .route("/admin/v1/operations", get(operations))
        .route(
            "/admin/v1/configurations/{kind}/{action}",
            post(configuration),
        )
}

pub(super) fn check_origin(state: &ServerState, request: &Request) -> Result<()> {
    if !request.uri().path().starts_with("/admin/")
        || matches!(
            *request.method(),
            axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
        )
    {
        return Ok(());
    }
    let headers = request.headers();
    let configured: axum::http::Uri = state
        .config
        .public_base_url
        .parse()
        .map_err(ServerError::io)?;
    let same_origin = |value: &HeaderValue| {
        let Ok(origin) = value.to_str().unwrap_or("").parse::<axum::http::Uri>() else {
            return false;
        };
        let effective_port = |uri: &axum::http::Uri| {
            uri.port_u16().or_else(|| match uri.scheme_str() {
                Some("http") => Some(80),
                Some("https") => Some(443),
                _ => None,
            })
        };
        origin.scheme_str() == configured.scheme_str()
            && origin
                .host()
                .zip(configured.host())
                .is_some_and(|(origin, configured)| origin.eq_ignore_ascii_case(configured))
            && effective_port(&origin) == effective_port(&configured)
            && origin.path() == "/"
            && origin.query().is_none()
            && origin
                .authority()
                .is_some_and(|authority| !authority.as_str().contains('@'))
    };
    if headers.get_all(header::ORIGIN).iter().count() > 1
        || headers.get_all("sec-fetch-site").iter().count() > 1
        || headers
            .get("sec-fetch-site")
            .is_some_and(|site| site == "cross-site")
        || headers
            .get(header::ORIGIN)
            .is_some_and(|value| !same_origin(value))
    {
        return Err(ServerError::new(
            403,
            "origin",
            "Cross-origin administration writes are forbidden",
        ));
    }
    Ok(())
}

fn static_response(
    headers: HeaderMap,
    bytes: &'static [u8],
    media: &'static str,
    html: bool,
) -> Response {
    let etag = format!("\"{}\"", blake3::hash(bytes).to_hex());
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|candidate| {
                let candidate = candidate.trim().trim_start_matches("W/");
                candidate == etag || candidate == "*"
            })
        });
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        bytes.into_response()
    };
    let target = response.headers_mut();
    target.insert(header::ETAG, etag.parse().unwrap());
    target.insert(header::CONTENT_TYPE, HeaderValue::from_static(media));
    target.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if html {
            "no-cache"
        } else {
            "private, max-age=0, must-revalidate"
        }),
    );
    target.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    target.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    target.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    target.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; connect-src 'self'; font-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; object-src 'none'"));
    response
}

async fn index(headers: HeaderMap) -> Response {
    static_response(
        headers,
        include_bytes!("admin/assets/index.html"),
        "text/html; charset=utf-8",
        true,
    )
}

async fn asset(Path(file): Path<String>, headers: HeaderMap) -> Result<Response> {
    let (bytes, media): (&'static [u8], _) = match file.as_str() {
        "app.js" => (
            include_bytes!("admin/assets/app.js"),
            "text/javascript; charset=utf-8",
        ),
        "requests.js" => (
            include_bytes!("admin/assets/requests.js"),
            "text/javascript; charset=utf-8",
        ),
        "preview.js" => (
            include_bytes!("admin/assets/preview.js"),
            "text/javascript; charset=utf-8",
        ),
        "style.css" => (
            include_bytes!("admin/assets/style.css"),
            "text/css; charset=utf-8",
        ),
        "vendor/ol.js" => (
            include_bytes!("admin/assets/vendor/ol.js"),
            "text/javascript; charset=utf-8",
        ),
        "vendor/ol.css" => (
            include_bytes!("admin/assets/vendor/ol.css"),
            "text/css; charset=utf-8",
        ),
        "vendor/LICENSE.md" => (
            include_bytes!("admin/assets/vendor/LICENSE.md"),
            "text/plain; charset=utf-8",
        ),
        "vendor/SHA256SUMS" => (
            include_bytes!("admin/assets/vendor/SHA256SUMS"),
            "text/plain; charset=utf-8",
        ),
        _ => return Err(ServerError::new(404, "asset-not-found", "Asset not found")),
    };
    Ok(static_response(headers, bytes, media, false))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OperationsQuery {
    offset: Option<usize>,
    limit: Option<usize>,
    status: Option<OperationStatus>,
    kind: Option<ResourceKind>,
    resource_id: Option<String>,
}

async fn operations(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<OperationsQuery>,
) -> Result<Response> {
    let (total, operations) = state.operations_accounted(
        query.offset.unwrap_or(0),
        query.limit.unwrap_or(state.config.profile.max_list_items),
        query.status,
        query.kind,
        query.resource_id.as_deref(),
    )?;
    #[derive(serde::Serialize)]
    struct Page<'a> {
        total: usize,
        operations: &'a [ResourceOperation],
    }
    super::api::json_response(
        &state,
        &Page {
            total,
            operations: &operations,
        },
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParseRequest {
    text: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SerializeRequest {
    document: serde_json::Value,
}

fn validate(kind: &str, text: &str) -> Result<()> {
    match kind {
        "style" => {
            crate::config::parse_style(text)?;
        }
        "map" => {
            crate::config::parse_map(text)?;
        }
        "animation" => {
            crate::config::parse_animation(text)?;
        }
        _ => {
            return Err(ServerError::invalid(
                "Configuration kind must be style, map or animation",
            ));
        }
    }
    Ok(())
}

fn safe_tree(value: &toml::Value) -> Result<()> {
    match value {
        toml::Value::Integer(value) if value.unsigned_abs() > 9_007_199_254_740_991 => {}
        toml::Value::Float(value) if !value.is_finite() => {}
        toml::Value::Datetime(_) => {}
        toml::Value::Array(values) => {
            for value in values {
                safe_tree(value)?;
            }
            return Ok(());
        }
        toml::Value::Table(values) => {
            for value in values.values() {
                safe_tree(value)?;
            }
            return Ok(());
        }
        _ => return Ok(()),
    }
    Err(ServerError::new(
        422,
        "configuration-conversion",
        "Configuration cannot be represented losslessly as browser JSON; retain the original TOML editor",
    ))
}

async fn configuration(
    State(state): State<Arc<ServerState>>,
    Path((kind, action)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response> {
    if !matches!(kind.as_str(), "style" | "map" | "animation")
        || !matches!(action.as_str(), "parse" | "serialize")
    {
        return Err(ServerError::new(
            404,
            "configuration-route",
            "Unknown configuration helper",
        ));
    }
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        != Some("application/json")
    {
        return Err(ServerError::new(
            415,
            "content-type",
            "Expected application/json",
        ));
    }
    let max = state.config.profile.max_registry_bytes;
    let context = state.scheduler.context();
    let hold = context.reserve(memory::envelope(max)?)?;
    let bytes = tokio::time::timeout(
        std::time::Duration::from_millis(state.config.profile.operation_timeout_ms),
        to_bytes(body, max),
    )
    .await
    .map_err(|_| ServerError::new(408, "request-timeout", "Configuration read timed out"))?
    .map_err(|_| ServerError::new(413, "body-limit", "Configuration exceeds profile"))?;
    let output = state
        .scheduler
        .blocking(WorkClass::Http, context, move |_| {
            let (text, tree) = if action == "parse" {
                let request: ParseRequest = serde_json::from_slice(&bytes)
                    .map_err(|_| ServerError::invalid("Invalid configuration JSON"))?;
                validate(&kind, &request.text)?;
                let tree: toml::Value = toml::from_str(&request.text)
                    .map_err(|error| ServerError::new(422, "configuration", error.to_string()))?;
                (request.text, tree)
            } else {
                let request: SerializeRequest = serde_json::from_slice(&bytes)
                    .map_err(|_| ServerError::invalid("Invalid configuration JSON"))?;
                let tree: toml::Value = serde_json::from_value(request.document)
                    .map_err(|error| ServerError::new(422, "configuration", error.to_string()))?;
                safe_tree(&tree)?;
                let text = toml::to_string_pretty(&tree)
                    .map_err(|error| ServerError::new(422, "configuration", error.to_string()))?;
                validate(&kind, &text)?;
                (text, tree)
            };
            safe_tree(&tree)?;
            if text.len() > max {
                return Err(ServerError::new(
                    413,
                    "body-limit",
                    "Configuration exceeds profile",
                ));
            }
            Ok(crate::Accounted::new(
                serde_json::json!({"text": text, "document": tree}),
                vec![hold],
            ))
        })
        .await?;
    super::api::json_response(&state, &*output)
}
