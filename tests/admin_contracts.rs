#![cfg(all(feature = "server", feature = "render-cpu", feature = "read-geojson"))]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use geello::server::*;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use tower::ServiceExt;

async fn setup(root: &Path) -> (Arc<ServerState>, Router) {
    let mut config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
    config.backend = BackendSelection::Cpu;
    config.public_base_url = "https://admin.example.test/prefix/geello/".into();
    config.external_roots = vec![root.to_owned()];
    config.data_directory = root.join("data");
    config.profile.max_list_items = 2;
    let state = ServerState::open(config).await.unwrap();
    (state.clone(), router(state))
}

async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: impl Into<Body>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    app.clone()
        .oneshot(builder.body(body.into()).unwrap())
        .await
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 4 << 20).await.unwrap()).unwrap()
}

async fn get(app: &Router, uri: &str) -> Value {
    let response = send(app, "GET", uri, &[], Body::empty()).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    json_body(response).await
}

async fn terminal(app: &Router, operation: &Value) -> Value {
    for _ in 0..500 {
        let current = get(
            app,
            &format!(
                "/admin/v1/operations/{}",
                operation["operation_id"].as_str().unwrap()
            ),
        )
        .await;
        if matches!(current["status"].as_str(), Some("ready" | "failed")) {
            return current;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("Operation did not finish: {operation}")
}

async fn publish_style(app: &Router, id: &str) {
    let text = format!("schema_version=1\nid='{id}'\nrules=[]\n");
    let response = send(
        app,
        "PUT",
        &format!("/admin/v1/resources/style/{id}/content"),
        &[("if-none-match", "*"), ("content-type", "application/toml")],
        text,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let operation = json_body(response).await;
    assert_eq!(terminal(app, &operation).await["status"], "ready");
}

#[tokio::test]
async fn embedded_admin_assets_are_offline_whitelisted_and_conditional() {
    let root = tempfile::tempdir().unwrap();
    let (state, app) = setup(root.path()).await;
    let redirect = send(&app, "GET", "/admin", &[], Body::empty()).await;
    assert_eq!(redirect.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(redirect.headers()["location"], "./admin/");
    for (uri, media) in [
        ("/admin/", "text/html"),
        ("/admin/assets/app.js", "text/javascript"),
        ("/admin/assets/requests.js", "text/javascript"),
        ("/admin/assets/preview.js", "text/javascript"),
        ("/admin/assets/style.css", "text/css"),
        ("/admin/assets/vendor/ol.js", "text/javascript"),
        ("/admin/assets/vendor/ol.css", "text/css"),
        ("/admin/assets/vendor/LICENSE.md", "text/plain"),
    ] {
        let response = send(&app, "GET", uri, &[], Body::empty()).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with(media)
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );
        let etag = response.headers()["etag"].to_str().unwrap().to_owned();
        let body = to_bytes(response.into_body(), 2 << 20).await.unwrap();
        assert!(!body.is_empty());
        let conditional = send(&app, "GET", uri, &[("if-none-match", &etag)], Body::empty()).await;
        assert_eq!(conditional.status(), StatusCode::NOT_MODIFIED);
        assert!(
            to_bytes(conditional.into_body(), 1)
                .await
                .unwrap()
                .is_empty()
        );
        let head = send(&app, "HEAD", uri, &[], Body::empty()).await;
        assert_eq!(head.status(), StatusCode::OK);
        assert!(to_bytes(head.into_body(), 1).await.unwrap().is_empty());
    }
    for uri in [
        "/admin/assets/missing.js",
        "/admin/assets/../api.rs",
        "/admin/assets/%2e%2e/api.rs",
        "/admin/v1/missing",
        "/admin/missing",
    ] {
        let response = send(&app, "GET", uri, &[], Body::empty()).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(
            response.headers()["content-type"],
            "application/problem+json"
        );
    }
    let ready = get(&app, "/health/ready").await;
    assert_eq!(ready["response_cache"]["area_renders"], 0);
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn browser_writes_require_same_origin_but_cli_remains_compatible() {
    let root = tempfile::tempdir().unwrap();
    let (state, app) = setup(root.path()).await;
    let body = r#"{"text":"schema_version=1\nid='test:map'\noutput_crs=['OGC:CRS84']\nitems=[]"}"#;
    for headers in [
        vec![("origin", "https://evil.example.test")],
        vec![("origin", "null")],
        vec![("sec-fetch-site", "cross-site")],
        vec![
            ("origin", "https://admin.example.test"),
            ("origin", "https://evil.example.test"),
        ],
    ] {
        let mut headers = headers;
        headers.push(("content-type", "application/json"));
        let response = send(
            &app,
            "POST",
            "/admin/v1/configurations/map/parse",
            &headers,
            body,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    for origin in [
        None,
        Some("https://admin.example.test"),
        Some("https://admin.example.test:443"),
    ] {
        let mut headers = vec![("content-type", "application/json")];
        if let Some(origin) = origin {
            headers.push(("origin", origin));
        }
        let response = send(
            &app,
            "POST",
            "/admin/v1/configurations/map/parse",
            &headers,
            body,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn configuration_helpers_preserve_bindings_and_reject_invalid_or_unsafe_trees() {
    let root = tempfile::tempdir().unwrap();
    let (state, app) = setup(root.path()).await;
    let headers = [("content-type", "application/json")];
    for (kind, text) in [
        (
            "map",
            include_str!("../examples/geometry-animation/map.toml"),
        ),
        (
            "animation",
            include_str!("../examples/geometry-animation/animation.toml"),
        ),
        (
            "style",
            include_str!("../examples/open-data/cities-style.toml"),
        ),
    ] {
        let parse = send(
            &app,
            "POST",
            &format!("/admin/v1/configurations/{kind}/parse"),
            &headers,
            json!({"text": text}).to_string(),
        )
        .await;
        assert_eq!(parse.status(), StatusCode::OK);
        let parsed = json_body(parse).await;
        assert_eq!(parsed["text"], text);
        let serialized = send(
            &app,
            "POST",
            &format!("/admin/v1/configurations/{kind}/serialize"),
            &headers,
            json!({"document": parsed["document"]}).to_string(),
        )
        .await;
        assert_eq!(serialized.status(), StatusCode::OK);
        assert_eq!(json_body(serialized).await["document"], parsed["document"]);
    }
    for (action, body, expected) in [
        ("parse", json!({"text":include_str!("../examples/geometry-animation/map.toml").replace("[\"moving\"]", "[9007199254740993]")}).to_string(), StatusCode::UNPROCESSABLE_ENTITY),
        ("parse", "{".to_owned(), StatusCode::BAD_REQUEST),
        ("parse", json!({"text":"not toml!"}).to_string(), StatusCode::UNPROCESSABLE_ENTITY),
        ("parse", json!({"text":"schema_version=1\nid='test:map'\noutput_crs=['OGC:CRS84']\nitems=[]", "unknown":1}).to_string(), StatusCode::BAD_REQUEST),
        ("serialize", r#"{"document":{"schema_version":1,"id":"test:map","output_crs":["OGC:CRS84"],"items":[],"unknown":9007199254740993}}"#.into(), StatusCode::UNPROCESSABLE_ENTITY),
        ("serialize", json!({"document":{"schema_version":1,"id":"test:map","output_crs":["OGC:CRS84"],"items":[],"unknown":null}}).to_string(), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let response = send(&app, "POST", &format!("/admin/v1/configurations/map/{action}"), &headers, body).await;
        assert_eq!(response.status(), expected);
    }
    assert_eq!(get(&app, "/admin/v1/resources").await["total"], 0);
    assert_eq!(get(&app, "/admin/v1/operations").await["total"], 0);
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn resource_and_operation_filters_count_before_paging() {
    let root = tempfile::tempdir().unwrap();
    let (state, app) = setup(root.path()).await;
    for id in ["test:Alpha", "test:Beta", "test:Gamma"] {
        publish_style(&app, id).await;
    }
    let page = get(
        &app,
        "/admin/v1/resources?kind=style&q=TEST:&published=true&offset=1&limit=1",
    )
    .await;
    assert_eq!(page["total"], 3);
    assert_eq!(page["resources"].as_array().unwrap().len(), 1);
    assert_eq!(page["resources"][0]["key"]["id"], "test:Beta");
    assert_eq!(get(&app, "/admin/v1/resources?q=alpha").await["total"], 1);
    assert_eq!(
        get(&app, "/admin/v1/resources?published=false").await["total"],
        0
    );
    let operations = get(&app, "/admin/v1/operations?kind=style&status=ready&limit=1").await;
    assert_eq!(operations["total"], 3);
    assert_eq!(operations["operations"][0]["resource"]["id"], "test:Gamma");
    assert_eq!(
        get(&app, "/admin/v1/operations?resource_id=test:Beta").await["total"],
        1
    );
    assert_eq!(
        get(&app, "/admin/v1/operations?resource_id=test:B").await["total"],
        0
    );
    for uri in [
        "/admin/v1/resources?limit=3",
        "/admin/v1/resources?kind=invalid",
        "/admin/v1/resources?published=yes",
        "/admin/v1/resources?kind=style&kind=map",
        "/admin/v1/operations?limit=3",
        "/admin/v1/operations?status=invalid",
        "/admin/v1/operations?offset=-1",
    ] {
        assert_eq!(
            send(&app, "GET", uri, &[], Body::empty()).await.status(),
            StatusCode::BAD_REQUEST,
            "{uri}"
        );
    }
    let long_query = format!("/admin/v1/resources?q={}", "a".repeat(257));
    assert_eq!(
        send(&app, "GET", &long_query, &[], Body::empty())
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn republish_preserves_managed_source_and_conditional_lifecycle() {
    let root = tempfile::tempdir().unwrap();
    let (state, app) = setup(root.path()).await;
    publish_style(&app, "test:style").await;
    let uri = "/admin/v1/resources/style/test:style";
    let response = send(&app, "GET", uri, &[], Body::empty()).await;
    let etag = response.headers()["etag"].to_str().unwrap().to_owned();
    let original = json_body(response).await;
    let action = format!("{uri}:publish");
    assert_eq!(
        send(&app, "POST", &action, &[], Body::empty())
            .await
            .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &action,
            &[("if-none-match", "*")],
            Body::empty()
        )
        .await
        .status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &action,
            &[("if-match", "\"stale\"")],
            Body::empty()
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(&app, "POST", &action, &[("if-match", &etag)], "x")
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let response = send(
        &app,
        "POST",
        &format!("{uri}:unpublish"),
        &[("if-match", &etag)],
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let unpublish_tag = response.headers()["etag"].to_str().unwrap().to_owned();
    let response = send(
        &app,
        "POST",
        &action,
        &[("if-match", &unpublish_tag)],
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let operation = json_body(response).await;
    assert_eq!(terminal(&app, &operation).await["status"], "ready");
    let response = send(&app, "GET", uri, &[], Body::empty()).await;
    let current_tag = response.headers()["etag"].to_str().unwrap().to_owned();
    let current = json_body(response).await;
    assert_eq!(current["ownership"], "managed");
    assert_eq!(current["managed_directory"], original["managed_directory"]);
    assert_eq!(current["source"], original["source"]);
    assert_eq!(current["published"], true);
    let source = current["source"]["path"].as_str().unwrap();
    std::fs::write(source, "invalid toml").unwrap();
    let response = send(
        &app,
        "POST",
        &action,
        &[("if-match", &current_tag)],
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let operation = json_body(response).await;
    assert_eq!(terminal(&app, &operation).await["status"], "failed");
    assert!(Path::new(source).exists());
    assert_eq!(get(&app, uri).await["version"], current["version"]);
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn external_republish_revalidates_path_and_keeps_ownership() {
    let root = tempfile::tempdir().unwrap();
    let (state, app) = setup(root.path()).await;
    let source = root.path().join("external.toml");
    std::fs::write(&source, "schema_version=1\nid='test:external'\nrules=[]").unwrap();
    let uri = "/admin/v1/resources/style/test:external";
    let response = send(
        &app,
        "PUT",
        &format!("{uri}/source"),
        &[("if-none-match", "*"), ("content-type", "application/json")],
        json!({"path":source,"format":"toml"}).to_string(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let operation = json_body(response).await;
    assert_eq!(terminal(&app, &operation).await["status"], "ready");
    for missing in [false, true] {
        if missing {
            std::fs::remove_file(&source).unwrap();
        }
        let response = send(&app, "GET", uri, &[], Body::empty()).await;
        let etag = response.headers()["etag"].to_str().unwrap().to_owned();
        let response = send(
            &app,
            "POST",
            &format!("{uri}:publish"),
            &[("if-match", &etag)],
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let operation = json_body(response).await;
        assert_eq!(
            terminal(&app, &operation).await["status"],
            if missing { "failed" } else { "ready" }
        );
        assert_eq!(get(&app, uri).await["ownership"], "external");
        assert_eq!(
            send(
                &app,
                "DELETE",
                &format!("{uri}/content"),
                &[("if-match", &etag)],
                Body::empty()
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
    }
    state.shutdown().await.unwrap();
}
