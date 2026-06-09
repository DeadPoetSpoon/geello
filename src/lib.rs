pub mod core;
pub mod device;
pub mod error;
pub mod render;

#[cfg(feature = "server")]
pub mod server;

pub use core::*;
pub use device::{DeviceContext, DeviceHandle, TexturePool};
pub use error::{GeelloError, GeelloResult};
pub use render::*;
use vello::{
    Renderer,
    kurbo::Affine,
    util::block_on_wgpu,
    wgpu::{self, Device, ImageSubresourceRange, Queue, Texture, TextureAspect},
};

/// Synchronous: render geometry to an existing texture.
/// Suitable for library use where the caller manages threading.
pub fn render_to_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    transform: Affine,
    option: &mut RenderOption,
) -> GeelloResult<()> {
    let mut scene = vello::Scene::new();
    let rect = option.get_region_rect();
    let g_transform = option.get_view_transform(&rect);
    let g_transform = option.get_scale_transform(&rect) * g_transform;
    for renderer in option.renderers.iter_mut() {
        let renderer = renderer.as_mut();
        renderer
            .draw(&mut scene, transform * g_transform, geoms, rect)
            .map_err(GeelloError::render)?;
    }
    let render_params = option.get_render_params();
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer
        .render_to_texture(device, queue, &scene, &view, &render_params)
        .map_err(GeelloError::render)?;
    Ok(())
}

pub fn render_to_texture_with_new_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    transform: Affine,
    option: &mut RenderOption,
) -> GeelloResult<Texture> {
    let texture_desc = option.get_texture_descriptor();
    let texture = device.create_texture(&texture_desc);
    render_to_texture(geoms, device, queue, renderer, &texture, transform, option)?;
    Ok(texture)
}

/// Synchronous: render geometry to a CPU-accessible buffer.
///
/// Uses `block_on_wgpu` which blocks the calling thread. For async
/// contexts (e.g. server), use `render_to_buffer_async` instead.
pub fn render_to_buffer(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    transform: Affine,
    option: &mut RenderOption,
) -> GeelloResult<Vec<u8>> {
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
        recv_result.map_err(GeelloError::gpu)?;
    } else {
        return Err(GeelloError::Gpu("channel was closed".to_string()));
    }
    let data = buf_slice.get_mapped_range();
    let (width, height) = option.get_pixel_size();
    let mut result_unpadded = Vec::<u8>::with_capacity(
        (width * height * 4)
            .try_into()
            .map_err(|_| GeelloError::Internal("invalid capacity".into()))?,
    );
    for row in 0..height {
        let start = (row * padded_byte_width)
            .try_into()
            .map_err(|_| GeelloError::Internal("write buffer error, too large".into()))?;
        result_unpadded.extend(&data[start..start + (width * 4) as usize]);
    }
    Ok(result_unpadded)
}

/// Async version of `render_to_buffer`.
///
/// Offloads the blocking GPU buffer read to a dedicated blocking thread
/// via `tokio::task::spawn_blocking`, avoiding starvation of the async runtime.
/// Prefer this in server/async contexts.
pub async fn render_to_buffer_async(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    texture: &Texture,
    transform: Affine,
    option: &mut RenderOption,
) -> GeelloResult<Vec<u8>> {
    render_to_texture(geoms, device, queue, renderer, texture, transform, option)?;
    let padded_byte_width = option.get_padded_byte_width();
    let buffer_size = option.get_buffer_size();
    let (width, height) = option.get_pixel_size();
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
    let device = device.clone();
    tokio::task::spawn_blocking(move || {
        let buf_slice = buffer.slice(..);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        buf_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
        if let Ok(recv_result) = block_on_wgpu(&device, receiver) {
            recv_result.map_err(GeelloError::gpu)?;
        } else {
            return Err(GeelloError::Gpu("channel was closed".to_string()));
        }
        let data = buf_slice.get_mapped_range();
        let mut result_unpadded = Vec::<u8>::with_capacity(
            (width * height * 4)
                .try_into()
                .map_err(|_| GeelloError::Internal("invalid capacity".into()))?,
        );
        for row in 0..height {
            let start = (row * padded_byte_width)
                .try_into()
                .map_err(|_| GeelloError::Internal("write buffer error, too large".into()))?;
            result_unpadded.extend(&data[start..start + (width * 4) as usize]);
        }
        Ok(result_unpadded)
    })
    .await
    .map_err(|e| GeelloError::Internal(format!("spawn_blocking error: {}", e)))?
}

pub fn render_to_buffer_with_new_texture(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    transform: Affine,
    option: &mut RenderOption,
) -> GeelloResult<Vec<u8>> {
    let texture_desc = option.get_texture_descriptor();
    let texture = device.create_texture(&texture_desc);
    render_to_buffer(geoms, device, queue, renderer, &texture, transform, option)
}

pub async fn render_to_buffer_with_new_texture_async(
    geoms: &mut Vec<RenderedGeometry>,
    device: &Device,
    queue: &Queue,
    renderer: &mut Renderer,
    transform: Affine,
    option: &mut RenderOption,
) -> GeelloResult<Vec<u8>> {
    let texture_desc = option.get_texture_descriptor();
    let texture = device.create_texture(&texture_desc);
    render_to_buffer_async(geoms, device, queue, renderer, &texture, transform, option).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_option_defaults() {
        let opt = PixelOption::default();
        assert_eq!(opt.width, 256);
        assert_eq!(opt.height, 256);
    }

    #[test]
    fn tile_proj_default_is_4326() {
        assert_eq!(TileProj::default(), TileProj::EPSG4326);
    }

    #[test]
    fn render_option_defaults_empty() {
        let opt = RenderOption::default();
        assert_eq!(opt.region, RenderRegion::All);
        assert!(!opt.need_proj_geom);
        assert!(opt.renderers.is_empty());
    }

    #[test]
    fn tile_index_produces_rect() {
        let r = RenderRegion::TileIndex(0, 0, 3).get_rect(&TileProj::EPSG3857);
        assert!(r.is_some());
    }

    #[test]
    fn prop_value_f64_roundtrip() {
        let v = PropValue::Float64(3.14);
        let f: f64 = v.try_into().unwrap();
        assert!((f - 3.14).abs() < 0.001);
    }

    #[test]
    fn prop_value_string_roundtrip() {
        let v = PropValue::String("hello".into());
        let s: String = v.try_into().unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn filter_none_matches_all() {
        let rg = RenderedGeometry::new_temp(
            Default::default(),
            geo::Geometry::Point(geo::Point::new(0.0, 0.0)),
        );
        assert!(rg.fit_filter(&RenderedGeometryFilter::None));
    }

    #[test]
    fn filter_layer_matches_exact() {
        let rg = RenderedGeometry::new(
            Some("roads".into()),
            Default::default(),
            geo::Geometry::Point(geo::Point::new(0.0, 0.0)),
            &None,
        );
        assert!(rg.fit_filter(&RenderedGeometryFilter::Layer("roads".into())));
        assert!(!rg.fit_filter(&RenderedGeometryFilter::Layer("buildings".into())));
    }
}
