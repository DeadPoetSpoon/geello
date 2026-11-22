//! M3 administration/publication and M4 timed HTTP rendering contracts.
#![cfg(all(feature = "server", feature = "render-cpu", feature = "read-geojson"))]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use geello::server::*;
use std::{path::Path, sync::Arc};
use tower::ServiceExt;
fn profile() -> ServerProfile {
    ServerProfile {
        max_upload_bytes: 2 << 20,
        max_expanded_bytes: 4 << 20,
        max_archive_files: 8,
        max_member_bytes: 2 << 20,
        max_compression_ratio: 100,
        max_registry_bytes: 2 << 20,
        max_resources: 10000,
        max_operations: 10000,
        max_source_bytes: 4 << 20,
        max_record_bytes: 1 << 16,
        max_features: 10000,
        max_live_bytes: 256 << 20,
        max_work: 1_000_000_000,
        max_commands: 100000,
        max_derived_bytes: 64 << 20,
        max_managed_bytes: 64 << 20,
        block_size: 32,
        shard_features: 32,
        max_image_bytes: 4 << 20,
        max_dimension: 2048,
        object_cache_bytes: 16 << 20,
        response_memory_bytes: 8 << 20,
        response_disk_bytes: 64 << 20,
        active_http: 2,
        active_animation: 1,
        active_background: 2,
        queued_per_class: 8,
        operation_timeout_ms: 60000,
        max_feature_cursors: 128,
        feature_cursor_ttl_ms: 60000,
        max_animation_fps: 60,
        max_websocket_sessions: 64,
        websocket_idle_timeout_ms: 120_000,
        max_list_items: 100,
        metatile_size: 2,
        metatile_buffer: 16,
        wmts_prewarm_size: 3,
    }
}
async fn state(root: &Path) -> Arc<ServerState> {
    ServerState::open(ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        public_base_url: "http://localhost/geello".into(),
        data_directory: root.join("data"),
        external_roots: vec![root.into()],
        backend: BackendSelection::Cpu,
        profile: profile(),
    })
    .await
    .unwrap()
}
async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    etag: Option<&str>,
    content_type: Option<&str>,
    body: impl Into<Body>,
) -> axum::response::Response {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(v) = etag {
        req = req.header(
            if v == "*" {
                "if-none-match"
            } else {
                "if-match"
            },
            v,
        );
    }
    if let Some(v) = content_type {
        req = req.header("content-type", v);
    }
    app.clone()
        .oneshot(req.body(body.into()).unwrap())
        .await
        .unwrap()
}
async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 8 << 20).await.unwrap()).unwrap()
}
async fn wait(s: &Arc<ServerState>, op: serde_json::Value) -> ResourceOperation {
    let id = op["operation_id"].as_str().unwrap();
    for _ in 0..3000 {
        let op = s.operation(id).unwrap();
        if matches!(op.status, OperationStatus::Ready | OperationStatus::Failed) {
            return op;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("operation timed out")
}
async fn upload(
    s: &Arc<ServerState>,
    app: &Router,
    kind: &str,
    id: &str,
    ctype: &str,
    bytes: impl Into<Body>,
) -> ResourceOperation {
    let response = request(
        app,
        "PUT",
        &format!("/admin/v1/resources/{kind}/{id}/content"),
        Some("*"),
        Some(ctype),
        bytes,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    wait(s, json(response).await).await
}
const DATA: &str = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":"a","properties":{"size":10},"geometry":{"type":"Point","coordinates":[0,0]}}]}"#;
const STYLE: &str = "schema_version=1\nid='test:style'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='point'\nkind='point'\nfill='#ff0000'\nsize={value=12,unit='px'}\n";
const MAP: &str = "schema_version=1\nid='test:map'\noutput_crs=['OGC:CRS84','EPSG:3857']\n[[items]]\nid='points'\ndataset='test:dataset'\nstyle='test:style'\n";

#[cfg(feature = "native-proj")]
#[tokio::test]
async fn explicitly_native_map_renders_projected_xy_and_reuses_its_own_cache() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let map = MAP.replacen(
        "schema_version=1",
        "schema_version=1\nprojection_backend='native'",
        1,
    );
    for (kind, id, media, body) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", map.as_str()),
    ] {
        let operation = upload(&s, &app, kind, id, media, body.to_owned()).await;
        assert_eq!(operation.status, OperationStatus::Ready, "{operation:?}");
    }
    let route =
        "/maps/test:map/image?bbox=-111319,-111325,111319,111325&crs=EPSG:3857&width=32&height=32";
    let first = request(&app, "GET", route, None, None, Body::empty()).await;
    assert_eq!(first.status(), StatusCode::OK);
    let revision = first.headers()["x-geello-render-revision"].clone();
    let bytes = to_bytes(first.into_body(), 1 << 20).await.unwrap();
    let pixels = image::load_from_memory(&bytes).unwrap().into_rgba8();
    assert!(pixels.get_pixel(16, 16).0[0] > 200 && pixels.get_pixel(16, 16).0[3] > 200);
    let second = request(&app, "GET", route, None, None, Body::empty()).await;
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(second.headers()["x-geello-render-revision"], revision);
    assert_eq!(to_bytes(second.into_body(), 1 << 20).await.unwrap(), bytes);
    assert_eq!(s.execution_counts().0, 1);
}

#[tokio::test]
async fn liveness_is_separate_from_readiness_and_closes_during_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let state = state(root.path()).await;
    let app = router(state.clone());
    let live = request(&app, "GET", "/health/live", None, None, Body::empty()).await;
    assert_eq!(live.status(), StatusCode::OK);
    assert_eq!(json(live).await["status"], "alive");
    state.begin_shutdown();
    let live = request(&app, "GET", "/health/live", None, None, Body::empty()).await;
    assert_eq!(live.status(), StatusCode::SERVICE_UNAVAILABLE);
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn geometry_animation_moves_offscreen_ids_before_projection_and_rebinds_on_refresh() {
    use geello::derived::DerivedVector;
    use geello::readers::VectorQuery;
    use geello::{OperationContext, OperationLimits};
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let outside = DATA.replace("[0,0]", "[10,0]");
    let animation = "schema_version=1\nid='test:move'\nduration=2.0\n[[geometry_tracks]]\nid='position'\nsource_crs={definition='OGC:CRS84',space='GeographicXy'}\ninterpolation='linear'\ntrajectory={anchor=[10.0,0.0],points=[[10.0,0.0],[0.0,0.0]],timing='distance'}";
    let map = format!(
        "{MAP}\n[[items.geometry_bindings]]\nid='motion'\nanimation='test:move'\ntrack='position'\nfeature_ids=['a']\nmax_features=1\nstart=0.0\nspeed=1.0\nrepeat='once'"
    );
    for (kind, id, media, body) in [
        (
            "dataset",
            "test:dataset",
            "application/geo+json",
            outside.as_str(),
        ),
        ("style", "test:style", "application/toml", STYLE),
        ("animation", "test:move", "application/toml", animation),
        ("map", "test:map", "application/toml", map.as_str()),
    ] {
        let op = upload(&s, &app, kind, id, media, body.to_owned()).await;
        assert_eq!(op.status, OperationStatus::Ready, "{op:?}");
    }
    let dataset_key = ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap();
    let original = s.catalog.get(&dataset_key).unwrap();
    let original_bytes = std::fs::read(&original.source.path).unwrap();
    let uri = "/maps/test:map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32";
    let mut first = None;
    for (time, visible) in [(2., true), (0., false), (2., true)] {
        let response = request(
            &app,
            "GET",
            &format!("{uri}&geello_time={time}"),
            None,
            None,
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-geello-incomplete"], "false");
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let pixels = image::load_from_memory(&bytes).unwrap().into_rgba8();
        assert_eq!(pixels.get_pixel(16, 16).0[3] > 200, visible);
        if visible {
            if let Some(first) = &first {
                assert_eq!(first, &bytes);
            } else {
                first = Some(bytes);
            }
        }
    }
    let response = request(&app,"GET","/maps/test:map/image?bbox=-111319,-111325,111319,111325&crs=EPSG:3857&width=32&height=32&geello_time=2",None,None,Body::empty()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let pixels = image::load_from_memory(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
        .unwrap()
        .into_rgba8();
    assert!(pixels.get_pixel(16, 16).0[3] > 200);
    // Source bytes and derived feature queries retain the original coordinates.
    assert_eq!(
        std::fs::read(&original.source.path).unwrap(),
        original_bytes
    );
    let ctx = OperationContext::new(
        OperationLimits {
            max_live_bytes: 64 << 20,
            max_work: 1_000_000,
            max_commands: 1000,
            max_diagnostics: 8,
        },
        None,
    );
    let store = DerivedVector::open_with_context(
        original.derived_path.as_ref().unwrap(),
        profile().store_limits(),
        &ctx,
    )
    .unwrap();
    let mut cursor = store.query(VectorQuery::default(), &ctx).unwrap();
    let batch = cursor.next_batch(32, 1 << 20, &ctx).unwrap().unwrap();
    assert!(
        matches!(&batch.features[0].geometry,Some(geo_types::Geometry::Point(p)) if p.x()==10. && p.y()==0.)
    );
    drop(batch);
    drop(cursor);
    drop(store);
    // A replacement animation with a different coordinate interpretation fails
    // against the map in the generation being committed.
    let key = ResourceKey::new(ResourceKind::Animation, "test:move").unwrap();
    let old_animation = s.catalog.get(&key).unwrap();
    let response = request(
        &app,
        "PUT",
        "/admin/v1/resources/animation/test:move/content",
        Some(&old_animation.etag()),
        Some("application/toml"),
        animation
            .replace("OGC:CRS84", "EPSG:3857")
            .replace("GeographicXy", "Planar"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Failed
    );
    assert_eq!(s.catalog.get(&key).unwrap().etag(), old_animation.etag());
    // The same map time is reevaluated from the new geometry, with no accumulated
    // motion or clock reset. Missing IDs produce an incomplete response.
    for (body, incomplete, expected_x) in [
        (outside.replace("[10,0]", "[10.5,0]"), false, 24),
        (outside.replace("\"a\"", "\"b\""), true, 16),
        (outside.clone(), false, 16),
    ] {
        let old = s.catalog.get(&dataset_key).unwrap();
        let response = request(
            &app,
            "PUT",
            "/admin/v1/resources/dataset/test:dataset/content",
            Some(&old.etag()),
            Some("application/geo+json"),
            body,
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let operation = wait(&s, json(response).await).await;
        assert_eq!(operation.status, OperationStatus::Ready, "{operation:?}");
        let response = request(
            &app,
            "GET",
            &format!("{uri}&geello_time=2"),
            None,
            None,
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["x-geello-incomplete"],
            if incomplete { "true" } else { "false" }
        );
        let pixels =
            image::load_from_memory(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                .unwrap()
                .into_rgba8();
        assert_eq!(pixels.get_pixel(expected_x, 16).0[3] > 200, !incomplete);
    }
    // Dataset refresh with a new CRS also rolls back, retaining the live render.
    let old = s.catalog.get(&dataset_key).unwrap();
    let req = Request::builder()
        .method("PUT")
        .uri("/admin/v1/resources/dataset/test:dataset/content")
        .header("if-match", old.etag())
        .header("content-type", "application/geo+json")
        .header("x-geello-crs", "EPSG:3857")
        .body(Body::from(outside))
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Failed
    );
    assert_eq!(s.catalog.get(&dataset_key).unwrap().etag(), old.etag());
    // Retain a legacy record without effective_crs, as written by M3. The
    // binding can validate through its existing derived manifest.
    let mut legacy = old.clone();
    legacy.effective_crs = None;
    legacy.version = "legacy-geometry-metadata".into();
    s.catalog
        .publish(legacy, &Precondition::Version(old.version.clone()))
        .unwrap();
    let legacy = s.catalog.get(&dataset_key).unwrap();
    let mut current = legacy.clone();
    current.effective_crs = old.effective_crs.clone();
    current.version = "retained-geometry-metadata".into();
    s.catalog
        .publish(current, &Precondition::Version(legacy.version))
        .unwrap();
    let current = s.catalog.get(&dataset_key).unwrap();
    let derived = current.derived_path.clone().unwrap();
    let config = s.config.clone();
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);
    // Quarantine only this disposable derived directory; authoritative source
    // coordinates suffice to restart and schedule bounded lazy reconstruction.
    let quarantine = root.path().join("geometry-derived-quarantine");
    assert!(
        derived.starts_with(root.path().canonicalize().unwrap())
            && quarantine.starts_with(root.path())
    );
    std::fs::rename(&derived, &quarantine).unwrap();
    let s = ServerState::open(config).await.unwrap();
    let app = router(s.clone());
    let response = request(
        &app,
        "GET",
        &format!("{uri}&geello_time=2.5"),
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let problem = json(response).await;
    let op_id = problem["title"]
        .as_str()
        .unwrap()
        .split("operation ")
        .nth(1)
        .unwrap();
    let operation = wait(&s, serde_json::json!({"operation_id":op_id})).await;
    assert_eq!(operation.status, OperationStatus::Ready, "{operation:?}");
    let rebuilt = s.catalog.get(&dataset_key).unwrap();
    assert_eq!(rebuilt.data_version, current.data_version);
    assert_eq!(rebuilt.effective_crs, current.effective_crs);
    let response = request(
        &app,
        "GET",
        &format!("{uri}&geello_time=2.5"),
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    assert!(
        image::load_from_memory(&bytes)
            .unwrap()
            .into_rgba8()
            .get_pixel(16, 16)
            .0[3]
            > 200
    );
    let mut tiles = vec![];
    for time in [0, 2] {
        let response = request(
            &app,
            "GET",
            &format!("/tiles/test:map/0/0/0?geello_time={time}"),
            None,
            None,
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        tiles.push(to_bytes(response.into_body(), 1 << 20).await.unwrap());
    }
    assert_ne!(tiles[0], tiles[1]);
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn stateless_style_animation_keys_time_and_rejects_broken_dependency_updates() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let animation = "schema_version=1\nid='test:pulse'\nduration=2.0\n[[tracks]]\nid='diameter'\nparameter='size'\nunit='px'\ninterpolation='linear'\nkeyframes=[{time=0.0,value=6.0},{time=2.0,value=24.0}]";
    let map = format!(
        "{MAP}\n[[items.animation_bindings]]\nid='pulse'\nanimation='test:pulse'\ntrack='diameter'\ntarget={{rule='base',symbol='point'}}\nstart=0.0\nspeed=1.0\nrepeat='once'"
    );
    for (kind, id, media, body) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("animation", "test:pulse", "application/toml", animation),
        ("map", "test:map", "application/toml", map.as_str()),
    ] {
        let op = upload(&s, &app, kind, id, media, body.to_owned()).await;
        assert_eq!(op.status, OperationStatus::Ready, "{op:?}");
    }
    let base_uri = "/maps/test:map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32";
    let mut first = None;
    let mut first_etag = None;
    let mut revision = None;
    for (time, expected_diameter) in [(0., 6), (2., 24), (0., 6)] {
        let uri = format!("{base_uri}&geello_time={time}");
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let current = response.headers()["x-geello-render-revision"].clone();
        let etag = response.headers()["etag"].clone();
        if let Some(revision) = &revision {
            assert_eq!(revision, &current);
        } else {
            revision = Some(current);
        }
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let pixels = image::load_from_memory(&bytes).unwrap().into_rgba8();
        let colored = pixels.pixels().filter(|pixel| pixel.0[3] != 0).count();
        if expected_diameter == 6 {
            assert!(colored < 60);
            if let Some(first) = &first {
                assert_eq!(first, &bytes);
            } else {
                first = Some(bytes);
                first_etag = Some(etag);
            }
        } else {
            assert!(colored > 400);
        }
    }
    assert_eq!(s.execution_counts(), (2, 2));
    // Omitted time and negative zero have the same canonical cache identity.
    for uri in [base_uri.to_owned(), format!("{base_uri}&geello_time=-0")] {
        let req = Request::builder()
            .uri(uri)
            .header("if-none-match", first_etag.as_ref().unwrap())
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::NOT_MODIFIED
        );
    }
    assert_eq!(s.execution_counts(), (2, 2));
    for time in ["-1", "NaN", "inf"] {
        let response = request(
            &app,
            "GET",
            &format!("{base_uri}&geello_time={time}"),
            None,
            None,
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    for uri in [
        format!("{base_uri}&geello_time=0&geello_time=1"),
        format!("{base_uri}&GEELLO_TIME=1"),
        "/tiles/test:map/0/0/0?geello_time=0&geello_time=1".to_owned(),
        "/tiles/test:map/0/0/0?unknown=1".to_owned(),
    ] {
        assert_eq!(
            request(&app, "GET", &uri, None, None, Body::empty())
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    // An incompatible replacement cannot invalidate an already published map.
    let style_key = ResourceKey::new(ResourceKind::Style, "test:style").unwrap();
    let previous = s.catalog.get(&style_key).unwrap();
    let line = "schema_version=1\nid='test:style'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='point'\nkind='line'\nwidth={value=2,unit='px'}";
    let response = request(
        &app,
        "PUT",
        "/admin/v1/resources/style/test:style/content",
        Some(&previous.etag()),
        Some("application/toml"),
        line,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Failed
    );
    assert_eq!(s.catalog.get(&style_key).unwrap().version, previous.version);
    let dataset = s
        .catalog
        .get(&ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap())
        .unwrap();
    assert_eq!(std::fs::read_to_string(&dataset.source.path).unwrap(), DATA);
    for time in [0, 2] {
        let response = request(
            &app,
            "GET",
            &format!("/tiles/test:map/0/0/0?geello_time={time}"),
            None,
            None,
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let image = image::load_from_memory(&bytes).unwrap().into_rgba8();
        let colored = image.pixels().filter(|pixel| pixel.0[3] != 0).count();
        if time == 0 {
            assert!(colored < 60);
        } else {
            assert!(colored > 400);
        }
    }
    s.shutdown().await.unwrap();
}

#[cfg(feature = "read-geotiff")]
#[tokio::test]
async fn local_utm_raster_world_previews_and_outside_tiles_return_images() {
    use tiff::{
        encoder::{TiffEncoder, colortype},
        tags::Tag,
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("utm.tif");
    {
        let mut encoder = TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
        let mut image = encoder.new_image::<colortype::Gray8>(8, 8).unwrap();
        image
            .encoder()
            .write_tag(Tag::ModelPixelScaleTag, &[25000f64, 25000., 0.][..])
            .unwrap();
        image
            .encoder()
            .write_tag(
                Tag::ModelTiepointTag,
                &[0f64, 0., 0., 400000., 2800000., 0.][..],
            )
            .unwrap();
        image.write_data(&[200; 64]).unwrap();
    }
    let s = state(root.path()).await;
    let app = router(s.clone());
    let response = request(&app, "PUT", "/admin/v1/resources/dataset/test:utm/source", Some("*"), Some("application/json"), serde_json::json!({"path":path,"format":"geotiff","crs":"EPSG:32618","sample_kind":"continuous"}).to_string()).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let op = wait(&s, json(response).await).await;
    assert_eq!(op.status, OperationStatus::Ready, "{op:?}");
    for method in ["nearest", "bilinear"] {
        let style = format!(
            "schema_version=1\nid='test:{method}'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='image'\nkind='raster'\nmode='gray'\nband=1\nrange={{kind='explicit',min=0,max=255}}\nresampling={{enlarge='{method}',reduce='{method}'}}\n"
        );
        let map = format!(
            "schema_version=1\nid='test:{method}'\noutput_crs=['OGC:CRS84','EPSG:3857']\ntiles={{matrix_sets=['WebMercatorQuad','WorldCRS84Quad'],min_zoom=0,max_zoom=5}}\n[[items]]\nid='raster'\ndataset='test:utm'\nstyle='test:{method}'\n"
        );
        for (kind, body) in [("style", style), ("map", map)] {
            let op = upload(
                &s,
                &app,
                kind,
                &format!("test:{method}"),
                "application/toml",
                body,
            )
            .await;
            assert_eq!(op.status, OperationStatus::Ready, "{op:?}");
        }
        for (uri, has_coverage) in [
            (
                format!(
                    "/ogc/maps/collections/test:{method}/map?bbox=-180,-90,180,90&crs=OGC:CRS84&width=128&height=64"
                ),
                None,
            ),
            (
                format!(
                    "/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=test:{method}&STYLES=&CRS=EPSG:3857&BBOX=-20037508.342789244,-20037508.342789244,20037508.342789244,20037508.342789244&WIDTH=128&HEIGHT=128&FORMAT=image/png&TRANSPARENT=TRUE"
                ),
                None,
            ),
            (
                format!(
                    "/wmts?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile&LAYER=test:{method}&STYLE=default&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX=1&TILEROW=1&TILECOL=1&FORMAT=image/png"
                ),
                Some(false),
            ),
            (
                format!("/ogc/maps/collections/test:{method}/map/tiles/WorldCRS84Quad/0/0/1"),
                Some(false),
            ),
            (
                format!(
                    "/ogc/maps/collections/test:{method}/map?bbox=-77,23,-73,26&crs=OGC:CRS84&width=128&height=96"
                ),
                Some(true),
            ),
        ] {
            let response = request(&app, "GET", &uri, None, None, Body::empty()).await;
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 4 << 20).await.unwrap();
            assert_eq!(
                status,
                StatusCode::OK,
                "{uri}: {}",
                String::from_utf8_lossy(&bytes)
            );
            let image = image::load_from_memory(&bytes).unwrap().into_rgba8();
            if let Some(expected) = has_coverage {
                assert_eq!(
                    image.pixels().any(|pixel| pixel.0[3] > 0),
                    expected,
                    "{uri}"
                );
            }
        }
    }
    s.shutdown().await.unwrap();
}

#[cfg(feature = "read-geotiff")]
#[tokio::test]
async fn raster_animation_changes_only_opacity_and_keeps_native_source_bytes() {
    use tiff::{
        encoder::{TiffEncoder, colortype},
        tags::Tag,
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.tif");
    {
        let mut encoder = TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
        let mut image = encoder.new_image::<colortype::Gray8>(1, 1).unwrap();
        image
            .encoder()
            .write_tag(Tag::ModelPixelScaleTag, &[1f64, 1., 0.][..])
            .unwrap();
        image
            .encoder()
            .write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., 0., 1., 0.][..])
            .unwrap();
        image.write_data(&[255]).unwrap();
    }
    let original = std::fs::read(&path).unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let response = request(&app, "PUT", "/admin/v1/resources/dataset/test:raster/source", Some("*"), Some("application/json"), serde_json::json!({"path":path,"format":"geotiff","crs":"OGC:CRS84","sample_kind":"continuous"}).to_string()).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Ready
    );
    let style = "schema_version=1\nid='test:raster-style'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='image'\nkind='raster'\nmode='gray'\nband=1\nrange={kind='explicit',min=0,max=255}\nresampling={enlarge='nearest',reduce='nearest'}\nopacity=0.25";
    let animation = "schema_version=1\nid='test:fade'\nduration=2.0\n[[tracks]]\nid='alpha'\nparameter='opacity'\ninterpolation='linear'\nkeyframes=[{time=0.0,value=0.0},{time=2.0,value=1.0}]";
    let map = "schema_version=1\nid='test:raster-map'\noutput_crs=['OGC:CRS84']\n[[items]]\nid='raster'\ndataset='test:raster'\nstyle='test:raster-style'\n[[items.animation_bindings]]\nid='fade'\nanimation='test:fade'\ntrack='alpha'\ntarget={rule='base',symbol='image'}\nstart=1.0\nspeed=1.0\nrepeat='once'";
    for (kind, id, body) in [
        ("style", "test:raster-style", style),
        ("animation", "test:fade", animation),
        ("map", "test:raster-map", map),
    ] {
        let op = upload(&s, &app, kind, id, "application/toml", body).await;
        assert_eq!(op.status, OperationStatus::Ready, "{op:?}");
    }
    for (time, expected) in [
        (2, [255, 255, 255, 128]),
        (1, [0, 0, 0, 0]),
        (0, [255, 255, 255, 64]),
        (3, [255, 255, 255, 255]),
        (2, [255, 255, 255, 128]),
    ] {
        let response = request(&app, "GET", &format!("/maps/test:raster-map/image?bbox=0,0,1,1&crs=OGC:CRS84&width=1&height=1&geello_time={time}"), None, None, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let image = image::load_from_memory(&bytes).unwrap().into_rgba8();
        assert_eq!(image.get_pixel(0, 0).0, expected);
    }
    assert_eq!(std::fs::read(&path).unwrap(), original);
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn native_cache_does_not_retain_failed_or_cancelled_loads() {
    use geello::{GeelloError, derived::RasterBlockCache};
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let mut calls = 0;
    for _ in 0..2 {
        let mut load = || {
            calls += 1;
            Err(GeelloError::InvalidSource("damaged native block".into()))
        };
        assert!(s.get_or_load("native:failure".into(), &mut load).is_err());
    }
    assert_eq!(calls, 2);
    let mut cancelled = || Err(GeelloError::Cancelled);
    assert!(matches!(
        s.get_or_load("native:cancelled".into(), &mut cancelled),
        Err(GeelloError::Cancelled)
    ));
    let mut replacement = || {
        calls += 1;
        Err(GeelloError::InvalidSource("fresh loader".into()))
    };
    assert!(matches!(
        s.get_or_load("native:cancelled".into(), &mut replacement),
        Err(GeelloError::InvalidSource(_))
    ));
    assert_eq!(calls, 3);
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn slow_registration_bodies_have_bounded_admission_and_release_on_disconnect() {
    let root = tempfile::tempdir().unwrap();
    let mut limits = profile();
    limits.max_registry_bytes = 64 << 10;
    let s = ServerState::open(ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        public_base_url: "http://localhost/geello".into(),
        data_directory: root.path().join("data"),
        external_roots: vec![],
        backend: BackendSelection::Cpu,
        profile: limits,
    })
    .await
    .unwrap();
    let app = router(s.clone());
    let mut pending = vec![];
    let baseline = s.scheduler.live_bytes();
    let capacity = s.config.profile.active_http + s.config.profile.queued_per_class;
    for index in 0..capacity {
        let app = app.clone();
        pending.push(tokio::spawn(async move {
            let body = Body::from_stream(futures_util::stream::pending::<
                std::result::Result<Vec<u8>, std::io::Error>,
            >());
            request(
                &app,
                "PUT",
                &format!("/admin/v1/resources/dataset/test:slow-{index}/source"),
                Some("*"),
                Some("application/json"),
                body,
            )
            .await
        }));
    }
    // Each suspended registration owns the known registration-body envelope.
    // Wait for all of them rather than depending on a wall-clock sleep.
    let expected =
        baseline + capacity as u64 * (s.config.profile.max_registry_bytes as u64 * 32 + 1024);
    for _ in 0..1000 {
        if s.scheduler.live_bytes() >= expected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(s.scheduler.live_bytes() >= expected);
    let rejected = request(&app, "GET", "/health/ready", None, None, Body::empty()).await;
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        json(rejected).await["type"],
        "urn:geello:problem:request-capacity"
    );
    for task in pending {
        task.abort();
        let _ = task.await;
    }
    assert_eq!(
        request(&app, "GET", "/health/ready", None, None, Body::empty())
            .await
            .status(),
        StatusCode::OK
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn encoded_library_results_retain_admission_across_warm_cache_hits() {
    use geello::{AspectPolicy, Bounds, CoordinateSpace, Crs, View};
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (kind, id, ctype, data) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        assert_eq!(
            upload(&s, &app, kind, id, ctype, data).await.status,
            OperationStatus::Ready
        );
    }
    let view = View {
        crs: Crs::new("OGC:CRS84", CoordinateSpace::GeographicXy).unwrap(),
        bounds: Bounds {
            min_x: -1.,
            min_y: -1.,
            max_x: 1.,
            max_y: 1.,
        },
        width: 32,
        height: 32,
        aspect: AspectPolicy::Stretch,
    };
    let first = s.render_map("test:map", view.clone()).await.unwrap();
    let expected_etag = first.etag.clone();
    let encoded_bytes = first.bytes.len() as u64;
    let counts = s.execution_counts();
    drop(first);
    tokio::task::yield_now().await;
    let baseline = s.scheduler.live_bytes();
    let warm = s.render_map("test:map", view).await.unwrap();
    assert_eq!(warm.etag, expected_etag);
    assert_eq!(s.execution_counts(), counts);
    assert!(s.scheduler.live_bytes() >= baseline + encoded_bytes);
    let retained = warm.clone();
    let held = s.scheduler.live_bytes();
    drop(warm);
    assert_eq!(s.scheduler.live_bytes(), held);
    drop(retained);
    assert_eq!(s.scheduler.live_bytes(), baseline);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn disk_response_cache_survives_restart_and_revalidates_current_publication() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (kind, id, ctype, data) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        assert_eq!(
            upload(&s, &app, kind, id, ctype, data).await.status,
            OperationStatus::Ready
        );
    }
    let uri = "/maps/test:map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32";
    let first = request(&app, "GET", uri, None, None, Body::empty()).await;
    assert_eq!(first.status(), StatusCode::OK);
    let headers = first.headers().clone();
    let bytes = to_bytes(first.into_body(), 8 << 20).await.unwrap();
    assert_eq!(s.execution_counts(), (1, 1));
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);

    let s = state(root.path()).await;
    let app = router(s.clone());
    let recovered = request(&app, "GET", uri, None, None, Body::empty()).await;
    assert_eq!(recovered.status(), StatusCode::OK);
    for name in ["etag", "x-geello-render-revision", "x-geello-backend"] {
        assert_eq!(recovered.headers()[name], headers[name]);
    }
    assert_eq!(
        to_bytes(recovered.into_body(), 8 << 20).await.unwrap(),
        bytes
    );
    assert_eq!(s.execution_counts(), (0, 0));
    assert_eq!(s.cache_failures(), 0);

    let conditional = Request::builder()
        .uri(uri)
        .header("if-none-match", &headers["etag"])
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(conditional).await.unwrap().status(),
        StatusCode::NOT_MODIFIED
    );
    let key = ResourceKey::new(ResourceKind::Map, "test:map").unwrap();
    let record = s.catalog.get(&key).unwrap();
    s.catalog
        .unpublish(&key, &Precondition::Version(record.version))
        .unwrap();
    assert_eq!(
        request(&app, "GET", uri, None, None, Body::empty())
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(s.execution_counts(), (0, 0));
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn incomplete_scene_summary_survives_area_reuse_and_disk_restart() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let mixed = DATA.replace(
        r#"{"type":"Point","coordinates":[0,0]}"#,
        r#"{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[0,0]},{"type":"LineString","coordinates":[[0,0],[1,1]]}]}"#,
    );
    assert_ne!(mixed, DATA);
    for (kind, id, ctype, data) in [
        (
            "dataset",
            "test:dataset",
            "application/geo+json",
            mixed.as_str(),
        ),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        assert_eq!(
            upload(&s, &app, kind, id, ctype, data.to_owned())
                .await
                .status,
            OperationStatus::Ready
        );
    }
    for uri in ["/tiles/test:map/1/0/0", "/tiles/test:map/1/1/0"] {
        let response = request(&app, "GET", uri, None, None, Body::empty()).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-geello-incomplete"], "true");
        assert!(
            !to_bytes(response.into_body(), 8 << 20)
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(s.execution_counts(), (1, 2));
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);
    let s = state(root.path()).await;
    let app = router(s.clone());
    let response = request(
        &app,
        "GET",
        "/tiles/test:map/1/0/0",
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-geello-incomplete"], "true");
    drop(response);
    assert_eq!(s.execution_counts(), (0, 0));
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_bbox_is_rejected_before_resource_or_render_work() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for bbox in ["", "0,0,1", "0,0,1,1,2", "NaN,0,1,1", "0,0,inf,1", "0,,1,1"] {
        let response = request(
            &app,
            "GET",
            &format!("/maps/test:missing/image?bbox={bbox}&crs=OGC:CRS84&width=32&height=32"),
            None,
            None,
            Body::empty(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{bbox}");
    }
    assert_eq!(s.execution_counts(), (0, 0));
    assert_eq!(s.catalog.stats().pinned_versions, 0);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn upload_render_revalidate_reference_protection_and_restart() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (kind, id, ctype, data) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        let op = upload(&s, &app, kind, id, ctype, data).await;
        assert_eq!(op.status, OperationStatus::Ready, "{:?}", op);
    }
    let image = request(
        &app,
        "GET",
        "/maps/test:map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32",
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(image.status(), StatusCode::OK);
    let etag = image.headers()["etag"].clone();
    assert_eq!(image.headers()["x-geello-backend"], "cpu");
    let bytes = to_bytes(image.into_body(), 8 << 20).await.unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let decoded = image::load_from_memory(&bytes).unwrap().into_rgba8();
    assert!(decoded.get_pixel(16, 16).0[0] > 200);
    let req = Request::builder()
        .uri("/maps/test:map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32")
        .header("if-none-match", etag)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::NOT_MODIFIED
    );
    let dataset = s
        .catalog
        .get(&ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap())
        .unwrap();
    let content = dataset.source.path.clone();
    let conflict = request(
        &app,
        "DELETE",
        "/admin/v1/resources/dataset/test:dataset/content",
        Some(&dataset.etag()),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    let pin = s
        .catalog
        .pin(&ResourceKey::new(ResourceKind::Map, "test:map").unwrap())
        .unwrap();
    let map = s
        .catalog
        .get(&ResourceKey::new(ResourceKind::Map, "test:map").unwrap())
        .unwrap();
    let unpub = request(
        &app,
        "POST",
        "/admin/v1/resources/map/test:map:unpublish",
        Some(&map.etag()),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(unpub.status(), StatusCode::OK);
    let map = s.catalog.get(&map.key).unwrap();
    assert!(
        s.catalog
            .remove(&map.key, &Precondition::Version(map.version.clone()), false)
            .is_err()
    );
    drop(pin);
    let remove = request(
        &app,
        "DELETE",
        "/admin/v1/resources/map/test:map",
        Some(&map.etag()),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(remove.status(), StatusCode::NO_CONTENT);
    let remove = request(
        &app,
        "DELETE",
        "/admin/v1/resources/dataset/test:dataset",
        Some(&dataset.etag()),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(remove.status(), StatusCode::NO_CONTENT);
    assert!(content.exists());
    assert_eq!(s.catalog.stats().detached_content, 2);
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);
    let reopened = state(root.path()).await;
    assert_eq!(reopened.catalog.stats().resources, 1);
    assert!(content.exists());
    assert_eq!(reopened.catalog.stats().detached_content, 2);
    reopened.shutdown().await.unwrap();
}
#[tokio::test]
async fn external_ownership_failed_replacement_and_etag_conflicts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("external.geojson");
    std::fs::write(&path, DATA).unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let body = serde_json::to_vec(&SourceSpec {
        path: path.clone(),
        format: "geojson".into(),
        encoding: None,
        crs: None,
        sample_kind: None,
    })
    .unwrap();
    let response = request(
        &app,
        "PUT",
        "/admin/v1/resources/dataset/test:external/source",
        Some("*"),
        Some("application/json"),
        body,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Ready
    );
    let key = ResourceKey::new(ResourceKind::Dataset, "test:external").unwrap();
    let original = s.catalog.get(&key).unwrap();
    let response = request(
        &app,
        "DELETE",
        "/admin/v1/resources/dataset/test:external/content",
        Some(&original.etag()),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(path.exists());
    let response = request(
        &app,
        "PUT",
        "/admin/v1/resources/dataset/test:external/content",
        Some(&original.etag()),
        Some("application/geo+json"),
        "broken JSON",
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Failed
    );
    assert_eq!(s.catalog.get(&key).unwrap().version, original.version);
    let response = request(
        &app,
        "DELETE",
        "/admin/v1/resources/dataset/test:external",
        Some("\"stale\""),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(s.catalog.get(&key).unwrap().version, original.version);
    let response = request(
        &app,
        "DELETE",
        "/admin/v1/resources/dataset/test:external",
        Some(&original.etag()),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(path.exists());
    s.shutdown().await.unwrap();
}
fn zip_bytes(names: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
    for (name, bytes) in names {
        w.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}
use std::io::Write;
#[tokio::test]
async fn malicious_archive_is_rejected_without_publication() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let bytes = zip_bytes(&[
        ("../escape.shp", b"bad"),
        ("roads.shx", b"bad"),
        ("roads.dbf", b"bad"),
    ]);
    let op = upload(&s, &app, "dataset", "test:badzip", "application/zip", bytes).await;
    assert_eq!(op.status, OperationStatus::Failed);
    assert_eq!(s.catalog.stats().resources, 0);
    assert!(!root.path().join("escape.shp").exists());
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn archive_aliases_links_nested_archives_and_expansion_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (n, names) in [
        vec![
            ("C:roads.shp", b"x".as_slice()),
            ("roads.shx", b"x"),
            ("roads.dbf", b"x"),
        ],
        vec![
            ("folder\\roads.shp", b"x".as_slice()),
            ("roads.shx", b"x"),
            ("roads.dbf", b"x"),
        ],
        vec![
            ("roads.shp", b"x".as_slice()),
            ("ROADS.SHP", b"x"),
            ("roads.shx", b"x"),
            ("roads.dbf", b"x"),
        ],
        vec![
            ("nested.zip", b"x".as_slice()),
            ("roads.shp", b"x"),
            ("roads.shx", b"x"),
            ("roads.dbf", b"x"),
        ],
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            upload(
                &s,
                &app,
                "dataset",
                &format!("bad:zip-{n}"),
                "application/zip",
                zip_bytes(&names)
            )
            .await
            .status,
            OperationStatus::Failed
        );
    }
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
    writer
        .add_symlink(
            "roads.shp",
            "outside",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    let linked = writer.finish().unwrap().into_inner();
    assert_eq!(
        upload(&s, &app, "dataset", "bad:linked", "application/zip", linked)
            .await
            .status,
        OperationStatus::Failed
    );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
    writer
        .start_file(
            "roads.shp",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(&vec![0; 1 << 20]).unwrap();
    let bomb = writer.finish().unwrap().into_inner();
    assert_eq!(
        upload(&s, &app, "dataset", "bad:ratio", "application/zip", bomb)
            .await
            .status,
        OperationStatus::Failed
    );
    assert_eq!(s.catalog.stats().resources, 0);
    s.shutdown().await.unwrap();
}
#[cfg(feature = "read-shapefile")]
#[tokio::test]
async fn shapefile_zip_upload_and_explicit_managed_content_deletion() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("roads.shp");
    let mut writer = shapefile::Writer::from_path(
        &path,
        shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("name".try_into().unwrap(), 20),
    )
    .unwrap();
    let mut record = shapefile::dbase::Record::default();
    record.insert(
        "name".into(),
        shapefile::dbase::FieldValue::Character(Some("station".into())),
    );
    writer
        .write_shape_and_record(&shapefile::Point::new(0., 0.), &record)
        .unwrap();
    drop(writer);
    std::fs::write(path.with_extension("prj"),"GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",SPHEROID[\"WGS 84\",6378137,298.257223563]],PRIMEM[\"Greenwich\",0],UNIT[\"degree\",0.0174532925199433]]").unwrap();
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
    for ext in ["shp", "shx", "dbf", "prj"] {
        w.start_file(
            format!("roads.{ext}"),
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        w.write_all(&std::fs::read(path.with_extension(ext)).unwrap())
            .unwrap();
    }
    let bytes = w.finish().unwrap().into_inner();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let op = upload(&s, &app, "dataset", "test:shape", "application/zip", bytes).await;
    assert_eq!(op.status, OperationStatus::Ready, "{op:?}");
    let key = ResourceKey::new(ResourceKind::Dataset, "test:shape").unwrap();
    let resource = s.catalog.get(&key).unwrap();
    let content = resource.managed_directory.clone().unwrap();
    assert!(content.exists());
    let response = request(
        &app,
        "DELETE",
        "/admin/v1/resources/dataset/test:shape/content",
        Some(&resource.etag()),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!content.exists());
    assert!(path.exists());
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn all_render_waiters_disconnect_without_leaking_pins_or_poisoning_retry() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (kind, id, ctype, body) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        assert_eq!(
            upload(&s, &app, kind, id, ctype, body).await.status,
            OperationStatus::Ready
        );
    }
    let baseline = s.scheduler.live_bytes();
    let mut blockers = vec![];
    for _ in 0..s.config.profile.active_http {
        let scheduler = s.scheduler.clone();
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, wait_release) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            scheduler
                .blocking(WorkClass::Http, scheduler.context(), move |_| {
                    entered.send(()).unwrap();
                    wait_release.recv().unwrap();
                    Ok(())
                })
                .await
        });
        started.await.unwrap();
        blockers.push((release, task));
    }
    let rendering = s.clone();
    let task = tokio::spawn(async move { rendering.render_tile("test:map", 0, 0, 0).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while s.catalog.stats().pinned_versions == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.err().unwrap().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while s.catalog.stats().pinned_versions != 0 || s.scheduler.live_bytes() != baseline {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(s.execution_counts(), (0, 0));
    for (release, task) in blockers {
        release.send(()).unwrap();
        task.await.unwrap().unwrap();
    }
    let image = s.render_tile("test:map", 0, 0, 0).await.unwrap();
    assert_eq!(&image.bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(s.execution_counts(), (1, 1));
    drop(image);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn concurrent_tiles_share_area_render_and_encode_only_requested_tiles() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (kind, id, ctype, data) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        assert_eq!(
            upload(&s, &app, kind, id, ctype, data).await.status,
            OperationStatus::Ready
        );
    }
    let (a, b, c) = tokio::join!(
        s.render_tile("test:map", 2, 0, 0),
        s.render_tile("test:map", 2, 1, 0),
        s.render_tile("test:map", 2, 0, 0)
    );
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    assert_eq!(s.execution_counts(), (1, 2));
    let response = s.render_tile("test:map", 2, 0, 0).await.unwrap();
    assert_eq!(response.bytes, a.unwrap().bytes);
    assert_eq!(s.execution_counts(), (1, 2));
    let key = ResourceKey::new(ResourceKind::Map, "test:map").unwrap();
    let r = s.catalog.get(&key).unwrap();
    s.catalog
        .unpublish(&key, &Precondition::Version(r.version))
        .unwrap();
    assert!(s.render_tile("test:map", 2, 0, 0).await.is_err());
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn wmts_tile_warms_its_centered_window_and_cached_neighbor_extends_it() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (kind, id, ctype, data) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        assert_eq!(
            upload(&s, &app, kind, id, ctype, data).await.status,
            OperationStatus::Ready
        );
    }
    // The centered 3x3 test window clips to 2x2 at the world corner.
    let first = request(
        &app, "GET",
        "/wmts?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile&LAYER=test:map&STYLE=default&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX=2&TILEROW=0&TILECOL=0&FORMAT=image/png", None, None, Body::empty(),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_bytes = to_bytes(first.into_body(), 8 << 20).await.unwrap();
    assert_eq!(&first_bytes[..8], b"\x89PNG\r\n\x1a\n");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let stats = s.response_cache_stats();
            if stats["prewarm_completed"] == 3 && stats["prewarm_groups_active"] == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        s.execution_counts().0,
        1,
        "neighbors must reuse the metatile render"
    );
    assert_eq!(
        s.execution_counts().1,
        4,
        "all 2x2 tiles should be encoded once"
    );
    let hits_before = s.response_cache_stats()["hits"].as_u64().unwrap();
    let neighbor = request(
        &app, "GET",
        "/wmts?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile&LAYER=test:map&STYLE=default&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX=2&TILEROW=0&TILECOL=1&FORMAT=image/png", None, None, Body::empty(),
    ).await;
    assert_eq!(neighbor.status(), StatusCode::OK);
    drop(to_bytes(neighbor.into_body(), 8 << 20).await.unwrap());
    assert!(s.response_cache_stats()["hits"].as_u64().unwrap() > hits_before);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if s.response_cache_stats()["prewarm_groups_active"] == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        s.execution_counts(),
        (2, 6),
        "cached B extends the window with only two missing edge tiles"
    );
    s.shutdown().await.unwrap();
}
#[test]
fn retired_versions_are_pinned_and_record_only_content_is_retained() {
    let root = tempfile::tempdir().unwrap();
    let c = Catalog::open(root.path(), profile()).unwrap();
    let mut r = record("test:style");
    let old = root.path().canonicalize().unwrap().join("managed/old");
    std::fs::create_dir(&old).unwrap();
    r.ownership = Ownership::Managed;
    r.managed_directory = Some(old.clone());
    let key = r.key.clone();
    c.publish(r.clone(), &Precondition::Absent).unwrap();
    let pin = c.pin(&key).unwrap();
    let new = root.path().canonicalize().unwrap().join("managed/new");
    std::fs::create_dir(&new).unwrap();
    r.version = "replacement".into();
    r.managed_directory = Some(new.clone());
    c.publish(
        r.clone(),
        &Precondition::Version(format!("{}-v1", r.key.id)),
    )
    .unwrap();
    c.collect_garbage().unwrap();
    assert!(old.exists());
    drop(pin);
    c.collect_garbage().unwrap();
    assert!(!old.exists());
    c.remove(&key, &Precondition::Version(r.version), false)
        .unwrap();
    c.collect_garbage().unwrap();
    assert!(new.exists());
    assert_eq!(c.stats().detached_content, 1);
}
#[test]
#[ignore = "Opt-in catalog scale experiment; records results, not a formal capacity target"]
fn thousand_resource_catalog_scale_and_restart() {
    let root = tempfile::tempdir().unwrap();
    let start = std::time::Instant::now();
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    for n in 0..1000 {
        catalog
            .publish(record(&format!("scale:style-{n}")), &Precondition::Absent)
            .unwrap();
    }
    let elapsed = start.elapsed();
    assert_eq!(catalog.stats().resources, 1000);
    catalog.rebuild_index().unwrap();
    assert!(catalog.compact_index().unwrap() > 0);
    drop(catalog);
    let restart = std::time::Instant::now();
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    assert_eq!(catalog.stats().resources, 1000);
    eprintln!(
        "1000 resource writes={elapsed:?}; indexed restart={:?}",
        restart.elapsed()
    );
}

#[test]
#[ignore = "Opt-in multi-thousand catalog contention and recovery measurement"]
fn multi_thousand_resource_catalog_contention_and_restart() {
    let root = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    let start = std::time::Instant::now();
    let mut joins = vec![];
    for writer in 0..4 {
        let c = catalog.clone();
        joins.push(std::thread::spawn(move || {
            for n in 0..750 {
                let r = record(&format!("scale:writer-{writer}-{n}"));
                c.publish(r.clone(), &Precondition::Absent).unwrap();
                assert_eq!(c.get(&r.key).unwrap().version, r.version);
                if n == 749 {
                    eprintln!("writer {writer} completed");
                }
            }
        }));
    }
    for join in joins {
        join.join().unwrap();
    }
    let writes = start.elapsed();
    assert_eq!(catalog.stats().resources, 3000);
    // An interrupted writer can leave unpublished COW members behind. They
    // must never replace the authoritative pointer during restart.
    std::fs::write(
        catalog.root().join("registry/generations/incomplete.json"),
        b"{",
    )
    .unwrap();
    std::fs::create_dir(catalog.root().join("managed/unregistered")).unwrap();
    let generation = catalog.stats().generation;
    drop(catalog);
    let start = std::time::Instant::now();
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    let restart = start.elapsed();
    assert_eq!(catalog.stats().generation, generation);
    assert_eq!(catalog.stats().resources, 3000);
    assert_eq!(catalog.reconcile().unwrap(), vec!["unregistered"]);
    catalog.rebuild_index().unwrap();
    assert!(catalog.compact_index().unwrap() > 0);
    drop(catalog);
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    assert_eq!(catalog.stats().resources, 3000);
    for writer in 0..4 {
        for n in [0, 374, 749] {
            assert!(
                catalog
                    .get(
                        &ResourceKey::new(
                            ResourceKind::Style,
                            format!("scale:writer-{writer}-{n}")
                        )
                        .unwrap()
                    )
                    .is_ok()
            );
        }
    }
    eprintln!("3000 resources / 4 writers: writes+lookups={writes:?}; indexed restart={restart:?}");
}
fn record(id: &str) -> ResourceRecord {
    let key = ResourceKey::new(ResourceKind::Style, id).unwrap();
    ResourceRecord {
        key,
        ownership: Ownership::External,
        version: format!("{id}-v1"),
        published: true,
        source: SourceSpec {
            path: "unused".into(),
            format: "toml".into(),
            encoding: None,
            crs: None,
            sample_kind: None,
        },
        dependencies: vec![],
        data_version: "data-v1".into(),
        derived_version: None,
        derived_path: None,
        managed_directory: None,
        class: Some("vector".into()),
        effective_crs: None,
        definition: Some(String::new()),
        type_bindings: vec![],
    }
}
#[test]
fn catalog_aggregate_budget_preserves_the_last_complete_generation() {
    let root = tempfile::tempdir().unwrap();
    let mut limits = profile();
    limits.max_live_bytes = 2 << 20;
    let catalog = Catalog::open(root.path(), limits.clone()).unwrap();
    let mut accepted = 0;
    for index in 0..32 {
        let mut resource = record(&format!("budget:style-{index}"));
        resource.definition = Some(format!(
            "schema_version=1\nid='{}'\nrules=[]\n#{}",
            resource.key.id,
            "x".repeat(8192)
        ));
        let generation = catalog.stats().generation;
        let pointer = std::fs::read(root.path().join("registry/catalog.toml")).ok();
        match catalog.publish(resource, &Precondition::Absent) {
            Ok(_) => accepted += 1,
            Err(error) => {
                assert_eq!(error.status, 503);
                assert!(error.kind.ends_with(":budget"));
                assert_eq!(catalog.stats().generation, generation);
                assert_eq!(
                    std::fs::read(root.path().join("registry/catalog.toml")).ok(),
                    pointer
                );
                break;
            }
        }
    }
    assert!(accepted > 0 && accepted < 32);
    drop(catalog);
    let reopened = Catalog::open(root.path(), limits.clone()).unwrap();
    assert_eq!(reopened.stats().resources, accepted);
    drop(reopened);
    limits.max_live_bytes = 8192;
    let error = Catalog::open(root.path(), limits).err().unwrap();
    assert_eq!(error.status, 503);
}

#[tokio::test]
async fn catalog_pins_and_operation_retention_share_the_server_memory_budget() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let initial = s.scheduler.live_bytes();
    let op = upload(
        &s,
        &app,
        "dataset",
        "test:dataset",
        "application/geo+json",
        DATA,
    )
    .await;
    assert_eq!(op.status, OperationStatus::Ready);
    s.shutdown().await.unwrap();
    let retained = s.scheduler.live_bytes();
    assert!(retained > initial);
    let key = ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap();
    let pin = s.catalog.pin(&key).unwrap();
    assert!(s.scheduler.live_bytes() > retained);
    drop(pin);
    assert_eq!(s.scheduler.live_bytes(), retained);
    assert_eq!(
        s.prune_operations(
            std::time::SystemTime::now() + std::time::Duration::from_secs(1),
            1
        )
        .unwrap(),
        1
    );
    assert!(s.scheduler.live_bytes() < retained);
    assert!(s.catalog.get(&key).is_ok());
    let scheduler = s.scheduler.clone();
    drop(app);
    drop(s);
    assert_eq!(scheduler.live_bytes(), 0);
}

#[tokio::test]
async fn response_bodies_retain_admission_until_consumed_or_dropped() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    assert_eq!(
        upload(
            &s,
            &app,
            "dataset",
            "test:dataset",
            "application/geo+json",
            DATA
        )
        .await
        .status,
        OperationStatus::Ready
    );
    let baseline = s.scheduler.live_bytes();
    let response = request(
        &app,
        "GET",
        "/admin/v1/resources/dataset/test:dataset",
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(s.scheduler.live_bytes() > baseline);
    drop(response);
    assert_eq!(s.scheduler.live_bytes(), baseline);
    let response = request(
        &app,
        "GET",
        "/admin/v1/resources?limit=1",
        None,
        None,
        Body::empty(),
    )
    .await;
    assert!(s.scheduler.live_bytes() > baseline);
    let body = json(response).await;
    assert_eq!(body["resources"].as_array().unwrap().len(), 1);
    assert_eq!(body["generation"], s.catalog.stats().generation);
    assert_eq!(s.scheduler.live_bytes(), baseline);
    let key = ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap();
    let record = s.catalog.get(&key).unwrap();
    let response = request(
        &app,
        "POST",
        "/admin/v1/resources/dataset/test:dataset:unpublish",
        Some(&record.etag()),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let retained = s.scheduler.live_bytes();
    let body = json(response).await;
    assert_eq!(body["published"], false);
    assert!(s.scheduler.live_bytes() < retained);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn operation_ledger_startup_enforces_aggregate_memory_admission() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let mut config = s.config.clone();
    let startup_bytes = s.scheduler.live_bytes();
    let ledger = s.catalog.root().join("operations");
    s.shutdown().await.unwrap();
    drop(s);
    config.profile.max_live_bytes = startup_bytes + (2 << 20);
    let empty = ServerState::open(config.clone()).await.unwrap();
    empty.shutdown().await.unwrap();
    drop(empty);
    let mut originals = vec![];
    for index in 0..12 {
        let op = ResourceOperation {
            operation_id: format!("op-budget-{index}"),
            resource: ResourceKey::new(ResourceKind::Dataset, format!("budget:data-{index}"))
                .unwrap(),
            status: OperationStatus::Failed,
            version: None,
            diagnostics: vec!["x".repeat(8192)],
        };
        let bytes = serde_json::to_vec(&op).unwrap();
        let path = ledger.join(format!("{}.json", op.operation_id));
        std::fs::write(&path, &bytes).unwrap();
        originals.push((path, bytes));
    }
    let error = ServerState::open(config.clone()).await.err().unwrap();
    assert_eq!(error.status, 503);
    assert!(error.kind.ends_with(":budget"));
    for (path, bytes) in originals {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    config.profile.max_live_bytes = profile().max_live_bytes;
    let restarted = ServerState::open(config).await.unwrap();
    assert_eq!(
        restarted.operation("op-budget-11").unwrap().status,
        OperationStatus::Failed
    );
    restarted.shutdown().await.unwrap();
}

#[test]
fn concurrent_catalog_shards_preserve_both_commits_and_ignore_orphans() {
    let root = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    let mut joins = vec![];
    for id in ["one:style", "two:style"] {
        let c = catalog.clone();
        joins.push(std::thread::spawn(move || {
            c.publish(record(id), &Precondition::Absent).unwrap()
        }));
    }
    for j in joins {
        j.join().unwrap();
    }
    assert_eq!(catalog.stats().resources, 2);
    std::fs::create_dir(root.path().join("managed/orphan")).unwrap();
    assert_eq!(catalog.reconcile().unwrap(), vec!["orphan"]);
    let generation = catalog.stats().generation;
    drop(catalog);
    let reopened = Catalog::open(root.path(), profile()).unwrap();
    assert_eq!(reopened.stats().generation, generation);
    assert_eq!(reopened.stats().resources, 2);
}

#[test]
fn index_rebuild_repairs_damaged_content_addressed_shards() {
    let root = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    catalog
        .publish(record("repair:style"), &Precondition::Absent)
        .unwrap();
    let shard = std::fs::read_dir(root.path().join("registry/shards"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&shard, b"damaged").unwrap();
    catalog.rebuild_index().unwrap();
    drop(catalog);
    let reopened = Catalog::open(root.path(), profile()).unwrap();
    assert!(
        reopened
            .get(&ResourceKey::new(ResourceKind::Style, "repair:style").unwrap())
            .is_ok()
    );
}

#[cfg(windows)]
#[test]
fn pointer_replacement_failure_preserves_memory_and_restart_generation() {
    let root = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    catalog
        .publish(record("fault:old"), &Precondition::Absent)
        .unwrap();
    let pointer = catalog.root().join("registry/catalog.toml");
    let bytes = std::fs::read(&pointer).unwrap();
    let generation = catalog.stats().generation;
    // Windows refuses replacing a read-only destination even though the COW
    // shard and generation files can be written. Restore permissions before
    // asserting so the temporary directory can always be reclaimed.
    let permissions = std::fs::metadata(&pointer).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&pointer, readonly).unwrap();
    let outcome = catalog.publish(record("fault:new"), &Precondition::Absent);
    std::fs::set_permissions(&pointer, permissions).unwrap();
    assert!(outcome.is_err());
    assert_eq!(catalog.stats().resources, 1);
    assert_eq!(catalog.stats().generation, generation);
    assert_eq!(std::fs::read(&pointer).unwrap(), bytes);
    drop(catalog);
    let catalog = Catalog::open(root.path(), profile()).unwrap();
    assert_eq!(catalog.stats().generation, generation);
    assert!(
        catalog
            .get(&ResourceKey::new(ResourceKind::Style, "fault:new").unwrap())
            .is_err()
    );
    catalog
        .publish(record("fault:new"), &Precondition::Absent)
        .unwrap();
    assert_eq!(catalog.stats().resources, 2);
}

#[tokio::test]
async fn malformed_derived_orphans_do_not_block_unrelated_uploads() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let orphan = s.catalog.root().join("derived").join("a".repeat(64));
    std::fs::create_dir(&orphan).unwrap();
    let malformed = s.catalog.root().join("derived").join("b".repeat(64));
    std::fs::create_dir(&malformed).unwrap();
    std::fs::write(malformed.join("manifest.json"), b"broken").unwrap();
    let op = upload(
        &s,
        &app,
        "dataset",
        "test:healthy",
        "application/geo+json",
        DATA,
    )
    .await;
    assert_eq!(op.status, OperationStatus::Ready, "{op:?}");
    assert!(
        orphan.is_dir(),
        "Unidentified content is retained for diagnosis"
    );
    assert!(malformed.is_dir());
    let diagnostics = s.catalog.reconcile_derived().unwrap();
    assert!(diagnostics.contains(&"a".repeat(64)));
    assert!(diagnostics.contains(&"b".repeat(64)));
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn abandoned_upload_records_failure_and_terminal_retention_frees_capacity() {
    let root = tempfile::tempdir().unwrap();
    let mut config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        public_base_url: "http://localhost".into(),
        data_directory: root.path().join("data"),
        external_roots: vec![],
        backend: BackendSelection::Cpu,
        profile: profile(),
    };
    config.profile.max_operations = 1;
    let s = ServerState::open(config).await.unwrap();
    let app = router(s.clone());
    let stream = futures_util::stream::pending::<std::result::Result<Vec<u8>, std::io::Error>>();
    {
        let pending = request(
            &app,
            "PUT",
            "/admin/v1/resources/dataset/test:aborted/content",
            Some("*"),
            Some("application/geo+json"),
            Body::from_stream(stream),
        );
        tokio::pin!(pending);
        tokio::select! {
            r = &mut pending => panic!("Upload must wait for its body: {}", r.status()),
            _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
        }
        assert_eq!(
            s.prune_operations(
                std::time::SystemTime::now() + std::time::Duration::from_secs(1),
                1
            )
            .unwrap(),
            0
        );
        // The handler is dropped while awaiting the next request body chunk.
    }
    let ledger = std::fs::read_dir(s.catalog.root().join("operations"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let op: ResourceOperation = serde_json::from_slice(&std::fs::read(&ledger).unwrap()).unwrap();
    assert_eq!(
        s.operation(&op.operation_id).unwrap().status,
        OperationStatus::Failed
    );
    assert_eq!(s.prune_operations(std::time::UNIX_EPOCH, 1).unwrap(), 0);
    assert_eq!(
        s.prune_operations(
            std::time::SystemTime::now() + std::time::Duration::from_secs(1),
            1
        )
        .unwrap(),
        1
    );
    assert_eq!(s.operation(&op.operation_id).unwrap_err().status, 404);
    let ready = upload(
        &s,
        &app,
        "dataset",
        "test:healthy",
        "application/geo+json",
        DATA,
    )
    .await;
    assert_eq!(ready.status, OperationStatus::Ready);
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);
    let s = state(root.path()).await;
    assert_eq!(s.operation(&op.operation_id).unwrap_err().status, 404);
    assert_eq!(
        s.operation(&ready.operation_id).unwrap().status,
        OperationStatus::Ready
    );
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn restart_reclaims_private_upload_staging_and_retains_unknown_content() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let abandoned = s.catalog.root().join("staging/upload-abandoned");
    let unknown = s.catalog.root().join("staging/operator-content");
    std::fs::create_dir(&abandoned).unwrap();
    std::fs::create_dir(&unknown).unwrap();
    std::fs::write(abandoned.join("source.geojson"), b"incomplete").unwrap();
    std::fs::write(unknown.join("notes.txt"), b"retained").unwrap();
    s.shutdown().await.unwrap();
    drop(s);
    let s = state(root.path()).await;
    assert!(!abandoned.exists());
    assert!(unknown.join("notes.txt").exists());
    s.shutdown().await.unwrap();
}
#[tokio::test]
async fn scheduler_cancellation_retains_capacity_until_worker_drains() {
    let scheduler = Arc::new(Scheduler::new(profile()));
    let context = scheduler.context();
    let cloned = context.clone();
    let s = scheduler.clone();
    let task = tokio::spawn(async move {
        s.blocking(WorkClass::Http, context, move |ctx| {
            for _ in 0..1000 {
                ctx.check()?;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(())
        })
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    task.abort();
    let _ = task.await;
    assert!(cloned.check().is_err());
}

#[tokio::test]
async fn shutdown_waits_for_abandoned_async_driver_until_it_releases_capacity() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let (entered, started) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let scheduler = s.scheduler.clone();
    let context = scheduler.context();
    let observed = context.clone();
    let driver = tokio::spawn(async move {
        scheduler
            .asynchronous(WorkClass::Http, context, async move {
                entered.send(()).unwrap();
                // Model a submitted device job that must drain despite cancellation.
                released.await.unwrap();
                assert!(observed.check().is_err());
                Ok(())
            })
            .await
    });
    started.await.unwrap();
    driver.abort();
    assert!(driver.await.unwrap_err().is_cancelled());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), s.shutdown())
            .await
            .is_err()
    );
    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), s.shutdown())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn shutdown_waits_for_detached_blocking_work_and_rejects_new_requests() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let scheduler = s.scheduler.clone();
    let task = tokio::spawn(async move {
        scheduler
            .blocking(WorkClass::Background, scheduler.context(), move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            })
            .await
    });
    entered_rx.await.unwrap();
    task.abort();
    let _ = task.await;
    s.begin_shutdown();
    let response = request(&app, "GET", "/health/ready", None, None, Body::empty()).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error = s
        .scheduler
        .blocking(WorkClass::Http, s.scheduler.context(), |_| Ok(()))
        .await
        .unwrap_err();
    assert_eq!(error.status, 503);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), s.shutdown())
            .await
            .is_err()
    );
    release_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), s.shutdown())
        .await
        .unwrap()
        .unwrap();
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_cancels_pending_upload_before_restart() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let body = Body::from_stream(futures_util::stream::pending::<
        std::result::Result<axum::body::Bytes, std::io::Error>,
    >());
    let cloned = app.clone();
    let upload_task = tokio::spawn(async move {
        request(
            &cloned,
            "PUT",
            "/admin/v1/resources/dataset/test:incomplete/content",
            Some("*"),
            Some("application/geo+json"),
            body,
        )
        .await
    });
    let ledger = s.catalog.root().join("operations");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while std::fs::read_dir(&ledger).unwrap().count() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let operation: ResourceOperation = serde_json::from_slice(
        &std::fs::read(
            std::fs::read_dir(&ledger)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
        )
        .unwrap(),
    )
    .unwrap();
    let config = s.config.clone();
    tokio::time::timeout(std::time::Duration::from_secs(5), s.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        upload_task.await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        s.operation(&operation.operation_id).unwrap().status,
        OperationStatus::Failed
    );
    assert_eq!(
        std::fs::read_dir(s.catalog.root().join("staging"))
            .unwrap()
            .count(),
        0
    );
    drop(app);
    drop(s);
    let reopened = ServerState::open(config).await.unwrap();
    assert_eq!(
        reopened.operation(&operation.operation_id).unwrap().status,
        OperationStatus::Failed
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_drains_accepted_preparation_and_persists_terminal_state() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let mut workers: Vec<tokio::task::JoinHandle<Result<()>>> = vec![];
    for _ in 0..s.config.profile.active_background {
        let scheduler = s.scheduler.clone();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        workers.push(tokio::spawn(async move {
            scheduler
                .blocking(WorkClass::Background, scheduler.context(), move |ctx| {
                    entered_tx.send(()).unwrap();
                    loop {
                        ctx.check()?;
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                })
                .await
        }));
        entered_rx.await.unwrap();
    }
    let source = root.path().join("queued.geojson");
    std::fs::write(&source, DATA).unwrap();
    let response = request(
        &app,
        "PUT",
        "/admin/v1/resources/dataset/test:queued/source",
        Some("*"),
        Some("application/json"),
        serde_json::json!({"path":source,"format":"geojson","crs":"OGC:CRS84"}).to_string(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let operation_id = json(response).await["operation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let config = s.config.clone();
    tokio::time::timeout(std::time::Duration::from_secs(5), s.shutdown())
        .await
        .unwrap()
        .unwrap();
    for worker in workers {
        assert!(worker.await.unwrap().is_err());
    }
    assert_eq!(
        s.operation(&operation_id).unwrap().status,
        OperationStatus::Failed
    );
    assert_eq!(s.catalog.stats().resources, 0);
    drop(app);
    drop(s);
    let restarted = ServerState::open(config).await.unwrap();
    assert_eq!(
        restarted.operation(&operation_id).unwrap().status,
        OperationStatus::Failed
    );
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn process_kill_recovers_published_authority_and_interrupted_upload() {
    const CHILD_ROOT: &str = "GEELLO_M3_CRASH_TEST_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = std::path::PathBuf::from(root);
        let s = state(&root).await;
        let app = router(s.clone());
        let published = upload(
            &s,
            &app,
            "dataset",
            "test:dataset",
            "application/geo+json",
            DATA,
        )
        .await;
        assert_eq!(published.status, OperationStatus::Ready);
        let body = Body::from_stream(futures_util::stream::pending::<
            std::result::Result<axum::body::Bytes, std::io::Error>,
        >());
        let cloned = app.clone();
        let _pending = tokio::spawn(async move {
            request(
                &cloned,
                "PUT",
                "/admin/v1/resources/dataset/test:interrupted/content",
                Some("*"),
                Some("application/geo+json"),
                body,
            )
            .await
        });
        loop {
            let mut interrupted = None;
            for entry in std::fs::read_dir(s.catalog.root().join("operations")).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                let op: ResourceOperation =
                    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                if op.resource.id == "test:interrupted" {
                    interrupted = Some(op.operation_id);
                }
            }
            if let Some(id) = interrupted {
                let ready =
                    serde_json::json!({"interrupted":id,"published_version":published.version});
                std::fs::write(root.join("child-ready.tmp"), ready.to_string()).unwrap();
                std::fs::rename(root.join("child-ready.tmp"), root.join("child-ready.json"))
                    .unwrap();
                // The parent kills this process without shutdown or Drop. This
                // exercises OS lock release and durable startup reconciliation.
                std::future::pending::<()>().await;
            }
            tokio::task::yield_now().await;
        }
    }
    let root = tempfile::tempdir().unwrap();
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process_kill_recovers_published_authority_and_interrupted_upload",
                "--nocapture",
            ])
            .env(CHILD_ROOT, root.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let ready_path = root.path().join("child-ready.json");
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while !ready_path.exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child failed before crash checkpoint"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let ready: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&ready_path).unwrap()).unwrap();
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let restarted = state(root.path()).await;
    let published = restarted
        .catalog
        .get(&ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap())
        .unwrap();
    assert_eq!(
        published.version,
        ready["published_version"].as_str().unwrap()
    );
    assert!(published.source.path.exists());
    assert_eq!(
        restarted
            .operation(ready["interrupted"].as_str().unwrap())
            .unwrap()
            .status,
        OperationStatus::Failed
    );
    assert_eq!(
        std::fs::read_dir(restarted.catalog.root().join("staging"))
            .unwrap()
            .count(),
        0
    );
    let response = request(
        &router(restarted.clone()),
        "GET",
        "/admin/v1/resources/dataset/test:dataset",
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    restarted.shutdown().await.unwrap();
}

#[test]
fn deletion_journal_recovers_before_and_after_catalog_commit() {
    for committed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(root.path(), profile()).unwrap();
        let mut resource = record("recovery:style");
        let original = catalog.root().join("managed/original");
        let trash = catalog.root().join("staging/delete-interrupted");
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("definition.toml"), b"retained").unwrap();
        resource.ownership = Ownership::Managed;
        resource.managed_directory = Some(original.clone());
        catalog
            .publish(resource.clone(), &Precondition::Absent)
            .unwrap();
        if committed {
            catalog
                .remove(
                    &resource.key,
                    &Precondition::Version(resource.version.clone()),
                    false,
                )
                .unwrap();
        }
        let journal = catalog.root().join("registry/deletions/interrupted.json");
        std::fs::write(
            &journal,
            serde_json::to_vec(&serde_json::json!({
                "key":resource.key,"version":resource.version,"original":original,"trash":trash
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::rename(&original, &trash).unwrap();
        drop(catalog);
        let catalog = Catalog::open(root.path(), profile()).unwrap();
        assert!(!journal.exists());
        assert!(!trash.exists());
        assert_eq!(original.exists(), !committed);
        assert_eq!(catalog.get(&resource.key).is_ok(), !committed);
    }
}

#[tokio::test]
async fn interrupted_operation_ledger_recovers_committed_publication() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let op = upload(
        &s,
        &app,
        "dataset",
        "test:dataset",
        "application/geo+json",
        DATA,
    )
    .await;
    assert_eq!(op.status, OperationStatus::Ready);
    let path = s
        .catalog
        .root()
        .join("operations")
        .join(format!("{}.json", op.operation_id));
    let mut interrupted = op.clone();
    interrupted.status = OperationStatus::Preparing;
    interrupted.version = None;
    std::fs::write(path, serde_json::to_vec(&interrupted).unwrap()).unwrap();
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);
    let s = state(root.path()).await;
    let recovered = s.operation(&op.operation_id).unwrap();
    assert_eq!(recovered.status, OperationStatus::Ready);
    assert_eq!(recovered.version, op.version);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn damaged_operation_identity_fails_restart_without_writing_outside_ledger() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let mut op = upload(
        &s,
        &app,
        "dataset",
        "test:dataset",
        "application/geo+json",
        DATA,
    )
    .await;
    assert_eq!(op.status, OperationStatus::Ready);
    let path = s
        .catalog
        .root()
        .join("operations")
        .join(format!("{}.json", op.operation_id));
    op.operation_id = "../escaped".into();
    op.status = OperationStatus::Preparing;
    std::fs::write(&path, serde_json::to_vec(&op).unwrap()).unwrap();
    let config = s.config.clone();
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);
    let reopened = ServerState::open(config).await;
    assert!(matches!(reopened, Err(ServerError { status: 500, .. })));
    assert!(!root.path().join("data/escaped.json").exists());
    assert!(path.exists());
}

#[tokio::test]
async fn damaged_authority_fails_closed_and_offline_checkpoint_restores_service() {
    fn copy_tree(source: &Path, destination: &Path) {
        std::fs::create_dir_all(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            assert!(!entry.file_type().unwrap().is_symlink());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
                assert_eq!(
                    blake3::hash(&std::fs::read(entry.path()).unwrap()),
                    blake3::hash(&std::fs::read(target).unwrap())
                );
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let data_root = root.path().join("data");
    let catalog = Catalog::open(&data_root, profile()).unwrap();
    let mut resource = record("test:style");
    let managed = catalog.root().join("managed/checkpoint-style");
    std::fs::create_dir(&managed).unwrap();
    resource.source.path = managed.join("style.toml");
    resource.ownership = Ownership::Managed;
    resource.managed_directory = Some(managed);
    resource.definition = Some(STYLE.into());
    std::fs::write(&resource.source.path, STYLE).unwrap();
    catalog
        .publish(resource.clone(), &Precondition::Absent)
        .unwrap();
    let expected = catalog.get(&resource.key).unwrap();
    let generation = catalog.stats().generation;
    drop(catalog);
    let data = root.path().join("data");
    let checkpoint = root.path().join("checkpoint");
    copy_tree(&data, &checkpoint);
    // Never guess the newest orphan generation when the authority is damaged.
    std::fs::write(data.join("registry/catalog.toml"), "corrupted authority").unwrap();
    assert!(Catalog::open(&data, profile()).is_err());
    let quarantine = root.path().join("quarantine");
    assert!(data.starts_with(root.path()) && quarantine.starts_with(root.path()));
    std::fs::rename(&data, &quarantine).unwrap();
    copy_tree(&checkpoint, &data);
    let restored = state(root.path()).await;
    assert_eq!(restored.catalog.stats().generation, generation);
    let actual = restored.catalog.get(&resource.key).unwrap();
    assert_eq!(actual.etag(), expected.etag());
    assert_eq!(actual.data_version, expected.data_version);
    assert_eq!(std::fs::read(actual.source.path).unwrap(), STYLE.as_bytes());
    let app = router(restored.clone());
    let response = request(
        &app,
        "GET",
        "/admin/v1/resources/style/test:style",
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(quarantine.join("registry/catalog.toml").exists());
    restored.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_derived_store_is_rebuilt_once_without_changing_data_version() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    for (kind, id, ctype, data) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        assert_eq!(
            upload(&s, &app, kind, id, ctype, data).await.status,
            OperationStatus::Ready
        );
    }
    let key = ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap();
    let old = s.catalog.get(&key).unwrap();
    let path = old.derived_path.clone().unwrap();
    assert_eq!(
        path.parent(),
        Some(s.catalog.root().join("derived").as_path())
    );
    std::fs::remove_dir_all(path).unwrap();
    let uri = "/maps/test:map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32";
    let response = request(&app, "GET", uri, None, None, Body::empty()).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json(response).await["type"], "urn:geello:problem:preparing");
    let mut repaired = false;
    for _ in 0..1000 {
        let record = s.catalog.get(&key).unwrap();
        if record.version != old.version {
            assert_eq!(record.data_version, old.data_version);
            assert!(record.derived_path.unwrap().exists());
            repaired = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(repaired);
    let response = request(&app, "GET", uri, None, None, Body::empty()).await;
    assert_eq!(response.status(), StatusCode::OK);
    s.shutdown().await.unwrap();
}

#[tokio::test]
async fn damaged_derived_data_and_changed_original_require_explicit_refresh() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.geojson");
    std::fs::write(&source, DATA).unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let register = serde_json::json!({"path": source, "format": "geojson"}).to_string();
    let endpoint = "/admin/v1/resources/dataset/test:dataset/source";
    let response = request(
        &app,
        "PUT",
        endpoint,
        Some("*"),
        Some("application/json"),
        register.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Ready
    );
    for (kind, id, body) in [("style", "test:style", STYLE), ("map", "test:map", MAP)] {
        assert_eq!(
            upload(&s, &app, kind, id, "application/toml", body)
                .await
                .status,
            OperationStatus::Ready
        );
    }
    let key = ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap();
    let original = s.catalog.get(&key).unwrap();
    let derived = original.derived_path.as_ref().unwrap();
    let member = derived.join("0.records");
    let valid_bytes = std::fs::read(&member).unwrap();
    let mut corrupt = valid_bytes.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    std::fs::write(&member, &corrupt).unwrap();
    let uri = "/maps/test:map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32";
    let failed = request(&app, "GET", uri, None, None, Body::empty()).await;
    assert!(!failed.status().is_success());
    assert_eq!(std::fs::read(&member).unwrap(), corrupt);
    assert_eq!(s.catalog.get(&key).unwrap().etag(), original.etag());
    // Operator quarantine preserves diagnostic bytes; serving must not repair
    // corruption by silently overwriting an immutable published directory.
    let quarantine = root.path().join("quarantine-derived");
    assert!(derived.starts_with(s.catalog.root()) && quarantine.starts_with(root.path()));
    std::fs::rename(derived, &quarantine).unwrap();
    std::fs::write(&source, DATA.replace("[0,0]", "[0.5,0.5]")).unwrap();
    let operation_count = || {
        std::fs::read_dir(s.catalog.root().join("operations"))
            .unwrap()
            .count()
    };
    let before = operation_count();
    let preparing = request(&app, "GET", uri, None, None, Body::empty()).await;
    assert_eq!(preparing.status(), StatusCode::SERVICE_UNAVAILABLE);
    let problem = json(preparing).await;
    assert_eq!(problem["type"], "urn:geello:problem:preparing");
    let operation_id = problem["title"]
        .as_str()
        .unwrap()
        .split("operation ")
        .nth(1)
        .unwrap();
    let operation = wait(&s, serde_json::json!({"operation_id": operation_id})).await;
    assert_eq!(operation.status, OperationStatus::Failed);
    assert!(
        operation
            .diagnostics
            .iter()
            .any(|d| d.contains("explicit refresh"))
    );
    for _ in 0..3 {
        let failed = request(&app, "GET", uri, None, None, Body::empty()).await;
        assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            json(failed).await["type"],
            "urn:geello:problem:preparation-failed"
        );
    }
    assert_eq!(operation_count(), before + 1);
    assert_eq!(s.catalog.get(&key).unwrap().etag(), original.etag());
    assert_eq!(
        s.catalog.get(&key).unwrap().data_version,
        original.data_version
    );
    let response = request(
        &app,
        "PUT",
        endpoint,
        Some(&original.etag()),
        Some("application/json"),
        register,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Ready
    );
    let refreshed = s.catalog.get(&key).unwrap();
    assert_ne!(refreshed.data_version, original.data_version);
    let removed = s
        .prune_operations(
            std::time::SystemTime::now() + std::time::Duration::from_secs(1),
            s.config.profile.max_operations,
        )
        .unwrap();
    assert!(removed > 0);
    assert_eq!(
        s.operation(&operation.operation_id).unwrap_err().status,
        404
    );
    assert_eq!(
        std::fs::read(quarantine.join("0.records")).unwrap(),
        corrupt
    );
    assert_eq!(
        request(&app, "GET", uri, None, None, Body::empty())
            .await
            .status(),
        StatusCode::OK
    );
    s.shutdown().await.unwrap();
}

#[test]
fn unpublished_dependents_still_protect_deletion_and_cycles_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let c = Catalog::open(root.path(), profile()).unwrap();
    let a = record("test:a");
    let mut b = record("test:b");
    b.dependencies.push(a.key.clone());
    c.publish(a.clone(), &Precondition::Absent).unwrap();
    c.publish(b.clone(), &Precondition::Absent).unwrap();
    c.unpublish(&b.key, &Precondition::Version(b.version.clone()))
        .unwrap();
    assert_eq!(
        c.remove(&a.key, &Precondition::Version(a.version.clone()), false)
            .unwrap_err()
            .status,
        409
    );
    let mut cyclic = a.clone();
    cyclic.version = "cyclic".into();
    cyclic.dependencies.push(a.key.clone());
    assert_eq!(
        c.publish(cyclic, &Precondition::Version(a.version))
            .unwrap_err()
            .status,
        409
    );
}

#[tokio::test]
async fn upload_and_managed_disk_limits_preserve_a_live_version() {
    let root = tempfile::tempdir().unwrap();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let op = upload(
        &s,
        &app,
        "dataset",
        "test:dataset",
        "application/geo+json",
        DATA,
    )
    .await;
    assert_eq!(op.status, OperationStatus::Ready);
    let key = ResourceKey::new(ResourceKind::Dataset, "test:dataset").unwrap();
    let old = s.catalog.get(&key).unwrap();
    let response = request(
        &app,
        "PUT",
        "/admin/v1/resources/dataset/test:dataset/content",
        Some(&old.etag()),
        Some("application/geo+json"),
        vec![b' '; profile().max_upload_bytes + 1],
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(s.catalog.get(&key).unwrap().version, old.version);
    let mut config = s.config.clone();
    s.shutdown().await.unwrap();
    drop(app);
    drop(s);
    config.profile.max_managed_bytes = 1;
    let s = ServerState::open(config).await.unwrap();
    let app = router(s.clone());
    let response = request(
        &app,
        "PUT",
        "/admin/v1/resources/dataset/test:dataset/content",
        Some(&old.etag()),
        Some("application/geo+json"),
        DATA,
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Failed
    );
    assert_eq!(s.catalog.get(&key).unwrap().version, old.version);
    assert!(old.source.path.exists());
    s.shutdown().await.unwrap();
}

#[test]
fn runnable_experimental_profile_matches_startup_schema() {
    let config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
    assert_eq!(config.backend, BackendSelection::Gpu);
    assert!(config.external_roots.is_empty());
}

#[cfg(feature = "render-gpu")]
#[tokio::test]
#[ignore = "Requires a real DX12/Vulkan GPU; separate platform acceptance"]
async fn gpu_server_executes_a_real_map_and_reports_actual_backend() {
    let root = tempfile::tempdir().unwrap();
    let s = ServerState::open(ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        public_base_url: "http://localhost".into(),
        data_directory: root.path().join("data"),
        external_roots: vec![],
        backend: BackendSelection::Gpu,
        profile: profile(),
    })
    .await
    .unwrap();
    assert_eq!(s.backend_name, "gpu");
    let app = router(s.clone());
    for (kind, id, ctype, data) in [
        ("dataset", "test:dataset", "application/geo+json", DATA),
        ("style", "test:style", "application/toml", STYLE),
        ("map", "test:map", "application/toml", MAP),
    ] {
        assert_eq!(
            upload(&s, &app, kind, id, ctype, data).await.status,
            OperationStatus::Ready
        );
    }
    let response = request(
        &app,
        "GET",
        "/maps/test:map/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=32&height=32",
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-geello-backend"], "gpu");
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    assert!(
        image::load_from_memory(&bytes)
            .unwrap()
            .into_rgba8()
            .get_pixel(16, 16)
            .0[0]
            > 200
    );
    s.shutdown().await.unwrap();
}

#[cfg(feature = "read-geotiff")]
#[tokio::test]
async fn malformed_tiff_replacements_preserve_the_managed_original_and_publication() {
    use tiff::{
        encoder::{TiffEncoder, colortype},
        tags::Tag,
    };
    let root = tempfile::tempdir().unwrap();
    let mut cursor = std::io::Cursor::new(vec![]);
    {
        let mut encoder = TiffEncoder::new(&mut cursor).unwrap();
        let mut image = encoder.new_image::<colortype::Gray8>(2, 2).unwrap();
        image
            .encoder()
            .write_tag(Tag::ModelPixelScaleTag, &[1f64, 1., 0.][..])
            .unwrap();
        image
            .encoder()
            .write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., 0., 2., 0.][..])
            .unwrap();
        image.write_data(&[0, 1, 2, 3]).unwrap();
    }
    let bytes = cursor.into_inner();
    let s = state(root.path()).await;
    let app = router(s.clone());
    let endpoint = "/admin/v1/resources/dataset/test:raster/content";
    let req = Request::builder()
        .method("PUT")
        .uri(endpoint)
        .header("if-none-match", "*")
        .header("content-type", "image/tiff")
        .header("x-geello-crs", "OGC:CRS84")
        .header("x-geello-sample-kind", "continuous")
        .body(Body::from(bytes.clone()))
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Ready
    );
    let key = ResourceKey::new(ResourceKind::Dataset, "test:raster").unwrap();
    let old = s.catalog.get(&key).unwrap();
    let mut invalid_offset = bytes.clone();
    invalid_offset[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    for malformed in [
        b"not a TIFF".to_vec(),
        bytes[..bytes.len() / 2].to_vec(),
        invalid_offset,
    ] {
        let req = Request::builder()
            .method("PUT")
            .uri(endpoint)
            .header("if-match", old.etag())
            .header("content-type", "image/tiff")
            .header("x-geello-crs", "OGC:CRS84")
            .header("x-geello-sample-kind", "continuous")
            .body(Body::from(malformed))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let operation = wait(&s, json(response).await).await;
        assert_eq!(operation.status, OperationStatus::Failed);
        assert!(!operation.diagnostics.is_empty());
        let current = s.catalog.get(&key).unwrap();
        assert_eq!(current.etag(), old.etag());
        assert_eq!(current.data_version, old.data_version);
        assert_eq!(current.derived_version, old.derived_version);
        assert_eq!(std::fs::read(&current.source.path).unwrap(), bytes);
        assert!(
            current
                .derived_path
                .unwrap()
                .join("manifest.json")
                .is_file()
        );
    }
    s.shutdown().await.unwrap();
}

#[cfg(feature = "read-geotiff")]
#[tokio::test]
async fn raster_base_serves_while_lazy_overviews_publish_a_new_revision() {
    use tiff::{
        encoder::{TiffEncoder, colortype},
        tags::Tag,
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.tif");
    {
        let mut encoder = TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
        let mut image = encoder.new_image::<colortype::Gray8>(3, 3).unwrap();
        image
            .encoder()
            .write_tag(Tag::ModelPixelScaleTag, &[1f64, 1., 0.][..])
            .unwrap();
        image
            .encoder()
            .write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., 0., 3., 0.][..])
            .unwrap();
        image.write_data(&[0, 1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    }
    let s = state(root.path()).await;
    let app = router(s.clone());
    let response=request(&app,"PUT","/admin/v1/resources/dataset/test:raster/source",Some("*"),Some("application/json"),serde_json::json!({"path":path,"format":"geotiff","crs":"OGC:CRS84","sample_kind":"continuous"}).to_string()).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Ready
    );
    let key = ResourceKey::new(ResourceKind::Dataset, "test:raster").unwrap();
    let base = s.catalog.get(&key).unwrap();
    let old_pin = s.catalog.pin(&key).unwrap();
    let style = "schema_version=1\nid='test:raster-style'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='image'\nkind='raster'\nmode='gray'\nband=1\nrange={kind='dataset-minmax'}\nresampling={enlarge='nearest',reduce='area-average'}\n";
    assert_eq!(
        upload(
            &s,
            &app,
            "style",
            "test:raster-style",
            "application/toml",
            style
        )
        .await
        .status,
        OperationStatus::Ready
    );
    let map = "schema_version=1\nid='test:raster-map'\noutput_crs=['OGC:CRS84']\n[[items]]\nid='raster'\ndataset='test:raster'\nstyle='test:raster-style'\n";
    assert_eq!(
        upload(&s, &app, "map", "test:raster-map", "application/toml", map)
            .await
            .status,
        OperationStatus::Ready
    );
    let uri = "/maps/test:raster-map/image?bbox=0,0,3,3&crs=OGC:CRS84&width=3&height=3";
    let response = request(&app, "GET", uri, None, None, Body::empty()).await;
    if response.status() != StatusCode::OK {
        panic!("raster response: {:?}", json(response).await);
    }
    assert_eq!(response.status(), StatusCode::OK);
    let revision = response.headers()["x-geello-render-revision"].clone();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let image = image::load_from_memory(&bytes).unwrap().into_rgba8();
    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 0, 255]);
    assert_eq!(image.get_pixel(2, 2).0, [255, 255, 255, 255]);
    let mut upgraded = None;
    for _ in 0..1000 {
        let current = s.catalog.get(&key).unwrap();
        if current.derived_version != base.derived_version {
            upgraded = Some(current);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let upgraded = upgraded.expect("overview publication");
    assert_eq!(upgraded.data_version, base.data_version);
    s.collect_garbage().unwrap();
    assert!(base.derived_path.as_ref().unwrap().exists());
    drop(old_pin);
    s.collect_garbage().unwrap();
    assert!(!base.derived_path.as_ref().unwrap().exists());
    let response = request(&app, "GET", uri, None, None, Body::empty()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_ne!(response.headers()["x-geello-render-revision"], revision);
    let decodes = s.native_decode_count();
    let renders = s.execution_counts().0;
    assert!(decodes > 0);
    // A different output identity must render again while reusing the same
    // immutable native base block, not the cached completed image.
    let response = request(
        &app,
        "GET",
        "/maps/test:raster-map/image?bbox=0,0,3,3&crs=OGC:CRS84&width=4&height=4",
        None,
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(s.execution_counts().0, renders + 1);
    assert_eq!(s.native_decode_count(), decodes);
    // A nearest style must not consume the old weighted pyramid. First use
    // schedules a replacement without changing the source dataset version.
    let weighted = s.catalog.get(&key).unwrap();
    let nearest_style = style.replace("reduce='area-average'", "reduce='nearest'");
    let style_key = ResourceKey::new(ResourceKind::Style, "test:raster-style").unwrap();
    let etag = s.catalog.get(&style_key).unwrap().etag();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/admin/v1/resources/style/test:raster-style/content")
                .header("content-type", "application/toml")
                .header("if-match", etag)
                .body(Body::from(nearest_style))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        wait(&s, json(response).await).await.status,
        OperationStatus::Ready
    );
    let response = request(&app, "GET", uri, None, None, Body::empty()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut nearest = None;
    for _ in 0..1000 {
        let current = s.catalog.get(&key).unwrap();
        if current.derived_version != weighted.derived_version {
            nearest = Some(current);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let nearest = nearest.expect("nearest overview replacement");
    assert_eq!(nearest.data_version, weighted.data_version);
    let pyramid = geello::derived::DerivedRaster::open(
        nearest.derived_path.as_ref().unwrap(),
        s.config.profile.store_limits(),
    )
    .unwrap();
    assert_eq!(pyramid.overview_method(), "nearest");
    s.shutdown().await.unwrap();
}
