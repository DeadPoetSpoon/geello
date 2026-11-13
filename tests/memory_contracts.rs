use geello::*;
use geo_types::{Geometry, GeometryCollection, LineString, Point, Polygon};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Instant,
};
fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 64 * 1024 * 1024,
            max_work: 10_000_000,
            max_commands: 10_000,
            max_diagnostics: 8,
        },
        None,
    )
}
fn crs() -> Crs {
    Crs::new("local:xy", CoordinateSpace::Planar).unwrap()
}
fn view() -> View {
    View {
        crs: crs(),
        bounds: Bounds {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 32.0,
            max_y: 32.0,
        },
        width: 32,
        height: 32,
        aspect: AspectPolicy::Stretch,
    }
}
fn feature(g: Option<Geometry>, ordinal: u64) -> Feature {
    Feature {
        id: Some(FeatureId::U64(ordinal)),
        geometry: g,
        properties: BTreeMap::new(),
        source_ordinal: ordinal,
        source_ordinates: vec![],
    }
}
fn composition(features: Vec<Feature>, symbols: Vec<Symbol>) -> MapComposition {
    MapComposition {
        items: vec![DrawItem::Vector(VectorItem {
            batches: vec![Arc::new(VectorBatch {
                crs: crs(),
                features,
            })],
            symbols,
        })],
        background: None,
    }
}
fn line(points: &[(f64, f64)]) -> Geometry {
    Geometry::LineString(LineString::from(points.to_vec()))
}
fn polygon() -> Geometry {
    Geometry::Polygon(Polygon::new(
        LineString::from(vec![(2., 2.), (30., 2.), (30., 30.), (2., 30.), (2., 2.)]),
        vec![LineString::from(vec![
            (10., 10.),
            (22., 10.),
            (22., 22.),
            (10., 22.),
            (10., 10.),
        ])],
    ))
}
#[test]
fn properties_and_identity_are_lossless() {
    let text = r#"{"nested":[null,{"signed":-9223372036854775808,"unsigned":18446744073709551615,"float":1.25}],"null":null}"#;
    let p: Properties = serde_json::from_str(text).unwrap();
    assert!(p.contains_key("null"));
    assert!(!p.contains_key("missing"));
    assert_eq!(
        p,
        serde_json::from_str::<Properties>(&serde_json::to_string(&p).unwrap()).unwrap()
    );
    let mut set = HashSet::new();
    set.insert(FeatureId::I64(42));
    set.insert(FeatureId::U64(42));
    set.insert(FeatureId::String("42".into()));
    assert_eq!(set.len(), 2);
    assert_eq!(
        serde_json::from_str::<FeatureId>("18446744073709551615").unwrap(),
        FeatureId::U64(u64::MAX)
    );
}
#[test]
fn memory_budget_is_released_with_last_scene() {
    let ctx = context();
    let source = composition(
        vec![feature(Some(Geometry::Point(Point::new(16., 16.))), 0)],
        vec![Symbol::Point(PointStyle::default())],
    );
    let batch = match &source.items[0] {
        DrawItem::Vector(v) => Arc::downgrade(&v.batches[0]),
        _ => unreachable!(),
    };
    let s = prepare_scene(&source, &view(), &ctx).unwrap();
    drop(source);
    assert!(batch.upgrade().is_none());
    assert!(ctx.live_bytes() > 0);
    let copy = s.clone();
    drop(s);
    assert!(ctx.live_bytes() > 0);
    drop(copy);
    assert_eq!(ctx.live_bytes(), 0);
}
#[test]
fn failed_preparation_releases_reservations() {
    let ctx = OperationContext::new(
        OperationLimits {
            max_live_bytes: 1,
            max_work: 100,
            max_commands: 1,
            max_diagnostics: 0,
        },
        None,
    );
    let c = composition(
        vec![feature(Some(polygon()), 0)],
        vec![Symbol::PolygonFill {
            color: Color::GRAY,
            opacity: 1.0,
        }],
    );
    assert!(matches!(
        prepare_scene(&c, &view(), &ctx),
        Err(GeelloError::BudgetExceeded("memory"))
    ));
    assert_eq!(ctx.live_bytes(), 0);
}
#[test]
fn cancelled_empty_scene_and_deadline_fail() {
    let ctx = context();
    ctx.cancel();
    assert!(matches!(
        prepare_scene(&MapComposition::default(), &view(), &ctx),
        Err(GeelloError::Cancelled)
    ));
    let expired = OperationContext::new(context().limits(), Some(Instant::now()));
    assert_eq!(expired.check(), Err(GeelloError::DeadlineExceeded));
}
#[test]
fn budgets_do_not_wrap_or_charge_rejected_work() {
    let ctx = OperationContext::new(
        OperationLimits {
            max_live_bytes: 100,
            max_work: u64::MAX,
            max_commands: 0,
            max_diagnostics: 0,
        },
        None,
    );
    ctx.checkpoint(u64::MAX).unwrap();
    assert_eq!(ctx.checkpoint(1), Err(GeelloError::BudgetExceeded("work")));
    assert_eq!(ctx.work(), u64::MAX);
}
#[test]
fn views_reject_degenerate_and_excessive_dimensions() {
    let mut v = view();
    v.width = 65536;
    assert_eq!(v.validate(), Err(GeelloError::DimensionOverflow));
    v.width = 32;
    v.bounds.max_x = v.bounds.min_x;
    assert!(v.validate().is_err());
    v.bounds = v.bounds.expanded(2.).unwrap();
    assert!(v.validate().is_ok());
    v.bounds.min_x = f64::NAN;
    assert!(v.validate().is_err());
}
#[test]
fn mismatched_crs_requires_explicit_projection() {
    let mut v = view();
    v.crs = Crs::new("other", CoordinateSpace::Planar).unwrap();
    let c = composition(vec![], vec![]);
    assert!(matches!(
        prepare_scene(&c, &v, &context()),
        Err(GeelloError::ProjectionRequired)
    ));
}
#[test]
fn collections_keep_component_compatibility() {
    let c = composition(
        vec![feature(
            Some(Geometry::GeometryCollection(GeometryCollection(vec![
                Geometry::Point(Point::new(16., 16.)),
                line(&[(1., 1.), (20., 20.)]),
                polygon(),
            ]))),
            0,
        )],
        vec![Symbol::Point(PointStyle::default())],
    );
    let s = prepare_scene(&c, &view(), &context()).unwrap();
    assert_eq!(s.summary().command_count, 1);
    assert_eq!(s.summary().incompatible_components, 2);
}
#[test]
fn empty_and_zero_length_lines_are_safe() {
    let c = composition(
        vec![
            feature(None, 0),
            feature(Some(line(&[])), 1),
            feature(Some(line(&[(2., 2.), (2., 2.)])), 2),
        ],
        vec![Symbol::Line(StrokeStyle::default())],
    );
    assert_eq!(
        prepare_scene(&c, &view(), &context())
            .unwrap()
            .summary()
            .command_count,
        0
    );
}
#[test]
fn native_raster_keeps_integer_precision_grid_and_masks() {
    let b = RasterBlock {
        width: 1,
        height: 1,
        grid: RasterGrid {
            crs: crs(),
            affine: [10., 2., 0.5, 20., 0.25, -2.],
            interpretation: PixelInterpretation::Area,
        },
        bands: vec![
            RasterBand {
                samples: RasterSamples::U64(Arc::from([u64::MAX])),
                validity: Some(Arc::from([false])),
                no_data: Some(PropertySample::U64(u64::MAX)),
                scale: 1.,
                offset: 0.,
            },
            RasterBand {
                samples: RasterSamples::F32(Arc::from([f32::NAN])),
                validity: None,
                no_data: None,
                scale: 2.,
                offset: 1.,
            },
        ],
        alpha: Some(Arc::from([1.])),
    };
    b.validate().unwrap();
    assert!(matches!(
        b.view().block.bands[0].samples,
        RasterSamples::U64(_)
    ));
    let mut bad = b.clone();
    bad.grid.affine = [0.; 6];
    assert!(bad.validate().is_err());
}

#[cfg(feature = "render-cpu")]
mod cpu {
    use super::*;
    fn pixels(c: &MapComposition) -> RgbaImage {
        let s = prepare_scene(c, &view(), &context()).unwrap();
        CpuRenderer::new(0).render(&s, &context()).unwrap()
    }
    fn px(image: &RgbaImage, x: usize, y: usize) -> &[u8] {
        &image.pixels()
            [(y * image.width() as usize + x) * 4..(y * image.width() as usize + x + 1) * 4]
    }
    #[test]
    fn hole_is_transparent_and_source_can_be_released() {
        let c = composition(
            vec![feature(Some(polygon()), 0)],
            vec![Symbol::PolygonFill {
                color: Color([255, 0, 0, 128]),
                opacity: 1.0,
            }],
        );
        let s = prepare_scene(&c, &view(), &context()).unwrap();
        drop(c);
        let mut r = CpuRenderer::new(0);
        let a = r.render(&s, &context()).unwrap();
        assert_eq!(a, r.render(&s, &context()).unwrap());
        assert_eq!(px(&a, 16, 16), [0, 0, 0, 0]);
        assert_eq!(px(&a, 5, 5), [255, 0, 0, 128]);
    }
    #[test]
    fn open_line_has_no_closing_diagonal() {
        let c = composition(
            vec![feature(Some(line(&[(4., 4.), (28., 4.), (28., 28.)])), 0)],
            vec![Symbol::Line(StrokeStyle {
                width: Length::Pixels(2.),
                ..Default::default()
            })],
        );
        let a = pixels(&c);
        assert_eq!(px(&a, 16, 16), [0, 0, 0, 0]);
        assert!(a.pixels().chunks_exact(4).any(|p| p[3] > 0));
    }
    #[test]
    fn symbol_major_order_spans_batches() {
        let mut c = composition(
            vec![],
            vec![
                Symbol::Line(StrokeStyle {
                    color: Color([255, 0, 0, 255]),
                    width: Length::Pixels(6.),
                    ..Default::default()
                }),
                Symbol::Line(StrokeStyle {
                    color: Color([0, 0, 255, 255]),
                    width: Length::Pixels(2.),
                    ..Default::default()
                }),
            ],
        );
        if let DrawItem::Vector(v) = &mut c.items[0] {
            v.batches = vec![
                Arc::new(VectorBatch {
                    crs: crs(),
                    features: vec![feature(Some(line(&[(2., 16.), (30., 16.)])), 0)],
                }),
                Arc::new(VectorBatch {
                    crs: crs(),
                    features: vec![feature(Some(line(&[(16., 2.), (16., 30.)])), 1)],
                }),
            ];
        }
        let a = pixels(&c);
        assert_eq!(px(&a, 16, 16), [0, 0, 255, 255]);
    }
    #[test]
    fn image_alpha_and_source_over_are_straight() {
        let image = Arc::new(RgbaImage::from_rgba(1, 1, vec![255, 0, 0, 128]).unwrap());
        let c = MapComposition {
            items: vec![DrawItem::Image(ImagePlacement {
                image,
                crs: crs(),
                bounds: view().bounds,
                opacity: 0.5,
            })],
            background: Some(Color([0, 0, 255, 255])),
        };
        let a = pixels(&c);
        let p = px(&a, 16, 16);
        assert!(p[0].abs_diff(64) <= 1, "{p:?}");
        assert!(p[2].abs_diff(191) <= 1, "{p:?}");
        assert_eq!(p[3], 255);
    }
    #[test]
    fn fit_rebases_large_world_coordinates() {
        let mut v = view();
        v.bounds = Bounds {
            min_x: 1e12,
            min_y: 1e12,
            max_x: 1e12 + 32.,
            max_y: 1e12 + 16.,
        };
        v.aspect = AspectPolicy::Fit;
        let c = composition(
            vec![feature(
                Some(Geometry::Point(Point::new(1e12 + 16., 1e12 + 8.))),
                0,
            )],
            vec![Symbol::Point(PointStyle::default())],
        );
        let s = prepare_scene(&c, &v, &context()).unwrap();
        let a = CpuRenderer::new(0).render(&s, &context()).unwrap();
        assert_eq!(px(&a, 16, 16), [0, 0, 255, 255]);
    }
    #[test]
    fn transparent_pixels_and_threaded_reuse_are_canonical() {
        let c = composition(
            vec![feature(Some(polygon()), 0)],
            vec![Symbol::PolygonFill {
                color: Color([200, 100, 50, 0]),
                opacity: 1.0,
            }],
        );
        let s = prepare_scene(&c, &view(), &context()).unwrap();
        let a = CpuRenderer::new(0).render(&s, &context()).unwrap();
        assert!(a.pixels().iter().all(|p| *p == 0));
        assert_eq!(a, CpuRenderer::new(2).render(&s, &context()).unwrap());
    }
    #[test]
    fn cancelled_render_does_not_return_pixels() {
        let s = prepare_scene(&MapComposition::default(), &view(), &context()).unwrap();
        let ctx = context();
        ctx.cancel();
        assert!(matches!(
            CpuRenderer::new(0).render(&s, &ctx),
            Err(GeelloError::Cancelled)
        ));
    }
}
