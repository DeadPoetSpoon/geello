//! Fixed-seed synthetic M5 phase and catalog benchmark. Run in release mode.
use geello::{config::*, derived::*, readers::*, server::*, *};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Instant};

fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 2 << 30,
            max_work: 10_000_000_000,
            max_commands: 1_000_000,
            max_diagnostics: 32,
        },
        None,
    )
}
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}
fn measure<T>(phases: &mut BTreeMap<String, f64>, name: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = f();
    phases.insert(name.into(), ms(start));
    value
}
fn percentile(values: &mut [f64], percent: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * percent).ceil() as usize]
}
fn summary(mut values: Vec<f64>, elapsed: f64) -> Value {
    json!({"count": values.len(), "p50_ms": percentile(&mut values, 0.50), "p95_ms": percentile(&mut values, 0.95),
        "p99_ms": percentile(&mut values, 0.99), "throughput_per_second": values.len() as f64 / elapsed})
}
fn fixture(count: usize) -> Vec<u8> {
    let mut seed = 0x4d35_2026_u64;
    let mut random = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((seed >> 32) % 10_000) as f64 / 10_000.
    };
    let features = (0..count).map(|i| {
        let (x,y) = (110. + random() * 15., 30. + random() * 15.);
        let geometry = match i % 3 {
            0 => json!({"type":"Point","coordinates":[x,y]}),
            1 => json!({"type":"LineString","coordinates":(0..64).map(|j| vec![x + j as f64 * 0.0001, y + (j as f64 * 0.2).sin() * 0.005]).collect::<Vec<_>>()}),
            _ => json!({"type":"Polygon","coordinates":[[[x,y],[x+0.02,y],[x+0.02,y+0.02],[x,y+0.02],[x,y]],[[x+0.005,y+0.005],[x+0.015,y+0.005],[x+0.015,y+0.015],[x+0.005,y+0.015],[x+0.005,y+0.005]]]}),
        };
        json!({"type":"Feature","id":i,"properties":{"name":"中国地图 office مرحبا नमस्ते","payload":"x".repeat(512)},"geometry":geometry})
    }).collect::<Vec<_>>();
    serde_json::to_vec(&json!({"type":"FeatureCollection","features":features})).unwrap()
}
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let count = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3000);
    let catalogs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let gpu = args.get(3).is_some_and(|v| v == "gpu");
    let ctx = context();
    let directory = tempfile::tempdir()?;
    let mut phases = BTreeMap::new();
    let input = fixture(count);
    let options = VectorReadOptions {
        data_version: blake3::hash(&input).to_hex().to_string(),
        crs_override: None,
        limits: ReaderLimits {
            max_source_bytes: 128 << 20,
            max_record_bytes: 1 << 20,
            max_features: count + 1,
            max_coordinates_per_feature: 10000,
            max_depth: 32,
            batch_features: 256,
            batch_bytes: 8 << 20,
        },
        policy: ReadPolicy::Strict,
        identity: IdentityPolicy::OriginalOrOrdinal,
        queryables: vec![],
    };
    let snapshot = measure(&mut phases, "source_parse", || {
        read_geojson(input.as_slice(), &options, &ctx)
    })
    .unwrap();
    let limits = StoreLimits {
        max_disk_bytes: 1 << 30,
        max_manifest_bytes: 16 << 20,
        max_shard_features: 256,
        max_record_bytes: 1 << 20,
        max_block_bytes: 16 << 20,
    };
    let derived = measure(&mut phases, "derived_generation_storage", || {
        DerivedVector::build(directory.path(), &snapshot, limits, &ctx)
    })
    .unwrap();
    let batches = measure(&mut phases, "derived_query", || {
        let mut cursor = derived.query(VectorQuery::default(), &ctx).unwrap();
        let mut batches = Vec::new();
        while let Some(batch) = cursor.next_batch(256, 8 << 20, &ctx).unwrap() {
            batches.push(Arc::new((*batch).clone()));
        }
        batches
    });
    let target = Crs::new("EPSG:3857", CoordinateSpace::Planar)?;
    let projection =
        projection::RustProjection::new(snapshot.metadata().crs.clone(), target.clone())?;
    let projected = measure(&mut phases, "projection", || {
        batches
            .iter()
            .map(|b| projection.project_batch(b, &ctx).unwrap())
            .collect::<Vec<_>>()
    });
    let projected = projected
        .iter()
        .map(|b| Arc::new((**b).clone()))
        .collect::<Vec<_>>();
    let style = parse_style(
        "schema_version=1\nid='bench:style'\n[[rules]]\nid='base'\n[[rules.symbols]]\nid='point'\nkind='point'\nsize={value=3.0,unit='px'}\nfill='#3388ff'\n[[rules.symbols]]\nid='line'\nkind='line'\ncolor='#17212b'\nwidth={value=1.0,unit='px'}\n[[rules.symbols]]\nid='area'\nkind='area'\nfill='#44aa8866'\n",
    )?;
    let composition = measure(&mut phases, "style_resolution", || {
        resolve_vector_style(&style, &projected, &ctx)
    })
    .unwrap();
    let view = View {
        crs: target.clone(),
        bounds: Bounds {
            min_x: 12_245_143.,
            min_y: 3_503_549.,
            max_x: 13_914_937.,
            max_y: 5_621_521.,
        },
        width: 512,
        height: 512,
        aspect: AspectPolicy::Stretch,
    };
    let fonts = [
        (
            "latin",
            include_bytes!("../tests/fixtures/fonts/latin.ttf").as_slice(),
        ),
        (
            "arabic",
            include_bytes!("../tests/fixtures/fonts/arabic.ttf").as_slice(),
        ),
        (
            "indic",
            include_bytes!("../tests/fixtures/fonts/indic.ttf").as_slice(),
        ),
        (
            "cjk",
            include_bytes!("../tests/fixtures/fonts/cjk.otf").as_slice(),
        ),
    ]
    .into_iter()
    .map(|(id, bytes)| {
        Arc::new(FontResource {
            identity: id.into(),
            bytes: Arc::from(bytes),
            face_index: 0,
        })
    })
    .collect::<Vec<_>>();
    // Separate multilingual layout workload; deterministic placement includes collisions.
    let mut draws = composition.items.clone();
    for i in 0..128 {
        draws.push(DrawItem::Label(text::LabelItem {
            crs: target.clone(),
            geometry: geo_types::Point::new(
                12_300_000. + (i % 16) as f64 * 100_000.,
                3_550_000. + (i / 16) as f64 * 250_000.,
            )
            .into(),
            text: ["中国地图", "office ffi", "مرحبا", "नमस्ते"][i % 4].into(),
            style: TextStyle {
                fonts: fonts.clone(),
                size: Length::Pixels(14.),
                color: Color::BLACK,
                language: "und".into(),
            },
            placement: text::LabelPlacement::Point,
            priority: 0.,
            allow_overlap: false,
            wrap_width: None,
            repeat_distance: 300.,
            marker_source: None,
        }));
    }
    let raster = RasterBlock {
        width: 1024,
        height: 1024,
        grid: RasterGrid {
            crs: target.clone(),
            affine: [12_245_143., 1630.65, 0., 5_621_521., 0., -2068.33],
            interpretation: PixelInterpretation::Area,
        },
        bands: vec![RasterBand {
            samples: RasterSamples::F32(Arc::from(
                (0..1024 * 1024)
                    .map(|i| ((i * 17 % 65536) as f32) / 65535.)
                    .collect::<Vec<_>>(),
            )),
            no_data: None,
            validity: None,
            scale: 1.,
            offset: 0.,
        }],
        alpha: None,
        alpha_association: AlphaAssociation::Straight,
    };
    let processed = measure(&mut phases, "raster_colorize", || {
        raster_processing::colorize(
            &raster,
            &raster_processing::RasterColorOptions {
                bands: vec![0],
                ranges: vec![raster_processing::SampleRange { min: 0., max: 1. }],
                opacity: 0.3,
            },
            &ctx,
        )
    })
    .unwrap();
    draws.insert(
        0,
        DrawItem::Image(ImagePlacement {
            image: Arc::new(processed.image().clone()),
            crs: target,
            bounds: view.bounds,
            opacity: 1.,
        }),
    );
    let composition = MapComposition {
        items: draws,
        background: Some(Color([255, 255, 255, 255])),
    };
    let scene = measure(&mut phases, "layout_scene", || {
        prepare_scene(&composition, &view, &ctx)
    })
    .unwrap();
    let mut adapter = None;
    let mut renderer = None;
    #[cfg(feature = "render-gpu")]
    if gpu {
        let start = Instant::now();
        let (r, info) =
            GpuRenderer::new(wgpu::Backends::DX12 | wgpu::Backends::VULKAN, &ctx).await?;
        adapter = Some(format!("{info:?}"));
        renderer = Some(r);
        phases.insert("gpu_initialization".into(), ms(start));
    }
    let start = Instant::now();
    let pixels = if let Some(ref mut r) = renderer {
        r.render_pixels(&scene, &ctx).await?
    } else {
        CpuRenderer::new(1).render(&scene, &ctx)?
    };
    phases.insert("execute_including_gpu_readback".into(), ms(start));
    let encoded = measure(&mut phases, "png_encode", || {
        encoding::encode(&pixels, encoding::ImageFormat::Png, 16 << 20, &ctx)
    })
    .unwrap();
    let image_hash = blake3::hash(&encoded).to_hex().to_string();
    let mut latencies = Vec::new();
    let start = Instant::now();
    for _ in 0..20 {
        let tick = Instant::now();
        if let Some(ref mut r) = renderer {
            r.render_pixels(&scene, &ctx).await?;
        } else {
            CpuRenderer::new(1).render(&scene, &ctx)?;
        }
        latencies.push(ms(tick));
    }
    let render = summary(latencies, start.elapsed().as_secs_f64());
    let cancelled = context();
    cancelled.cancel();
    let tick = Instant::now();
    assert!(matches!(
        prepare_scene(&composition, &view, &cancelled),
        Err(GeelloError::Cancelled)
    ));
    let cancel_ms = ms(tick);
    assert_eq!(cancelled.live_bytes(), 0);
    let profile =
        ServerConfig::load(std::path::Path::new("examples/server.experimental.toml"))?.profile;
    let root = directory.path().join("catalog");
    let catalog = Catalog::open(&root, profile.clone())?;
    let record = |i: usize, version: usize| ResourceRecord {
        key: ResourceKey::new(ResourceKind::Style, format!("bench:s{i}")).unwrap(),
        ownership: Ownership::External,
        version: version.to_string(),
        published: true,
        source: SourceSpec {
            path: directory.path().join("style.toml"),
            format: "toml".into(),
            encoding: None,
            crs: None,
            sample_kind: None,
        },
        dependencies: vec![],
        data_version: version.to_string(),
        derived_version: None,
        derived_path: None,
        managed_directory: None,
        class: Some("style".into()),
        effective_crs: None,
        definition: Some(format!("schema_version=1\nid='bench:s{i}'\nrules=[]\n")),
        type_bindings: vec![],
    };
    let tick = Instant::now();
    for i in 0..catalogs {
        catalog.publish(record(i, 0), &Precondition::Absent)?;
    }
    let publish_ms = ms(tick);
    let tick = Instant::now();
    let mut lookup = Vec::new();
    for i in 0..10000 {
        let t = Instant::now();
        catalog.get(&ResourceKey::new(
            ResourceKind::Style,
            format!("bench:s{}", i % catalogs),
        )?)?;
        lookup.push(ms(t));
    }
    let lookup = summary(lookup, tick.elapsed().as_secs_f64());
    let updates = (0..4)
        .map(|worker| {
            let cat = catalog.clone();
            let records = (0..25)
                .map(|j| record(worker * 25 + j, 1))
                .collect::<Vec<_>>();
            std::thread::spawn(move || {
                let t = Instant::now();
                for r in records {
                    cat.publish(r, &Precondition::Version("0".into())).unwrap();
                }
                ms(t)
            })
        })
        .collect::<Vec<_>>();
    let update_ms = updates
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    drop(catalog);
    let tick = Instant::now();
    let catalog = Catalog::open(&root, profile)?;
    let restart_ms = ms(tick);
    let tick = Instant::now();
    catalog.rebuild_index()?;
    let rebuild_ms = ms(tick);
    let tick = Instant::now();
    let reclaimed = catalog.collect_garbage()?;
    let cleanup_ms = ms(tick);
    let disk_bytes = walk_bytes(directory.path());
    let output = json!({"schema_version":1,"seed":"0x4d352026","features":count,"catalog_records":catalogs,
        "build_mode":if cfg!(debug_assertions){"debug"}else{"release"},"platform":std::env::consts::OS,"architecture":std::env::consts::ARCH,
        "backend":if gpu {"gpu"} else {"cpu"},"adapter":adapter,"input_blake3":blake3::hash(&input).to_hex().to_string(),"input_bytes":input.len(),
        "phases_ms":phases,"render":render,"encoded_blake3":image_hash,
        "scene_accounted_bytes":scene.summary().estimated_accounted_bytes,"live_accounted_bytes":ctx.live_bytes(),
        "gpu_residency_lower_bound_bytes":if gpu {Some(512*512*4*2+1024*1024*4)} else {None},
        "gpu_residency_note":"output/readback plus input texture estimate; excludes driver/Vello allocations",
        "cancel":{"latency_ms":cancel_ms,"released_bytes":cancelled.live_bytes()},
        "catalog":{"initial_publish_ms":publish_ms,"lookup":lookup,"four_writer_25_updates_ms":update_ms,"restart_ms":restart_ms,
            "rebuild_ms":rebuild_ms,"cleanup_ms":cleanup_ms,"reclaimed":reclaimed,"disk_bytes":disk_bytes}});
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
fn walk_bytes(root: &std::path::Path) -> u64 {
    std::fs::read_dir(root)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk_bytes(&p)
            } else {
                p.metadata().unwrap().len()
            }
        })
        .sum()
}
