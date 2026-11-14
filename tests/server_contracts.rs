//! M3 administration, durable publication and HTTP rendering contracts.
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
        max_list_items: 100,
        metatile_size: 2,
        metatile_buffer: 16,
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
        definition: Some(String::new()),
        type_bindings: vec![],
    }
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
    assert_eq!(config.backend, BackendSelection::GpuPreferred);
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
    s.shutdown().await.unwrap();
}
