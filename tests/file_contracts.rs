//! Generated on-disk fixtures exercise third-party readers, not just memory types.
#![cfg(any(feature = "read-shapefile", feature = "read-geotiff"))]
use geello::*;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "geello-m2-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn context() -> OperationContext {
    OperationContext::new(
        OperationLimits {
            max_live_bytes: 64 << 20,
            max_work: 1_000_000,
            max_commands: 1000,
            max_diagnostics: 8,
        },
        None,
    )
}
#[cfg(feature = "read-shapefile")]
mod shp {
    use super::*;
    use geello::readers::*;
    fn opts() -> VectorReadOptions {
        VectorReadOptions {
            data_version: "fixture-v1".into(),
            crs_override: Some(Crs::new("local", CoordinateSpace::Planar).unwrap()),
            limits: ReaderLimits {
                max_source_bytes: 1 << 20,
                max_record_bytes: 1 << 16,
                max_features: 100,
                max_coordinates_per_feature: 1000,
                max_depth: 32,
                batch_features: 2,
                batch_bytes: 1 << 20,
            },
            policy: ReadPolicy::Strict,
            identity: IdentityPolicy::OriginalOrOrdinal,
            queryables: vec![],
        }
    }
    fn fixture() -> Fixture {
        let f = Fixture::new();
        let path = f.0.join("points.shp");
        let builder = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("name".try_into().unwrap(), 20)
            .add_numeric_field("number".try_into().unwrap(), 20, 0);
        let mut w = shapefile::Writer::from_path(&path, builder).unwrap();
        let mut rec = shapefile::dbase::Record::default();
        rec.insert(
            "name".into(),
            shapefile::dbase::FieldValue::Character(Some("station".into())),
        );
        rec.insert(
            "number".into(),
            shapefile::dbase::FieldValue::Numeric(Some(42.)),
        );
        w.write_shape_and_record(&shapefile::PointZ::new(2., 3., 8., 9.), &rec)
            .unwrap();
        drop(w);
        std::fs::write(f.0.join("points.cpg"), "UTF-8").unwrap();
        f
    }
    #[test]
    fn z_m_dbf_integer_and_path_registration_are_preserved() {
        let f = fixture();
        let ctx = context();
        let s = open_shapefile(f.0.join("points.shp"), &opts(), &ctx).unwrap();
        let b = s
            .query(VectorQuery::default())
            .unwrap()
            .next_batch(&ctx)
            .unwrap()
            .unwrap();
        assert_eq!(b.features[0].source_ordinates[0].trailing, vec![8., 9.]);
        assert_eq!(b.features[0].properties["number"], PropertyValue::I64(42));
        assert_eq!(
            b.features[0].properties["name"],
            PropertyValue::String("station".into())
        );
        #[cfg(feature = "render-cpu")]
        {
            let c = MapComposition {
                items: vec![DrawItem::Vector(VectorItem {
                    batches: vec![Arc::new((*b).clone())],
                    symbols: vec![Symbol::Point(PointStyle::default())],
                })],
                background: None,
            };
            let view = View {
                crs: s.metadata().crs.clone(),
                bounds: Bounds {
                    min_x: 0.,
                    min_y: 0.,
                    max_x: 4.,
                    max_y: 4.,
                },
                width: 32,
                height: 32,
                aspect: AspectPolicy::Stretch,
            };
            let scene = prepare_scene(&c, &view, &ctx).unwrap();
            assert!(
                CpuRenderer::new(0)
                    .render(&scene, &ctx)
                    .unwrap()
                    .pixels()
                    .chunks_exact(4)
                    .any(|p| p[3] > 0)
            );
        }
    }
    #[test]
    fn gbk_is_strict_and_sidecar_count_corruption_is_rejected() {
        let f = fixture();
        let path = f.0.join("points.dbf");
        let mut dbf = std::fs::read(&path).unwrap();
        let offset = u16::from_le_bytes(dbf[8..10].try_into().unwrap()) as usize + 1;
        dbf[offset..offset + 20].fill(b' ');
        dbf[offset..offset + 4].copy_from_slice(&[0xd6, 0xd0, 0xce, 0xc4]);
        std::fs::write(&path, &dbf).unwrap();
        std::fs::write(f.0.join("points.cpg"), "GBK").unwrap();
        let s = open_shapefile(f.0.join("points.shp"), &opts(), &context()).unwrap();
        assert_eq!(
            s.query(VectorQuery::default())
                .unwrap()
                .next_batch(&context())
                .unwrap()
                .unwrap()
                .features[0]
                .properties["name"],
            PropertyValue::String("中文".into())
        );
        dbf[offset] = 0xff;
        std::fs::write(&path, &dbf).unwrap();
        assert!(open_shapefile(f.0.join("points.shp"), &opts(), &context()).is_err());
        dbf[4..8].copy_from_slice(&2u32.to_le_bytes());
        std::fs::write(&path, &dbf).unwrap();
        assert!(open_shapefile(f.0.join("points.shp"), &opts(), &context()).is_err());
    }
    #[test]
    fn absent_optional_measure_is_not_synthesized_and_shx_mismatch_fails() {
        let f = fixture();
        let path = f.0.join("points.shp");
        let mut shp = std::fs::read(&path).unwrap();
        shp.truncate(shp.len() - 8);
        let words = (shp.len() / 2) as i32;
        shp[24..28].copy_from_slice(&words.to_be_bytes());
        shp[104..108].copy_from_slice(&14i32.to_be_bytes());
        std::fs::write(&path, &shp).unwrap();
        let index = f.0.join("points.shx");
        let mut shx = std::fs::read(&index).unwrap();
        assert!(open_shapefile(&path, &opts(), &context()).is_err());
        shx[104..108].copy_from_slice(&14i32.to_be_bytes());
        std::fs::write(&index, &shx).unwrap();
        let s = open_shapefile(&path, &opts(), &context()).unwrap();
        let batch = s
            .query(VectorQuery::default())
            .unwrap()
            .next_batch(&context())
            .unwrap()
            .unwrap();
        assert_eq!(batch.features[0].source_ordinates[0].trailing, vec![8.]);
    }
}
#[cfg(feature = "read-geotiff")]
mod raster {
    use super::*;
    use geello::readers::*;
    use tiff::{
        encoder::{TiffEncoder, colortype},
        tags::Tag,
    };
    fn opts() -> RasterReadOptions {
        RasterReadOptions {
            crs_override: Some(Crs::new("local", CoordinateSpace::Planar).unwrap()),
            primary_ifd: None,
            max_source_bytes: 1 << 20,
            max_decode_bytes: 1 << 20,
            max_tag_bytes: 1 << 16,
        }
    }
    fn persist_fixture(name: &str, bytes: &[u8]) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/test/generated/m2-tiff");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(name), bytes).unwrap();
    }
    fn gray_fixture<C: colortype::ColorType>(
        values: &[C::Inner],
        compression: tiff::encoder::Compression,
        predictor: tiff::encoder::Predictor,
        white: bool,
    ) -> Vec<u8>
    where
        [C::Inner]: tiff::encoder::TiffValue,
    {
        let mut stream = std::io::Cursor::new(vec![]);
        {
            let mut encoder = TiffEncoder::new(&mut stream)
                .unwrap()
                .with_compression(compression)
                .with_predictor(predictor);
            let mut image = encoder.new_image::<C>(values.len() as u32, 1).unwrap();
            image
                .encoder()
                .write_tag(Tag::ModelPixelScaleTag, &[1f64, 1., 0.][..])
                .unwrap();
            image
                .encoder()
                .write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., 0., 1., 0.][..])
                .unwrap();
            if white {
                image
                    .encoder()
                    .write_tag(Tag::PhotometricInterpretation, 0u16)
                    .unwrap();
            }
            image.write_data(values).unwrap();
        }
        stream.into_inner()
    }
    #[test]
    fn numeric_matrix_and_lossless_codecs_keep_native_samples() {
        use tiff::encoder::{Compression, DeflateLevel, Predictor};
        macro_rules! check {
            ($color:ty, $variant:ident, $values:expr, $integer:expr) => {{
                let values = $values;
                for (codec, compression) in [
                    ("raw", Compression::Uncompressed),
                    ("packbits", Compression::Packbits),
                    ("lzw", Compression::Lzw),
                    ("deflate", Compression::Deflate(DeflateLevel::default())),
                ] {
                    for horizontal in [false, true] {
                        if horizontal && !$integer { continue; }
                        let bytes = gray_fixture::<$color>(&values, compression,
                            if horizontal { Predictor::Horizontal } else { Predictor::None }, false);
                        persist_fixture(&format!("{}-{codec}-{}.tif", stringify!($variant), if horizontal { "horizontal" } else { "none" }), &bytes);
                        let ctx = context();
                        let snapshot = read_geotiff(bytes.as_slice(), &opts(), &ctx).unwrap();
                        let block = snapshot.read_native_window(NativeWindow { column: 0, row: 0, width: values.len() as u32, height: 1 }, &ctx).unwrap();
                        assert!(matches!(&block.bands[0].samples, RasterSamples::$variant(v) if v.as_ref() == values), "{} {codec} predictor={horizontal}", stringify!($variant));
                        drop(snapshot); drop(block);
                        assert_eq!(ctx.live_bytes(), 0);
                    }
                }
            }};
        }
        check!(colortype::Gray8, U8, [0u8, 1, u8::MAX], true);
        check!(colortype::Gray16, U16, [0u16, 1, u16::MAX], true);
        check!(colortype::Gray32, U32, [0u32, 1, u32::MAX], true);
        check!(
            colortype::Gray64,
            U64,
            [0u64, (1 << 53) + 1, u64::MAX],
            true
        );
        check!(colortype::GrayI8, I8, [i8::MIN, 0, i8::MAX], true);
        check!(colortype::GrayI16, I16, [i16::MIN, 0, i16::MAX], true);
        check!(colortype::GrayI32, I32, [i32::MIN, 0, i32::MAX], true);
        check!(
            colortype::GrayI64,
            I64,
            [i64::MIN, -(1 << 53) - 1, i64::MAX],
            true
        );
        check!(colortype::Gray32Float, F32, [-1.25f32, 0., f32::MAX], false);
        check!(colortype::Gray64Float, F64, [-1.25f64, 0., f64::MAX], false);
    }
    #[test]
    fn white_float_bit_patterns_and_half_samples_survive_raw_reading() {
        use tiff::encoder::{Compression, Predictor};
        let values = [
            -0f64,
            f64::from_bits(1),
            1e-30,
            f64::INFINITY,
            f64::from_bits(0x7ff8000000000123),
        ];
        let bytes = gray_fixture::<colortype::Gray64Float>(
            &values,
            Compression::Uncompressed,
            Predictor::None,
            true,
        );
        persist_fixture("white-f64-bits.tif", &bytes);
        let ctx = context();
        let snapshot = read_geotiff(bytes.as_slice(), &opts(), &ctx).unwrap();
        assert!(snapshot.metadata().white_is_zero);
        let block = snapshot
            .read_native_window(
                NativeWindow {
                    column: 0,
                    row: 0,
                    width: 5,
                    height: 1,
                },
                &ctx,
            )
            .unwrap();
        match &block.bands[0].samples {
            RasterSamples::F64(v) => assert_eq!(
                v.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                values.map(f64::to_bits)
            ),
            _ => panic!("wrong native type"),
        }
        let mut bytes = gray_fixture::<colortype::Gray16>(
            &[0x0000, 0x3c00, 0xc000, 0x0001, 0x7c00, 0x7e00],
            Compression::Uncompressed,
            Predictor::None,
            false,
        );
        // Encode IEEE binary16 bits independently of the decoder's half type.
        let offset = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let count = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
        for i in 0..count {
            let p = offset + 2 + i * 12;
            if u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) == 339 {
                bytes[p + 8..p + 10].copy_from_slice(&3u16.to_le_bytes());
            }
        }
        persist_fixture("half-float.tif", &bytes);
        let s = read_geotiff(bytes.as_slice(), &opts(), &ctx).unwrap();
        let b = s
            .read_native_window(
                NativeWindow {
                    column: 0,
                    row: 0,
                    width: 6,
                    height: 1,
                },
                &ctx,
            )
            .unwrap();
        match &b.bands[0].samples {
            RasterSamples::F32(v) => {
                assert_eq!(&v[..4], &[0., 1., -2., 2f32.powi(-24)]);
                assert!(v[4].is_infinite());
                assert!(v[5].is_nan());
            }
            _ => panic!("half samples must be promoted to f32"),
        }
    }
    #[test]
    fn projected_geokeys_never_fall_back_to_geographic_base_and_domain_is_checked() {
        fn keyed(model: u16, projected: Option<u16>, origin: f64) -> Vec<u8> {
            let mut stream = std::io::Cursor::new(vec![]);
            {
                let mut encoder = TiffEncoder::new(&mut stream).unwrap();
                let mut image = encoder.new_image::<colortype::Gray8>(2, 1).unwrap();
                image
                    .encoder()
                    .write_tag(Tag::ModelPixelScaleTag, &[1f64, 1., 0.][..])
                    .unwrap();
                image
                    .encoder()
                    .write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., origin, 1., 0.][..])
                    .unwrap();
                let mut keys = vec![
                    1u16,
                    1,
                    0,
                    2 + u16::from(projected.is_some()),
                    1024,
                    0,
                    1,
                    model,
                    2048,
                    0,
                    1,
                    4326,
                ];
                if let Some(code) = projected {
                    keys.extend([3072, 0, 1, code]);
                }
                image
                    .encoder()
                    .write_tag(Tag::GeoKeyDirectoryTag, keys.as_slice())
                    .unwrap();
                image.write_data(&[1, 2]).unwrap();
            }
            stream.into_inner()
        }
        let mut o = opts();
        o.crs_override = None;
        let ctx = context();
        for model in [1, 32767] {
            assert!(read_geotiff(keyed(model, Some(32767), 0.).as_slice(), &o, &ctx).is_err());
        }
        let s = read_geotiff(keyed(1, Some(3857), 13_000_000.).as_slice(), &o, &ctx).unwrap();
        assert_eq!(s.metadata().grid.crs.definition(), "EPSG:3857");
        assert!(read_geotiff(keyed(2, None, 13_000_000.).as_slice(), &o, &ctx).is_err());
        read_geotiff(keyed(2, None, 100.).as_slice(), &o, &ctx).unwrap();
        read_geotiff(
            keyed(32767, Some(32767), 13_000_000.).as_slice(),
            &opts(),
            &ctx,
        )
        .unwrap();
    }
    fn fixture(mask: bool, white: bool) -> Vec<u8> {
        let mut stream = std::io::Cursor::new(vec![]);
        {
            let mut encoder = TiffEncoder::new(&mut stream).unwrap();
            let mut image = encoder.new_image::<colortype::Gray16>(4, 3).unwrap();
            image.rows_per_strip(1).unwrap();
            image
                .encoder()
                .write_tag(Tag::ModelPixelScaleTag, &[2f64, 3., 0.][..])
                .unwrap();
            image
                .encoder()
                .write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., 100., 200., 0.][..])
                .unwrap();
            image
                .encoder()
                .write_tag(
                    Tag::GeoKeyDirectoryTag,
                    &[1u16, 1, 0, 2, 1025, 0, 1, 1, 2048, 0, 1, 4326][..],
                )
                .unwrap();
            if mask {
                image
                    .encoder()
                    .write_tag(Tag::NewSubfileType, 4u32)
                    .unwrap();
            }
            if white {
                image
                    .encoder()
                    .write_tag(Tag::PhotometricInterpretation, 0u16)
                    .unwrap();
            }
            image.encoder().write_tag(Tag::GdalNodata, "99").unwrap();
            image
                .write_data(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 99])
                .unwrap();
        }
        stream.into_inner()
    }
    /// Minimal independently framed uncompressed TIFF fixture. Planar RGB and
    /// tile layouts cannot be produced by the high-level encoder used above.
    fn layout_fixture(planar: bool, tiled: bool) -> Vec<u8> {
        let width = if tiled { 16u32 } else { 2 };
        let height = if tiled { 16u32 } else { 2 };
        let n = (width * height) as usize;
        let mut bytes = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        let mut tags: Vec<(u16, u16, u32, Vec<u8>)> = vec![];
        let short = |v: u16| v.to_le_bytes().to_vec();
        let long = |v: u32| v.to_le_bytes().to_vec();
        tags.extend([
            (256, 4, 1, long(width)),
            (257, 4, 1, long(height)),
            (258, 3, 3, [short(8), short(8), short(8)].concat()),
            (259, 3, 1, short(1)),
            (262, 3, 1, short(2)),
            (277, 3, 1, short(3)),
            (284, 3, 1, short(if planar { 2 } else { 1 })),
            (
                33550,
                12,
                3,
                [1f64, 1., 0.]
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect(),
            ),
            (
                33922,
                12,
                6,
                [0f64, 0., 0., 0., 16., 0.]
                    .iter()
                    .flat_map(|n| n.to_le_bytes())
                    .collect(),
            ),
        ]);
        let count = if planar { 3 } else { 1 };
        if tiled {
            tags.extend([
                (322, 4, 1, long(16)),
                (323, 4, 1, long(16)),
                (324, 4, count, vec![0; count as usize * 4]),
                (
                    325,
                    4,
                    count,
                    (0..count)
                        .flat_map(|_| long(if planar { n as u32 } else { n as u32 * 3 }))
                        .collect(),
                ),
            ]);
        } else {
            tags.extend([
                (273, 4, count, vec![0; count as usize * 4]),
                (278, 4, 1, long(height)),
                (
                    279,
                    4,
                    count,
                    (0..count)
                        .flat_map(|_| long(if planar { n as u32 } else { n as u32 * 3 }))
                        .collect(),
                ),
            ]);
        }
        tags.sort_by_key(|v| v.0);
        let directory_end = 8 + 2 + tags.len() * 12 + 4;
        let extra: usize = tags
            .iter()
            .filter(|v| v.3.len() > 4)
            .map(|v| v.3.len())
            .sum();
        let pixel_start = directory_end + extra;
        let offsets: Vec<u8> = (0..count)
            .flat_map(|i| long((pixel_start + i as usize * n) as u32))
            .collect();
        tags.iter_mut()
            .find(|v| v.0 == if tiled { 324 } else { 273 })
            .unwrap()
            .3 = offsets;
        bytes.extend_from_slice(&(tags.len() as u16).to_le_bytes());
        let mut extras = vec![];
        for (tag, kind, count, data) in tags {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&kind.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            if data.len() > 4 {
                bytes.extend_from_slice(&((directory_end + extras.len()) as u32).to_le_bytes());
                extras.extend(data);
            } else {
                let mut inline = [0; 4];
                inline[..data.len()].copy_from_slice(&data);
                bytes.extend_from_slice(&inline);
            }
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend(extras);
        if planar {
            for channel in 0..3 {
                bytes.extend(std::iter::repeat_n((channel + 1) as u8 * 30, n));
            }
        } else {
            for _ in 0..n {
                bytes.extend([30, 60, 90]);
            }
        }
        bytes
    }
    #[test]
    fn planar_and_interleaved_tiles_and_strips_decode_real_channel_values() {
        for planar in [false, true] {
            for tiled in [false, true] {
                let ctx = context();
                let s =
                    read_geotiff(layout_fixture(planar, tiled).as_slice(), &opts(), &ctx).unwrap();
                let b = s
                    .read_native_window(
                        NativeWindow {
                            column: 1,
                            row: 1,
                            width: 1,
                            height: 1,
                        },
                        &ctx,
                    )
                    .unwrap();
                for (i, band) in b.bands.iter().enumerate() {
                    assert!(
                        matches!(&band.samples,RasterSamples::U8(v) if v.as_ref()==[(i+1) as u8*30]),
                        "planar={planar}, tiled={tiled}"
                    );
                }
            }
        }
    }
    #[test]
    fn bigtiff_keeps_u64_precision_and_primary_selection_is_explicit() {
        let mut stream = std::io::Cursor::new(vec![]);
        {
            let mut encoder = TiffEncoder::new_big(&mut stream).unwrap();
            for values in [[u64::MAX, u64::MAX - 1], [4, 5]] {
                let mut image = encoder.new_image::<colortype::Gray64>(2, 1).unwrap();
                image
                    .encoder()
                    .write_tag(Tag::ModelPixelScaleTag, &[1f64, 1., 0.][..])
                    .unwrap();
                image
                    .encoder()
                    .write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., 0., 1., 0.][..])
                    .unwrap();
                image.write_data(&values).unwrap();
            }
        }
        let ctx = context();
        let bytes = stream.into_inner();
        assert!(read_geotiff(bytes.as_slice(), &opts(), &ctx).is_err());
        let mut o = opts();
        o.primary_ifd = Some(0);
        let s = read_geotiff(bytes.as_slice(), &o, &ctx).unwrap();
        let b = s
            .read_native_window(
                NativeWindow {
                    column: 0,
                    row: 0,
                    width: 2,
                    height: 1,
                },
                &ctx,
            )
            .unwrap();
        assert!(
            matches!(&b.bands[0].samples,RasterSamples::U64(v) if v.as_ref()==[u64::MAX,u64::MAX-1])
        );
    }
    #[test]
    fn associated_alpha_preserves_raw_values_and_unassociates_only_for_display() {
        use geello::raster_processing::*;
        let mut stream = std::io::Cursor::new(vec![]);
        {
            let mut encoder = TiffEncoder::new(&mut stream).unwrap();
            let mut image = encoder.new_image::<colortype::RGBA8>(2, 1).unwrap();
            image
                .encoder()
                .write_tag(Tag::ExtraSamples, &[1u16][..])
                .unwrap();
            image
                .encoder()
                .write_tag(Tag::ModelPixelScaleTag, &[1f64, 1., 0.][..])
                .unwrap();
            image
                .encoder()
                .write_tag(Tag::ModelTiepointTag, &[0f64, 0., 0., 0., 1., 0.][..])
                .unwrap();
            image.encoder().write_tag(Tag::GdalNodata, "99").unwrap();
            image.write_data(&[50, 25, 0, 128, 99, 0, 0, 128]).unwrap();
        }
        let ctx = context();
        let snapshot = read_geotiff(stream.into_inner().as_slice(), &opts(), &ctx).unwrap();
        let block = snapshot
            .read_native_window(
                NativeWindow {
                    column: 0,
                    row: 0,
                    width: 2,
                    height: 1,
                },
                &ctx,
            )
            .unwrap();
        assert_eq!(block.alpha_association, AlphaAssociation::Premultiplied);
        assert!(matches!(&block.bands[0].samples,RasterSamples::U8(v) if v.as_ref()==[50,99]));
        let colors = RasterColorOptions {
            bands: vec![0, 1, 2],
            ranges: vec![SampleRange { min: 0., max: 255. }; 3],
            opacity: 1.,
        };
        let output = colorize(&block, &colors, &ctx).unwrap();
        assert_eq!(output.image().pixels(), &[100, 50, 0, 128, 0, 0, 0, 0]);
        let nearest = resample(
            &block,
            2,
            1,
            block.grid.clone(),
            RasterKind::Continuous,
            Resampling::Nearest,
            Ok,
            &ctx,
        )
        .unwrap();
        assert_eq!(nearest.alpha_association, AlphaAssociation::Premultiplied);
        assert_eq!(
            colorize(&nearest, &colors, &ctx).unwrap().image(),
            output.image()
        );
    }
    #[test]
    fn native_chunk_window_keeps_georeference_type_nodata_and_survives_source() {
        let ctx = context();
        let bytes = fixture(false, false);
        let snapshot = read_geotiff(bytes.as_slice(), &opts(), &ctx).unwrap();
        assert_eq!(
            snapshot.metadata().grid.affine,
            [100., 2., 0., 200., 0., -3.]
        );
        let block = snapshot
            .read_native_window(
                NativeWindow {
                    column: 1,
                    row: 1,
                    width: 2,
                    height: 2,
                },
                &ctx,
            )
            .unwrap();
        assert_eq!(block.grid.affine, [102., 2., 0., 197., 0., -3.]);
        assert!(matches!(&block.bands[0].samples,RasterSamples::U16(v)if v.as_ref()==[5,6,9,10]));
        assert_eq!(block.bands[0].no_data, Some(PropertySample::U64(99)));
        drop(snapshot);
        assert!(ctx.live_bytes() > 0);
        drop(block);
        assert_eq!(ctx.live_bytes(), 0);
    }
    #[test]
    fn white_raw_recovery_masks_and_decode_limits() {
        let ctx = context();
        let bytes = fixture(false, true);
        let snapshot = read_geotiff(bytes.as_slice(), &opts(), &ctx).unwrap();
        let block = snapshot
            .read_native_window(
                NativeWindow {
                    column: 0,
                    row: 0,
                    width: 4,
                    height: 3,
                },
                &ctx,
            )
            .unwrap();
        assert!(matches!(&block.bands[0].samples,RasterSamples::U16(v)if v[0]==0&&v[11]==99));
        assert!(matches!(
            read_geotiff(fixture(true, false).as_slice(), &opts(), &ctx),
            Err(GeelloError::Unsupported(_))
        ));
        let mut o = opts();
        o.max_decode_bytes = 2;
        assert!(read_geotiff(fixture(false, false).as_slice(), &o, &ctx).is_err());
    }
    #[cfg(feature = "render-cpu")]
    #[test]
    fn tiff_colorized_pixels_render_through_shared_scene() {
        use geello::raster_processing::*;
        let ctx = context();
        let s = read_geotiff(fixture(false, false).as_slice(), &opts(), &ctx).unwrap();
        let b = s
            .read_native_window(
                NativeWindow {
                    column: 0,
                    row: 0,
                    width: 4,
                    height: 3,
                },
                &ctx,
            )
            .unwrap();
        let image = colorize(
            &b,
            &RasterColorOptions {
                bands: vec![0],
                ranges: vec![SampleRange { min: 0., max: 10. }],
                opacity: 1.,
            },
            &ctx,
        )
        .unwrap();
        let crs = b.grid.crs.clone();
        let bounds = Bounds {
            min_x: 100.,
            min_y: 191.,
            max_x: 108.,
            max_y: 200.,
        };
        let view = View {
            crs: crs.clone(),
            bounds,
            width: 32,
            height: 32,
            aspect: AspectPolicy::Stretch,
        };
        let c = MapComposition {
            items: vec![DrawItem::Image(ImagePlacement {
                image: Arc::new(image.image().clone()),
                crs,
                bounds,
                opacity: 1.,
            })],
            background: None,
        };
        let out = CpuRenderer::new(0)
            .render(&prepare_scene(&c, &view, &ctx).unwrap(), &ctx)
            .unwrap();
        assert!(out.pixels().chunks_exact(4).any(|p| p[3] == 255));
    }
}
