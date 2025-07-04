use geo::Rect;
use peniko::color::{AlphaColor, Srgb};
use vello::{kurbo::Affine, wgpu, wgpu::Extent3d};

use crate::{GeometryRenderer, MagicFetcher, MagicValue, utils};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TileProj {
    #[default]
    EPSG4326,
    EPSG3857,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RenderRegion {
    #[default]
    All,
    Rect(Rect),
    PointBuffer(f64, f64, f64),
    TileIndex(u32, u32, u32),
}

impl RenderRegion {
    pub fn get_rect(&self, proj: &TileProj) -> Option<Rect> {
        match self {
            RenderRegion::All => None,
            RenderRegion::Rect(rect) => Some(*rect),
            RenderRegion::TileIndex(x, y, z) => Some(utils::get_rect_from_xyz(*x, *y, *z, proj)),
            RenderRegion::PointBuffer(x, y, z) => Some(Rect::new((x - z, y + z), (x + z, y - z))),
        }
    }
}

#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenderOption {
    #[serde(default)]
    #[serde(skip_serializing_if = "crate::utils::is_default")]
    pub region: RenderRegion,
    #[serde(default)]
    #[serde(skip_serializing_if = "crate::utils::is_default")]
    pub pixel_option: PixelOption,
    #[serde(default)]
    pub renderers: Vec<MagicValue<GeometryRenderer>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "crate::utils::is_default")]
    pub tile_proj: TileProj,
    #[serde(default)]
    #[serde(skip_serializing_if = "crate::utils::is_default")]
    pub need_proj_geom: bool,
}

impl MagicFetcher for RenderOption {
    fn fetch(&mut self) -> Result<(), String> {
        for renderer in self.renderers.iter_mut() {
            renderer.fetch()?;
        }
        Ok(())
    }
}

impl RenderOption {
    pub fn get_view_transform(&self, rect: &Option<Rect>) -> Affine {
        match rect {
            Some(rect) => Affine::translate((-rect.min().x, -rect.max().y))
                .then_scale_non_uniform(1f64, -1f64),
            None => Affine::IDENTITY,
        }
    }
    pub fn get_scale_transform(&self, rect: &Option<Rect>) -> Affine {
        if let Some(rect) = rect {
            let pixel_size = self.get_pixel_size();
            let scale_x = pixel_size.0 as f64 / rect.width();
            let scale_y = pixel_size.1 as f64 / rect.height();
            Affine::scale(scale_x.min(scale_y))
        } else {
            Affine::IDENTITY
        }
    }
    pub fn get_region_rect(&self) -> Option<Rect> {
        self.region.get_rect(&self.tile_proj)
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
    pub fn get_padded_byte_width(&self) -> u32 {
        (self.pixel_option.width * 4).next_multiple_of(256)
    }
    pub fn get_buffer_size(&self) -> u64 {
        self.get_padded_byte_width() as u64 * self.pixel_option.height as u64
    }
}
