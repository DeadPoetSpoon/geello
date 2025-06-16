pub mod render_option;
pub mod rendered_geometry;
pub mod utils;
pub use rendered_geometry::*;
pub mod renderer;
pub use render_option::*;
pub use renderer::*;
use vello::{
    Renderer,
    kurbo::Affine,
    util::block_on_wgpu,
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
    let mut scene = vello::Scene::new();
    let rect = option.get_region_rect();
    let transform = transform * option.get_transform(&rect);
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
    transform: Affine,
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
    render_to_texture(geoms, device, queue, renderer, texture, transform, option)?;
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
    transform: Affine,
    option: &RenderOption,
) -> anyhow::Result<Vec<u8>> {
    let texture = render_to_new_texture(geoms, device, queue, renderer, transform, option)?;
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
