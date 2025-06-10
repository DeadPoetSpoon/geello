use std::{default, num::NonZero};

use geo::Rect;
use peniko::color::{AlphaColor, Srgb};
use vello::{kurbo::Affine, wgpu, wgpu::Extent3d};

use crate::GeometryRenderer;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PixelOption {
    pub width: u32,
    pub height: u32,
    pub base_color: AlphaColor<Srgb>,
}

impl Default for PixelOption {
    fn default() -> Self {
        Self {
            width: 256,
            height: 256,
            base_color: AlphaColor::TRANSPARENT,
        }
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum RenderRegion {
    #[default]
    All,
    Rect(Rect),
    PointBuffer(f64, f64, f64),
    TileIndex(u32, u32, u32),
}

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct RenderOption {
    pub region: RenderRegion,
    pub pixel_option: PixelOption,
    pub renderers: Vec<GeometryRenderer>,
    pub num_init_threads: Option<NonZero<usize>>,
}

impl RenderRegion {
    pub fn get_rect(&self) -> Option<Rect> {
        match self {
            RenderRegion::All => None,
            RenderRegion::Rect(rect) => Some(*rect),
            RenderRegion::TileIndex(x, y, z) => Some(RenderRegion::get_rect_from_xyz(*x, *y, *z)),
            RenderRegion::PointBuffer(x, y, z) => Some(Rect::new((x - z, y - z), (x + z, y + z))),
        }
    }
    fn get_rect_from_xyz(x: u32, y: u32, z: u32) -> Rect {
        let n = 2u32.pow(z);
        let lon_deg = 360.0 / n as f64;
        let lat_deg = 180.0 / n as f64;
        let min_lon = x as f64 * lon_deg - 180.0;
        let max_lat = 90.0 - y as f64 * lat_deg;
        let max_lon = min_lon + lon_deg;
        let min_lat = max_lat - lat_deg;
        Rect::new((min_lon, min_lat), (max_lon, max_lat))
    }
}

impl RenderOption {
    pub fn get_transform(&self) -> Affine {
        if let Some(rect) = self.region.get_rect() {
            let pixel_size = self.get_pixel_size();
            let scale_x = pixel_size.0 as f64 / rect.width();
            let scale_y = pixel_size.1 as f64 / rect.height();
            Affine::scale(scale_x.min(scale_y))
        } else {
            Affine::IDENTITY
        }
    }
    pub fn get_pixel_size(&self) -> (u32, u32) {
        (self.pixel_option.width, self.pixel_option.height)
    }
    pub fn get_extent3d(&self) -> Extent3d {
        Extent3d {
            width: self.pixel_option.width,
            height: self.pixel_option.height,
            depth_or_array_layers: 1,
        }
    }
    pub fn get_texture_descriptor(&self) -> wgpu::TextureDescriptor {
        wgpu::TextureDescriptor {
            label: Some("Rendered Texture"),
            size: self.get_extent3d(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }
    }
    pub fn get_render_params(&self) -> vello::RenderParams {
        vello::RenderParams {
            base_color: self.pixel_option.base_color,
            width: self.pixel_option.width,
            height: self.pixel_option.height,
            antialiasing_method: vello::AaConfig::Area,
        }
    }
    pub fn get_vello_renderer_options(&self) -> vello::RendererOptions {
        vello::RendererOptions {
            num_init_threads: self.num_init_threads,
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        }
    }
    pub fn get_vello_renderer(&self, device: &wgpu::Device) -> anyhow::Result<vello::Renderer> {
        vello::Renderer::new(device, self.get_vello_renderer_options())
            .or_else(|_| anyhow::bail!("Got non-Send/Sync error from creating renderer"))
    }
    pub fn get_padded_byte_width(&self) -> u32 {
        (self.pixel_option.width * 4).next_multiple_of(256)
    }
    pub fn get_buffer_size(&self) -> u64 {
        self.get_padded_byte_width() as u64 * self.pixel_option.height as u64
    }
}
