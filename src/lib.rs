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
) -> Result<(), String> {
    let mut scene = vello::Scene::new();
    let rect = option.get_region_rect();
    let g_transform = option.get_view_transform(&rect);
    let g_transform = option.get_scale_transform(&rect) * g_transform;
    option.renderers.iter().for_each(|renderer| {
        renderer.draw(&mut scene, transform * g_transform, geoms, rect);
    });
    let render_params = option.get_render_params();
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer
        .render_to_texture(device, queue, &scene, &view, &render_params)
        .map_err(|e| format!("render error: {}", e.to_string()))?;
    Ok(())
}

pub fn render_to_texture_with_new_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    transform: Affine,
    option: &RenderOption,
) -> Result<Texture, String> {
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
) -> Result<Vec<u8>, String> {
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
    buf_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
    if let Ok(recv_result) = block_on_wgpu(device, receiver) {
        recv_result.map_err(|e| format!("recv data from gpu error: {}", e.to_string()))?;
    } else {
        return Err(format!("recv data from gpu error: channel was closed"));
    }
    let data = buf_slice.get_mapped_range();
    let (width, height) = option.get_pixel_size();
    let mut result_unpadded = Vec::<u8>::with_capacity(
        (width * height * 4)
            .try_into()
            .map_err(|_| format!("invalid capacity"))?,
    );
    for row in 0..height {
        let start = (row * padded_byte_width)
            .try_into()
            .map_err(|_| format!("write buffer error, too large"))?;
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
) -> Result<Vec<u8>, String> {
    let texture_desc = option.get_texture_descriptor();
    let texture = device.create_texture(&texture_desc);
    render_to_buffer(geoms, device, queue, renderer, &texture, transform, option)
}
