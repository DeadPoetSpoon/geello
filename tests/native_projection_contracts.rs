#![cfg(feature = "native-proj")]
use geello::{native_projection::*, projection::Projection, *};

fn crs(id: &str, space: CoordinateSpace) -> Crs {
    Crs::new(id, space).unwrap()
}

#[test]
fn fixed_offline_distribution_and_xy_roundtrip() {
    assert_eq!(distribution_identity().unwrap().len(), 64);
    let geographic = crs("EPSG:4326", CoordinateSpace::GeographicXy);
    let mercator = crs("EPSG:3857", CoordinateSpace::Planar);
    let forward = Projection::new(Some("native"), geographic.clone(), mercator.clone()).unwrap();
    let p = forward.transform_xy([120., 30.]).unwrap();
    assert!((p[0] - 13358338.895192828).abs() < 1e-6);
    assert!((p[1] - 3503549.8435043753).abs() < 1e-6);
    let reverse = NativeProjection::new(mercator, geographic).unwrap();
    let p = reverse.transform_xy(p).unwrap();
    assert!((p[0] - 120.).abs() < 1e-9 && (p[1] - 30.).abs() < 1e-9);
    assert!(forward.transform_xy([181., 30.]).is_err());
    assert!(forward.transform_xy([f64::NAN, 0.]).is_err());
}

#[test]
fn invalid_crs_and_missing_required_grid_fail_without_substitution() {
    let target = crs("EPSG:4326", CoordinateSpace::GeographicXy);
    for source in [
        crs("EPSG:4979", CoordinateSpace::GeographicXy),
        crs("EPSG:4326", CoordinateSpace::Planar),
        crs("EPSG:4978", CoordinateSpace::Planar),
    ] {
        assert!(NativeProjection::new(source, target.clone()).is_err());
    }
    assert!(
        NativeProjection::new(
            crs("EPSG:99999999", CoordinateSpace::Planar),
            target.clone()
        )
        .is_err()
    );
    assert!(
        NativeProjection::new(
            crs(
                "+proj=longlat +ellps=clrk66 +nadgrids=geello-missing-grid.gsb +type=crs",
                CoordinateSpace::GeographicXy
            ),
            target,
        )
        .and_then(|p| p.transform_xy([-100., 40.]))
        .is_err()
    );
    assert!(
        Projection::new(
            Some("unknown"),
            crs("EPSG:4326", CoordinateSpace::GeographicXy),
            crs("EPSG:3857", CoordinateSpace::Planar)
        )
        .is_err()
    );
}

#[test]
fn batch_projection_preserves_source_identity_and_cancellation() {
    use std::sync::Arc;
    let source = crs("EPSG:4326", CoordinateSpace::GeographicXy);
    let batch = VectorBatch {
        crs: source.clone(),
        features: vec![Feature {
            source_id: Some(PropertyValue::String("source".into())),
            id: Some(FeatureId::U64(7)),
            source_ordinal: 7,
            geometry: Some(geo_types::Point::new(120., 30.).into()),
            properties: Default::default(),
            source_ordinates: vec![],
        }],
    };
    let ctx = OperationContext::new(
        OperationLimits {
            max_live_bytes: 1 << 20,
            max_work: 1000,
            max_commands: 100,
            max_diagnostics: 8,
        },
        None,
    );
    let operation = Projection::new(
        Some("native"),
        source,
        crs("EPSG:3857", CoordinateSpace::Planar),
    )
    .unwrap();
    let original = Arc::new(batch);
    let projected = operation.project_batch(&original, &ctx).unwrap();
    assert_eq!(projected.features[0].id, original.features[0].id);
    assert_eq!(
        projected.features[0].source_id,
        original.features[0].source_id
    );
    assert_eq!(
        original.features[0].geometry,
        Some(geo_types::Point::new(120., 30.).into())
    );
    drop(projected);
    assert_eq!(ctx.live_bytes(), 0);
    ctx.cancel();
    assert!(matches!(
        operation.project_batch(&original, &ctx),
        Err(GeelloError::Cancelled)
    ));
    assert_eq!(ctx.live_bytes(), 0);
}
