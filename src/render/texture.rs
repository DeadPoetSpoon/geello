use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use vello::wgpu::{
    Device, Extent3d, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

use crate::core::error::{GeelloError, GeelloResult};

// ---------------------------------------------------------------------------
// TexturePool
// ---------------------------------------------------------------------------

/// Pre-allocated pool of reusable wgpu textures with a semaphore for
/// concurrency control.
///
/// Used primarily in server mode to bound the number of simultaneous
/// render operations.  Each call to [`acquire`](TexturePool::acquire)
/// claims a texture; the texture is automatically returned to the pool
/// when the returned [`PooledTexture`] is dropped.
pub struct TexturePool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    _device: Arc<Device>,
    textures: Vec<Arc<Texture>>,
    available: Mutex<VecDeque<usize>>,
    semaphore: Arc<Semaphore>,
    width: u32,
    height: u32,
    format: TextureFormat,
}

impl TexturePool {
    /// Create a pool with `count` pre-allocated textures.
    ///
    /// * `width`, `height` — texture dimensions in pixels.
    /// * `format` — pixel format (typically `Rgba8Unorm` for RGBA output).
    /// * `usage` — allowed texture usages (must include `COPY_SRC` for read-back).
    ///
    /// # Panics
    ///
    /// Panics if `count == 0`.
    pub fn new(
        device: Arc<Device>,
        count: usize,
        width: u32,
        height: u32,
        format: TextureFormat,
        usage: TextureUsages,
    ) -> Self {
        assert!(count > 0, "TexturePool count must be > 0");

        let textures: Vec<Arc<Texture>> = (0..count)
            .map(|i| {
                let label = format!("geello-texture-pool-{i}");
                Arc::new(device.create_texture(&TextureDescriptor {
                    label: Some(&label),
                    size: Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                }))
            })
            .collect();

        let available = Mutex::new((0..count).collect::<VecDeque<_>>());

        Self {
            inner: Arc::new(PoolInner {
                _device: device,
                textures,
                available,
                semaphore: Arc::new(Semaphore::new(count)),
                width,
                height,
                format,
            }),
        }
    }

    /// Number of textures in the pool.
    pub fn count(&self) -> usize {
        self.inner.textures.len()
    }

    /// Texture dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.inner.width, self.inner.height)
    }

    /// Pixel format of all textures.
    pub fn format(&self) -> TextureFormat {
        self.inner.format
    }

    /// Synchronously try to acquire a texture.  Returns `None` if all
    /// textures are currently in use.
    ///
    /// Prefer this in library / blocking contexts.  For async server code,
    /// use [`acquire_async`](Self::acquire_async).
    pub fn try_acquire(&self) -> Option<PooledTexture> {
        let permit = self.inner.semaphore.clone().try_acquire_owned().ok()?;
        self.inner
            .pop_available()
            .map(|(index, texture)| PooledTexture {
                _permit: permit,
                index,
                texture,
                pool: Arc::clone(&self.inner),
            })
    }

    /// Asynchronously acquire a texture.  This will wait (yield) if all
    /// textures are currently in use.
    pub async fn acquire_async(&self) -> GeelloResult<PooledTexture> {
        let permit = self
            .inner
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| GeelloError::Wgpu(format!("Semaphore closed: {e}")))?;

        // SAFETY: the semaphore guarantees an index is available.
        let (index, texture) = self
            .inner
            .pop_available()
            .expect("semaphore permits > available textures");
        Ok(PooledTexture {
            _permit: permit,
            index,
            texture,
            pool: Arc::clone(&self.inner),
        })
    }
}

impl PoolInner {
    fn pop_available(&self) -> Option<(usize, Arc<Texture>)> {
        let mut available = self.available.lock().unwrap();
        let idx = available.pop_front()?;
        let texture = Arc::clone(&self.textures[idx]);
        Some((idx, texture))
    }

    fn release(&self, index: usize) {
        let mut available = self.available.lock().unwrap();
        available.push_back(index);
    }
}

// ---------------------------------------------------------------------------
// PooledTexture
// ---------------------------------------------------------------------------

/// A texture borrowed from a [`TexturePool`].
///
/// When this value is dropped, the texture is automatically returned to
/// the pool for reuse and the concurrency permit is released.
pub struct PooledTexture {
    _permit: OwnedSemaphorePermit,
    index: usize,
    texture: Arc<Texture>,
    pool: Arc<PoolInner>,
}

impl PooledTexture {
    /// Reference to the underlying wgpu texture.
    pub fn texture(&self) -> &Texture {
        &self.texture
    }

    /// Index of this texture within the pool (stable identifier).
    pub fn index(&self) -> usize {
        self.index
    }
}

impl Drop for PooledTexture {
    fn drop(&mut self) {
        self.pool.release(self.index);
    }
}
