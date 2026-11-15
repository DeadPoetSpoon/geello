//! Runtime-independent GPU execution. No executor thread blocks for device polling.
use crate::{GeelloError, OperationContext, PreparedScene, RgbaImage};
use crate::{backend, scene::Command};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};
use vello_common::{
    TextureId,
    kurbo::{Affine, Rect},
    paint::{ImageSource, TextureRegion},
    peniko::{self, Fill},
};

/// An external target retains its real device identity and cannot forge size/format metadata.
/// The view is always created at mip zero. Success replaces all pixels with premultiplied sRGB RGBA.
pub struct ExternalTarget<'a> {
    pub texture: &'a wgpu::Texture,
}
/// Reusable Vello GPU adapter. Concurrent/drop-and-retry submissions are rejected.
/// The supplied device and queue must form the same wgpu context.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
    busy: Arc<AtomicBool>,
    fault: Arc<Mutex<Option<String>>>,
}
impl GpuRenderer {
    /// Create on a caller-owned device without requiring Tokio. Errors are captured before use.
    pub async fn from_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        context: &OperationContext,
    ) -> Result<Self, GeelloError> {
        context.check()?;
        let guard = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let config = vello_gpu::RenderTargetConfig {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: 1,
            height: 1,
        };
        let (renderer, resources) = vello_gpu::Renderer::new(&device, &queue, &config);
        if let Some(e) = guard.pop().await {
            return Err(GeelloError::Backend(e.to_string()));
        }
        context.check()?;
        let fault = Arc::new(Mutex::new(None));
        let f = fault.clone();
        device.set_device_lost_callback(move |reason, message| {
            *f.lock().unwrap() = Some(format!("{reason:?}: {message}"));
        });
        Ok(Self {
            device,
            queue,
            renderer,
            resources,
            busy: Arc::new(AtomicBool::new(false)),
            fault,
        })
    }
    /// Request a headless device from an explicit backend selection; never falls back to CPU.
    pub async fn new(
        backends: wgpu::Backends,
        context: &OperationContext,
    ) -> Result<(Self, wgpu::AdapterInfo), GeelloError> {
        context.check()?;
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|e| GeelloError::Backend(e.to_string()))?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| GeelloError::Backend(e.to_string()))?;
        Ok((Self::from_device(device, queue, context).await?, info))
    }
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    /// Validate the actual texture, not caller-reported attributes. No commands are submitted.
    pub fn validate_target(
        &self,
        scene: &PreparedScene,
        target: &ExternalTarget<'_>,
    ) -> Result<(), GeelloError> {
        let t = target.texture;
        if t.width() != scene.summary().width
            || t.height() != scene.summary().height
            || t.depth_or_array_layers() != 1
            || t.dimension() != wgpu::TextureDimension::D2
            || t.format() != wgpu::TextureFormat::Rgba8Unorm
            || t.sample_count() != 1
            || !t.usage().contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
            || self.device.limits().max_texture_dimension_2d < t.width().max(t.height())
        {
            return Err(GeelloError::InvalidTarget(
                "expected same-size single-sample D2 RGBA8Unorm render attachment".into(),
            ));
        }
        self.validate_size(scene)
    }
    fn validate_size(&self, scene: &PreparedScene) -> Result<(), GeelloError> {
        // Dispatch pending device-loss callbacks before allocating another target.
        // Poll is nonblocking; submission completion still uses its worker.
        self.device
            .poll(wgpu::PollType::Poll)
            .map_err(|error| GeelloError::Backend(error.to_string()))?;
        let limit = self.device.limits().max_texture_dimension_2d;
        if scene.summary().width > limit || scene.summary().height > limit {
            return Err(GeelloError::DimensionOverflow);
        }
        if let Some(e) = self.fault.lock().unwrap().as_ref() {
            return Err(GeelloError::Backend(e.clone()));
        }
        Ok(())
    }
    /// A post-submit error or cancellation invalidates the caller's target; no rollback is claimed.
    pub async fn render_to_target(
        &mut self,
        scene: &PreparedScene,
        target: &ExternalTarget<'_>,
        context: &OperationContext,
    ) -> Result<(), GeelloError> {
        context.check()?;
        self.validate_target(scene, target)?;
        self.execute(scene, target.texture, false, context)
            .await
            .map(|_| ())
    }
    /// Render and asynchronously read back tight straight RGBA8 pixels.
    pub async fn render_pixels(
        &mut self,
        scene: &PreparedScene,
        context: &OperationContext,
    ) -> Result<RgbaImage, GeelloError> {
        context.check()?;
        self.validate_size(scene)?;
        let s = scene.summary();
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("geello pixels"),
            size: wgpu::Extent3d {
                width: s.width,
                height: s.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let bytes = self
            .execute(scene, &target, true, context)
            .await?
            .expect("pixel request produces bytes");
        backend::straight(s.width, s.height, bytes)
    }
    async fn execute(
        &mut self,
        scene: &PreparedScene,
        target: &wgpu::Texture,
        readback: bool,
        context: &OperationContext,
    ) -> Result<Option<Vec<u8>>, GeelloError> {
        context.check()?;
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(GeelloError::Backend(
                "previous GPU submission still in flight".into(),
            ));
        }
        let mut busy = BusyGuard(Some(self.busy.clone()));
        let w = scene.summary().width;
        let h = scene.summary().height;
        let row = w.checked_mul(4).ok_or(GeelloError::SizeOverflow)?;
        let stride = row.div_ceil(256) * 256;
        let size = (stride as usize)
            .checked_mul(h as usize)
            .ok_or(GeelloError::SizeOverflow)?;
        let estimate = size
            .checked_mul(6)
            .and_then(|n| n.checked_add(scene.summary().estimated_accounted_bytes))
            .ok_or(GeelloError::SizeOverflow)?;
        let reservation = context.reserve(estimate)?;
        context.checkpoint(w as u64 * h as u64)?;
        let guard = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut gpu_scene = vello_gpu::Scene::new(w as u16, h as u16);
        gpu_scene.set_fill_rule(Fill::EvenOdd);
        let mut bindings = vello_gpu::TextureBindings::new();
        let mut images = Vec::new();
        let mut next_id = 1u64;
        for command in &scene.inner.commands {
            context.checkpoint(1)?;
            match command {
                Command::Path {
                    path,
                    color,
                    opacity,
                    stroke,
                } => {
                    gpu_scene.set_paint(backend::color(*color, *opacity));
                    gpu_scene.set_paint_transform(Affine::IDENTITY);
                    let p = backend::path(path);
                    if let Some(s) = stroke {
                        gpu_scene.set_stroke(backend::stroke(s));
                        gpu_scene.stroke_path(&p)
                    } else {
                        gpu_scene.fill_path(&p)
                    }
                }
                Command::Image {
                    image,
                    rect,
                    opacity,
                } => {
                    let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("geello image"),
                        size: wgpu::Extent3d {
                            width: image.width(),
                            height: image.height(),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_DST
                            | wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    });
                    self.queue.write_texture(
                        texture.as_image_copy(),
                        &backend::premultiply(image.pixels()),
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(image.width() * 4),
                            rows_per_image: Some(image.height()),
                        },
                        texture.size(),
                    );
                    let id = TextureId(next_id);
                    next_id += 1;
                    bindings.insert(id, texture.create_view(&Default::default()));
                    gpu_scene.set_paint(peniko::ImageBrush {
                        image: ImageSource::external_texture(id, TextureRegion::Full, true),
                        sampler: peniko::ImageSampler::new()
                            .with_quality(peniko::ImageQuality::Low)
                            .with_alpha(*opacity as f32),
                    });
                    gpu_scene.set_paint_transform(Affine::new([
                        (rect[2] - rect[0]) / image.width() as f64,
                        0.0,
                        0.0,
                        (rect[3] - rect[1]) / image.height() as f64,
                        rect[0],
                        rect[1],
                    ]));
                    gpu_scene.fill_rect(&Rect::new(rect[0], rect[1], rect[2], rect[3]));
                    images.push(texture);
                }
            }
        }
        let view = target.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.renderer
            .render(
                &gpu_scene,
                &mut self.resources,
                &mut encoder,
                vello_gpu::RenderTarget {
                    view: &view,
                    depth_view: None,
                },
                &bindings,
                vello_gpu::RenderOptions::default(),
            )
            .map_err(|e| GeelloError::Backend(e.to_string()))?;
        let buffer = readback.then(|| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("geello readback"),
                size: size as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        if let Some(buffer) = &buffer {
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: Some(h),
                    },
                },
                target.size(),
            );
        }
        let command_buffer = encoder.finish();
        if let Some(e) = guard.pop().await {
            return Err(GeelloError::InvalidTarget(e.to_string()));
        }
        context.check()?;
        // Loss can race with the pre-submit checks. Keep submit/map validation
        // scoped as well, so an invalid readback buffer becomes an error rather
        // than reaching wgpu's default uncaptured-error panic handler.
        let submission_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        self.queue.submit([command_buffer]);
        let completed = Arc::new(AtomicBool::new(false));
        let done = completed.clone();
        self.queue
            .on_submitted_work_done(move || done.store(true, Ordering::Release));
        let mapped = Arc::new(Mutex::new(None));
        if let Some(buffer) = &buffer {
            let status = mapped.clone();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                *status.lock().unwrap() = Some(r.map_err(|e| e.to_string()))
            });
        }
        let state = Arc::new(Mutex::new(CompletionState {
            result: None,
            waker: None,
        }));
        let worker_state = state.clone();
        let device = self.device.clone();
        let fault = self.fault.clone();
        let ctx = context.clone();
        let active = self.busy.clone();
        let keep_scene = scene.clone();
        std::thread::Builder::new()
            .name("geello-gpu-completion".into())
            .spawn(move || {
                let _reservation = reservation;
                let _scene = keep_scene;
                let _images = images;
                let _active = BusyGuard(Some(active));
                let mut operation_failure = None;
                let result = loop {
                    if let Err(e) = ctx.check() {
                        operation_failure = Some(e.to_string());
                    }
                    if let Some(error) = fault.lock().unwrap().clone() {
                        break Err(GeelloError::TargetInvalid(error));
                    }
                    if let Err(e) = device.poll(wgpu::PollType::Poll) {
                        break Err(GeelloError::TargetInvalid(e.to_string()));
                    }
                    let ready = completed.load(Ordering::Acquire)
                        && (!readback || mapped.lock().unwrap().is_some());
                    if ready {
                        if let Some(error) = operation_failure {
                            break Err(GeelloError::TargetInvalid(error));
                        }
                        if let Some(buffer) = &buffer {
                            if let Some(Err(e)) = mapped.lock().unwrap().take() {
                                break Err(GeelloError::TargetInvalid(e));
                            }
                            let mapped = match buffer.slice(..).get_mapped_range() {
                                Ok(view) => view,
                                Err(error) => {
                                    break Err(GeelloError::TargetInvalid(error.to_string()));
                                }
                            };
                            let mut bytes = Vec::with_capacity(row as usize * h as usize);
                            for line in mapped.chunks_exact(stride as usize) {
                                bytes.extend_from_slice(&line[..row as usize]);
                            }
                            drop(mapped);
                            buffer.unmap();
                            break Ok(Some(bytes));
                        }
                        break Ok(None);
                    }
                    std::thread::sleep(OperationContext::poll_interval());
                };
                let waker = {
                    let mut s = worker_state.lock().unwrap();
                    s.result = Some(result);
                    s.waker.take()
                };
                if let Some(w) = waker {
                    w.wake()
                }
            })
            .map_err(|e| GeelloError::TargetInvalid(e.to_string()))?;
        // Transfer lifetime ownership before the next await. Dropping the
        // caller while an error scope resolves must not free submitted work's
        // scene, reservations or exclusive-submission guard prematurely.
        busy.0.take();
        let submission_error = submission_scope.pop().await;
        let completion = Completion { state }.await;
        if let Some(error) = submission_error {
            return Err(GeelloError::TargetInvalid(error.to_string()));
        }
        completion
    }
}
struct BusyGuard(Option<Arc<AtomicBool>>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Some(b) = &self.0 {
            b.store(false, Ordering::Release)
        }
    }
}
struct CompletionState {
    result: Option<Result<Option<Vec<u8>>, GeelloError>>,
    waker: Option<Waker>,
}
struct Completion {
    state: Arc<Mutex<CompletionState>>,
}
impl Future for Completion {
    type Output = Result<Option<Vec<u8>>, GeelloError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().unwrap();
        if let Some(result) = state.result.take() {
            Poll::Ready(result)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
