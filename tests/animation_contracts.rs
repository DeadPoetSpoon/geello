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
