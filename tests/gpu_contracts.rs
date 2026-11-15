#![cfg(feature = "render-gpu")]
use geello::*;
use geo_types::{Geometry, LineString, Point, Polygon};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};
fn run<F: Future>(f: F) -> F::Output {
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark()
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark()
        }
    }
    let w = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&w);
    let mut f = std::pin::pin!(f);
    loop {
        match f.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::park_timeout(Duration::from_millis(10)),
        }
    }
}
fn ctx() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 128 * 1024 * 1024,
            max_work: 1_000_000,
            max_commands: 1000,
            max_diagnostics: 8,
        },
        Some(std::time::Instant::now() + Duration::from_secs(30)),
    )
}
fn crs() -> Crs {
    Crs::new("local", CoordinateSpace::Planar).unwrap()
}
fn fixture() -> PreparedScene {
    let features = vec![
        Geometry::Polygon(Polygon::new(
            LineString::from(vec![(2., 2.), (28., 2.), (28., 28.), (2., 28.), (2., 2.)]),
            vec![LineString::from(vec![
                (8., 8.),
                (20., 8.),
                (20., 20.),
                (8., 20.),
                (8., 8.),
            ])],
        )),
        Geometry::LineString(LineString::from(vec![(1., 1.), (30., 1.), (30., 30.)])),
        Geometry::Point(Point::new(24., 24.)),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, g)| Feature {
        source_id: None,
        id: None,
        geometry: Some(g),
        properties: Default::default(),
        source_ordinal: i as u64,
        source_ordinates: vec![],
    })
    .collect();
    let c = MapComposition {
        items: vec![
            DrawItem::Vector(VectorItem {
                batches: vec![Arc::new(VectorBatch {
                    crs: crs(),
                    features,
                })],
                symbols: vec![
                    Symbol::PolygonFill {
                        color: Color([200, 100, 50, 128]),
                        opacity: 1.0,
                    },
                    Symbol::Line(StrokeStyle {
                        color: Color([0, 255, 0, 255]),
                        width: Length::Pixels(2.),
                        ..Default::default()
                    }),
                    Symbol::Point(PointStyle::default()),
                ],
            }),
            DrawItem::Image(ImagePlacement {
                image: Arc::new(RgbaImage::from_rgba(1, 1, vec![255, 0, 0, 128]).unwrap()),
                crs: crs(),
                bounds: Bounds {
                    min_x: 0.,
                    min_y: 28.,
                    max_x: 4.,
                    max_y: 32.,
                },
                opacity: 0.5,
            }),
        ],
        background: None,
    };
    prepare_scene(
        &c,
        &View {
            crs: crs(),
            bounds: Bounds {
                min_x: 0.,
                min_y: 0.,
                max_x: 32.,
                max_y: 32.,
            },
            width: 32,
            height: 32,
            aspect: AspectPolicy::Stretch,
        },
        &ctx(),
    )
    .unwrap()
}
fn texture(
    device: &wgpu::Device,
    w: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("contract target"),
        size: wgpu::Extent3d {
            width: w,
            height: 32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}
/// Run explicitly on a real adapter; absence is a failure, never a skipped parity claim.
#[test]
#[ignore = "requires a real GPU; run with --ignored --nocapture"]
fn real_gpu_contract() {
    run(async {
        let selected = match std::env::var("GEELLO_TEST_BACKEND").as_deref() {
            Ok("vulkan") => wgpu::Backends::VULKAN,
            _ => wgpu::Backends::DX12,
        };
        let (mut r, info) = GpuRenderer::new(selected, &ctx())
            .await
            .expect("real GPU required");
        println!("GPU evidence: {info:?}");
        let s = fixture();
        let a = r.render_pixels(&s, &ctx()).await.unwrap();
        assert_eq!(a, r.render_pixels(&s, &ctx()).await.unwrap());
        assert_eq!(
            &a.pixels()[(16 * 32 + 16) * 4..(16 * 32 + 16) * 4 + 4],
            &[0, 0, 0, 0]
        );
        #[cfg(feature = "render-cpu")]
        {
            let cpu = CpuRenderer::new(0).render(&s, &ctx()).unwrap();
            let mut max = 0u8;
            let mut sum = 0u64;
            let mut large = 0;
            for (x, y) in cpu.pixels().iter().zip(a.pixels()) {
                let d = x.abs_diff(*y);
                max = max.max(d);
                sum += d as u64;
                if d > 8 {
                    large += 1
                }
            }
            println!(
                "parity: max={max} mean={} channels_above_8={large}",
                sum as f64 / a.pixels().len() as f64
            );
            assert!(sum as f64 / (a.pixels().len() as f64) < 3.0);
            assert!(large < 100);
        }
        let valid = texture(
            r.device(),
            32,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        #[cfg(all(feature = "text", feature = "render-cpu"))]
        {
            let fonts = [
                (
                    "latin",
                    include_bytes!("fixtures/fonts/latin.ttf").as_slice(),
                ),
                (
                    "arabic",
                    include_bytes!("fixtures/fonts/arabic.ttf").as_slice(),
                ),
                (
                    "indic",
                    include_bytes!("fixtures/fonts/indic.ttf").as_slice(),
                ),
                ("cjk", include_bytes!("fixtures/fonts/cjk.otf").as_slice()),
            ]
            .into_iter()
            .map(|(identity, bytes)| {
                Arc::new(FontResource {
                    identity: identity.into(),
                    bytes: Arc::from(bytes),
                    face_index: 0,
                })
            })
            .collect();
            let text_scene = prepare_scene(
                &MapComposition {
                    items: vec![DrawItem::Label(text::LabelItem {
                        crs: crs(),
                        geometry: Point::new(4., 16.).into(),
                        text: "office العربية हिन्दी 中国地图".into(),
                        style: TextStyle {
                            fonts,
                            size: Length::Pixels(18.),
                            color: Color::BLACK,
                            language: "und".into(),
                        },
                        placement: text::LabelPlacement::Point,
                        priority: 0.,
                        allow_overlap: false,
                        wrap_width: None,
                        repeat_distance: 300.,
                        marker_source: None,
                    })],
                    background: None,
                },
                &View {
                    crs: crs(),
                    bounds: Bounds {
                        min_x: 0.,
                        min_y: 0.,
                        max_x: 512.,
                        max_y: 64.,
                    },
                    width: 512,
                    height: 64,
                    aspect: AspectPolicy::Stretch,
                },
                &ctx(),
            )
            .unwrap();
            let cpu = CpuRenderer::new(1).render(&text_scene, &ctx()).unwrap();
            let gpu = r.render_pixels(&text_scene, &ctx()).await.unwrap();
            assert!(cpu.pixels().chunks_exact(4).filter(|p| p[3] > 0).count() > 100);
            let sum: u64 = cpu
                .pixels()
                .iter()
                .zip(gpu.pixels())
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum();
            let mean = sum as f64 / gpu.pixels().len() as f64;
            println!("M4 multilingual outline CPU/GPU mean channel delta={mean}");
            assert!(mean < 3.0);
        }
        r.render_to_target(&s, &ExternalTarget { texture: &valid }, &ctx())
            .await
            .unwrap();
        for wrong in [
            texture(
                r.device(),
                16,
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            texture(
                r.device(),
                32,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            ),
            texture(
                r.device(),
                32,
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        ] {
            assert!(matches!(
                r.render_to_target(&s, &ExternalTarget { texture: &wrong }, &ctx())
                    .await,
                Err(GeelloError::InvalidTarget(_))
            ));
        }
        let other_instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let other_adapter = other_instance
            .request_adapter(&Default::default())
            .await
            .unwrap();
        let (other_device, _) = other_adapter
            .request_device(&Default::default())
            .await
            .unwrap();
        let other = texture(
            &other_device,
            32,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        assert!(matches!(
            r.render_to_target(&s, &ExternalTarget { texture: &other }, &ctx())
                .await,
            Err(GeelloError::InvalidTarget(_))
        ));
        let cancelled = ctx();
        cancelled.cancel();
        assert!(matches!(
            r.render_to_target(&s, &ExternalTarget { texture: &valid }, &cancelled)
                .await,
            Err(GeelloError::Cancelled)
        ));
    });
}
