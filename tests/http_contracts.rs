//! M4 implemented HTTP discovery/documentation and sanitized rejection contracts.
#![cfg(all(feature = "server", feature = "render-cpu", feature = "read-geojson"))]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use geello::server::*;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use tower::ServiceExt;

async fn state(root: &Path) -> Arc<ServerState> {
    let mut config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
    config.backend = BackendSelection::Cpu;
    config.profile.response_memory_bytes = 8 << 20;
    config.profile.response_disk_bytes = 64 << 20;
    config.public_base_url = "https://configured.example.test/prefix/geello/".into();
    config.data_directory = root.join("data");
    ServerState::open(config).await.unwrap()
}
async fn response(app: &Router, method: &str, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 2 << 20).await.unwrap()).unwrap()
}
async fn publish(app: &Router, kind: &str, id: &str, media: &str, body: impl Into<Body>) {
    let accepted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/admin/v1/resources/{kind}/{id}/content"))
                .header("if-none-match", "*")
                .header("content-type", media)
                .body(body.into())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let operation = json_body(accepted).await;
    let uri = format!(
        "/admin/v1/operations/{}",
        operation["operation_id"].as_str().unwrap()
    );
    for _ in 0..500 {
        let status = json_body(response(app, "GET", &uri).await).await;
        match status["status"].as_str() {
            Some("ready") => return,
            Some("failed") => panic!("{status}"),
            _ => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
        }
    }
    panic!("publication timed out: {id}");
}

#[tokio::test]
async fn explicit_fonts_render_through_publication_maps_and_buffered_metatiles() {
    let root = tempfile::tempdir().unwrap();
    // This test isolates label layout and adjacent crops. Centered WMTS
    // prewarming is exercised separately, including its additional areas.
    let mut config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
    config.backend = BackendSelection::Cpu;
    config.profile.response_memory_bytes = 8 << 20;
    config.profile.response_disk_bytes = 64 << 20;
    config.profile.wmts_prewarm_size = 1;
    config.data_directory = root.path().join("data");
    let s = ServerState::open(config).await.unwrap();
    let app = router(s.clone());
    publish(
        &app,
        "font",
        "test:latin",
        "font/ttf",
        include_bytes!("fixtures/fonts/latin.ttf").as_slice(),
    )
    .await;
    publish(
        &app,
        "font",
        "test:cjk",
        "font/otf",
        include_bytes!("fixtures/fonts/cjk.otf").as_slice(),
    )
    .await;
    publish(&app,"dataset","test:labels","application/geo+json",r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":"a","properties":{"label":"office 中国地图"},"geometry":{"type":"Point","coordinates":[1,1]}}]}"#).await;
    let style = "schema_version=1\nid='test:labels'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='label'\nkind='label'\nfield='/label'\nfonts=[{resource='test:latin',face_index=0},{resource='test:cjk',face_index=0}]\nsize={value=20,unit='px'}\nplacement='point'\ncolor='#000000'\n";
    publish(&app, "style", "test:labels", "application/toml", style).await;
    publish(&app,"map","test:labels","application/toml","schema_version=1\nid='test:labels'\noutput_crs=['OGC:CRS84','EPSG:3857']\n[[items]]\nid='labels'\ndataset='test:labels'\nstyle='test:labels'\n").await;
    let uri = "/maps/test:labels/image?bbox=0,0,10,10&crs=OGC:CRS84&width=256&height=256";
    let result = response(&app, "GET", uri).await;
    assert_eq!(result.status(), StatusCode::OK);
    let image = image::load_from_memory(&to_bytes(result.into_body(), 1 << 20).await.unwrap())
        .unwrap()
        .to_rgba8();
    assert!(image.pixels().filter(|p| p[3] > 0).count() > 100);
    // Same aligned 4x4+128px area is laid out once, then both adjacent tiles
    // are encoded from it. A repeated request reuses the identical crop.
    let before = s.execution_counts();
    let left_uri = "/ogc/tiles/collections/test:labels/map/tiles/WebMercatorQuad/4/7/8";
    let left = response(&app, "GET", left_uri).await;
    assert_eq!(left.status(), StatusCode::OK);
    let left_bytes = to_bytes(left.into_body(), 1 << 20).await.unwrap();
    let after = s.execution_counts();
    let right = response(
        &app,
        "GET",
        "/ogc/tiles/collections/test:labels/map/tiles/WebMercatorQuad/4/7/9",
    )
    .await;
    assert_eq!(right.status(), StatusCode::OK);
    drop(right);
    let final_counts = s.execution_counts();
    assert!(after.0 > before.0);
    assert_eq!(
        after.0, final_counts.0,
        "adjacent crops must share layout/render"
    );
    let again = response(&app, "GET", left_uri).await;
    assert_eq!(
        left_bytes,
        to_bytes(again.into_body(), 1 << 20).await.unwrap()
    );
    let font = s
        .catalog
        .get(&ResourceKey::new(ResourceKind::Font, "test:latin").unwrap())
        .unwrap();
    let rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/v1/resources/font/test:latin/content")
                .header("if-match", font.etag())
                .header("content-type", "font/ttf")
                .body(Body::from("invalid font"))
                .unwrap(),
        )
        .await
        .unwrap();
    let op = json_body(rejected).await;
    let id = op["operation_id"].as_str().unwrap();
    for _ in 0..500 {
        if s.operation(id).unwrap().status == OperationStatus::Failed {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(s.operation(id).unwrap().status, OperationStatus::Failed);
    assert_eq!(s.catalog.get(&font.key).unwrap().etag(), font.etag());
    assert_eq!(response(&app, "GET", uri).await.status(), StatusCode::OK);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn exact_feature_queries_typed_equalities_hits_and_namespace_aware_wfs_agree() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("features.geojson");
    std::fs::write(&source, r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"line","properties":{"name":"A & B","count":9007199254740993,"enabled":true,"mixed":1},"geometry":{"type":"LineString","coordinates":[[0,0],[10,10]]}},
        {"type":"Feature","id":"null","properties":{"name":"A & B","count":9007199254740993,"enabled":false,"mixed":"other","nested":{"x":1},"nullable":null},"geometry":null},
        {"type":"Feature","id":"empty","properties":{"name":"other"},"geometry":{"type":"GeometryCollection","geometries":[]}},
        {"type":"Feature","id":"point","properties":{"name":"other","count":9007199254740992,"enabled":true},"geometry":{"type":"Point","coordinates":[0,10]}}
    ]}"#).unwrap();
    let mut config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
    config.backend = BackendSelection::Cpu;
    config.profile.response_memory_bytes = 8 << 20;
    config.profile.response_disk_bytes = 64 << 20;
    config.data_directory = root.path().join("data");
    config.external_roots = vec![root.path().into()];
    let s = ServerState::open(config).await.unwrap();
    let app = router(s.clone());
    let definition = format!(
        "schema_version=1\nid='test:query'\nsource={{kind='geojson',path='{}'}}\nqueryables=[{{name='name',field='/name',type='string'}},{{name='population',field='/count',type='integer'}},{{name='enabled',field='/enabled',type='boolean'}}]\n",
        source.to_string_lossy().replace('\\', "/")
    );
    let definition_path = root.path().join("dataset.toml");
    std::fs::write(&definition_path, definition).unwrap();
    let accepted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/v1/resources/dataset/test:query/source")
                .header("if-none-match", "*")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"path":definition_path,"format":"toml"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let operation = json_body(accepted).await;
    let operation_id = operation["operation_id"].as_str().unwrap();
    for _ in 0..500 {
        let status = s.operation(operation_id).unwrap();
        if status.status == OperationStatus::Ready {
            break;
        }
        assert_ne!(status.status, OperationStatus::Failed, "{status:?}");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        s.operation(operation_id).unwrap().status,
        OperationStatus::Ready
    );
    let bbox = "bbox=0,9,1,10";
    let page = json_body(
        response(
            &app,
            "GET",
            &format!("/ogc/features/collections/test:query/items?{bbox}"),
        )
        .await,
    )
    .await;
    assert_eq!(page["numberMatched"], 2); // null plus point, never the diagonal's bounding box
    assert_eq!(page["features"][0]["id"], "null");
    let wfs = "/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=test:query";
    let page = json_body(response(&app, "GET", &format!("{wfs}&BBOX=0,9,1,10")).await).await;
    assert_eq!(page["numberMatched"], 1);
    assert_eq!(page["features"][0]["id"], "point");
    let page = json_body(response(&app, "GET", "/ogc/features/collections/test:query/items?population=9007199254740993&enabled=true&name=A%20%26%20B").await).await;
    assert_eq!(page["numberMatched"], 1);
    assert_eq!(page["features"][0]["id"], "line");
    assert_eq!(
        page["features"][0]["geometry"]["coordinates"],
        json!([[0.0, 0.0], [10.0, 10.0]])
    );
    let hits = json_body(
        response(
            &app,
            "GET",
            &format!("{wfs}&RESULTTYPE=hits&name=A%20%26%20B&COUNT=1&STARTINDEX=1"),
        )
        .await,
    )
    .await;
    assert_eq!(hits["numberMatched"], 2);
    assert_eq!(hits["numberReturned"], 0);
    assert_eq!(hits["features"], json!([]));
    let xml = r#"<w:GetFeature xmlns:w="http://www.opengis.net/wfs/2.0" xmlns:f="http://www.opengis.net/fes/2.0" resultType="hits"><w:Query typeNames="test:query"><f:Filter><f:PropertyIsEqualTo><f:ValueReference>name</f:ValueReference><f:Literal>A &amp; B</f:Literal></f:PropertyIsEqualTo></f:Filter></w:Query></w:GetFeature>"#;
    let result = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/wfs")
                .header("content-type", "application/xml")
                .body(Body::from(xml))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(json_body(result).await["numberMatched"], 2);
    let selected =
        json_body(response(&app, "GET", &format!("{wfs}&RESOURCEID=s6c696e65")).await).await;
    assert_eq!(selected["features"][0]["id"], "line");
    for suffix in [
        "enabled=1",
        "population=1.1",
        "unknown=1",
        "population=18446744073709551616",
    ] {
        assert_eq!(
            response(
                &app,
                "GET",
                &format!("/ogc/features/collections/test:query/items?{suffix}")
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let schema = response(
        &app,
        "GET",
        "/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=DescribeFeatureType&TYPENAMES=test:query",
    )
    .await;
    let schema = String::from_utf8(
        to_bytes(schema.into_body(), 1 << 20)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(schema.contains("does not validate JSON"));
    assert!(schema.contains("name=\"p6d69786564\" type=\"xs:anyType\""));
    assert!(schema.contains("name=\"p636f756e74\" type=\"xs:integer\" minOccurs=\"0\""));
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn wms_composes_in_request_order_normalizes_axes_and_encodes_final_background_once() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    publish(&app,"dataset","test:pixel","application/geo+json", r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":1,"properties":{},"geometry":{"type":"Point","coordinates":[10,20]}}]}"#).await;
    for (id, color) in [("red", "#ff0000"), ("blue", "#0000ff")] {
        publish(&app,"style",&format!("test:{id}"),"application/toml", format!("schema_version=1\nid='test:{id}'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='point'\nkind='point'\nfill='{color}'\nsize={{value=16,unit='px'}}\n")).await;
        publish(&app,"map",&format!("test:{id}"),"application/toml",format!("schema_version=1\nid='test:{id}'\noutput_crs=['OGC:CRS84']\n[[items]]\nid='points'\ndataset='test:pixel'\nstyle='test:{id}'\n")).await;
    }
    let base = "/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&STYLES=&WIDTH=32&HEIGHT=32";
    async fn image(app: &Router, uri: String) -> image::RgbaImage {
        let result = response(app, "GET", &uri).await;
        assert_eq!(result.status(), StatusCode::OK, "{uri}");
        image::load_from_memory(&to_bytes(result.into_body(), 1 << 20).await.unwrap())
            .unwrap()
            .to_rgba8()
    }
    let blue = image(&app,format!("{base}&LAYERS=test:red,test:blue&CRS=CRS:84&BBOX=9,19,11,21&FORMAT=image/png&TRANSPARENT=true")).await;
    assert_eq!(blue.get_pixel(16, 16).0, [0, 0, 255, 255]);
    assert_eq!(blue.get_pixel(0, 0).0, [0, 0, 0, 0]);
    let axes = image(&app,format!("{base}&LAYERS=test:red,test:blue&CRS=EPSG:4326&BBOX=19,9,21,11&FORMAT=image/png&TRANSPARENT=true")).await;
    assert_eq!(blue, axes);
    let red = image(&app,format!("{base}&LAYERS=test:blue,test:red&CRS=CRS:84&BBOX=9,19,11,21&FORMAT=image/png&BGCOLOR=0x00ff00")).await;
    assert_eq!(red.get_pixel(16, 16).0, [255, 0, 0, 255]);
    assert_eq!(red.get_pixel(0, 0).0, [0, 255, 0, 255]);
    let jpeg = image(&app,format!("{base}&LAYERS=test:blue&CRS=CRS:84&BBOX=9,19,11,21&FORMAT=image/jpeg&TRANSPARENT=true&BGCOLOR=0x00ff00")).await;
    let corner = jpeg.get_pixel(0, 0).0;
    assert!(corner[0] < 10 && corner[1] > 245 && corner[2] < 10 && corner[3] == 255);
    for uri in [
        format!(
            "{base}&LAYERS=test:blue&CRS=CRS:84&BBOX=9,19,11,21&FORMAT=image/png&TIME=0&geello_time=1"
        ),
        "/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=Unknown".into(),
    ] {
        let result = response(&app, "GET", &uri).await;
        assert!(result.status().is_client_error() || result.status().is_server_error());
        assert_eq!(result.headers()["content-type"], "text/xml; charset=utf-8");
        assert!(
            String::from_utf8(to_bytes(result.into_body(), 8192).await.unwrap().to_vec())
                .unwrap()
                .contains("ServiceExceptionReport")
        );
    }
    s.shutdown().await.unwrap();
}
fn assert_schema(document: &Value, schema: &Value, value: &Value) {
    if let Some(reference) = schema["$ref"].as_str() {
        return assert_schema(
            document,
            document
                .pointer(reference.strip_prefix('#').unwrap())
                .unwrap(),
            value,
        );
    }
    if value.is_null() && schema["nullable"] == true {
        return;
    }
    if let Some(variants) = schema["enum"].as_array() {
        assert!(variants.contains(value), "{value} not in {variants:?}");
    }
    match schema["type"].as_str() {
        Some("object") => {
            let object = value.as_object().unwrap();
            if let Some(required) = schema["required"].as_array() {
                for key in required {
                    assert!(
                        object.contains_key(key.as_str().unwrap()),
                        "missing {key}: {value}"
                    );
                }
            }
            for (key, value) in object {
                if let Some(property) = schema["properties"].get(key) {
                    assert_schema(document, property, value);
                } else {
                    assert_ne!(
                        schema["additionalProperties"], false,
                        "unexpected field {key}"
                    );
                }
            }
        }
        Some("array") => {
            let array = value.as_array().unwrap();
            for element in array {
                assert_schema(document, &schema["items"], element);
            }
            if let Some(minimum) = schema["minItems"].as_u64() {
                assert!(array.len() as u64 >= minimum);
            }
            if let Some(maximum) = schema["maxItems"].as_u64() {
                assert!(array.len() as u64 <= maximum);
            }
        }
        Some("string") => assert!(value.is_string(), "expected string: {value}"),
        Some("boolean") => assert!(value.is_boolean()),
        Some("integer") => assert!(value.as_u64().is_some() || value.as_i64().is_some()),
        Some("number") => assert!(value.is_number()),
        Some(other) => panic!("unsupported contract type {other}"),
        None => {}
    }
    if let Some(minimum) = schema["minimum"].as_f64() {
        assert!(value.as_f64().unwrap() >= minimum);
    }
    if let Some(maximum) = schema["maximum"].as_f64() {
        assert!(value.as_f64().unwrap() <= maximum);
    }
}
fn check_references(document: &Value, value: &Value) {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref") {
                assert!(
                    document
                        .pointer(reference.as_str().unwrap().strip_prefix('#').unwrap())
                        .is_some(),
                    "unresolved {reference}"
                );
            }
            for value in map.values() {
                check_references(document, value);
            }
        }
        Value::Array(array) => {
            for value in array {
                check_references(document, value);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn served_openapi_describes_real_routes_schemas_and_configured_proxy_prefix() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let request = Request::builder()
        .uri("/api")
        .header("host", "attacker.example.test")
        .header("forwarded", "host=attacker.example.test;proto=http")
        .header("x-forwarded-prefix", "/attacker")
        .body(Body::empty())
        .unwrap();
    let result = app.clone().oneshot(request).await.unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(result.headers()["content-type"], "application/json");
    let document = json_body(result).await;
    assert_eq!(document["openapi"], "3.0.3");
    assert_eq!(
        document["servers"],
        json!([{"url":"https://configured.example.test/prefix/geello"}])
    );
    assert!(document["paths"].get("/ws/maps/{id}").is_some());
    assert_eq!(document["x-geello-enabled"]["read-geojson"], true);
    assert_eq!(
        document["x-geello-enabled"]["read-shapefile"],
        cfg!(feature = "read-shapefile")
    );
    assert_eq!(
        document["x-geello-enabled"]["render-gpu"],
        cfg!(feature = "render-gpu")
    );
    assert_eq!(document["x-geello-enabled"]["actual-backend"], "cpu");
    assert_eq!(
        document["x-geello-enabled"]["max-dimension"],
        s.config.profile.max_dimension
    );
    check_references(&document, &document);
    let expected = [
        ("/api", "get"),
        ("/api.html", "get"),
        ("/health/ready", "get"),
        ("/health/live", "get"),
        ("/admin/v1/resources", "get"),
        ("/admin/v1/resources/{kind}/{id}", "get"),
        ("/admin/v1/resources/{kind}/{id}", "delete"),
        ("/admin/v1/resources/{kind}/{id}:unpublish", "post"),
        ("/admin/v1/resources/{kind}/{id}/content", "put"),
        ("/admin/v1/resources/{kind}/{id}/content", "delete"),
        ("/admin/v1/resources/{kind}/{id}/source", "put"),
        ("/admin/v1/operations/{id}", "get"),
        ("/maps/{id}/image", "get"),
        ("/tiles/{id}/{z}/{x}/{y}", "get"),
    ];
    let mut expected = expected
        .into_iter()
        .map(|(path, method)| (path.to_owned(), method.to_owned()))
        .collect::<BTreeSet<_>>();
    for root in ["/ogc/maps", "/ogc/tiles"] {
        for suffix in [
            "",
            "/",
            "/api",
            "/api.html",
            "/conformance",
            "/collections",
            "/collections/{id}",
            "/tileMatrixSets",
            "/tileMatrixSets/{id}",
            "/collections/{id}/map/tiles",
            "/collections/{id}/map/tiles/{grid}",
            "/collections/{id}/map/tiles/{grid}/{z}/{row}/{col}",
        ] {
            expected.insert((format!("{root}{suffix}"), "get".to_owned()));
        }
    }
    expected.insert(("/ogc/maps/collections/{id}/map".into(), "get".into()));
    expected.insert(("/wms".into(), "get".into()));
    expected.insert(("/wmts".into(), "get".into()));
    expected.insert(("/wfs".into(), "get".into()));
    expected.insert(("/wfs".into(), "post".into()));
    expected.insert(("/ws/maps/{id}".into(), "get".into()));
    for suffix in [
        "",
        "/",
        "/conformance",
        "/collections",
        "/collections/{id}",
        "/collections/{id}/items",
        "/collections/{id}/items/{feature_id}",
    ] {
        expected.insert((format!("/ogc/features{suffix}"), "get".into()));
    }
    let mut declared = BTreeSet::new();
    let mut operation_ids = BTreeSet::new();
    for (path, methods) in document["paths"].as_object().unwrap() {
        for (method, operation) in methods.as_object().unwrap() {
            declared.insert((path.to_owned(), method.to_owned()));
            assert!(operation_ids.insert(operation["operationId"].as_str().unwrap()));
            let mut parameters = BTreeSet::new();
            for parameter in operation["parameters"].as_array().unwrap() {
                assert!(parameters.insert((
                    parameter["in"].as_str().unwrap(),
                    parameter["name"].as_str().unwrap()
                )));
            }
            // Exercise each documented method at its real path with no data.
            // Handler-specific validation/lookup may fail, route matching must not.
            let concrete = path
                .replace("{kind}", "style")
                .replace("{id}", "test:missing")
                .replace("{z}", "0")
                .replace("{x}", "0")
                .replace("{y}", "0");
            let concrete = concrete
                .replace("{grid}", "WebMercatorQuad")
                .replace("{row}", "0")
                .replace("{col}", "0");
            let result = response(&app, &method.to_uppercase(), &concrete).await;
            assert_ne!(
                result.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {path}"
            );
            if result.status() == StatusCode::NOT_FOUND {
                assert_ne!(
                    json_body(result).await["type"],
                    "urn:geello:problem:route-not-found",
                    "{path}"
                );
            }
        }
    }
    assert_eq!(declared, expected);
    let ready = json_body(response(&app, "GET", "/health/ready").await).await;
    assert_schema(&document, &ref_schema("Ready"), &ready);
    let page = json_body(response(&app, "GET", "/admin/v1/resources?limit=1").await).await;
    assert_schema(&document, &ref_schema("ResourcePage"), &page);
    let problem =
        json_body(response(&app, "GET", "/admin/v1/resources?unexpected=true").await).await;
    assert_schema(&document, &ref_schema("Problem"), &problem);
    for uri in [] {
        assert!(document["paths"].get(uri).is_none());
        assert_eq!(
            response(&app, "GET", uri).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    s.shutdown().await.unwrap();
}
fn ref_schema(name: &str) -> Value {
    json!({"$ref":format!("#/components/schemas/{name}")})
}

#[tokio::test]
async fn documented_resource_and_operation_schemas_match_live_publication_and_revalidation() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let document = json_body(response(&app, "GET", "/api").await).await;
    let endpoint = "/admin/v1/resources/dataset/test:dataset/content";
    let data = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":42,"properties":{"name":"station"},"geometry":{"type":"Point","coordinates":[0,0]}}]}"#;
    let request = Request::builder()
        .method("PUT")
        .uri(endpoint)
        .header("if-none-match", "*")
        .header("content-type", "application/geo+json")
        .body(Body::from(data))
        .unwrap();
    let accepted = app.clone().oneshot(request).await.unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let accepted = json_body(accepted).await;
    assert_schema(&document, &ref_schema("Operation"), &accepted);
    let operation_uri = format!(
        "/admin/v1/operations/{}",
        accepted["operation_id"].as_str().unwrap()
    );
    let terminal = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let operation = json_body(response(&app, "GET", &operation_uri).await).await;
            assert_schema(&document, &ref_schema("Operation"), &operation);
            if matches!(operation["status"].as_str(), Some("ready" | "failed")) {
                break operation;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(terminal["status"], "ready", "{terminal}");
    let resource_uri = "/admin/v1/resources/dataset/test:dataset";
    let result = response(&app, "GET", resource_uri).await;
    assert_eq!(result.status(), StatusCode::OK);
    let etag = result.headers()["etag"].clone();
    let record = json_body(result).await;
    assert_schema(&document, &ref_schema("ResourceRecord"), &record);
    assert_eq!(
        etag.to_str().unwrap(),
        format!("\"{}\"", record["version"].as_str().unwrap())
    );
    assert_eq!(
        record["effective_crs"],
        json!({"definition":"OGC:CRS84","space":"GeographicXy"})
    );
    let source = json!({"path":"explicit.geojson","format":"geojson"});
    assert_schema(&document, &ref_schema("SourceSpec"), &source);
    let page = json_body(response(&app, "GET", "/admin/v1/resources").await).await;
    assert_schema(&document, &ref_schema("ResourcePage"), &page);
    assert_eq!(page["resources"][0], record);
    let failed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(resource_uri)
                .header("if-match", "\"stale\"")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::CONFLICT);
    assert_schema(&document, &ref_schema("Problem"), &json_body(failed).await);
    let result = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{resource_uri}:unpublish"))
                .header("if-match", etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let etag = result.headers()["etag"].clone();
    let record = json_body(result).await;
    assert_schema(&document, &ref_schema("ResourceRecord"), &record);
    assert_eq!(record["published"], false);
    let result = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(resource_uri)
                .header("if-match", etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::NO_CONTENT);
    assert!(
        to_bytes(result.into_body(), 1 << 20)
            .await
            .unwrap()
            .is_empty()
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn openapi_response_retains_admission_and_small_output_profiles_fail_cleanly() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let before = s.scheduler.live_bytes();
    let result = response(&app, "GET", "/api").await;
    assert_eq!(result.status(), StatusCode::OK);
    assert!(s.scheduler.live_bytes() > before);
    drop(result);
    assert_eq!(s.scheduler.live_bytes(), before);
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);
    let mut config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
    config.backend = BackendSelection::Cpu;
    config.profile.response_memory_bytes = 8 << 20;
    config.profile.response_disk_bytes = 64 << 20;
    config.data_directory = root.path().join("small");
    config.profile.max_registry_bytes = 1024;
    let s = ServerState::open(config).await.unwrap();
    let app = router(s.clone());
    let before = s.scheduler.live_bytes();
    let result = response(&app, "GET", "/api").await;
    assert_eq!(result.status(), StatusCode::SERVICE_UNAVAILABLE);
    let problem = json_body(result).await;
    assert_eq!(problem["type"], "urn:geello:problem:budget");
    assert_eq!(s.scheduler.live_bytes(), before);
    assert_eq!(
        response(&app, "GET", "/api.html").await.status(),
        StatusCode::OK
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn ogc_maps_tiles_subset_discovers_only_published_maps_and_uses_declared_grids() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let data = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":"a","properties":{},"geometry":{"type":"Point","coordinates":[0,0]}},{"type":"Feature","id":"b","properties":{},"geometry":{"type":"Point","coordinates":[0.5,0.5]}},{"type":"Feature","id":"c","properties":{},"geometry":{"type":"Point","coordinates":[0.75,0.75]}}]}"#;
    let style = "schema_version=1\nid='test:ogc-style'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='point'\nkind='point'\nfill='#00ff00'\nsize={value=12,unit='px'}\n";
    let map = "schema_version=1\nid='test:ogc-map'\noutput_crs=['OGC:CRS84','EPSG:3857']\ntiles={matrix_sets=['WebMercatorQuad','WorldCRS84Quad'],min_zoom=0,max_zoom=2}\n[[items]]\nid='points'\ndataset='test:ogc-data'\nstyle='test:ogc-style'\n";
    for (kind, id, media, body) in [
        ("dataset", "test:ogc-data", "application/geo+json", data),
        ("style", "test:ogc-style", "application/toml", style),
        ("map", "test:ogc-map", "application/toml", map),
    ] {
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/admin/v1/resources/{kind}/{id}/content"))
            .header("if-none-match", "*")
            .header("content-type", media)
            .body(Body::from(body))
            .unwrap();
        let accepted = app.clone().oneshot(request).await.unwrap();
        assert_eq!(accepted.status(), StatusCode::ACCEPTED);
        let op = json_body(accepted).await;
        let operation_uri = format!("/admin/v1/operations/{}", op["operation_id"]);
        for _ in 0..300 {
            let current = json_body(response(&app, "GET", &operation_uri).await).await;
            if matches!(current["status"].as_str(), Some("ready" | "failed")) {
                assert_eq!(current["status"], "ready", "{current}");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    let landing = json_body(response(&app, "GET", "/ogc/maps").await).await;
    assert_eq!(
        landing["links"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["rel"] == "conformance")
            .count(),
        1
    );
    assert!(
        landing["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["href"].as_str().unwrap().ends_with("/api"))
    );
    let feature_landing = json_body(response(&app, "GET", "/ogc/features").await).await;
    assert_eq!(feature_landing["title"], "Geello Features tested subset");
    let feature_collections =
        json_body(response(&app, "GET", "/ogc/features/collections").await).await;
    assert_eq!(
        feature_collections["collections"].as_array().unwrap().len(),
        1
    );
    let feature_page = json_body(
        response(
            &app,
            "GET",
            "/ogc/features/collections/test:ogc-data/items?bbox=-1,-1,1,1&limit=1",
        )
        .await,
    )
    .await;
    assert_eq!(feature_page["type"], "FeatureCollection");
    assert_eq!(feature_page["numberReturned"], 1);
    assert_eq!(feature_page["features"][0]["id"], "a");
    let next = feature_page["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["rel"] == "next")
        .and_then(|link| link["href"].as_str())
        .unwrap();
    let next_path = next
        .strip_prefix("https://configured.example.test/prefix/geello")
        .unwrap();
    let next_page = json_body(response(&app, "GET", next_path).await).await;
    assert_eq!(next_page["features"][0]["id"], "b");
    assert_eq!(next_page["numberReturned"], 1);
    let filtered = json_body(
        response(
            &app,
            "GET",
            "/ogc/features/collections/test:ogc-data/items?bbox=0.1,0.1,1,1&limit=1",
        )
        .await,
    )
    .await;
    assert_eq!(filtered["features"][0]["id"], "b");
    let next = filtered["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["rel"] == "next")
        .unwrap()["href"]
        .as_str()
        .unwrap();
    assert!(next.contains("?cursor=v2."));
    assert!(!next.contains("&bbox="));
    let next_path = next
        .strip_prefix("https://configured.example.test/prefix/geello")
        .unwrap();
    let next_page = json_body(response(&app, "GET", next_path).await).await;
    assert_eq!(next_page["features"][0]["id"], "c");
    assert!(
        !next_page["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link["rel"] == "next")
    );
    assert_eq!(
        response(
            &app,
            "GET",
            "/ogc/features/collections/test:ogc-data/items?cursor=v1.0.1"
        )
        .await
        .status(),
        StatusCode::GONE
    );
    assert_eq!(
        response(
            &app,
            "GET",
            "/ogc/features/collections/test:ogc-data/items?cursor=bad&limit=1"
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        response(
            &app,
            "GET",
            "/ogc/features/collections/test:ogc-data/items?limit=1&limit=2"
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        response(
            &app,
            "GET",
            "/ogc/features/collections/test:ogc-data/items?private=x"
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let tile_landing = json_body(response(&app, "GET", "/ogc/tiles/").await).await;
    assert_eq!(
        tile_landing["description"],
        "Implemented OGC API Tiles discovery and PNG tile subset."
    );
    let conform = json_body(response(&app, "GET", "/ogc/maps/conformance").await).await;
    assert_eq!(
        conform["x-geello-status"],
        "tested subset; not an official conformance certification"
    );
    assert_eq!(conform["conformsTo"].as_array().unwrap().len(), 0);
    let list = json_body(response(&app, "GET", "/ogc/maps/collections").await).await;
    assert_eq!(list["collections"].as_array().unwrap().len(), 1);
    assert_eq!(list["collections"][0]["id"], "test:ogc-map");
    let collection =
        json_body(response(&app, "GET", "/ogc/maps/collections/test:ogc-map").await).await;
    assert_eq!(collection["itemType"], "map");
    let sets = json_body(response(&app, "GET", "/ogc/tiles/tileMatrixSets").await).await;
    assert_eq!(sets["tileMatrixSets"].as_array().unwrap().len(), 2);
    let world =
        json_body(response(&app, "GET", "/ogc/maps/tileMatrixSets/WorldCRS84Quad").await).await;
    assert_eq!(world["crs"], "http://www.opengis.net/def/crs/OGC/1.3/CRS84");
    assert_eq!(world["tileMatrices"][0]["matrixWidth"], 2);
    assert_eq!(world["tileMatrices"][0]["matrixHeight"], 1);
    let tilesets =
        json_body(response(&app, "GET", "/ogc/tiles/collections/test:ogc-map/map/tiles").await)
            .await;
    assert_eq!(tilesets["tilesets"].as_array().unwrap().len(), 2);
    assert_eq!(
        tilesets["tilesets"][0]["tileMatrixSetLimits"][0]["maxTileCol"],
        0
    );
    assert_eq!(
        response(&app, "GET", "/ogc/maps?unexpected=x")
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let image=response(&app,"GET","/ogc/maps/collections/test:ogc-map/map?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32&geello_time=0").await;
    assert_eq!(image.status(), StatusCode::OK);
    assert_eq!(image.headers()["content-type"], "image/png");
    let etag = image.headers()["etag"].clone();
    let revalidated = app.clone().oneshot(Request::builder()
        .uri("/ogc/maps/collections/test:ogc-map/map?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32&geello_time=0")
        .header("if-none-match", etag).header("x-request-id", "cached-304")
        .body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(revalidated.headers()["x-request-id"], "cached-304");
    let tile = response(
        &app,
        "GET",
        "/ogc/tiles/collections/test:ogc-map/map/tiles/WebMercatorQuad/0/0/0?geello_time=0",
    )
    .await;
    assert_eq!(tile.status(), StatusCode::OK);
    assert_eq!(tile.headers()["content-type"], "image/png");
    let world_tile = response(
        &app,
        "GET",
        "/ogc/maps/collections/test:ogc-map/map/tiles/WorldCRS84Quad/0/0/0",
    )
    .await;
    assert_eq!(
        world_tile.status(),
        StatusCode::OK,
        "{}",
        json_body(world_tile).await
    );
    for uri in [
        "/ogc/tiles/collections/test:ogc-map/map/tiles/WebMercatorQuad/3/0/0",
        "/ogc/tiles/collections/test:ogc-map/map/tiles/WorldCRS84Quad/0/2/0",
        "/ogc/tiles/collections/test:ogc-map/map/tiles/WebMercatorQuad/0/0/0?geello_time=0&geello_time=1",
        "/ogc/maps/collections/test:ogc-map/map?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32&unexpected=x",
    ] {
        let result = response(&app, "GET", uri).await;
        let expected = if uri.contains("/3/0/0") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        assert_eq!(result.status(), expected, "{uri}");
        assert_eq!(
            result.headers()["content-type"],
            "application/problem+json",
            "{uri}"
        );
    }
    let unknown = response(
        &app,
        "GET",
        "/ogc/tiles/collections/test:ogc-map/map/tiles/WebMercatorQuad/0/0/0?geello_time=0.5",
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::OK);
    let wms_caps = response(
        &app,
        "GET",
        "/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetCapabilities",
    )
    .await;
    assert_eq!(wms_caps.status(), StatusCode::OK);
    assert_eq!(
        wms_caps.headers()["content-type"],
        "text/xml; charset=utf-8"
    );
    let wms_map = response(
        &app,
        "GET",
        "/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=test:ogc-map&STYLES=&CRS=OGC:CRS84&BBOX=-1,-1,1,1&WIDTH=32&HEIGHT=32&FORMAT=image/png",
    )
    .await;
    assert_eq!(wms_map.status(), StatusCode::OK);
    assert_eq!(wms_map.headers()["content-type"], "image/png");
    let wmts_caps = response(
        &app,
        "GET",
        "/wmts?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetCapabilities",
    )
    .await;
    assert_eq!(wmts_caps.status(), StatusCode::OK);
    let wmts_tile = response(
        &app,
        "GET",
        "/wmts?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile&LAYER=test:ogc-map&STYLE=default&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX=0&TILEROW=0&TILECOL=0&FORMAT=image/png",
    )
    .await;
    assert_eq!(wmts_tile.status(), StatusCode::OK);
    let wfs_caps = response(
        &app,
        "GET",
        "/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetCapabilities",
    )
    .await;
    assert_eq!(wfs_caps.status(), StatusCode::OK);
    let wfs_schema = response(
        &app,
        "GET",
        "/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=DescribeFeatureType&TYPENAMES=test:ogc-data",
    )
    .await;
    assert_eq!(wfs_schema.status(), StatusCode::OK);
    let wfs_features = response(&app, "GET", "/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=test:ogc-data&OUTPUTFORMAT=application/json&COUNT=1&BBOX=-1,-1,1,1").await;
    assert_eq!(wfs_features.status(), StatusCode::OK);
    assert_eq!(json_body(wfs_features).await["numberReturned"], 1);
    let xml = Request::builder()
        .method("POST")
        .uri("/wfs")
        .header("content-type", "application/xml")
        .body(Body::from(r#"<wfs:GetFeature xmlns:wfs="http://www.opengis.net/wfs/2.0" service="WFS" version="2.0.0" typeNames="test:ogc-data" outputFormat="application/json" count="1"/>"#))
        .unwrap();
    let wfs_xml = app.clone().oneshot(xml).await.unwrap();
    assert_eq!(wfs_xml.status(), StatusCode::OK);
    assert_eq!(json_body(wfs_xml).await["numberReturned"], 1);
    for (body, expected_type) in [
        (
            r#"<?xml version='1.0'?><q:GetFeature xmlns:q='http://www.opengis.net/wfs/2.0' count='1'><q:Query typeNames='test:ogc-data'/></q:GetFeature>"#,
            "application/geo+json",
        ),
        (
            r#"<GetCapabilities xmlns='http://www.opengis.net/wfs/2.0'/>"#,
            "text/xml; charset=utf-8",
        ),
        (
            r#"<DescribeFeatureType xmlns='http://www.opengis.net/wfs/2.0' typeNames='test:ogc-data'/>"#,
            "text/xml; charset=utf-8",
        ),
    ] {
        let result = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/wfs")
                    .header("content-type", "application/xml")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::OK);
        assert_eq!(result.headers()["content-type"], expected_type);
    }
    for body in [
        r#"<GetFeature typeNames="test:ogc-data"><Query/></GetFeature>"#,
        r#"<GetFeature typeNames="test:ogc-data"/><Query/>"#,
    ] {
        let result = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/wfs")
                    .header("content-type", "application/xml")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::BAD_REQUEST);
        assert!(
            String::from_utf8(to_bytes(result.into_body(), 8192).await.unwrap().to_vec())
                .unwrap()
                .contains("ExceptionReport")
        );
    }
    let bad_xml = Request::builder()
        .method("POST")
        .uri("/wfs")
        .header("content-type", "application/xml")
        .body(Body::from(
            r#"<wfs:GetFeature service="WFS" version="2.0.0" private="x"/>"#,
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(bad_xml).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        response(
            &app,
            "GET",
            "/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=test:ogc-data&private=x"
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        response(
            &app,
            "GET",
            "/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetCapabilities&REQUEST=GetMap",
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn http_rejections_use_sanitized_problem_json_and_preserve_allow() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let before = s.execution_counts();
    for uri in [
        "/admin/v1/resources?limit=private-secret",
        "/admin/v1/resources?limit=1&limit=2",
        "/admin/v1/resources?unexpected=private-secret",
        "/admin/v1/resources?offset=-1",
        "/maps/test:missing/image?bbox=0,0,1,1&crs=OGC:CRS84&width=private-secret&height=32",
        "/maps/test:missing/image?bbox=0,0,1,1&crs=OGC:CRS84&width=32&height=32&geello_time=0&geello_time=1",
        "/maps/test:missing/image?bbox=0,0,1,1&crs=OGC:CRS84&width=32&height=32&TIME=1",
        "/tiles/test:missing/private-secret/0/0",
        "/tiles/test:missing/0/-1/0",
        "/tiles/test:missing/0/0/0?geello_time=private-secret",
        "/tiles/test:missing/0/0/0?geello_time=0&geello_time=1",
        "/tiles/test:missing/0/0/0?unexpected=private-secret",
        "/admin/v1/resources/style/%FF",
    ] {
        let result = response(&app, "GET", uri).await;
        assert_eq!(result.status(), StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(result.headers()["content-type"], "application/problem+json");
        let body = json_body(result).await;
        assert_eq!(body["status"], 400);
        assert_eq!(body["type"], "urn:geello:problem:invalid-request");
        assert!(!body.to_string().contains("private-secret"));
        assert!(matches!(
            body["title"].as_str().unwrap(),
            "Invalid query parameters" | "Invalid path parameters"
        ));
    }
    let missing = response(&app, "GET", "/private-secret").await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json_body(missing).await,
        json!({"type":"urn:geello:problem:route-not-found","title":"Route not found","status":404})
    );
    let result = response(&app, "PATCH", "/maps/test:missing/image").await;
    assert_eq!(result.status(), StatusCode::METHOD_NOT_ALLOWED);
    let allow = result.headers()["allow"]
        .to_str()
        .unwrap()
        .split(',')
        .collect::<BTreeSet<_>>();
    assert_eq!(allow, BTreeSet::from(["GET", "HEAD"]));
    let body = json_body(result).await;
    assert_eq!(body["status"], 405);
    assert_eq!(body["type"], "urn:geello:problem:method-not-allowed");
    assert_eq!(s.execution_counts(), before);
    s.shutdown().await.unwrap();
    let result = response(&app, "GET", "/api").await;
    assert_eq!(result.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(result.headers()["content-type"], "application/problem+json");
}

#[tokio::test]
async fn feature_snapshots_survive_refresh_unpublish_and_reject_query_or_cursor_tampering() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    async fn publish(app: &Router, data: &'static str, etag: Option<&str>) {
        let mut request = Request::builder()
            .method("PUT")
            .uri("/admin/v1/resources/dataset/test:snapshot/content")
            .header("content-type", "application/geo+json");
        request = if let Some(etag) = etag {
            request.header("if-match", etag)
        } else {
            request.header("if-none-match", "*")
        };
        let result = app
            .clone()
            .oneshot(request.body(Body::from(data)).unwrap())
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::ACCEPTED);
        let op = json_body(result).await;
        let path = format!(
            "/admin/v1/operations/{}",
            op["operation_id"].as_str().unwrap()
        );
        for _ in 0..500 {
            let result = json_body(response(app, "GET", &path).await).await;
            match result["status"].as_str() {
                Some("ready") => return,
                Some("failed") => panic!("{result}"),
                _ => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
            }
        }
        panic!("publication did not complete");
    }
    fn next(page: &Value) -> String {
        page["links"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["rel"] == "next")
            .unwrap()["href"]
            .as_str()
            .unwrap()
            .strip_prefix("https://configured.example.test/prefix/geello")
            .unwrap()
            .into()
    }
    let old = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":42,"properties":{"version":"old-number"},"geometry":{"type":"Point","coordinates":[0,0]}},{"type":"Feature","id":"42","properties":{"version":"old-string"},"geometry":null},{"type":"Feature","id":"empty","properties":{},"geometry":{"type":"GeometryCollection","geometries":[]}},{"type":"Feature","id":"last","properties":{},"geometry":{"type":"Point","coordinates":[0.5,0.5]}}]}"#;
    publish(&app, old, None).await;
    let path = "/ogc/features/collections/test:snapshot/items?bbox=-1,-1,1,1&limit=1";
    let result = response(&app, "GET", path).await;
    assert_eq!(result.headers()["content-type"], "application/geo+json");
    let first = json_body(result).await;
    assert_eq!(first["features"][0]["id"], 42);
    let second_path = next(&first);
    for suffix in ["&limit=2", "&offset=0", "&bbox=-2,-2,2,2"] {
        assert_eq!(
            response(&app, "GET", &format!("{second_path}{suffix}"))
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut tampered = second_path.clone();
    let index = tampered.rfind('.').unwrap() + 1;
    tampered.replace_range(
        index..index + 1,
        if &tampered[index..index + 1] == "a" {
            "b"
        } else {
            "a"
        },
    );
    assert_eq!(
        response(&app, "GET", &tampered).await.status(),
        StatusCode::GONE
    );
    for (token, expected) in [("n42", "old-number"), ("s3432", "old-string")] {
        let item = json_body(
            response(
                &app,
                "GET",
                &format!("/ogc/features/collections/test:snapshot/items/{token}"),
            )
            .await,
        )
        .await;
        assert_eq!(item["properties"]["version"], expected);
    }
    let key = ResourceKey::new(ResourceKind::Dataset, "test:snapshot").unwrap();
    let original = s.catalog.get(&key).unwrap();
    let replacement = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":"new","properties":{},"geometry":{"type":"Point","coordinates":[0,0]}}]}"#;
    publish(&app, replacement, Some(&original.etag())).await;
    s.catalog.collect_garbage().unwrap();
    assert!(original.derived_path.as_ref().unwrap().exists());
    let second = json_body(response(&app, "GET", &second_path).await).await;
    assert_eq!(second["features"][0]["id"], "42");
    assert_eq!(
        second["x-geello-data-version"],
        first["x-geello-data-version"]
    );
    let current = json_body(response(&app, "GET", path).await).await;
    assert_eq!(current["features"][0]["id"], "new");
    let record = s.catalog.get(&key).unwrap();
    s.catalog
        .unpublish(&key, &Precondition::Version(record.version))
        .unwrap();
    assert_eq!(
        response(&app, "GET", path).await.status(),
        StatusCode::NOT_FOUND
    );
    let last = json_body(response(&app, "GET", &next(&second)).await).await;
    assert_eq!(last["features"][0]["id"], "last");
    assert_eq!(last["features"].as_array().unwrap().len(), 1);
    // Repeating an earlier cursor remains deterministic until absolute expiry.
    assert_eq!(
        json_body(response(&app, "GET", &second_path).await).await,
        second
    );
    let config = s.config.clone();
    s.shutdown().await.unwrap();
    assert_eq!(s.catalog.stats().pinned_versions, 0);
    drop((app, s));
    let s = ServerState::open(config).await.unwrap();
    assert_eq!(
        response(&router(s.clone()), "GET", &second_path)
            .await
            .status(),
        StatusCode::GONE
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn feature_cursor_capacity_expiry_and_projected_crs_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let mut config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
    config.backend = BackendSelection::Cpu;
    config.profile.response_memory_bytes = 8 << 20;
    config.profile.response_disk_bytes = 64 << 20;
    config.data_directory = root.path().join("data");
    config.profile.max_feature_cursors = 1;
    config.profile.feature_cursor_ttl_ms = 600;
    let s = ServerState::open(config).await.unwrap();
    let app = router(s.clone());
    let crs = geello::Crs::new("EPSG:3857", geello::CoordinateSpace::Planar).unwrap();
    let source = geello::readers::read_geojson(br#"{"type":"FeatureCollection","features":[{"type":"Feature","id":1,"properties":{},"geometry":{"type":"Point","coordinates":[111319.49079327357,111325.1428663851]}},{"type":"Feature","id":2,"properties":{},"geometry":null}]}"#.as_slice(),
        &geello::readers::VectorReadOptions { crs_override:Some(crs.clone()),data_version:"projected".into(),limits:s.config.profile.reader_limits(),policy:geello::readers::ReadPolicy::Strict,identity:geello::readers::IdentityPolicy::OriginalOrOrdinal,queryables:vec![] },&s.scheduler.context()).unwrap();
    let store = geello::derived::DerivedVector::build(
        &s.catalog.root().join("derived"),
        &source,
        s.config.profile.store_limits(),
        &s.scheduler.context(),
    )
    .unwrap();
    let key = ResourceKey::new(ResourceKind::Dataset, "test:projected").unwrap();
    s.catalog
        .publish(
            ResourceRecord {
                key: key.clone(),
                ownership: Ownership::External,
                version: "v1".into(),
                published: true,
                source: SourceSpec {
                    path: root.path().join("source.json"),
                    format: "geojson".into(),
                    encoding: None,
                    crs: Some("EPSG:3857".into()),
                    sample_kind: None,
                },
                dependencies: vec![],
                data_version: "projected".into(),
                derived_version: Some(store.derived_version().into()),
                derived_path: Some(
                    s.catalog
                        .root()
                        .join("derived")
                        .join(store.derived_version()),
                ),
                managed_directory: None,
                class: Some("vector".into()),
                effective_crs: Some(crs),
                definition: None,
                type_bindings: vec![],
            },
            &Precondition::Absent,
        )
        .unwrap();
    let path = "/ogc/features/collections/test:projected/items?bbox=0.9,0.9,1.1,1.1&limit=1";
    let first = json_body(response(&app, "GET", path).await).await;
    let coordinates = &first["features"][0]["geometry"]["coordinates"];
    assert!(
        (coordinates[0].as_f64().unwrap() - 1.).abs() < 1e-6,
        "{first}"
    );
    assert!((coordinates[1].as_f64().unwrap() - 1.).abs() < 1e-6);
    let next = first["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["rel"] == "next")
        .unwrap()["href"]
        .as_str()
        .unwrap();
    let next = next
        .strip_prefix("http://127.0.0.1:3000")
        .unwrap()
        .to_owned();
    assert_eq!(
        response(&app, "GET", path).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    assert_eq!(
        json_body(response(&app, "GET", &next).await).await["features"][0]["id"],
        2
    );
    // Access did not renew the original deadline. Maintenance releases the pin
    // even without another request; an in-flight Arc would retain it separately.
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while s.catalog.stats().pinned_versions != 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        response(&app, "GET", &next).await.status(),
        StatusCode::GONE
    );
    assert_eq!(response(&app, "GET", path).await.status(), StatusCode::OK);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn correlation_covers_success_rejections_and_admission_without_reflecting_invalid_values() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (method, uri, expected) in [
        ("GET", "/health/ready", StatusCode::OK),
        ("HEAD", "/api.html", StatusCode::OK),
        (
            "GET",
            "/admin/v1/resources?limit=bad",
            StatusCode::BAD_REQUEST,
        ),
        ("GET", "/missing", StatusCode::NOT_FOUND),
        ("PATCH", "/api", StatusCode::METHOD_NOT_ALLOWED),
    ] {
        let result = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("x-request-id", "client:trace-42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), expected);
        assert_eq!(result.headers()["x-request-id"], "client:trace-42");
    }
    let mut generated = BTreeSet::new();
    for values in [
        vec![],
        vec!["".to_owned()],
        vec!["a b".to_owned()],
        vec!["a,b".to_owned()],
        vec!["\"quoted\"".to_owned()],
        vec!["x".repeat(129)],
        vec!["first".to_owned(), "second".to_owned()],
    ] {
        let mut req = Request::builder().uri("/health/ready");
        for value in &values {
            req = req.header("x-request-id", value);
        }
        let result = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let id = result.headers()["x-request-id"].to_str().unwrap();
        assert!(id.starts_with("req-op-"));
        assert!(id.len() <= 128);
        assert!(!values.iter().any(|value| value == id));
        assert!(generated.insert(id.to_owned()));
    }
    s.shutdown().await.unwrap();
    let result = app
        .oneshot(
            Request::builder()
                .uri("/health/ready")
                .header("x-request-id", "after-shutdown")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(result.headers()["x-request-id"], "after-shutdown");
}

#[tokio::test]
async fn offline_html_and_head_are_available_without_external_assets() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let result = response(&app, "GET", "/api.html").await;
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(result.headers()["content-type"], "text/html; charset=utf-8");
    assert_eq!(result.headers()["x-content-type-options"], "nosniff");
    assert!(
        result.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("default-src 'none'")
    );
    let html = String::from_utf8(
        to_bytes(result.into_body(), 2 << 20)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("href=\"api\""));
    assert!(!html.contains("<script") && !html.contains("<iframe") && !html.contains("https://"));
    let document = json_body(response(&app, "GET", "/api").await).await;
    for (_, methods) in document["paths"].as_object().unwrap() {
        for (_, operation) in methods.as_object().unwrap() {
            assert!(html.contains(&format!(
                "id='{}'",
                operation["operationId"].as_str().unwrap()
            )));
        }
    }
    for uri in ["/api", "/api.html", "/health/ready", "/admin/v1/resources"] {
        let result = response(&app, "HEAD", uri).await;
        assert_eq!(result.status(), StatusCode::OK, "{uri}");
        assert!(
            to_bytes(result.into_body(), 2 << 20)
                .await
                .unwrap()
                .is_empty()
        );
    }
    s.shutdown().await.unwrap();
}
