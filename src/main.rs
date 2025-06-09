pub mod rendered_geometry;
pub use rendered_geometry::*;
pub mod renderer;
use anyhow::{anyhow, bail};
use geo::{Area, BooleanOps, MultiPolygon, Rect, haversine_length};
use geojson::{Bbox, FeatureCollection, GeoJson};
pub use renderer::*;
use skrifa::{FontRef, MetadataProvider};
use std::{fs::File, io::Read, num::NonZeroUsize, path::Path, sync::Arc};
use vello::{
    Glyph, RendererOptions,
    kurbo::{Affine, BezPath, Circle, PathEl, Point, Stroke, Vec2},
    peniko::{Blob, Color, Fill, Font, color::palette},
    util::{RenderContext, block_on_wgpu},
    wgpu::{
        self, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d,
        TexelCopyBufferInfo, TextureDescriptor, TextureFormat, TextureUsages,
    },
};
const FS_FONT: &[u8] = include_bytes!("../assets/test/fsong_gb2312.ttf");
#[pollster::main]
async fn main() -> anyhow::Result<()> {
    // let rect = Rect::new((72.502355, 14.986898), (136.08722, 54.563624));
    // let rect = Rect::new((72.502355, 14.986898), (136.08722, 54.563624));
    let rect = Rect::new((83.221730391, 40.605857134), (90.358539000, 44.310998810));
    let (width_size, height_size) = (rect.width(), rect.height());
    let size_per_pixel = 0.00124f64;
    let (width, height) = (
        width_size / size_per_pixel + 1.,
        height_size / size_per_pixel + 1.,
    );
    let (width, height) = (width as u32, height as u32);
    // let all_center = (bbox[0]/2f64+bbox[2]/2f64,bbox[1]/2f64+bbox[3]/2f64);

    let mut file = File::open("assets/test/polygon.geojson").unwrap();
    let mut geojson_str = String::new();
    let _ = file.read_to_string(&mut geojson_str);
    let geojson = geojson_str.parse::<GeoJson>().unwrap();
    let coll = FeatureCollection::try_from(geojson).unwrap();

    // vello
    // let width = 6000u32;
    // let height = f64::from(width) * all_size.1 / all_size.0;
    // let height = height as u32;
    let affine = Affine::scale(1. / size_per_pixel);
    let mut context = RenderContext::new();
    let device_id = context
        .device(None)
        .await
        .ok_or_else(|| anyhow!("No compatible device found"))?;
    let device_handle = &mut context.devices[device_id];
    let device = &device_handle.device;
    let queue = &device_handle.queue;
    let mut renderer = vello::Renderer::new(
        device,
        RendererOptions {
            num_init_threads: None,
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .or_else(|_| bail!("Got non-Send/Sync error from creating renderer"))?;
    let mut scene = vello::Scene::new();
    // scene.fill(
    //     vello::peniko::Fill::NonZero,
    //     affine,
    //     Color::from_rgb8(242, 140, 168),
    //     None,
    //     &Circle::new((420.0, 200.0), 120.0),
    // );
    let stroke = Stroke::new(0.1f64);
    let font = Font::new(Blob::new(Arc::new(FS_FONT)), 0);
    let font_ref = FontRef::new(font.data.as_ref())?;
    let charmap = font_ref.charmap();
    let default_center = geojson::JsonValue::Array(vec![73.502355.into(), 17.986898.into()]);
    let renderers = vec![
        GeometryRenderer::Area(AreaRenderer::default()),
        GeometryRenderer::Line(LineRenderer::default()),
        GeometryRenderer::Point(PointRenderer::default()),
    ];
    for (index, feature) in coll.features.iter().enumerate() {
        let name = feature.property("name").unwrap();
        let center = feature
            .property("center")
            .unwrap_or(&default_center)
            .as_array()
            .unwrap();
        let point = geo_types::Point::new(center[0].as_f64().unwrap(), center[1].as_f64().unwrap());
        let geometry =
            &geo_types::Geometry::<f64>::try_from(feature.geometry.clone().unwrap()).unwrap();
        let mut rendered_geom: RenderedGeometry = geometry.into();
        rendered_geom.with_rect(rect);
        rendered_geom.draw(&mut scene, affine, &renderers);
    }

    let size = Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("Target texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let render_params = vello::RenderParams {
        base_color: palette::css::TRANSPARENT,
        width,
        height,
        antialiasing_method: vello::AaConfig::Area,
    };
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer
        .render_to_texture(device, queue, &scene, &view, &render_params)
        .or_else(|_| bail!("Got non-Send/Sync error from rendering"))?;
    let padded_byte_width = (width * 4).next_multiple_of(256);
    let buffer_size = padded_byte_width as u64 * height as u64;
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("val"),
        size: buffer_size,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("Copy out buffer"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_byte_width),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let buf_slice = buffer.slice(..);
    let (sender, receiver) = futures_intrusive::channel::shared::oneshot_channel();
    buf_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
    if let Some(recv_result) = block_on_wgpu(device, receiver.receive()) {
        recv_result?;
    } else {
        bail!("channel was closed");
    }
    let data = buf_slice.get_mapped_range();
    let mut result_unpadded = Vec::<u8>::with_capacity((width * height * 4).try_into()?);
    for row in 0..height {
        let start = (row * padded_byte_width).try_into()?;
        result_unpadded.extend(&data[start..start + (width * 4) as usize]);
    }
    let out_path = Path::new("assets/output")
        .join("test")
        .with_extension("png");
    let mut file = File::create(&out_path)?;
    let mut png_encoder = png::Encoder::new(&mut file, width, height);
    png_encoder.set_color(png::ColorType::Rgba);
    png_encoder.set_depth(png::BitDepth::Eight);
    let mut writer = png_encoder.write_header()?;
    writer.write_image_data(&result_unpadded)?;
    writer.finish()?;
    println!("Wrote result ({width}x{height}) to {out_path:?}");
    Ok(())
}
