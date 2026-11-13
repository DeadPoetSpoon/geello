//! Opt-in checks for owner-provided, ignored files. Never required by CI.
#![cfg(all(
    feature = "read-geojson",
    feature = "read-geotiff",
    feature = "render-cpu"
))]
use geello::{readers::*, *};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/test")
}
fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 12 << 30,
            max_work: 100_000_000,
            max_commands: 100_000,
            max_diagnostics: 16,
        },
        None,
    )
}
fn save_ppm(name: &str, image: &RgbaImage) {
    let directory = root().join("generated/m2-local");
    std::fs::create_dir_all(&directory).unwrap();
    let mut file = File::create(directory.join(name)).unwrap();
    write!(file, "P6\n{} {}\n255\n", image.width(), image.height()).unwrap();
    for p in image.pixels().chunks_exact(4) {
        let a = p[3] as u32;
        let rgb = [0, 1, 2].map(|i| ((p[i] as u32 * a + 255 * (255 - a) + 127) / 255) as u8);
        file.write_all(&rgb).unwrap();
    }
}
#[test]
#[ignore = "requires owner-provided assets/test/china.geo.json"]
fn owner_geojson_queries_and_renders_without_source_retention() {
    let ctx = context();
    let started = Instant::now();
    let snapshot = open_geojson(
        root().join("china.geo.json"),
        &VectorReadOptions {
            data_version: "owner-china-v1".into(),
            crs_override: None,
            limits: ReaderLimits {
                max_source_bytes: 2 << 20,
                max_record_bytes: 1 << 20,
                max_features: 1000,
                max_coordinates_per_feature: 1_000_000,
                max_depth: 32,
                batch_features: 8,
                batch_bytes: 64 << 20,
            },
            policy: ReadPolicy::Strict,
            identity: IdentityPolicy::OriginalOrOrdinal,
            queryables: vec![],
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(snapshot.metadata().feature_count, 36);
    let parse_time = started.elapsed();
    let mut cursor = snapshot.query(VectorQuery::default()).unwrap();
    let mut batches = vec![];
    while let Some(batch) = cursor.next_batch(&ctx).unwrap() {
        batches.push(Arc::new((*batch).clone()));
    }
    let count: usize = batches.iter().map(|b| b.features.len()).sum();
    assert_eq!(count, 36);
    let bounds = snapshot.metadata().bounds.unwrap();
    let view = View {
        crs: snapshot.metadata().crs.clone(),
        bounds,
        width: 1024,
        height: 768,
        aspect: AspectPolicy::Fit,
    };
    let composition = MapComposition {
        background: Some(Color([255, 255, 255, 255])),
        items: vec![DrawItem::Vector(VectorItem {
            batches,
            symbols: vec![Symbol::PolygonFill {
                color: Color([40, 100, 180, 255]),
                opacity: 1.0,
            }],
        })],
    };
    let scene = prepare_scene(&composition, &view, &ctx).unwrap();
    drop(composition);
    drop(cursor);
    drop(snapshot);
    let image = CpuRenderer::new(0).render(&scene, &ctx).unwrap();
    assert!(
        image
            .pixels()
            .chunks_exact(4)
            .any(|p| p[..3] != [255, 255, 255])
    );
    save_ppm("china.ppm", &image);
    println!(
        "china: features={count} parse={parse_time:?} total={:?} scene_commands={}",
        started.elapsed(),
        scene.summary().command_count
    );
    drop(scene);
    assert_eq!(ctx.live_bytes(), 0);
}
/// Independent byte-level oracle for these little-endian uncompressed strip files.
/// It does not use the application reader or the TIFF decoder for sample values.
fn strip_pixel(path: &Path, x: u32, y: u32) -> Vec<u8> {
    let mut f = File::open(path).unwrap();
    let mut header = [0u8; 8];
    f.read_exact(&mut header).unwrap();
    assert_eq!(&header[..4], b"II*\0");
    f.seek(SeekFrom::Start(
        u32::from_le_bytes(header[4..].try_into().unwrap()) as u64,
    ))
    .unwrap();
    let mut count = [0; 2];
    f.read_exact(&mut count).unwrap();
    let mut tags = std::collections::BTreeMap::new();
    for _ in 0..u16::from_le_bytes(count) {
        let mut e = [0; 12];
        f.read_exact(&mut e).unwrap();
        tags.insert(u16::from_le_bytes(e[..2].try_into().unwrap()), e);
    }
    let value = |tag: u16| u32::from_le_bytes(tags[&tag][8..12].try_into().unwrap());
    let width = value(256);
    let samples = tags.get(&277).map_or(1, |_| value(277));
    let rows = value(278);
    let bits_entry = tags[&258];
    let count = u32::from_le_bytes(bits_entry[4..8].try_into().unwrap());
    let bits = if count > 2 {
        f.seek(SeekFrom::Start(value(258) as u64)).unwrap();
        let mut b = [0; 2];
        f.read_exact(&mut b).unwrap();
        u16::from_le_bytes(b)
    } else {
        u16::from_le_bytes(bits_entry[8..10].try_into().unwrap())
    };
    let offsets = tags[&273];
    let strip_count = u32::from_le_bytes(offsets[4..8].try_into().unwrap());
    let offset = if strip_count == 1 {
        value(273)
    } else {
        f.seek(SeekFrom::Start(value(273) as u64 + (y / rows) as u64 * 4))
            .unwrap();
        let mut b = [0; 4];
        f.read_exact(&mut b).unwrap();
        u32::from_le_bytes(b)
    };
    let size = samples as usize * (bits as usize / 8);
    f.seek(SeekFrom::Start(
        offset as u64 + ((y % rows) as u64 * width as u64 + x as u64) * size as u64,
    ))
    .unwrap();
    let mut bytes = vec![0; size];
    f.read_exact(&mut bytes).unwrap();
    bytes
}
#[test]
#[ignore = "requires both owner-provided TIFFs and several GiB of snapshot memory"]
fn owner_large_tiffs_require_honest_crs_and_decode_native_windows() {
    for name in ["jd.tif", "jd_4326.tif"] {
        let path = root().join(name);
        let length = std::fs::metadata(&path).unwrap().len() as usize;
        let ctx = context();
        let started = Instant::now();
        let mut options = RasterReadOptions {
            crs_override: None,
            primary_ifd: None,
            max_source_bytes: length,
            max_decode_bytes: 16 << 20,
            max_tag_bytes: 1 << 20,
        };
        let error = open_geotiff(&path, &options, &ctx).unwrap_err();
        assert!(
            matches!(error, GeelloError::InvalidSource(_)),
            "{name}: {error}"
        );
        assert_eq!(ctx.live_bytes(), 0);
        println!("{name}: original metadata rejected: {error}");
        // This override is an explicit experiment based on the shared affine
        // grid. It is not a correction of or a guarantee about the source CRS.
        options.crs_override = Some(Crs::new("EPSG:3857", CoordinateSpace::Planar).unwrap());
        let snapshot = open_geotiff(&path, &options, &ctx).unwrap();
        assert_eq!(
            (snapshot.metadata().width, snapshot.metadata().height),
            (16384, 12544)
        );
        println!(
            "{name}: prepare={:?} accounted_snapshot_bytes={}",
            started.elapsed(),
            ctx.live_bytes()
        );
        let window_started = Instant::now();
        for (x, y) in [(0, 0), (8192, 6272), (16352, 12512)] {
            let block = snapshot
                .read_native_window(
                    NativeWindow {
                        column: x,
                        row: y,
                        width: 32,
                        height: 32,
                    },
                    &ctx,
                )
                .unwrap();
            let oracle = strip_pixel(&path, x, y);
            if name == "jd.tif" {
                assert_eq!(block.bands.len(), 3);
                for (j, band) in block.bands.iter().enumerate() {
                    assert!(matches!(&band.samples,RasterSamples::U8(v) if v[0]==oracle[j]));
                }
                assert_eq!(block.alpha.as_ref().unwrap()[0], oracle[3] as f32 / 255.);
            } else {
                assert!(
                    matches!(&block.bands[0].samples,RasterSamples::F64(v) if v[0].to_bits()==u64::from_le_bytes(oracle.try_into().unwrap()))
                );
            }
            if x == 8192 {
                use geello::raster_processing::*;
                let bands = if block.bands.len() == 3 {
                    vec![0, 1, 2]
                } else {
                    vec![0]
                };
                let colors = colorize(
                    &block,
                    &RasterColorOptions {
                        ranges: vec![SampleRange { min: 0., max: 255. }; bands.len()],
                        bands,
                        opacity: 1.,
                    },
                    &ctx,
                )
                .unwrap();
                save_ppm(&format!("{name}-window.ppm"), colors.image());
            }
        }
        println!(
            "{name}: three windows={:?}, total={:?}",
            window_started.elapsed(),
            started.elapsed()
        );
        drop(snapshot);
        assert_eq!(ctx.live_bytes(), 0);
    }
}
