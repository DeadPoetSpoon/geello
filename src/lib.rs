pub mod render_option;
mod utils;
use std::{borrow::BorrowMut, fs::File, io::Read, path::Path};

pub use render_option::*;
pub mod rendered_geometry;
pub use rendered_geometry::*;
pub mod renderer;

use geojson::{FeatureCollection, GeoJson, Geometry};
pub use renderer::*;
use vello::{
    Renderer,
    kurbo::{Affine, StrokeOpts},
    util::{DeviceHandle, RenderContext, block_on_wgpu},
    wgpu::{self, Device, ImageSubresourceRange, Queue, Texture, TextureAspect},
};

pub fn render_to_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    transform: Affine,
    option: &RenderOption,
) -> anyhow::Result<()> {
    log::debug!("Rendering to texture：{:?}", option.get_region_rect());
    let mut scene = vello::Scene::new();
    let transform = transform * option.get_transform();
    let rect = option.get_region_rect();
    geoms.iter_mut().for_each(|geom| {
        geom.with_rect(rect);
        geom.draw(&mut scene, transform, &option.renderers);
    });
    let render_params = option.get_render_params();
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer.render_to_texture(device, queue, &scene, &view, &render_params)?;
    Ok(())
}

pub fn render_to_new_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    transform: Affine,
    option: &RenderOption,
) -> anyhow::Result<Texture> {
    let texture_desc = option.get_texture_descriptor();
    let texture = device.create_texture(&texture_desc);
    render_to_texture(geoms, device, queue, renderer, &texture, transform, option)?;
    Ok(texture)
}

pub fn render_to_buffer(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let mut clear_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Clear Texture"),
    });
    clear_encoder.clear_texture(
        texture,
        &ImageSubresourceRange {
            aspect: TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
        },
    );
    queue.submit([clear_encoder.finish()]);
    render_to_texture(
        geoms,
        device,
        queue,
        renderer,
        texture,
        Affine::IDENTITY,
        option,
    )?;
    let padded_byte_width = option.get_padded_byte_width();
    let buffer_size = option.get_buffer_size();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("val"),
        size: buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Copy out buffer"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_byte_width),
                rows_per_image: None,
            },
        },
        option.get_extent3d(),
    );
    queue.submit([encoder.finish()]);
    let buf_slice = buffer.slice(..);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // let (sender, receiver) = futures_intrusive::channel::shared::oneshot_channel();
    buf_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
    if let Ok(recv_result) = block_on_wgpu(device, receiver) {
        recv_result?;
    } else {
        anyhow::bail!("channel was closed");
    }
    let data = buf_slice.get_mapped_range();
    let (width, height) = option.get_pixel_size();
    let mut result_unpadded = Vec::<u8>::with_capacity((width * height * 4).try_into()?);
    for row in 0..height {
        let start = (row * padded_byte_width).try_into()?;
        result_unpadded.extend(&data[start..start + (width * 4) as usize]);
    }
    Ok(result_unpadded)
}

pub fn render_to_buffer_with_new_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let texture = render_to_new_texture(geoms, device, queue, renderer, Affine::IDENTITY, option)?;
    let padded_byte_width = option.get_padded_byte_width();
    let buffer_size = option.get_buffer_size();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("val"),
        size: buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("Copy out buffer"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_byte_width),
                rows_per_image: None,
            },
        },
        option.get_extent3d(),
    );
    queue.submit([encoder.finish()]);
    let buf_slice = buffer.slice(..);
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // let (sender, receiver) = futures_intrusive::channel::shared::oneshot_channel();
    buf_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
    if let Ok(recv_result) = block_on_wgpu(device, receiver) {
        recv_result?;
    } else {
        anyhow::bail!("channel was closed");
    }
    let data = buf_slice.get_mapped_range();
    let (width, height) = option.get_pixel_size();
    let mut result_unpadded = Vec::<u8>::with_capacity((width * height * 4).try_into()?);
    for row in 0..height {
        let start = (row * padded_byte_width).try_into()?;
        result_unpadded.extend(&data[start..start + (width * 4) as usize]);
    }
    Ok(result_unpadded)
}

#[cfg(feature = "server")]
pub async fn render_geojson_to_buffer(
    geojson: &GeoJson,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let mut geom_to_render_vec = Vec::new();
    match geojson {
        GeoJson::Geometry(geometry) => {
            let geom = geo_types::Geometry::<f64>::try_from(geometry)?;
            geom_to_render_vec.push(geom);
        }
        GeoJson::Feature(feature) => {
            let geom = feature
                .geometry
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("only feature has no geometry"))?;
            let geom = geo_types::Geometry::<f64>::try_from(geom)?;
            geom_to_render_vec.push(geom);
        }
        GeoJson::FeatureCollection(feature_collection) => {
            for (index, feature) in feature_collection.features.iter().enumerate() {
                let geom = feature
                    .geometry
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("feature (index:{index}) has no geometry"))?;
                let geom = geo_types::Geometry::<f64>::try_from(geom)?;
                geom_to_render_vec.push(geom);
            }
        }
    };
    if option.need_proj_geom {
        geom_to_render_vec.iter_mut().for_each(|mut geom| {
            utils::transform(&mut geom, &option.tile_proj);
        });
    }
    let mut geom_s: Vec<RenderedGeometry> =
        geom_to_render_vec.iter().map(|geom| geom.into()).collect();
    render_to_buffer(&mut geom_s, device, queue, renderer, texture, option)
}

#[cfg(feature = "server")]
pub async fn render_geojson_file_to_buffer<P: AsRef<Path>>(
    path: P,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut geojson_str = String::new();
    let _ = file.read_to_string(&mut geojson_str);
    let geojson = geojson_str.parse::<GeoJson>().unwrap();
    render_geojson_to_buffer(&geojson, device, queue, renderer, texture, option).await
}

#[cfg(feature = "server")]
pub fn render_to_image<P: AsRef<Path>>(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    option: &RenderOption,
    path: P,
) -> anyhow::Result<()> {
    let data = render_to_buffer_with_new_texture(geoms, device, queue, renderer, option)?;
    let (width, height) = option.get_pixel_size();
    let image = image::RgbaImage::from_raw(width, height, data)
        .ok_or_else(|| anyhow::anyhow!("create image error."))?;
    image.save(path)?;
    Ok(())
}
