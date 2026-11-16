//! Offline M3 losslessness, query order, corruption and publication contracts.
#![cfg(all(feature = "derived-store", feature = "read-geojson"))]
use geello::{derived::*, readers::*, *};
use std::{io::Cursor, sync::Arc};
fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 128 << 20,
            max_work: 100_000_000,
            max_commands: 10000,
            max_diagnostics: 16,
        },
        None,
    )
}
fn limits() -> StoreLimits {
    StoreLimits {
        max_disk_bytes: 16 << 20,
        max_manifest_bytes: 1 << 20,
        max_shard_features: 2,
        max_record_bytes: 1 << 16,
        max_block_bytes: 2 << 20,
    }
}
fn vector(ctx: &OperationContext) -> VectorSnapshot {
    let text = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":18446744073709551615,"properties":{"nested":[null,18446744073709551615,1.5]},"geometry":{"type":"LineString","coordinates":[[20,20,99,7],[40,40,100,8]]}},{"type":"Feature","properties":{"a":"empty"},"geometry":{"type":"LineString","coordinates":[]}},{"type":"Feature","properties":{"a":"null"},"geometry":null},{"type":"Feature","id":"point","properties":{"a":true},"geometry":{"type":"Point","coordinates":[0,0]}}]}"#;
    read_geojson(
        Cursor::new(text),
        &VectorReadOptions {
            data_version: "source-v1".into(),
            crs_override: None,
            limits: ReaderLimits {
                max_source_bytes: 1 << 20,
                max_record_bytes: 1 << 16,
                max_features: 100,
                max_coordinates_per_feature: 100,
                max_depth: 32,
                batch_features: 2,
                batch_bytes: 1 << 20,
            },
            policy: ReadPolicy::Strict,
            identity: IdentityPolicy::OriginalOrOrdinal,
            queryables: vec![],
        },
        ctx,
    )
    .unwrap()
}
#[test]
fn fgb_companions_restore_full_source_order_and_native_queries() {
    let ctx = context();
    let dir = tempfile::tempdir().unwrap();
    let snapshot = vector(&ctx);
    let store = DerivedVector::build(dir.path(), &snapshot, limits(), &ctx).unwrap();
    let mut cursor = store.query(VectorQuery::default(), &ctx).unwrap();
    let mut actual = vec![];
    while let Some(batch) = cursor.next_batch(1, 1 << 20, &ctx).unwrap() {
        actual.extend(batch.features.clone());
    }
    let mut original = snapshot.query(VectorQuery::default()).unwrap();
    let mut expected = vec![];
    while let Some(batch) = original.next_batch(&ctx).unwrap() {
        expected.extend(batch.features.clone());
    }
    assert_eq!(actual, expected);
    assert!(actual[1].geometry.is_some());
    assert!(actual[2].geometry.is_none());
    let query = VectorQuery {
        spatial: Some((
            Bounds {
                min_x: 19.,
                min_y: 19.,
                max_x: 21.,
                max_y: 21.,
            },
            SpatialMatch::ExactIntersection,
        )),
        ..Default::default()
    };
    let mut cursor = store.query(query, &ctx).unwrap();
    let batch = cursor.next_batch(10, 1 << 20, &ctx).unwrap().unwrap();
    assert_eq!(batch.features.len(), 1);
    assert_eq!(batch.features[0], expected[0]);
    assert_eq!(store.data_version(), "source-v1");
    let mut by_id = store
        .query(
            VectorQuery {
                ids: vec![FeatureId::String("point".into())],
                ..Default::default()
            },
            &ctx,
        )
        .unwrap();
    assert_eq!(
        by_id
            .next_batch(1, 1 << 20, &ctx)
            .unwrap()
            .unwrap()
            .features[0]
            .source_ordinal,
        3
    );
}
#[test]
fn derived_corruption_and_disk_exhaustion_do_not_replace_a_complete_store() {
    let ctx = context();
    let dir = tempfile::tempdir().unwrap();
    let snapshot = vector(&ctx);
    let store = DerivedVector::build(dir.path(), &snapshot, limits(), &ctx).unwrap();
    let version = store.derived_version().to_owned();
    let mut tiny = limits();
    tiny.max_disk_bytes = 1;
    assert!(DerivedVector::build(dir.path(), &snapshot, tiny, &ctx).is_err());
    assert!(store.query(VectorQuery::default(), &ctx).is_ok());
    let path = dir.path().join(version).join("0.records");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] ^= 1;
    std::fs::write(path, bytes).unwrap();
    assert!(store.query(VectorQuery::default(), &ctx).is_err());
    assert!(DerivedVector::build(dir.path(), &snapshot, limits(), &ctx).is_err());
}
#[test]
fn vector_exact_disk_admission_and_failed_build_cleanup() {
    let ctx = context();
    let snapshot = vector(&ctx);
    let first = tempfile::tempdir().unwrap();
    let store = DerivedVector::build(first.path(), &snapshot, limits(), &ctx).unwrap();
    let root = first.path().join(store.derived_version());
    let exact: u64 = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    let mut bounded = limits();
    bounded.max_disk_bytes = exact;
    let second = tempfile::tempdir().unwrap();
    let rebuilt = DerivedVector::build(second.path(), &snapshot, bounded, &ctx).unwrap();
    assert_eq!(rebuilt.derived_version(), store.derived_version());
    let third = tempfile::tempdir().unwrap();
    bounded.max_disk_bytes -= 1;
    let before = ctx.live_bytes();
    assert!(DerivedVector::build(third.path(), &snapshot, bounded, &ctx).is_err());
    assert_eq!(std::fs::read_dir(third.path()).unwrap().count(), 0);
    assert_eq!(ctx.live_bytes(), before);
    let tiny = OperationContext::new(
        OperationLimits {
            max_live_bytes: 1,
            ..ctx.limits()
        },
        None,
    );
    assert!(matches!(
        DerivedVector::open_with_context(
            &first.path().join(store.derived_version()),
            limits(),
            &tiny
        ),
        Err(GeelloError::BudgetExceeded(_))
    ));
    assert_eq!(tiny.live_bytes(), 0);
}

#[test]
fn curved_raster_window_matches_full_source_with_bounded_memory() {
    use geello::raster_processing::*;
    let ctx = context();
    let dir = tempfile::tempdir().unwrap();
    let source = RasterGrid {
        affine: [0., 1., 0.2, 0., 0.1, 1.],
        ..grid()
    };
    let store = DerivedRaster::build_base(
        dir.path(),
        "curved",
        256,
        256,
        &source,
        16,
        limits(),
        &ctx,
        |w, _| {
            let mut samples = Vec::new();
            let mut valid = Vec::new();
            let mut alpha = Vec::new();
            for y in w.row..w.row + w.height {
                for x in w.column..w.column + w.width {
                    samples.push((x * x + y * 7) as u64);
                    valid.push((x + y) % 9 != 0);
                    alpha.push(if (x + y) % 7 == 0 { 0.25 } else { 1. });
                }
            }
            Ok(RasterBlock {
                width: w.width,
                height: w.height,
                grid: RasterGrid {
                    affine: [
                        w.column as f64 + 0.2 * w.row as f64,
                        1.,
                        0.2,
                        0.1 * w.column as f64 + w.row as f64,
                        0.1,
                        1.,
                    ],
                    ..source.clone()
                },
                bands: vec![RasterBand {
                    samples: RasterSamples::U64(samples.into()),
                    validity: Some(valid.into()),
                    no_data: None,
                    scale: 2.,
                    offset: 3.,
                }],
                alpha: Some(alpha.into()),
                alpha_association: AlphaAssociation::Straight,
            })
        },
    )
    .unwrap();
    let full = store
        .read_window(
            0,
            BlockWindow {
                column: 0,
                row: 0,
                width: 256,
                height: 256,
            },
            &ctx,
        )
        .unwrap();
    let target = RasterGrid {
        affine: [24., 0.9, 0., 22., 0., 0.7],
        ..source.clone()
    };
    // The interior sinusoidal extrema extend beyond the corner envelope.
    let map = |p: [f64; 2]| Ok([p[0] + 8. * (p[1] / 2.).sin(), p[1] + 0.003 * p[0] * p[0]]);
    for method in [
        Resampling::Nearest,
        Resampling::Bilinear,
        Resampling::AreaAverage,
    ] {
        let expected = resample(
            &full,
            12,
            16,
            target.clone(),
            RasterKind::Continuous,
            method,
            map,
            &ctx,
        )
        .unwrap();
        let small = OperationContext::new(
            OperationLimits {
                max_live_bytes: 512 << 10,
                ..ctx.limits()
            },
            None,
        );
        assert!(
            store
                .read_window(
                    0,
                    BlockWindow {
                        column: 0,
                        row: 0,
                        width: 256,
                        height: 256
                    },
                    &small
                )
                .is_err()
        );
        let actual = store
            .resample_view(
                0,
                12,
                16,
                target.clone(),
                RasterKind::Continuous,
                method,
                map,
                &small,
            )
            .unwrap()
            .unwrap();
        assert_eq!(actual.bands[0].validity, expected.bands[0].validity);
        match (&actual.bands[0].samples, &expected.bands[0].samples) {
            (RasterSamples::U64(a), RasterSamples::U64(b)) => assert_eq!(a, b),
            (RasterSamples::F64(a), RasterSamples::F64(b)) => {
                for (a, b) in a.iter().zip(b.iter()) {
                    assert!((a - b).abs() < 1e-8, "{method:?}: {a} vs {b}");
                }
            }
            _ => panic!("unexpected output sample type"),
        }
        for (a, b) in actual
            .alpha
            .as_ref()
            .unwrap()
            .iter()
            .zip(expected.alpha.as_ref().unwrap().iter())
        {
            assert!((a - b).abs() < 1e-6);
        }
        drop(actual);
        assert_eq!(small.live_bytes(), 0);
    }
    let cancelled = context();
    cancelled.cancel();
    assert!(matches!(
        store.resample_view(
            0,
            12,
            16,
            target,
            RasterKind::Continuous,
            Resampling::AreaAverage,
            map,
            &cancelled
        ),
        Err(GeelloError::Cancelled)
    ));
}
fn grid() -> RasterGrid {
    RasterGrid {
        crs: Crs::new("EPSG:3857", CoordinateSpace::Planar).unwrap(),
        affine: [0., 1., 0., 3., 0., -1.],
        interpretation: PixelInterpretation::Area,
    }
}
fn build_raster(parent: &std::path::Path, ctx: &OperationContext) -> DerivedRaster {
    DerivedRaster::build_base(
        parent,
        "raster-v1",
        3,
        3,
        &grid(),
        2,
        limits(),
        ctx,
        |w, _| {
            let mut samples = vec![];
            let mut valid = vec![];
            for row in w.row..w.row + w.height {
                for col in w.column..w.column + w.width {
                    samples.push((row * 3 + col) as f64);
                    valid.push(row != 0 || col != 0);
                }
            }
            Ok(RasterBlock {
                width: w.width,
                height: w.height,
                grid: RasterGrid {
                    affine: [w.column as f64, 1., 0., 3. - w.row as f64, 0., -1.],
                    ..grid()
                },
                bands: vec![RasterBand {
                    samples: RasterSamples::F64(samples.into()),
                    validity: Some(valid.into()),
                    no_data: None,
                    scale: 1.,
                    offset: 0.,
                }],
                alpha: None,
                alpha_association: AlphaAssociation::Straight,
            })
        },
    )
    .unwrap()
}
#[test]
fn raster_blocks_and_weighted_overviews_preserve_coverage_counts() {
    let ctx = context();
    let dir = tempfile::tempdir().unwrap();
    let base = build_raster(dir.path(), &ctx);
    let window = base
        .read_window(
            0,
            BlockWindow {
                column: 1,
                row: 1,
                width: 2,
                height: 2,
            },
            &ctx,
        )
        .unwrap();
    assert_eq!(
        window.bands[0].samples,
        RasterSamples::F64(Arc::from([4., 5., 7., 8.]))
    );
    let overview = base.build_overviews(dir.path(), false, &ctx).unwrap();
    assert_ne!(base.derived_version(), overview.derived_version());
    assert_eq!(base.data_version(), overview.data_version());
    assert_eq!(base.level_count(), 1);
    assert_eq!(overview.level_count(), 3);
    let pixel = overview
        .read_window(
            2,
            BlockWindow {
                column: 0,
                row: 0,
                width: 1,
                height: 1,
            },
            &ctx,
        )
        .unwrap();
    assert_eq!(pixel.bands[0].samples, RasterSamples::F64(Arc::from([4.5])));
    assert_eq!(overview.select_level(3.), 1);
}
#[test]
fn raster_native_bits_and_alpha_are_lossless() {
    let ctx = context();
    let dir = tempfile::tempdir().unwrap();
    let native = vec![
        f64::from_bits(0x7ff8_0000_0000_0011),
        -0.,
        f64::MIN_POSITIVE,
        f64::INFINITY,
    ];
    let expected = native.clone();
    let store = DerivedRaster::build_base(
        dir.path(),
        "raw",
        2,
        2,
        &grid(),
        2,
        limits(),
        &ctx,
        |_, _| {
            Ok(RasterBlock {
                width: 2,
                height: 2,
                grid: grid(),
                bands: vec![RasterBand {
                    samples: RasterSamples::F64(native.clone().into()),
                    validity: Some(vec![true, false, true, true].into()),
                    no_data: Some(PropertySample::F64(f64::from_bits(0x7ff8_0000_0000_0022))),
                    scale: 1.,
                    offset: 0.,
                }],
                alpha: Some(vec![0., 0.25, 0.5, 1.].into()),
                alpha_association: AlphaAssociation::Premultiplied,
            })
        },
    )
    .unwrap();
    let read = store
        .read_window(
            0,
            BlockWindow {
                column: 0,
                row: 0,
                width: 2,
                height: 2,
            },
            &ctx,
        )
        .unwrap();
    let RasterSamples::F64(v) = &read.bands[0].samples else {
        panic!()
    };
    assert_eq!(
        v.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    assert_eq!(read.alpha_association, AlphaAssociation::Premultiplied);
}

#[test]
fn weighted_overview_unassociates_alpha_and_rejects_tampered_manifest() {
    let ctx = context();
    let dir = tempfile::tempdir().unwrap();
    let base = DerivedRaster::build_base(
        dir.path(),
        "alpha",
        2,
        2,
        &grid(),
        2,
        limits(),
        &ctx,
        |_, _| {
            Ok(RasterBlock {
                width: 2,
                height: 2,
                grid: grid(),
                bands: vec![RasterBand {
                    samples: RasterSamples::F64(Arc::from([0., 10., 20., 999.])),
                    validity: Some(Arc::from([true, true, true, false])),
                    no_data: None,
                    scale: 2.,
                    offset: 5.,
                }],
                alpha: Some(Arc::from([0., 0.5, 1., 1.])),
                alpha_association: AlphaAssociation::Premultiplied,
            })
        },
    )
    .unwrap();
    let overview = base.build_overviews(dir.path(), false, &ctx).unwrap();
    let block = overview
        .read_window(
            1,
            BlockWindow {
                column: 0,
                row: 0,
                width: 1,
                height: 1,
            },
            &ctx,
        )
        .unwrap();
    assert_eq!(block.alpha_association, AlphaAssociation::Straight);
    assert_eq!(block.bands[0].samples, RasterSamples::F64(Arc::from([20.])));
    assert_eq!(block.bands[0].scale, 2.);
    assert_eq!(block.bands[0].offset, 5.);
    assert_eq!(block.alpha.as_ref().unwrap()[0], 0.625);
    let path = dir.path().join(base.derived_version());
    let manifest_path = path.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["levels"][0]["tiles"] = serde_json::json!([]);
    std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(DerivedRaster::open(&path, limits()).is_err());
}

#[test]
fn categorical_overview_samples_each_level_center_from_base() {
    let ctx = context();
    let dir = tempfile::tempdir().unwrap();
    let base = DerivedRaster::build_base(
        dir.path(),
        "categories",
        4,
        4,
        &grid(),
        4,
        limits(),
        &ctx,
        |_, _| {
            Ok(RasterBlock {
                width: 4,
                height: 4,
                grid: grid(),
                bands: vec![RasterBand {
                    samples: RasterSamples::U64((0..16).collect::<Vec<_>>().into()),
                    validity: None,
                    no_data: None,
                    scale: 1.,
                    offset: 0.,
                }],
                alpha: None,
                alpha_association: AlphaAssociation::Straight,
            })
        },
    )
    .unwrap();
    let nearest = base.build_overviews(dir.path(), true, &ctx).unwrap();
    let pixel = nearest
        .read_window(
            2,
            BlockWindow {
                column: 0,
                row: 0,
                width: 1,
                height: 1,
            },
            &ctx,
        )
        .unwrap();
    assert_eq!(pixel.bands[0].samples, RasterSamples::U64(Arc::from([10])));
}

#[test]
fn nearest_pyramid_reads_blocks_once_and_preserves_odd_edges_masks_and_u64() {
    let ctx = OperationContext::new(
        OperationLimits {
            max_live_bytes: 128 << 20,
            max_work: 100_000,
            max_commands: 1000,
            max_diagnostics: 16,
        },
        None,
    );
    let dir = tempfile::tempdir().unwrap();
    let base = DerivedRaster::build_base(
        dir.path(),
        "odd-nearest",
        9,
        7,
        &grid(),
        4,
        limits(),
        &ctx,
        |w, _| {
            let mut values = vec![];
            let mut masks = vec![];
            let mut alpha = vec![];
            for y in w.row..w.row + w.height {
                for x in w.column..w.column + w.width {
                    values.push(u64::MAX - (y * 9 + x) as u64);
                    masks.push((x + y) % 3 != 0);
                    alpha.push(if x % 2 == 0 { 0.5 } else { 1. });
                }
            }
            let mut g = grid();
            g.affine[0] += g.affine[1] * w.column as f64 + g.affine[2] * w.row as f64;
            g.affine[3] += g.affine[4] * w.column as f64 + g.affine[5] * w.row as f64;
            Ok(RasterBlock {
                width: w.width,
                height: w.height,
                grid: g,
                bands: vec![RasterBand {
                    samples: RasterSamples::U64(values.into()),
                    validity: Some(masks.into()),
                    no_data: None,
                    scale: 2.,
                    offset: 3.,
                }],
                alpha: Some(alpha.into()),
                alpha_association: AlphaAssociation::Straight,
            })
        },
    )
    .unwrap();
    let pyramid = base.build_overviews(dir.path(), true, &ctx).unwrap();
    assert_eq!(pyramid.overview_method(), "nearest");
    for level in 1..pyramid.level_count() {
        let (width, height, factor) = pyramid.level_dimensions(level).unwrap();
        assert_eq!(pyramid.select_level(factor as f64), level);
        assert!(pyramid.select_level(factor as f64 - 0.01) < level);
        let b = pyramid
            .read_window(
                level,
                BlockWindow {
                    column: 0,
                    row: 0,
                    width,
                    height,
                },
                &ctx,
            )
            .unwrap();
        let RasterSamples::U64(samples) = &b.bands[0].samples else {
            panic!("native u64 lost")
        };
        for y in 0..height {
            for x in 0..width {
                let sx = (x * factor + factor / 2).min(8);
                let sy = (y * factor + factor / 2).min(6);
                let i = (y * width + x) as usize;
                assert_eq!(samples[i], u64::MAX - (sy * 9 + sx) as u64);
                assert_eq!(b.bands[0].validity.as_ref().unwrap()[i], (sx + sy) % 3 != 0);
                assert_eq!(
                    b.alpha.as_ref().unwrap()[i],
                    if sx % 2 == 0 { 0.5 } else { 1. }
                );
            }
        }
    }
    assert!(
        ctx.work() < 100_000,
        "per-pixel block decode exceeds this work budget"
    );
}
