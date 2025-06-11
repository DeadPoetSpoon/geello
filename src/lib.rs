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
    kurbo::{Affine, StrokeOpts},
    util::{DeviceHandle, RenderContext, block_on_wgpu},
    wgpu::{self, Device, ImageSubresourceRange, Queue, Texture, TextureAspect},
};

pub fn render_to_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
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
    let mut renderer = option.get_vello_renderer(device)?;
    renderer.render_to_texture(device, queue, &scene, &view, &render_params)?;
    Ok(())
}

pub fn render_to_new_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    transform: Affine,
    option: &RenderOption,
) -> anyhow::Result<Texture> {
    log::debug!("Rendering to new texture：{:?}", option.get_region_rect());
    let mut scene = vello::Scene::new();
    let rect = option.get_region_rect();
    let transform = transform * option.get_transform();
    geoms.iter_mut().for_each(|geom| {
        geom.with_rect(rect);
        geom.draw(&mut scene, transform, &option.renderers);
    });
    let texture_desc = option.get_texture_descriptor();
    let texture = device.create_texture(&texture_desc);
    let render_params = option.get_render_params();
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    // init once
    let mut renderer = option.get_vello_renderer(device)?;
    renderer.render_to_texture(device, queue, &scene, &view, &render_params)?;
    Ok(texture)
}

pub fn render_to_buffer(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
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
    render_to_texture(geoms, device, queue, texture, Affine::IDENTITY, option)?;
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
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let texture = render_to_new_texture(geoms, device, queue, Affine::IDENTITY, option)?;
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
    geojson: GeoJson,
    device: &Device,
    queue: &Queue,
    texture: &Texture,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let mut geom_to_render_vec = Vec::new();
    match &geojson {
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
    render_to_buffer(&mut geom_s, device, queue, texture, option)
}

#[cfg(feature = "server")]
pub async fn render_geojson_file_to_buffer<P: AsRef<Path>>(
    path: P,
    device: &Device,
    queue: &Queue,
    texture: &Texture,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut geojson_str = String::new();
    let _ = file.read_to_string(&mut geojson_str);
    let geojson = geojson_str.parse::<GeoJson>().unwrap();
    render_geojson_to_buffer(geojson, device, queue, texture, option).await
}

#[cfg(feature = "server")]
pub fn render_to_image<P: AsRef<Path>>(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    option: &RenderOption,
    path: P,
) -> anyhow::Result<()> {
    let data = render_to_buffer_with_new_texture(geoms, device, queue, option)?;
    let (width, height) = option.get_pixel_size();
    let image = image::RgbaImage::from_raw(width, height, data)
        .ok_or_else(|| anyhow::anyhow!("create image error."))?;
    image.save(path)?;
    Ok(())
}

#[cfg(feature = "server")]
pub async fn render_to_image_with_default_device<'a, P: AsRef<Path>>(
    geoms: &'a mut Vec<RenderedGeometry<'a>>,
    option: &RenderOption,
    path: P,
) -> anyhow::Result<()> {
    let mut context = RenderContext::new();
    let device_id = context
        .device(None)
        .await
        .ok_or_else(|| anyhow::anyhow!("No compatible device found"))?;
    let device_handle = &mut context.devices[device_id];
    render_to_image(
        geoms,
        &device_handle.device,
        &device_handle.queue,
        option,
        path,
    )?;
    Ok(())
}

#[cfg(feature = "server")]
pub async fn render_geojson_to_image<P: AsRef<Path>>(
    geojson: GeoJson,
    option: &RenderOption,
    path: P,
) -> anyhow::Result<()> {
    match &geojson {
        GeoJson::Geometry(geometry) => {
            let mut geom = geo_types::Geometry::<f64>::try_from(geometry)?;
            if option.need_proj_geom {
                utils::transform(&mut geom, &option.tile_proj);
            }
            let geom = &geom;
            render_to_image_with_default_device(&mut vec![geom.into()], option, path).await?;
        }
        GeoJson::Feature(feature) => {
            let geom = feature
                .geometry
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("only feature has no geometry"))?;
            let mut geom = geo_types::Geometry::<f64>::try_from(geom)?;
            if option.need_proj_geom {
                utils::transform(&mut geom, &option.tile_proj);
            }
            let geom = &geom;
            render_to_image_with_default_device(&mut vec![geom.into()], option, path).await?;
        }
        GeoJson::FeatureCollection(feature_collection) => {
            let mut geoms = Vec::new();
            for (index, feature) in feature_collection.features.iter().enumerate() {
                let geom = feature
                    .geometry
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("feature (index:{index}) has no geometry"))?;
                let geom = geo_types::Geometry::<f64>::try_from(geom)?;
                geoms.push(geom);
            }
            if option.need_proj_geom {
                geoms.iter_mut().for_each(|mut geom| {
                    utils::transform(&mut geom, &option.tile_proj);
                });
            }
            let mut geoms: Vec<RenderedGeometry> = geoms.iter().map(|x| x.into()).collect();
            render_to_image_with_default_device(&mut geoms, option, path).await?;
        }
    };
    Ok(())
}

#[cfg(feature = "server")]
pub async fn render_geojson_file_to_image<P: AsRef<Path>>(
    geojson_path: P,
    option: &RenderOption,
    path: P,
) -> anyhow::Result<()> {
    let mut file = File::open(geojson_path)?;
    let mut geojson_str = String::new();
    let _ = file.read_to_string(&mut geojson_str);
    let geojson = geojson_str.parse::<GeoJson>().unwrap();
    render_geojson_to_image(geojson, option, path).await
}

#[cfg(feature = "server")]
pub async fn render_geojson_file_to_image_with_option_file<P: AsRef<Path>>(
    geojson_path: P,
    option_path: P,
    path: P,
) -> anyhow::Result<()> {
    let mut file = File::open(option_path)?;
    let mut ron_str = String::new();
    let _ = file.read_to_string(&mut ron_str);
    let render_option: RenderOption = ron::de::from_str(&ron_str)?;
    render_geojson_file_to_image(geojson_path, &render_option, path).await
}
