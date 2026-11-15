use geello::animation::*;
use geello::*;

fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 1 << 20,
            max_work: 100_000,
            max_commands: 1000,
            max_diagnostics: 8,
        },
        None,
    )
}
fn frame(time: f64, n: f64) -> Keyframe {
    Keyframe {
        time,
        value: Value::Number(n),
    }
}

fn vector_track(
    values: &[(f64, [f64; 2])],
    kind: ValueKind,
    ctx: &OperationContext,
) -> Accounted<Track> {
    Track::new(
        2.,
        Interpolation::Linear,
        kind,
        &values
            .iter()
            .map(|(time, value)| Keyframe {
                time: *time,
                value: Value::Vector(*value),
            })
            .collect::<Vec<_>>(),
        ctx,
    )
    .unwrap()
}
fn geometry_batch() -> VectorBatch {
    VectorBatch {
        crs: Crs::new("local", CoordinateSpace::Planar).unwrap(),
        features: vec![Feature {
            source_id: Some(PropertyValue::U64(42)),
            id: Some(FeatureId::U64(42)),
            geometry: Some(geo_types::LineString::from(vec![(10., 0.), (12., 0.)]).into()),
            properties: std::collections::BTreeMap::from([("size".into(), PropertyValue::U64(10))]),
            source_ordinal: 7,
            source_ordinates: vec![SourceOrdinate {
                coordinate_path: vec![0],
                trailing: vec![5.],
            }],
        }],
    }
}
fn coordinates(batch: &VectorBatch) -> Vec<[f64; 2]> {
    use geo::CoordsIter;
    batch.features[0]
        .geometry
        .as_ref()
        .unwrap()
        .coords_iter()
        .map(|c| [c.x, c.y])
        .collect()
}
fn near(actual: [f64; 2], expected: [f64; 2]) {
    assert!(
        (actual[0] - expected[0]).abs() < 1e-9 && (actual[1] - expected[1]).abs() < 1e-9,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn ordered_geometry_transforms_follow_anchors_and_preserve_originals_at_arbitrary_times() {
    let ctx = context();
    let original = geometry_batch();
    let actions = vec![
        GeometryAction::Translate(vector_track(
            &[(0., [0., 0.]), (2., [5., 0.])],
            ValueKind::Vector,
            &ctx,
        )),
        GeometryAction::Rotate {
            anchor: [10., 0.],
            track: Track::new(
                2.,
                Interpolation::Linear,
                ValueKind::Number,
                &[frame(0., 0.), frame(2., 90.)],
                &ctx,
            )
            .unwrap(),
        },
        GeometryAction::Scale {
            anchor: [10., 0.],
            track: vector_track(
                &[(0., [1., 1.]), (2., [2., 2.])],
                ValueKind::NonnegativeVector,
                &ctx,
            ),
        },
        GeometryAction::Translate(vector_track(
            &[(0., [0., 0.]), (2., [0., 3.])],
            ValueKind::Vector,
            &ctx,
        )),
    ];
    let binding = GeometryBinding::new(
        original.crs.clone(),
        &[FeatureId::I64(42)],
        BindingClock::new(1., 1., Repeat::Once).unwrap(),
        None,
        actions,
        1,
        &ctx,
    )
    .unwrap();
    for (time, expected) in [
        (3., [15., 7.]),
        (0., [12., 0.]),
        (2., [12.5 + 1.5 * 2f64.sqrt(), 1.5 + 1.5 * 2f64.sqrt()]),
        (3., [15., 7.]),
    ] {
        let output = binding.evaluate_batch(&original, time, &ctx).unwrap();
        near(coordinates(&output)[1], expected);
        let mut copy = output.features[0].clone();
        copy.geometry = original.features[0].geometry.clone();
        assert_eq!(copy, original.features[0]);
    }
    assert_eq!(coordinates(&original), vec![[10., 0.], [12., 0.]]);
    drop(binding);
    assert_eq!(ctx.live_bytes(), 0);
}

#[test]
fn trajectories_use_source_plane_distance_or_timestamps_with_endpoint_and_loop_rules() {
    let ctx = context();
    let source = geometry_batch();
    for (times, midpoint) in [
        (None, [10., 10.]),
        (Some(vec![0., 1.5, 2.]), [20. / 3., 10.]),
    ] {
        let points = [[0., 10.], [10., 10.], [10., 10.], [10., 20.]];
        let points = if times.is_some() {
            vec![points[0], points[1], points[3]]
        } else {
            points.to_vec()
        };
        let trajectory = Trajectory::new(
            [10., 0.],
            &points,
            times.as_deref(),
            2.,
            Interpolation::Linear,
            &ctx,
        )
        .unwrap();
        let binding = GeometryBinding::new(
            source.crs.clone(),
            &[FeatureId::U64(42)],
            BindingClock::new(1., 2., Repeat::Loop).unwrap(),
            Some(trajectory),
            vec![],
            1,
            &ctx,
        )
        .unwrap();
        for (time, expected) in [
            (0., [10., 0.]),
            (1., [0., 10.]),
            (1.5, midpoint),
            (2., [0., 10.]),
            (1.5, midpoint),
        ] {
            near(
                coordinates(&binding.evaluate_batch(&source, time, &ctx).unwrap())[0],
                expected,
            );
        }
    }
    let path = Trajectory::new(
        [10., 0.],
        &[[1., 2.], [8., 9.]],
        Some(&[0.5, 1.5]),
        2.,
        Interpolation::Step,
        &ctx,
    )
    .unwrap();
    let binding = GeometryBinding::new(
        source.crs.clone(),
        &[FeatureId::I64(42)],
        BindingClock::new(0., 1., Repeat::Once).unwrap(),
        Some(path),
        vec![],
        1,
        &ctx,
    )
    .unwrap();
    for (time, expected) in [
        (0., [1., 2.]),
        (1.49, [1., 2.]),
        (1.5, [8., 9.]),
        (20., [8., 9.]),
    ] {
        near(
            coordinates(&binding.evaluate_batch(&source, time, &ctx).unwrap())[0],
            expected,
        );
    }
}

#[test]
fn geometry_collections_rectangles_and_nulls_preserve_whole_feature_shape() {
    use geo_types::{Geometry, GeometryCollection, Rect};
    let ctx = context();
    let mut source = geometry_batch();
    source.features[0].geometry = Some(Geometry::GeometryCollection(GeometryCollection(vec![
        Geometry::Rect(Rect::new(
            geo_types::Coord { x: 0., y: 0. },
            geo_types::Coord { x: 2., y: 1. },
        )),
        Geometry::Polygon(geo_types::Polygon::new(
            geo_types::LineString::from(vec![(0., 0.), (4., 0.), (4., 4.), (0., 4.), (0., 0.)]),
            vec![geo_types::LineString::from(vec![
                (1., 1.),
                (1., 2.),
                (2., 2.),
                (1., 1.),
            ])],
        )),
        Geometry::GeometryCollection(GeometryCollection(vec![])),
    ])));
    let binding = GeometryBinding::new(
        source.crs.clone(),
        &[FeatureId::U64(42)],
        BindingClock::new(0., 1., Repeat::Once).unwrap(),
        None,
        vec![GeometryAction::Rotate {
            anchor: [0., 0.],
            track: Track::new(
                2.,
                Interpolation::Linear,
                ValueKind::Number,
                &[frame(0., 45.)],
                &ctx,
            )
            .unwrap(),
        }],
        1,
        &ctx,
    )
    .unwrap();
    let output = binding.evaluate_batch(&source, 1., &ctx).unwrap();
    let Some(Geometry::GeometryCollection(c)) = &output.features[0].geometry else {
        panic!("collection");
    };
    let Geometry::Polygon(rect) = &c[0] else {
        panic!("rotated rectangle must retain all four corners");
    };
    assert_eq!(rect.exterior().0.len(), 5);
    use geo::CoordsIter;
    let expected_corners = [
        [0., 0.],
        [2f64.sqrt(), 2f64.sqrt()],
        [0.5 * 2f64.sqrt(), 1.5 * 2f64.sqrt()],
        [-0.5 * 2f64.sqrt(), 0.5 * 2f64.sqrt()],
    ];
    for expected in expected_corners {
        assert!(
            rect.coords_iter()
                .any(|c| (c.x - expected[0]).abs() < 1e-9 && (c.y - expected[1]).abs() < 1e-9)
        );
    }
    assert!(matches!(&c[1], Geometry::Polygon(p) if p.interiors().len() == 1));
    assert!(matches!(&c[2], Geometry::GeometryCollection(c) if c.0.is_empty()));
    source.features[0].geometry = None;
    assert!(
        binding.evaluate_batch(&source, 1., &ctx).unwrap().features[0]
            .geometry
            .is_none()
    );
}

#[test]
fn geometry_selection_is_typed_bounded_and_failures_release_admission() {
    let ctx = context();
    let source = geometry_batch();
    for ids in [
        vec![],
        vec![FeatureId::U64(42), FeatureId::I64(42)],
        vec![FeatureId::String("generated:version:7".into())],
    ] {
        let action =
            GeometryAction::Translate(vector_track(&[(0., [1., 0.])], ValueKind::Vector, &ctx));
        assert!(
            GeometryBinding::new(
                source.crs.clone(),
                &ids,
                BindingClock::new(0., 1., Repeat::Once).unwrap(),
                None,
                vec![action],
                1,
                &ctx
            )
            .is_err()
        );
        assert_eq!(ctx.live_bytes(), 0);
    }
    let binding = GeometryBinding::new(
        source.crs.clone(),
        &[FeatureId::String("42".into())],
        BindingClock::new(0., 1., Repeat::Once).unwrap(),
        None,
        vec![GeometryAction::Translate(vector_track(
            &[(0., [1., 0.])],
            ValueKind::Vector,
            &ctx,
        ))],
        1,
        &ctx,
    )
    .unwrap();
    assert_eq!(*binding.evaluate_batch(&source, 1., &ctx).unwrap(), source);
    let mut incompatible = source.clone();
    incompatible.crs = Crs::new("other", CoordinateSpace::Planar).unwrap();
    assert!(binding.evaluate_batch(&incompatible, 1., &ctx).is_err());
    assert!(binding.evaluate_batch(&source, -1., &ctx).is_err());
    for (points, times) in [
        (vec![[0., 0.], [0., 0.]], None),
        (vec![[0., 0.], [1., 0.]], Some(vec![1., 1.])),
        (vec![[0., 0.], [f64::INFINITY, 0.]], None),
    ] {
        let before = ctx.live_bytes();
        assert!(
            Trajectory::new(
                [0., 0.],
                &points,
                times.as_deref(),
                2.,
                Interpolation::Linear,
                &ctx
            )
            .is_err()
        );
        assert_eq!(ctx.live_bytes(), before);
    }
    let tiny = OperationContext::new(
        OperationLimits {
            max_live_bytes: 1,
            ..context().limits()
        },
        None,
    );
    assert!(binding.evaluate_batch(&source, 1., &tiny).is_err());
    assert_eq!(tiny.live_bytes(), 0);
    let cancelled = context();
    cancelled.cancel();
    assert!(matches!(
        binding.evaluate_batch(&source, 1., &cancelled),
        Err(GeelloError::Cancelled)
    ));
    let no_work = OperationContext::new(
        OperationLimits {
            max_work: 0,
            ..context().limits()
        },
        None,
    );
    assert!(matches!(
        binding.evaluate_batch(&source, 1., &no_work),
        Err(GeelloError::BudgetExceeded("work"))
    ));
    assert_eq!(no_work.live_bytes(), 0);
    let overflow = GeometryBinding::new(
        source.crs.clone(),
        &[FeatureId::U64(42)],
        BindingClock::new(0., 1., Repeat::Once).unwrap(),
        None,
        vec![GeometryAction::Scale {
            anchor: [0., 0.],
            track: vector_track(
                &[(0., [f64::MAX, f64::MAX])],
                ValueKind::NonnegativeVector,
                &ctx,
            ),
        }],
        1,
        &ctx,
    )
    .unwrap();
    let before = ctx.live_bytes();
    assert!(overflow.evaluate_batch(&source, 1., &ctx).is_err());
    assert_eq!(ctx.live_bytes(), before);
}

#[cfg(feature = "config-toml")]
#[test]
fn geometry_toml_rejects_conflicts_and_incompatible_refresh_coordinates() {
    use geello::config::*;
    let ctx = context();
    let body = "schema_version=1\nid='demo:move'\nduration=2.0\n[[geometry_tracks]]\nid='move'\nsource_crs={definition='local',space='Planar'}\ninterpolation='linear'\ntrajectory={anchor=[10.0,0.0],points=[[0.0,0.0],[10.0,0.0]],timing='distance'}\n[[geometry_tracks.transforms]]\nkind='rotate'\nanchor=[10.0,0.0]\nkeyframes=[{time=0.0,value=350.0},{time=2.0,value=370.0}]";
    let animation = parse_animation(body).unwrap();
    let animations = std::collections::BTreeMap::from([(animation.id.clone(), animation)]);
    let map_body = "schema_version=1\nid='demo:map'\noutput_crs=['OGC:CRS84']\n[[items]]\nid='moving'\ndataset='demo:points'\nstyle='demo:style'\n[[items.geometry_bindings]]\nid='position'\nanimation='demo:move'\ntrack='move'\nfeature_ids=[42]\nmax_features=1\nstart=0.0\nspeed=1.0\nrepeat='once'";
    let map = parse_map(map_body).unwrap();
    let source = geometry_batch();
    let bindings =
        prepare_geometry_animations(&map.items[0], &animations, &source.crs, &ctx).unwrap();
    near(
        coordinates(&bindings[0].evaluate_batch(&source, 1., &ctx).unwrap())[0],
        [5., 0.],
    );
    assert!(
        prepare_geometry_animations(
            &map.items[0],
            &animations,
            &Crs::new("different", CoordinateSpace::Planar).unwrap(),
            &ctx
        )
        .is_err()
    );
    let mut item = map.items[0].clone();
    let mut duplicate = item.geometry_bindings[0].clone();
    duplicate.id = "other".into();
    item.geometry_bindings.push(duplicate);
    assert!(prepare_geometry_animations(&item, &animations, &source.crs, &ctx).is_err());
    let mut independent_item = map.items[0].clone();
    independent_item.id = "independent".into();
    assert!(prepare_geometry_animations(&independent_item, &animations, &source.crs, &ctx).is_ok());
    for invalid in [
        body.replace("timing='distance'", "timing='geodesic'"),
        body.replace("value=370.0", "value=nan"),
        body.replace("kind='rotate'", "kind='morph'"),
        body.replace("anchor=[10.0,0.0]\nkeyframes", "keyframes"),
    ] {
        assert!(parse_animation(&invalid).is_err(), "{invalid}");
    }
    assert!(parse_map(&map_body.replace("max_features=1", "max_features=0")).is_err());
    assert!(
        parse_map(
            &map_body
                .replace("feature_ids=[42]", "feature_ids=[42,42]")
                .replace("max_features=1", "max_features=2")
        )
        .is_err()
    );
}

#[cfg(feature = "config-toml")]
#[test]
fn committed_geometry_demo_prepares_at_arbitrary_times_without_a_renderer() {
    use geello::config::*;
    let ctx = context();
    let style = parse_style(include_str!("../examples/geometry-animation/style.toml")).unwrap();
    let animation = parse_animation(include_str!(
        "../examples/geometry-animation/animation.toml"
    ))
    .unwrap();
    let map = parse_map(include_str!("../examples/geometry-animation/map.toml")).unwrap();
    let animations = std::collections::BTreeMap::from([(animation.id.clone(), animation)]);
    let mut source = geometry_batch();
    source.crs = Crs::new("OGC:CRS84", CoordinateSpace::GeographicXy).unwrap();
    source.features[0].id = Some(FeatureId::String("moving".into()));
    let bindings =
        prepare_geometry_animations(&map.items[0], &animations, &source.crs, &ctx).unwrap();
    assert_eq!(style.id, map.items[0].style);
    for (time, expected) in [(2., [0., 0.25]), (0., [10., 0.]), (2., [0., 0.25])] {
        near(
            coordinates(&bindings[0].evaluate_batch(&source, time, &ctx).unwrap())[0],
            expected,
        );
    }
}

#[test]
fn arbitrary_time_endpoint_holds_step_boundaries_and_raw_angles() {
    let ctx = context();
    let frames = [frame(0.5, 350.), frame(1.5, 10.)];
    let once = BindingClock::new(1., 2., Repeat::Once).unwrap();
    let track = Track::new(2., Interpolation::Linear, ValueKind::Number, &frames, &ctx).unwrap();
    for (time, expected) in [
        (4., Some(10.)),
        (0., None),
        (1., Some(350.)),
        (1.5, Some(180.)),
        (1.25, Some(350.)),
        (4., Some(10.)),
    ] {
        assert_eq!(
            track.evaluate(once, time, &ctx).unwrap(),
            expected.map(Value::Number)
        );
    }
    let looping = BindingClock::new(1., 2., Repeat::Loop).unwrap();
    assert_eq!(
        track.evaluate(looping, 2., &ctx).unwrap(),
        Some(Value::Number(350.))
    );
    let step = Track::new(2., Interpolation::Step, ValueKind::Number, &frames, &ctx).unwrap();
    assert_eq!(
        step.evaluate(once, 1.749, &ctx).unwrap(),
        Some(Value::Number(350.))
    );
    assert_eq!(
        step.evaluate(once, 1.75, &ctx).unwrap(),
        Some(Value::Number(10.))
    );
    let clock = BindingClock::new(0., 1., Repeat::Once).unwrap();
    let turns = Track::new(
        2.,
        Interpolation::Linear,
        ValueKind::Number,
        &[frame(0., 350.), frame(2., 1090.)],
        &ctx,
    )
    .unwrap();
    assert_eq!(
        turns.evaluate(clock, 1., &ctx).unwrap(),
        Some(Value::Number(720.))
    );
}

#[test]
fn colors_interpolate_in_linear_light_with_premultiplied_alpha() {
    let ctx = context();
    let clock = BindingClock::new(0., 1., Repeat::Once).unwrap();
    for (a, b, expected) in [
        ([0, 0, 0, 255], [255, 255, 255, 255], [188, 188, 188, 255]),
        ([255, 0, 0, 0], [0, 0, 255, 255], [0, 0, 255, 128]),
        ([255, 0, 0, 0], [0, 255, 0, 0], [0, 0, 0, 0]),
    ] {
        let frames = [
            Keyframe {
                time: 0.,
                value: Value::Color(Color(a)),
            },
            Keyframe {
                time: 1.,
                value: Value::Color(Color(b)),
            },
        ];
        let track = Track::new(1., Interpolation::Linear, ValueKind::Color, &frames, &ctx).unwrap();
        assert_eq!(
            track.evaluate(clock, 0.5, &ctx).unwrap(),
            Some(Value::Color(Color(expected)))
        );
        if a[3] == 0 {
            assert_eq!(
                track.evaluate(clock, 0., &ctx).unwrap(),
                Some(Value::Color(Color::TRANSPARENT))
            );
        }
    }
}

#[test]
fn invalid_tracks_clocks_and_times_fail_without_retaining_admission() {
    let ctx = context();
    for (kind, frames) in [
        (ValueKind::Number, vec![]),
        (ValueKind::Number, vec![frame(1., 0.), frame(1., 1.)]),
        (ValueKind::Number, vec![frame(-1., 0.)]),
        (ValueKind::Number, vec![frame(3., 0.)]),
        (ValueKind::Number, vec![frame(0., f64::INFINITY)]),
        (ValueKind::Nonnegative, vec![frame(0., -1.)]),
        (ValueKind::Opacity, vec![frame(0., 1.1)]),
        (ValueKind::Color, vec![frame(0., 0.)]),
    ] {
        assert!(Track::new(2., Interpolation::Step, kind, &frames, &ctx).is_err());
        assert_eq!(ctx.live_bytes(), 0);
    }
    for (start, speed) in [
        (-1., 1.),
        (0., 0.),
        (0., -1.),
        (f64::NAN, 1.),
        (0., f64::INFINITY),
    ] {
        assert!(BindingClock::new(start, speed, Repeat::Once).is_err());
    }
    let track = Track::new(
        2.,
        Interpolation::Linear,
        ValueKind::Number,
        &[frame(0., -f64::MAX), frame(2., f64::MAX)],
        &ctx,
    )
    .unwrap();
    let clock = BindingClock::new(0., 1., Repeat::Once).unwrap();
    assert_eq!(
        track.evaluate(clock, 1., &ctx).unwrap(),
        Some(Value::Number(0.))
    );
    for t in [-1., f64::NAN, f64::INFINITY] {
        assert!(track.evaluate(clock, t, &ctx).is_err());
    }
    let loop_clock = BindingClock::new(0., f64::MAX, Repeat::Loop).unwrap();
    assert!(track.evaluate(loop_clock, 2., &ctx).is_err());
}

#[test]
fn track_admission_cancellation_and_work_limits_are_enforced() {
    let tiny = OperationContext::new(
        OperationLimits {
            max_live_bytes: 1,
            ..context().limits()
        },
        None,
    );
    assert!(
        Track::new(
            1.,
            Interpolation::Linear,
            ValueKind::Number,
            &[frame(0., 1.)],
            &tiny
        )
        .is_err()
    );
    assert_eq!(tiny.live_bytes(), 0);
    let ctx = context();
    let track = Track::new(
        1.,
        Interpolation::Linear,
        ValueKind::Number,
        &[frame(0., 1.)],
        &ctx,
    )
    .unwrap();
    assert!(ctx.live_bytes() > 0);
    ctx.cancel();
    assert!(matches!(
        track.evaluate(BindingClock::new(0., 1., Repeat::Once).unwrap(), 0., &ctx),
        Err(GeelloError::Cancelled)
    ));
    drop(track);
    assert_eq!(ctx.live_bytes(), 0);
    let limited = OperationContext::new(
        OperationLimits {
            max_work: 0,
            ..context().limits()
        },
        None,
    );
    assert!(
        Track::new(
            1.,
            Interpolation::Step,
            ValueKind::Number,
            &[frame(0., 1.)],
            &limited
        )
        .is_err()
    );
    assert_eq!(limited.live_bytes(), 0);
}

#[test]
fn symbol_overrides_replace_base_values_without_accumulating_or_creating_strokes() {
    let ctx = context();
    let clock = BindingClock::new(1., 1., Repeat::Once).unwrap();
    let size = SymbolTrack::new(
        SymbolParameter::Size,
        Some(LengthUnit::MapUnits),
        clock,
        2.,
        Interpolation::Linear,
        &[frame(0., 6.), frame(2., 12.)],
        &ctx,
    )
    .unwrap();
    let mut symbol = Symbol::Point(PointStyle {
        size: Length::Pixels(99.),
        ..Default::default()
    });
    size.apply(&mut symbol, 0., &ctx).unwrap();
    assert!(matches!(&symbol, Symbol::Point(p) if p.size == Length::Pixels(99.)));
    for t in [2., 4., 2.] {
        size.apply(&mut symbol, t, &ctx).unwrap();
        let expected = if t == 4. { 12. } else { 9. };
        assert!(matches!(&symbol, Symbol::Point(p) if p.size == Length::MapUnits(expected)));
    }
    let width = SymbolTrack::new(
        SymbolParameter::Width,
        Some(LengthUnit::Pixels),
        clock,
        2.,
        Interpolation::Linear,
        &[frame(0., 2.)],
        &ctx,
    )
    .unwrap();
    let before = symbol.clone();
    assert!(width.apply(&mut symbol, 0., &ctx).is_err());
    assert_eq!(before, symbol);
    assert!(size.raster_opacity(1., 1., &ctx).is_err());
    let alpha = SymbolTrack::new(
        SymbolParameter::Opacity,
        None,
        clock,
        2.,
        Interpolation::Linear,
        &[frame(0., 0.2), frame(2., 0.8)],
        &ctx,
    )
    .unwrap();
    assert_eq!(alpha.raster_opacity(0.7, 0., &ctx).unwrap(), 0.7);
    assert_eq!(alpha.raster_opacity(0.7, 2., &ctx).unwrap(), 0.5);
    assert!(
        SymbolTrack::new(
            SymbolParameter::Angle,
            Some(LengthUnit::Pixels),
            clock,
            1.,
            Interpolation::Linear,
            &[frame(0., 0.)],
            &ctx
        )
        .is_err()
    );
}

#[cfg(feature = "config-toml")]
#[test]
fn toml_bindings_override_original_property_values_without_changing_features() {
    use geello::config::*;
    use std::{collections::BTreeMap, sync::Arc};
    let ctx = context();
    let style = parse_style("schema_version=1\nid='demo:style'\n[[rules]]\nid='all'\nfilter={op='exists',field='/size'}\n[[rules.symbols]]\nid='marker'\nkind='point'\nsize={field='/size',unit='px'}").unwrap();
    let map = parse_map("schema_version=1\nid='demo:map'\noutput_crs=['OGC:CRS84']\n[[items]]\nid='points'\ndataset='demo:points'\nstyle='demo:style'\n[[items.animation_bindings]]\nid='pulse'\nanimation='demo:pulse'\ntrack='diameter'\ntarget={rule='all',symbol='marker'}\nstart=1.0\nspeed=1.0\nrepeat='once'").unwrap();
    let animation = parse_animation("schema_version=1\nid='demo:pulse'\nduration=2.0\n[[tracks]]\nid='diameter'\nparameter='size'\ninterpolation='linear'\nunit='px'\nkeyframes=[{time=0.0,value=6.0},{time=2.0,value=12.0}]").unwrap();
    let mut animations = BTreeMap::from([(animation.id.clone(), animation)]);
    let bindings = prepare_style_animations(&style, &map.items[0], &animations, &ctx).unwrap();
    let batches = vec![Arc::new(VectorBatch {
        crs: Crs::new("local", CoordinateSpace::Planar).unwrap(),
        features: vec![Feature {
            id: Some(FeatureId::U64(1)),
            source_id: None,
            source_ordinal: 0,
            source_ordinates: vec![],
            geometry: Some(geo_types::Point::new(0., 0.).into()),
            properties: BTreeMap::from([("size".into(), PropertyValue::F64(99.))]),
        }],
    })];
    let original = batches.clone();
    for (time, expected) in [(2., 9.), (0., 99.), (20., 12.), (2., 9.)] {
        let composition = resolve_vector_style_at(&style, &batches, &bindings, time, &ctx).unwrap();
        let DrawItem::Vector(item) = &composition.items[0] else {
            panic!("vector item");
        };
        assert!(matches!(&item.symbols[0], Symbol::Point(p) if p.size == Length::Pixels(expected)));
        assert_eq!(batches, original);
    }
    let mut conflict = map.items[0].clone();
    let mut duplicate = conflict.animation_bindings[0].clone();
    duplicate.id = "another-writer".into();
    conflict.animation_bindings.push(duplicate);
    assert!(prepare_style_animations(&style, &conflict, &animations, &ctx).is_err());
    let track = &mut animations.get_mut("demo:pulse").unwrap().tracks[0];
    track.parameter = "opacity".into();
    track.unit = None;
    assert!(prepare_style_animations(&style, &map.items[0], &animations, &ctx).is_err());
    let track = &mut animations.get_mut("demo:pulse").unwrap().tracks[0];
    track.parameter = "text-layout".into();
    assert!(prepare_style_animations(&style, &map.items[0], &animations, &ctx).is_err());
}
