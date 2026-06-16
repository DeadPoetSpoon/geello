use std::sync::Arc;

use vello::Renderer;
use vello::wgpu::Adapter;
use vello::wgpu::{
    Backend, Device, DeviceDescriptor, Features, Limits, PowerPreference, Queue,
    RequestAdapterOptions,
};

use crate::core::error::{GeelloError, GeelloResult};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Controls which GPU adapter is selected during initialisation.
#[derive(Debug, Clone, Copy, Default)]
pub enum AdapterSelector {
    #[default]
    Auto,
    Index(usize),
}

#[derive(Debug, Clone)]
pub struct AdapterInfo {
    pub name: String,
    pub backend: Backend,
    pub is_integrated: bool,
}

/// Central rendering context holding the GPU device, command queue,
/// and the vello renderer.
#[derive(Clone)]
pub struct RenderContext {
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
    pub renderer: Arc<Renderer>,
    pub adapter_info: AdapterInfo,
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct RenderContextBuilder {
    selector: AdapterSelector,
    features: Features,
    limits: Limits,
}

impl Default for RenderContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderContextBuilder {
    pub fn new() -> Self {
        Self {
            selector: AdapterSelector::Auto,
            features: Features::empty(),
            limits: Limits::default(),
        }
    }

    pub fn adapter(mut self, selector: AdapterSelector) -> Self {
        self.selector = selector;
        self
    }

    pub fn features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }

    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Initialise wgpu, select an adapter, create device + queue, and
    /// construct the vello renderer.
    pub async fn build(self) -> GeelloResult<RenderContext> {
        let instance = vello::wgpu::Instance::default();

        // ---- select adapter ----
        let adapter = match self.selector {
            AdapterSelector::Auto => instance
                .request_adapter(&RequestAdapterOptions {
                    power_preference: PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await
                .ok(),
            AdapterSelector::Index(idx) => instance
                .enumerate_adapters(vello::wgpu::Backends::all())
                .await
                .into_iter()
                .nth(idx),
        }
        .ok_or(GeelloError::Wgpu("No suitable GPU adapter found".into()))?;

        let adapter_info = gather_adapter_info(&adapter);
        tracing::info!("GPU adapter selected: {:?}", adapter_info);

        // ---- request device ----
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("geello"),
                required_features: self.features,
                required_limits: self.limits,
                ..Default::default()
            })
            .await
            .map_err(|e| GeelloError::Wgpu(format!("Failed to create device: {e}")))?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        // ---- vello renderer ----
        let renderer = Arc::new(
            Renderer::new(
                &device,
                vello::RendererOptions {
                    pipeline_cache: None,
                    ..Default::default()
                },
            )
            .map_err(|e| GeelloError::Wgpu(format!("Failed to create vello renderer: {e}")))?,
        );

        Ok(RenderContext {
            device,
            queue,
            renderer,
            adapter_info,
        })
    }
}

fn gather_adapter_info(adapter: &Adapter) -> AdapterInfo {
    let info = adapter.get_info();
    AdapterInfo {
        name: info.name,
        backend: info.backend,
        is_integrated: info.device_type == vello::wgpu::DeviceType::IntegratedGpu,
    }
}
