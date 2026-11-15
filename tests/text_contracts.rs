#![cfg(feature = "text")]
use geello::{text::*, *};
use geo_types::{Geometry, LineString, Point, Polygon};
use std::sync::Arc;
fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 128 * 1024 * 1024,
            max_work: 20_000_000,
            max_commands: 100_000,
            max_diagnostics: 16,
        },
        None,
    )
}
fn font(key: &str) -> Arc<FontResource> {
    let bytes: &[u8] = match key {
        "latin" => include_bytes!("fixtures/fonts/latin.ttf"),
        "arabic" => include_bytes!("fixtures/fonts/arabic.ttf"),
        "indic" => include_bytes!("fixtures/fonts/indic.ttf"),
        _ => include_bytes!("fixtures/fonts/cjk.otf"),
    };
    Arc::new(FontResource {
        identity: key.into(),
        bytes: Arc::from(bytes),
        face_index: 0,
    })
}
fn style() -> TextStyle {
    TextStyle {
        fonts: vec![font("latin"), font("arabic"), font("indic"), font("cjk")],
        size: Length::Pixels(20.),
        color: Color::BLACK,
        language: "und".into(),
    }
}
fn view() -> View {
    View {
        crs: Crs::new("local:xy", CoordinateSpace::Planar).unwrap(),
        bounds: Bounds {
            min_x: 0.,
            min_y: 0.,
            max_x: 512.,
            max_y: 256.,
        },
        width: 512,
        height: 256,
        aspect: AspectPolicy::Stretch,
    }
}
fn label(text: &str, geometry: Geometry, placement: LabelPlacement) -> LabelItem {
    LabelItem {
        crs: view().crs,
        geometry,
        text: text.into(),
        style: style(),
        placement,
        priority: 0.,
        allow_overlap: false,
        wrap_width: None,
        repeat_distance: 300.,
        marker_source: None,
    }
}
fn scene(labels: Vec<LabelItem>, ctx: &OperationContext) -> PreparedScene {
    prepare_scene(
        &MapComposition {
            items: labels.into_iter().map(DrawItem::Label).collect(),
            background: None,
        },
        &view(),
        ctx,
    )
    .unwrap()
}

#[test]
fn unicode_shaping_bidi_clusters_fallback_and_stack_isolation() {
    let ctx = context();
    let latin = shape_text("office ffi", &style(), 20., None, &ctx).unwrap();
    assert_eq!(latin.missing_glyphs, 0);
    assert!(
        latin.glyphs.iter().any(|g| g.cluster.len() > 1),
        "ligature must retain a multi-byte cluster range"
    );
    let arabic = shape_text("مرحبا بالعالم", &style(), 20., None, &ctx).unwrap();
    assert_eq!(arabic.missing_glyphs, 0);
    assert!(
        arabic
            .glyphs
            .iter()
            .filter(|g| &"مرحبا بالعالم"[g.cluster.clone()] != " ")
            .all(|g| g.font_identity == "arabic")
    );
    assert!(
        arabic
            .glyphs
            .windows(2)
            .any(|g| g[0].cluster.start < g[1].cluster.start && g[0].origin[0] > g[1].origin[0]),
        "RTL visual order"
    );
    let indic = shape_text("नमस्ते हिन्दी", &style(), 20., None, &ctx).unwrap();
    assert_eq!(indic.missing_glyphs, 0);
    assert!(
        indic
            .glyphs
            .iter()
            .filter(|g| &"नमस्ते हिन्दी"[g.cluster.clone()] != " ")
            .all(|g| g.font_identity == "indic")
    );
    assert!(indic.glyphs.iter().any(|g| g.cluster.len() > 3));
    let cjk = shape_text("中国地图", &style(), 20., None, &ctx).unwrap();
    assert_eq!(cjk.missing_glyphs, 0);
    assert!(cjk.glyphs.iter().all(|g| g.font_identity == "cjk"));
    let mut isolated = style();
    isolated.fonts = vec![font("latin")];
    assert!(
        shape_text("中国地图", &isolated, 20., None, &ctx)
            .unwrap()
            .missing_glyphs
            > 0
    );
    isolated.language.clear();
    assert_eq!(
        shape_text("office", &isolated, 20., None, &ctx)
            .unwrap()
            .glyphs,
        shape_text(
            "office",
            &TextStyle {
                language: "und".into(),
                ..isolated.clone()
            },
            20.,
            None,
            &ctx
        )
        .unwrap()
        .glyphs
    );
    isolated.language = "ar-EG".into();
    assert!(shape_text("office", &isolated, 20., None, &ctx).is_ok());
    isolated.language = "invalid_tag!".into();
    assert!(shape_text("office", &isolated, 20., None, &ctx).is_err());
    let invalid = FontResource {
        identity: "bad-face".into(),
        bytes: font("latin").bytes.clone(),
        face_index: 1,
    };
    assert!(validate_font(&invalid).is_err());
    drop((latin, arabic, indic, cjk));
    assert_eq!(ctx.live_bytes(), 0);
}

#[test]
fn wrapping_collision_polygon_holes_and_scene_ownership() {
    let ctx = context();
    let wrapped = shape_text(
        "e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}e\u{301}",
        &style(),
        20.,
        Some(18.),
        &ctx,
    )
    .unwrap();
    assert!(wrapped.height > 40.);
    assert!(
        wrapped
            .glyphs
            .iter()
            .all(|g| g.cluster.start % 3 == 0 && g.cluster.end % 3 == 0)
    );
    drop(wrapped);
    let a = label(
        "China",
        Point::new(256., 128.).into(),
        LabelPlacement::Point,
    );
    let mut b = a.clone();
    b.style.color = Color::BLUE;
    let s = scene(vec![a.clone(), b.clone()], &ctx);
    assert_eq!(s.summary().command_count, 2);
    #[cfg(feature = "render-cpu")]
    {
        let pixels = CpuRenderer::new(1).render(&s, &ctx).unwrap();
        assert!(
            pixels
                .pixels()
                .chunks_exact(4)
                .any(|p| p[2] > 0 && p[0] == 0 && p[3] > 0)
        );
    }
    b.allow_overlap = true;
    assert_eq!(scene(vec![a.clone(), b], &ctx).summary().command_count, 2);
    let polygon = Polygon::new(
        LineString::from(vec![
            (0., 0.),
            (512., 0.),
            (512., 256.),
            (0., 256.),
            (0., 0.),
        ]),
        vec![LineString::from(vec![
            (1., 1.),
            (511., 1.),
            (511., 255.),
            (1., 255.),
            (1., 1.),
        ])],
    );
    assert_eq!(
        scene(
            vec![label("China", polygon.into(), LabelPlacement::Polygon)],
            &ctx
        )
        .summary()
        .command_count,
        0
    );
    let s = scene(vec![a], &ctx);
    let summary = s.summary().clone();
    assert!(summary.command_count > 0);
    assert_eq!(
        summary,
        scene(
            vec![label(
                "China",
                Point::new(256., 128.).into(),
                LabelPlacement::Point
            )],
            &context()
        )
        .summary()
        .clone()
    );
    drop(s);
}

#[test]
fn curved_repeat_midpoint_upright_and_reject_wrapping() {
    let ctx = context();
    let line = LineString::from(vec![(10., 128.), (502., 128.)]);
    let mut a = label("office", line.clone().into(), LabelPlacement::Curved);
    assert_eq!(scene(vec![a.clone()], &ctx).summary().command_count, 2);
    a.repeat_distance = 150.;
    assert_eq!(scene(vec![a.clone()], &ctx).summary().command_count, 3);
    a.repeat_distance = 300.;
    a.geometry = LineString::from(vec![(10., 128.), (200., 128.)]).into();
    assert_eq!(scene(vec![a.clone()], &ctx).summary().command_count, 1);
    a.geometry = LineString::from(vec![(200., 128.), (10., 128.)]).into();
    let reversed = scene(vec![a.clone()], &ctx);
    a.geometry = LineString::from(vec![(10., 128.), (200., 128.)]).into();
    let forward = scene(vec![a.clone()], &ctx);
    #[cfg(feature = "render-cpu")]
    {
        assert!(
            CpuRenderer::new(1)
                .render(&reversed, &ctx)
                .unwrap()
                .pixels()
                == CpuRenderer::new(1).render(&forward, &ctx).unwrap().pixels(),
            "reversed short line must retain upright layout"
        );
    }
    a.wrap_width = Some(50.);
    assert!(
        prepare_scene(
            &MapComposition {
                items: vec![DrawItem::Label(a)],
                background: None
            },
            &view(),
            &ctx
        )
        .is_err()
    );
}

#[test]
fn text_admission_and_cancellation_release_all_memory() {
    let ctx = OperationContext::new(
        OperationLimits {
            max_live_bytes: 1024,
            max_work: 1_000_000,
            max_commands: 100,
            max_diagnostics: 1,
        },
        None,
    );
    assert!(matches!(
        shape_text("office", &style(), 20., None, &ctx),
        Err(GeelloError::BudgetExceeded("memory"))
    ));
    assert_eq!(ctx.live_bytes(), 0);
    ctx.cancel();
    assert!(matches!(
        shape_text("office", &style(), 20., None, &ctx),
        Err(GeelloError::Cancelled)
    ));
}
