//! M2 behavior fixtures are generated locally and require no network or assets.
use geello::query::{Filter, Truth};
use geello::*;
use std::sync::Arc;
fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 128 << 20,
            max_work: 10_000_000,
            max_commands: 10000,
            max_diagnostics: 8,
        },
        None,
    )
}
fn crs() -> Crs {
    Crs::new("OGC:CRS84", CoordinateSpace::GeographicXy).unwrap()
}
#[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
fn options() -> geello::readers::VectorReadOptions {
    use geello::readers::*;
    VectorReadOptions {
        data_version: "fixture-v1".into(),
        crs_override: None,
        limits: ReaderLimits {
            max_source_bytes: 1 << 20,
            max_record_bytes: 1 << 16,
            max_features: 1000,
            max_coordinates_per_feature: 10000,
            max_depth: 32,
            batch_features: 2,
            batch_bytes: 1 << 20,
        },
        policy: ReadPolicy::Strict,
        identity: IdentityPolicy::OriginalOrOrdinal,
        queryables: vec![],
    }
}

#[test]
fn typed_filters_keep_unknown_and_integer_precision() {
    let p: Properties =
        serde_json::from_str(r#"{"a/b":{"~n":18446744073709551615},"nil":null,"arr":["x"]}"#)
            .unwrap();
    assert_eq!(
        Filter::Eq {
            field: "/a~1b/~0n".into(),
            value: PropertyValue::U64(u64::MAX)
        }
        .evaluate(&p)
        .unwrap(),
        Truth::True
    );
    assert_eq!(
        Filter::Eq {
            field: "/nil".into(),
            value: PropertyValue::Null
        }
        .evaluate(&p)
        .unwrap(),
        Truth::Unknown
    );
    assert_eq!(
        Filter::IsNull {
            field: "/nil".into()
        }
        .evaluate(&p)
        .unwrap(),
        Truth::True
    );
    assert_eq!(
        Filter::Exists {
            field: "/arr/01".into()
        }
        .evaluate(&p)
        .unwrap(),
        Truth::False
    );
    assert_eq!(
        Filter::And {
            args: vec![
                Filter::Eq {
                    field: "/absent".into(),
                    value: PropertyValue::Bool(true)
                },
                Filter::Exists {
                    field: "/absent".into()
                }
            ]
        }
        .evaluate(&p)
        .unwrap(),
        Truth::False
    );
    assert!(
        Filter::Exists {
            field: "/bad~2escape".into()
        }
        .evaluate(&p)
        .is_err()
    );
    assert!(
        Filter::Exists {
            field: "/arr/+1".into()
        }
        .evaluate(&p)
        .unwrap()
            == Truth::False
    );
}
#[cfg(feature = "read-geojson")]
mod json {
    use super::*;
    use geello::readers::*;
    #[test]
    fn reordered_root_nested_values_null_empty_z_and_independent_cursors() {
        let input = r#"{"bbox":[0,0,10,10],"extension":["escaped } ], \\\"",[3]],"features":[{"type":"Feature","id":18446744073709551615,"properties":{"x":[null,{"n":-9223372036854775808}]},"geometry":{"type":"Point","coordinates":[1,2,7,9]}},{"type":"Feature","id":"empty","properties":{},"geometry":{"type":"LineString","coordinates":[]}},{"type":"Feature","id":"null","properties":{},"geometry":null}],"type":"FeatureCollection"}"#;
        let ctx = context();
        let snapshot = read_geojson(input.as_bytes(), &options(), &ctx).unwrap();
        assert_eq!(snapshot.metadata().feature_count, 3);
        assert_eq!(snapshot.metadata().null_geometries, 1);
        assert!(snapshot.metadata().schema["x"].optional);
        let mut a = snapshot.query(VectorQuery::default()).unwrap();
        let mut b = snapshot.query(VectorQuery::default()).unwrap();
        let batch = a.next_batch(&ctx).unwrap().unwrap();
        assert_eq!(batch.features.len(), 2);
        assert_eq!(batch.features[0].id, Some(FeatureId::U64(u64::MAX)));
        assert_eq!(batch.features[0].source_ordinates[0].trailing, vec![7., 9.]);
        assert_eq!(
            b.next_batch(&ctx).unwrap().unwrap().features,
            batch.features
        );
        assert_eq!(
            a.next_batch(&ctx).unwrap().unwrap().features[0].id,
            Some(FeatureId::String("null".into()))
        );
        assert!(a.next_batch(&ctx).unwrap().is_none());
        drop(a);
        drop(b);
        drop(snapshot);
        assert!(ctx.live_bytes() > 0);
        drop(batch);
        assert_eq!(ctx.live_bytes(), 0);
    }
    #[test]
    fn exact_spatial_intersection_excludes_polygon_hole_and_keeps_full_geometry() {
        let input = r#"{"type":"Feature","id":"area","properties":{},"geometry":{"type":"Polygon","coordinates":[[[0,0],[10,0],[10,10],[0,10],[0,0]],[[3,3],[3,7],[7,7],[7,3],[3,3]]]}}"#;
        let ctx = context();
        let s = read_geojson(input.as_bytes(), &options(), &ctx).unwrap();
        let bbox = Bounds {
            min_x: 4.,
            min_y: 4.,
            max_x: 6.,
            max_y: 6.,
        };
        assert!(
            s.query(VectorQuery {
                spatial: Some((bbox, SpatialMatch::ExactIntersection)),
                ..Default::default()
            })
            .unwrap()
            .next_batch(&ctx)
            .unwrap()
            .is_none()
        );
        let b = s
            .query(VectorQuery {
                spatial: Some((bbox, SpatialMatch::BoundingBoxCandidates)),
                ..Default::default()
            })
            .unwrap()
            .next_batch(&ctx)
            .unwrap()
            .unwrap();
        match b.features[0].geometry.as_ref().unwrap() {
            geo_types::Geometry::Polygon(p) => assert_eq!(p.exterior().0.len(), 5),
            _ => panic!(),
        }
    }
    #[test]
    fn recovery_only_skips_framed_semantic_records_and_budgets_are_hard() {
        let text = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":1,"geometry":null,"properties":{}},{"type":"Feature","id":1,"geometry":null,"properties":{}},{"type":"Feature","id":2,"geometry":{"type":"Point","coordinates":[1]},"properties":{}}]}"#;
        let ctx = context();
        assert!(read_geojson(text.as_bytes(), &options(), &ctx).is_err());
        assert_eq!(ctx.live_bytes(), 0);
        let mut o = options();
        o.policy = ReadPolicy::SkipInvalidRecords;
        let s = read_geojson(text.as_bytes(), &o, &ctx).unwrap();
        assert_eq!(s.metadata().feature_count, 1);
        assert_eq!(s.metadata().skipped_records, 2);
        assert!(
            read_geojson(
                br#"{"type":"FeatureCollection","features":[{"type":"Feature",}]}"# as &[u8],
                &o,
                &ctx
            )
            .is_err()
        );
        o.limits.max_depth = 2;
        assert!(matches!(
            read_geojson(
                br#"{"type":"Point","coordinates":[[[1,2]]]}"# as &[u8],
                &o,
                &ctx
            ),
            Err(GeelloError::BudgetExceeded("JSON depth"))
        ));
        let cancelled = context();
        cancelled.cancel();
        assert!(matches!(
            read_geojson(b"{}" as &[u8], &options(), &cancelled),
            Err(GeelloError::Cancelled)
        ));
    }
    #[test]
    fn override_applies_to_all_root_types() {
        let mut o = options();
        o.crs_override = Some(Crs::new("local", CoordinateSpace::Planar).unwrap());
        for text in [
            r#"{"type":"Point","coordinates":[1,2]}"#,
            r#"{"type":"Feature","properties":{},"geometry":null}"#,
            r#"{"type":"FeatureCollection","features":[]}"#,
        ] {
            assert_eq!(
                read_geojson(text.as_bytes(), &o, &context())
                    .unwrap()
                    .metadata()
                    .crs,
                o.crs_override.clone().unwrap()
            );
        }
    }
    #[test]
    fn generated_ids_are_versioned_disjoint_and_cursor_errors_are_terminal() {
        let text = r#"{"type":"FeatureCollection","features":[{"type":"Feature","id":"generated:fixture-v1:1","geometry":null,"properties":{"a":null}},{"type":"Feature","id":1.25,"geometry":null,"properties":{"b":true}}]}"#;
        let ctx = context();
        let snapshot = read_geojson(text.as_bytes(), &options(), &ctx).unwrap();
        let batch = snapshot
            .query(VectorQuery::default())
            .unwrap()
            .next_batch(&ctx)
            .unwrap()
            .unwrap();
        assert_ne!(batch.features[0].id, batch.features[1].id);
        assert_eq!(batch.features[1].source_id, Some(PropertyValue::F64(1.25)));
        assert!(snapshot.metadata().schema["a"].optional);
        assert!(snapshot.metadata().schema["b"].optional);
        let mut changed = options();
        changed.data_version = "fixture-v2".into();
        let other = read_geojson(text.as_bytes(), &changed, &ctx).unwrap();
        assert_ne!(
            snapshot.metadata().generated_ids,
            other.metadata().generated_ids
        );
        let mut cursor = snapshot.query(VectorQuery::default()).unwrap();
        let cancelled = context();
        cancelled.cancel();
        assert!(matches!(
            cursor.next_batch(&cancelled),
            Err(GeelloError::Cancelled)
        ));
        assert!(matches!(
            cursor.next_batch(&ctx),
            Err(GeelloError::Cancelled)
        ));
    }
    #[cfg(all(feature = "config-toml", feature = "rust-proj", feature = "render-cpu"))]
    #[test]
    fn styled_projected_snapshot_releases_sources_before_execution() {
        let ctx = context();
        let s=read_geojson(br#"{"type":"Feature","properties":{"size":10},"geometry":{"type":"Point","coordinates":[10,45]}}"# as &[u8],&options(),&ctx).unwrap();
        let style=geello::config::parse_style("schema_version=1\nid='test:points'\n[[rules]]\nid='all'\n[[rules.symbols]]\nid='point'\nkind='point'\nsize={field='/size',unit='px'}").unwrap();
        let view = View {
            crs: Crs::new("EPSG:3857", CoordinateSpace::Planar).unwrap(),
            bounds: Bounds {
                min_x: 1_100_000.,
                min_y: 5_610_000.,
                max_x: 1_130_000.,
                max_y: 5_640_000.,
            },
            width: 32,
            height: 32,
            aspect: AspectPolicy::Stretch,
        };
        let scene = prepare_styled_vector_snapshot(&s, VectorQuery::default(), &style, &view, &ctx)
            .unwrap();
        drop(s);
        drop(style);
        let image = CpuRenderer::new(0).render(&scene, &ctx).unwrap();
        assert!(image.pixels().chunks_exact(4).any(|p| p[3] > 0));
        drop(scene);
        assert_eq!(ctx.live_bytes(), 0);
    }
    #[cfg(all(feature = "config-toml", feature = "rust-proj"))]
    #[test]
    fn dataset_definition_applies_crs_identity_and_queryable_validation() {
        let path = std::env::temp_dir().join(format!(
            "geello-dataset-composition-{}.geojson",
            std::process::id()
        ));
        std::fs::write(&path, r#"{"type":"Feature","id":"source-id","properties":{"key":"effective-id","class":7},"geometry":{"type":"Point","coordinates":[10,45]}}"#).unwrap();
        let mut dataset = geello::config::parse_dataset(
            "id='test:data'\nsource={kind='geojson',path='placeholder',crs='EPSG:4326'}\nidentity={kind='attribute',field='key',stable=true}\nqueryables=[{name='category',field='class',type='integer'}]",
            "dataset.toml",
        )
        .unwrap();
        dataset.source.path = path.clone();
        let style = geello::config::parse_style(
            "schema_version=1\nid='test:style'\n[[rules]]\nid='all'\n[[rules.symbols]]\nid='point'\nkind='point'",
        )
        .unwrap();
        let view = View {
            crs: Crs::new("EPSG:3857", CoordinateSpace::Planar).unwrap(),
            bounds: Bounds {
                min_x: 1_100_000.,
                min_y: 5_610_000.,
                max_x: 1_130_000.,
                max_y: 5_640_000.,
            },
            width: 32,
            height: 32,
            aspect: AspectPolicy::Stretch,
        };
        let ctx = context();
        let mut options = options();
        // The definition must replace caller defaults before parsing and querying.
        options.crs_override = Some(Crs::new("local", CoordinateSpace::Planar).unwrap());
        let query = VectorQuery {
            ids: vec![FeatureId::String("effective-id".into())],
            ..Default::default()
        };
        let success =
            prepare_vector_dataset(&dataset, &options, query.clone(), &style, &view, &ctx);
        dataset.queryables[0].kind = "string".into();
        let invalid_type =
            prepare_vector_dataset(&dataset, &options, query.clone(), &style, &view, &ctx);
        dataset.queryables[0].kind = "integer".into();
        dataset.identity.as_mut().unwrap().field = Some("missing".into());
        let invalid_identity =
            prepare_vector_dataset(&dataset, &options, query, &style, &view, &ctx);
        std::fs::remove_file(path).unwrap();
        let scene = success.unwrap();
        assert_eq!(scene.view().crs, view.crs);
        assert_eq!(scene.summary().command_count, 1);
        assert!(matches!(invalid_type, Err(GeelloError::InvalidSource(_))));
        assert!(matches!(
            invalid_identity,
            Err(GeelloError::InvalidSource(_))
        ));
        drop(scene);
        assert_eq!(ctx.live_bytes(), 0);
    }
}

#[cfg(feature = "rust-proj")]
#[test]
fn projection_known_coordinates_roundtrip_and_source_immutability() {
    use geello::projection::RustProjection;
    let merc = Crs::new("EPSG:3857", CoordinateSpace::Planar).unwrap();
    let f = RustProjection::new(crs(), merc.clone()).unwrap();
    let p = f.transform_xy([10., 45.]).unwrap();
    assert!((p[0] - 1_113_194.9079327357).abs() < 1e-5);
    assert!((p[1] - 5_621_521.486192066).abs() < 1e-5);
    let inverse = RustProjection::new(merc, crs()).unwrap();
    let q = inverse.transform_xy(p).unwrap();
    assert!((q[0] - 10.).abs() < 1e-10);
    assert!((q[1] - 45.).abs() < 1e-10);
    assert!(f.transform_xy([0., 91.]).is_err());
    let source = VectorBatch {
        crs: crs(),
        features: vec![Feature {
            source_id: None,
            id: Some(FeatureId::String("x".into())),
            geometry: Some(geo_types::Point::new(10., 45.).into()),
            properties: Properties::new(),
            source_ordinal: 0,
            source_ordinates: vec![SourceOrdinate {
                coordinate_path: vec![],
                trailing: vec![12.],
            }],
        }],
    };
    let output = f.project_batch(&source, &context()).unwrap();
    assert_ne!(output.features[0].geometry, source.features[0].geometry);
    assert_eq!(
        output.features[0].source_ordinates,
        source.features[0].source_ordinates
    );
}

#[test]
fn native_raster_nodata_alpha_calibration_and_nearest_integer_precision() {
    use geello::raster_processing::*;
    let ctx = context();
    let block = RasterBlock {
        width: 2,
        height: 1,
        grid: RasterGrid {
            crs: crs(),
            affine: [0., 1., 0., 1., 0., -1.],
            interpretation: PixelInterpretation::Area,
        },
        bands: vec![RasterBand {
            samples: RasterSamples::U64(Arc::from([u64::MAX, u64::MAX - 1])),
            no_data: Some(PropertySample::U64(u64::MAX)),
            validity: None,
            scale: 1.,
            offset: 0.,
        }],
        alpha: Some(Arc::from([1., 0.5])),
        alpha_association: AlphaAssociation::Straight,
    };
    assert!(!valid_sample(&block.bands[0], 0));
    assert!(valid_sample(&block.bands[0], 1));
    let colored = colorize(
        &block,
        &RasterColorOptions {
            bands: vec![0],
            ranges: vec![SampleRange {
                min: 0.,
                max: u64::MAX as f64,
            }],
            opacity: 0.5,
        },
        &ctx,
    )
    .unwrap();
    assert_eq!(colored.image().pixels(), &[0, 0, 0, 0, 255, 255, 255, 64]);
    assert!(colored.precision_warning);
    let sampled = resample(
        &block,
        2,
        1,
        block.grid.clone(),
        RasterKind::Categorical,
        Resampling::Nearest,
        Ok,
        &ctx,
    )
    .unwrap();
    assert!(matches!(&sampled.bands[0].samples,RasterSamples::U64(v)if v[1]==u64::MAX-1));
    assert!(
        resample(
            &block,
            2,
            1,
            block.grid.clone(),
            RasterKind::Categorical,
            Resampling::Bilinear,
            Ok,
            &ctx
        )
        .is_err()
    );
}
#[test]
fn area_and_bilinear_normalize_valid_weights_without_double_alpha() {
    use geello::raster_processing::*;
    let ctx = context();
    let block = RasterBlock {
        width: 2,
        height: 2,
        grid: RasterGrid {
            crs: crs(),
            affine: [0., 1., 0., 2., 0., -1.],
            interpretation: PixelInterpretation::Area,
        },
        bands: vec![RasterBand {
            samples: RasterSamples::F64(Arc::from([10., 30., f64::NAN, 50.])),
            no_data: None,
            validity: None,
            scale: 2.,
            offset: 1.,
        }],
        alpha: Some(Arc::from([1., 1., 1., 1.])),
        alpha_association: AlphaAssociation::Straight,
    };
    let mut grid = block.grid.clone();
    grid.affine = [0., 2., 0., 2., 0., -2.];
    for method in [Resampling::Bilinear, Resampling::AreaAverage] {
        let sampled = resample(
            &block,
            1,
            1,
            grid.clone(),
            RasterKind::Continuous,
            method,
            Ok,
            &ctx,
        )
        .unwrap();
        assert!(matches!(&sampled.bands[0].samples,RasterSamples::F64(v)if (v[0]-61.).abs()<1e-8));
        assert_eq!(sampled.alpha.as_ref().unwrap()[0], 1.);
    }
}

#[test]
fn rotated_area_footprint_is_processed_without_axis_alignment_assumption() {
    use geello::raster_processing::*;
    let ctx = context();
    let block = RasterBlock {
        width: 2,
        height: 2,
        grid: RasterGrid {
            crs: crs(),
            affine: [0., 1., 0.5, 2., 0.5, -1.],
            interpretation: PixelInterpretation::Area,
        },
        bands: vec![RasterBand {
            samples: RasterSamples::F64(Arc::from([1., 2., 3., 4.])),
            no_data: None,
            validity: None,
            scale: 1.,
            offset: 0.,
        }],
        alpha: None,
        alpha_association: AlphaAssociation::Straight,
    };
    let mut target = block.grid.clone();
    target.affine = [0., 2., 1., 2., 1., -2.];
    let result = resample(
        &block,
        1,
        1,
        target,
        RasterKind::Continuous,
        Resampling::AreaAverage,
        Ok,
        &ctx,
    )
    .unwrap();
    assert!(matches!(&result.bands[0].samples, RasterSamples::F64(v) if (v[0]-2.5).abs()<1e-12));
}
