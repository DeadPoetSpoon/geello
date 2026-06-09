//! GPU 设备上下文管理
//!
//! 负责 wgpu 设备初始化、Queue 获取、纹理池管理。
//! 架构预留多 GPU 支持（当前单设备，接口设计允许未来扩展为 Vec<DeviceHandle>）。

use std::sync::Mutex;
use tokio::sync::Semaphore;
use vello::wgpu;

/// 单个 GPU 设备句柄
pub struct DeviceHandle {
    pub id: usize,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: wgpu::AdapterInfo,
}

/// GPU 设备上下文
///
/// 封装 wgpu 设备初始化流程，管理设备和命令队列。
/// 当前仅使用第一个可用设备，预留多 GPU 接口。
pub struct DeviceContext {
    /// 所有可用设备（当前仅含一个）
    pub handles: Vec<DeviceHandle>,
    /// 下一个要使用的设备索引（用于请求轮询调度）
    #[allow(dead_code)]
    next_device: std::sync::atomic::AtomicUsize,
}

/// 纹理池：多个预创建的 Texture 供 WMTS 请求复用
///
/// vello Renderer 是 !Sync，每次请求需新建，但 Texture 可以复用。
pub struct TexturePool {
    pub textures: Mutex<Vec<wgpu::Texture>>,
    pub semaphore: Semaphore,
}

impl TexturePool {
    /// 创建纹理池，预分配 `count` 个指定尺寸的纹理
    pub fn new(device: &wgpu::Device, count: u32, width: u32, height: u32) -> Self {
        let mut textures = Vec::with_capacity(count as usize);
        for i in 0..count {
            let label = format!("TexturePool-{}", i);
            let desc = wgpu::TextureDescriptor {
                label: Some(&label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            };
            textures.push(device.create_texture(&desc));
        }
        Self {
            textures: Mutex::new(textures),
            semaphore: Semaphore::new(count as usize),
        }
    }

    /// 从池中获取一个纹理（阻塞直到可用）
    pub async fn acquire(&self) -> wgpu::Texture {
        let _permit = self.semaphore.acquire().await.unwrap();
        let mut textures = self.textures.lock().unwrap();
        textures.pop().unwrap()
    }

    /// 归还纹理到池中
    pub fn release(&self, texture: wgpu::Texture) {
        let mut textures = self.textures.lock().unwrap();
        textures.push(texture);
        // semaphore permit 在 _permit drop 时自动释放
    }
}
