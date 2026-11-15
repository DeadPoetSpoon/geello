//! Resource preparation and render orchestration. Every serving graph fixes its
//! source/derived/style versions before entering queues or response caches.
use super::*;
use crate::{config::*, derived::*, readers::*, *};
use foyer::DeviceBuilder;
use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OperationStatus {
    Staging,
    Preparing,
    Ready,
    Failed,
    Unpublished,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceOperation {
    pub operation_id: String,
    pub resource: ResourceKey,
    pub status: OperationStatus,
    pub version: Option<String>,
    pub diagnostics: Vec<String>,
}
struct RecordedOperation {
    operation: ResourceOperation,
    _memory: crate::operation::Reservation,
}
impl std::ops::Deref for RecordedOperation {
    type Target = ResourceOperation;
    fn deref(&self) -> &Self::Target {
        &self.operation
    }
}
pub struct EncodedResponse {
    pub bytes: Vec<u8>,
    pub etag: String,
    pub content_type: String,
    pub render_revision: String,
    pub backend: String,
    pub incomplete: bool,
    _memory: Vec<crate::operation::Reservation>,
}
enum Backend {
    #[cfg(feature = "render-cpu")]
    Cpu,
    #[cfg(feature = "render-gpu")]
    Gpu(Arc<tokio::sync::Mutex<GpuRenderer>>),
}
type Flight = watch::Sender<Option<Result<Arc<EncodedResponse>>>>;
#[derive(Clone, Copy)]
enum RenderDelivery {
    Http,
    Animation,
}
/// Area caches retain the preparation outcome alongside pixels so a different
/// tile encode cannot lose the scene's incomplete-result summary.
struct RenderedArea {
    image: Accounted<RgbaImage>,
    incomplete: bool,
}
impl std::ops::Deref for RenderedArea {
    type Target = RgbaImage;
    fn deref(&self) -> &Self::Target {
        &self.image
    }
}
type PixelFlight = watch::Sender<Option<Result<Arc<RenderedArea>>>>;
#[derive(Clone)]
enum CachedObject {
    Image(Arc<RenderedArea>),
    Native(Arc<Accounted<RasterBlock>>),
}
impl CachedObject {
    fn weight(&self, key: &str) -> u64 {
        let bytes = match self {
            Self::Image(image) => image.pixels().len() as u64,
            Self::Native(block) => {
                // Eight bytes/sample conservatively covers every native type;
                // include masks, alpha and metadata rather than encoded length.
                let n = block.width as u64 * block.height as u64;
                n.saturating_mul(
                    (block.bands.len() as u64)
                        .saturating_mul(9)
                        .saturating_add(4),
                )
                .saturating_add(1024)
                .saturating_add((block.bands.len() as u64).saturating_mul(128))
                .saturating_add((block.grid.crs.definition().len() as u64).saturating_mul(2))
            }
        };
        bytes.saturating_add(key.len() as u64).saturating_add(256)
    }
}
pub struct ServerState {
    pub config: ServerConfig,
    pub catalog: Arc<Catalog>,
    pub scheduler: Arc<Scheduler>,
    backend: Backend,
    pub backend_name: String,
    pub backend_diagnostic: Option<String>,
    operations: Mutex<BTreeMap<String, RecordedOperation>>,
    objects: Cache<String, CachedObject>,
    responses: foyer::HybridCache<String, Vec<u8>>,
    _response_memory: crate::operation::Reservation,
    render_flights: Mutex<BTreeMap<String, PixelFlight>>,
    encode_flights: Mutex<BTreeMap<String, Flight>>,
    preparation_commit: Mutex<()>,
    overview_attempts: Mutex<BTreeMap<(ResourceKey, String), crate::operation::Reservation>>,
    restore_attempts: Mutex<BTreeMap<(ResourceKey, String), Accounted<String>>>,
    cache_failures: std::sync::atomic::AtomicU64,
    renders: std::sync::atomic::AtomicU64,
    encodes: std::sync::atomic::AtomicU64,
    native_decodes: std::sync::atomic::AtomicU64,
    pub(crate) uploads: Arc<tokio::sync::Semaphore>,
    pub(crate) requests: Arc<tokio::sync::Semaphore>,
    pub(crate) activities: Arc<super::activity::Activities>,
    pub(crate) feature_cursors: Arc<super::features::CursorStore>,
    shutdown_lock: tokio::sync::Mutex<bool>,
}
impl ServerState {
    pub async fn open(config: ServerConfig) -> Result<Arc<Self>> {
        Self::open_with_initializer(config, |ctx| async move {
            #[cfg(feature = "render-gpu")]
            {
                let (renderer, _) =
                    GpuRenderer::new(wgpu::Backends::DX12 | wgpu::Backends::VULKAN, &ctx).await?;
                Ok(Backend::Gpu(Arc::new(tokio::sync::Mutex::new(renderer))))
            }
            #[cfg(not(feature = "render-gpu"))]
            {
                let _ = ctx;
                Err(ServerError::invalid(
                    "GPU backend is not compiled; select cpu explicitly",
                ))
            }
        })
        .await
    }
    /// Injection is private and used by offline policy tests. Backend selection
    /// happens exactly once during startup; execution never invokes this hook.
    async fn open_with_initializer<F, Fut>(config: ServerConfig, initialize: F) -> Result<Arc<Self>>
    where
        F: FnOnce(OperationContext) -> Fut,
        Fut: std::future::Future<Output = Result<Backend>>,
    {
        config.validate()?;
        // Native distribution IO belongs to startup, before per-request deadlines.
        // This also fails early if the build-time data/library inventory changed.
        crate::projection::runtime_identity()?;
        let scheduler = Arc::new(Scheduler::new(config.profile.clone()));
        let memory = scheduler.memory_context();
        let catalog = Catalog::open_with_context(
            &config.data_directory,
            config.profile.clone(),
            memory.clone(),
        )?;
        let mut diagnostic = None;
        let backend = match config.backend {
            BackendSelection::Cpu => cpu_backend()?,
            BackendSelection::Gpu | BackendSelection::GpuPreferred => {
                match initialize(scheduler.context()).await {
                    Ok(backend) => backend,
                    Err(e)
                        if cfg!(feature = "render-gpu")
                            && config.backend == BackendSelection::GpuPreferred =>
                    {
                        diagnostic = Some(format!("GPU initialization failed: {e}"));
                        cpu_backend()?
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        let backend_name = match &backend {
            #[cfg(feature = "render-cpu")]
            Backend::Cpu => "cpu",
            #[cfg(feature = "render-gpu")]
            Backend::Gpu(_) => "gpu",
        }
        .to_owned();
        let p = &config.profile;
        let cache_buffer_bytes = p.response_memory_bytes.max(4096) / 4096 * 4096;
        // Foyer has two alternating flush buffers and a soft submit threshold.
        // Cover concurrent producer overshoot separately from memory retention.
        // Engine indexes and OS/allocator overhead remain external estimates.
        let cache_memory_bytes = cache_buffer_bytes
            .checked_mul(2)
            .and_then(|n| n.checked_add(p.response_memory_bytes.checked_mul(2)?))
            .and_then(|n| {
                n.checked_add(
                    response_limit(p.max_image_bytes)
                        .ok()?
                        .checked_mul(p.active_http.checked_add(p.queued_per_class)?)?,
                )
            })
            .ok_or(GeelloError::SizeOverflow)?;
        let response_memory = memory.reserve(cache_memory_bytes)?;
        let device = foyer::FsDeviceBuilder::new(catalog.root().join("response-cache"))
            .with_capacity(p.response_disk_bytes)
            .build()
            .map_err(ServerError::io)?;
        let responses = foyer::HybridCacheBuilder::new()
            .with_name("geello-responses")
            .memory(p.response_memory_bytes)
            .with_weighter(|k: &String, v: &Vec<u8>| k.len() + v.len())
            .storage()
            .with_io_engine_config(foyer::PsyncIoEngineConfig::new())
            .with_compression(foyer::Compression::None)
            .with_engine_config(
                foyer::BlockEngineConfig::new(device)
                    .with_flushers(1)
                    .with_buffer_pool_size(cache_buffer_bytes)
                    .with_submit_queue_size_threshold(p.response_memory_bytes),
            )
            .build()
            .await
            .map_err(ServerError::io)?;
        let objects = Cache::builder()
            .max_capacity(p.object_cache_bytes)
            .time_to_live(std::time::Duration::from_millis(p.operation_timeout_ms))
            .weigher(|key: &String, v: &CachedObject| v.weight(key).min(u32::MAX as u64) as u32)
            .build();
        let mut operations = BTreeMap::new();
        for entry in fs::read_dir(catalog.root().join("operations")).map_err(ServerError::io)? {
            let path = entry.map_err(ServerError::io)?.path();
            if path.extension().is_some_and(|e| e == "json") {
                if operations.len() >= p.max_operations {
                    return Err(ServerError::new(
                        503,
                        "operations-budget",
                        "Operation ledger exceeds profile",
                    ));
                }
                let bytes = memory::read_file(&path, p.max_registry_bytes, &memory)?;
                let mut op: ResourceOperation =
                    serde_json::from_slice(&bytes).map_err(ServerError::io)?;
                // Persisted metadata must not choose a write destination during
                // restart recovery. Reject a damaged authority without touching
                // paths outside the operation ledger.
                if !op.operation_id.starts_with("op-")
                    || op.operation_id.len() > 128
                    || !op
                        .operation_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    || path.file_name().and_then(|n| n.to_str())
                        != Some(format!("{}.json", op.operation_id).as_str())
                {
                    return Err(ServerError::io("invalid operation identity"));
                }
                ResourceKey::new(op.resource.kind, op.resource.id.clone())?;
                if matches!(
                    op.status,
                    OperationStatus::Staging | OperationStatus::Preparing
                ) {
                    op.status = OperationStatus::Failed;
                    op.diagnostics = vec![
                        "Server restarted before operation completion; retry with current ETag"
                            .into(),
                    ];
                    if let Ok(record) = catalog.get(&op.resource)
                        && record.version == op.operation_id
                    {
                        op.status = OperationStatus::Ready;
                        op.version = Some(record.version);
                        op.diagnostics.clear();
                    }
                    atomic_operation(catalog.root(), &op, p.max_registry_bytes, &memory)?;
                }
                let hold = memory.reserve(memory::model_bytes(&op)?)?;
                operations.insert(
                    op.operation_id.clone(),
                    RecordedOperation {
                        operation: op,
                        _memory: hold,
                    },
                );
            }
        }
        // The writer lock and recovered deletion journals prove there are no
        // live upload owners at startup. Only private upload directories are
        // reclaimable; originals and unknown staging entries remain untouched.
        catalog.collect_abandoned_uploads()?;
        let uploads = Arc::new(tokio::sync::Semaphore::new(p.active_background));
        let requests = Arc::new(tokio::sync::Semaphore::new(
            p.active_http + p.queued_per_class,
        ));
        let activities = super::activity::Activities::new();
        let feature_cursors =
            super::features::CursorStore::new(&config.profile, memory.clone(), activities.clone());
        Ok(Arc::new(Self {
            config,
            catalog,
            scheduler,
            backend,
            backend_name,
            backend_diagnostic: diagnostic,
            operations: Mutex::new(operations),
            objects,
            responses,
            _response_memory: response_memory,
            render_flights: Mutex::new(BTreeMap::new()),
            encode_flights: Mutex::new(BTreeMap::new()),
            preparation_commit: Mutex::new(()),
            overview_attempts: Mutex::new(BTreeMap::new()),
            restore_attempts: Mutex::new(BTreeMap::new()),
            cache_failures: std::sync::atomic::AtomicU64::new(0),
            renders: std::sync::atomic::AtomicU64::new(0),
            encodes: std::sync::atomic::AtomicU64::new(0),
            native_decodes: std::sync::atomic::AtomicU64::new(0),
            uploads,
            requests,
            activities,
            feature_cursors,
            shutdown_lock: tokio::sync::Mutex::new(false),
        }))
    }
    pub fn operation(&self, id: &str) -> Result<ResourceOperation> {
        self.operations
            .lock()
            .unwrap()
            .get(id)
            .map(|entry| entry.operation.clone())
            .ok_or_else(|| {
                ServerError::new(404, "operation-not-found", "Operation is not recorded")
            })
    }
    pub(crate) fn operation_accounted(&self, id: &str) -> Result<Accounted<ResourceOperation>> {
        let operations = self.operations.lock().unwrap();
        let entry = operations.get(id).ok_or_else(|| {
            ServerError::new(404, "operation-not-found", "Operation is not recorded")
        })?;
        let hold = self
            .scheduler
            .memory_context()
            .reserve(memory::model_bytes(&entry.operation)?)?;
        Ok(Accounted::new(entry.operation.clone(), vec![hold]))
    }
    /// Indexed catalog maintenance is explicit and separate from public lookup.
    /// It does not register or remove detached original content.
    pub fn collect_garbage(&self) -> Result<usize> {
        let _disk = self.preparation_commit.lock().unwrap();
        Ok(self.catalog.collect_garbage()? + self.catalog.collect_derived_orphans()?)
    }
    /// Explicit terminal-ledger retention, without an invented deployment TTL.
    /// The caller supplies an absolute modification-time cutoff and batch cap.
    /// Active work and repair diagnostics still referenced by retry suppression
    /// are retained. Removed operation IDs subsequently return 404, including
    /// after restart; this never removes resource records or original content.
    pub fn prune_operations(
        &self,
        before: std::time::SystemTime,
        max_remove: usize,
    ) -> Result<usize> {
        if max_remove == 0 || max_remove > self.config.profile.max_operations {
            return Err(ServerError::invalid(
                "Operation retention batch exceeds profile",
            ));
        }
        // Match request_base_restore's lock order to avoid a maintenance/repair
        // inversion. Do not drop failed-repair evidence and trigger retry storms.
        let mut repairs = self.restore_attempts.lock().unwrap();
        repairs.retain(|(key, version), _| self.catalog.is_current_version(key, version));
        self.overview_attempts
            .lock()
            .unwrap()
            .retain(|(key, version), _| self.catalog.is_current_version(key, version));
        let mut operations = self.operations.lock().unwrap();
        let mut removed = vec![];
        for (id, op) in operations.iter() {
            if !matches!(op.status, OperationStatus::Ready | OperationStatus::Failed)
                || repairs.values().any(|repair| &**repair == id)
            {
                continue;
            }
            let path = self
                .catalog
                .root()
                .join("operations")
                .join(format!("{id}.json"));
            let modified = fs::metadata(&path)
                .and_then(|m| m.modified())
                .map_err(ServerError::io)?;
            if modified < before {
                removed.push(id.clone());
                if removed.len() == max_remove {
                    break;
                }
            }
        }
        for id in &removed {
            fs::remove_file(
                self.catalog
                    .root()
                    .join("operations")
                    .join(format!("{id}.json")),
            )
            .map_err(ServerError::io)?;
            // Keep memory consistent after each successful deletion even if a
            // later filesystem operation in the same batch fails.
            operations.remove(id);
        }
        Ok(removed.len())
    }
    pub fn execution_counts(&self) -> (u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.renders.load(Ordering::Relaxed),
            self.encodes.load(Ordering::Relaxed),
        )
    }
    /// Native cache misses that reached the checksum/decode loader.
    pub fn native_decode_count(&self) -> u64 {
        self.native_decodes
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    /// Cache failures are observable even when a correct uncached render can
    /// proceed. A damaged encoded entry is discarded before regeneration.
    pub fn cache_failures(&self) -> u64 {
        self.cache_failures
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    pub(crate) fn new_operation(&self, key: ResourceKey) -> Result<ResourceOperation> {
        let mut all = self.operations.lock().unwrap();
        if all.len() >= self.config.profile.max_operations {
            return Err(ServerError::new(
                503,
                "operations-budget",
                "Operation ledger is full",
            ));
        }
        let op = ResourceOperation {
            operation_id: super::catalog::operation_id(),
            resource: key,
            status: OperationStatus::Staging,
            version: None,
            diagnostics: vec![],
        };
        let memory = self.scheduler.memory_context();
        let hold = memory.reserve(memory::model_bytes(&op)?)?;
        atomic_operation(
            self.catalog.root(),
            &op,
            self.config.profile.max_registry_bytes,
            &memory,
        )?;
        all.insert(
            op.operation_id.clone(),
            RecordedOperation {
                operation: op.clone(),
                _memory: hold,
            },
        );
        Ok(op)
    }
    fn set_operation(
        &self,
        mut op: ResourceOperation,
        status: OperationStatus,
        result: Option<&Result<ResourceRecord>>,
    ) -> Result<()> {
        op.status = status;
        if let Some(result) = result {
            match result {
                Ok(r) => op.version = Some(r.version.clone()),
                Err(e) => op.diagnostics = vec![e.title.clone()],
            }
        }
        let memory = self.scheduler.memory_context();
        let hold = memory.reserve(memory::model_bytes(&op)?)?;
        atomic_operation(
            self.catalog.root(),
            &op,
            self.config.profile.max_registry_bytes,
            &memory,
        )?;
        self.operations.lock().unwrap().insert(
            op.operation_id.clone(),
            RecordedOperation {
                operation: op,
                _memory: hold,
            },
        );
        Ok(())
    }
    pub(crate) fn fail_upload(&self, op: ResourceOperation, error: ServerError) {
        let _ = self.set_operation(op, OperationStatus::Failed, Some(&Err(error)));
    }
    pub(crate) fn start_prepare(
        self: &Arc<Self>,
        op: ResourceOperation,
        mut source: SourceSpec,
        ownership: Ownership,
        condition: Precondition,
        staging: Option<Arc<tempfile::TempDir>>,
    ) -> Result<()> {
        let activity = match self.activities.enter() {
            Ok(activity) => activity,
            Err(e) => {
                self.fail_upload(op, e.clone());
                return Err(e);
            }
        };
        let state = self.clone();
        tokio::spawn(async move {
            let _activity = activity;
            let _ = state.set_operation(op.clone(), OperationStatus::Preparing, None);
            let worker = state.clone();
            let key = op.resource.clone();
            let opid = op.operation_id.clone();
            let ctx = state.scheduler.context();
            let result = state
                .scheduler
                .blocking(WorkClass::Background, ctx, move |ctx| {
                    // Serialize disk-producing preparation separately from the
                    // catalog mutex. Readers keep serving the complete old graph.
                    let _disk = worker.preparation_commit.lock().unwrap();
                    ctx.check()?;
                    worker.catalog.check(&key, &condition)?;
                    worker.catalog.collect_garbage()?;
                    worker.catalog.collect_derived_orphans()?;
                    let derived_used =
                        directory_bytes(&worker.catalog.root().join("derived"), &ctx)?;
                    if derived_used >= worker.config.profile.max_derived_bytes {
                        return Err(ServerError::new(
                            503,
                            "derived-disk",
                            "Derived disk profile is full",
                        ));
                    }
                    let mut final_directory = None;
                    if let Some(stage) = &staging {
                        if source.format == "shapefile-zip" {
                            source.path = super::archive::extract(
                                &source.path,
                                &stage.path().join("unpacked"),
                                &worker.config.profile,
                                &ctx,
                            )?;
                            source.format = "shapefile".into();
                        }
                        let final_path = worker.catalog.root().join("managed").join(&opid);
                        let relative = source
                            .path
                            .strip_prefix(stage.path())
                            .map_err(ServerError::io)?
                            .to_owned();
                        // Source validation precedes ownership publication. A failed
                        // version remains private staging and cannot replace live data.
                        final_directory = Some((final_path, relative));
                    } else {
                        source.path = worker.validate_external(&source.path, &source.format)?;
                    }
                    let mut record = worker.prepare_resource(key, source, ownership, &ctx)?;
                    record.version = opid;
                    if let Some((dest, relative)) = final_directory {
                        let stage = staging.as_ref().unwrap();
                        let current =
                            directory_bytes(&worker.catalog.root().join("managed"), &ctx)?;
                        let extra = directory_bytes(stage.path(), &ctx)?;
                        if current
                            .checked_add(extra)
                            .is_none_or(|v| v > worker.config.profile.max_managed_bytes)
                        {
                            return Err(ServerError::new(
                                503,
                                "managed-disk",
                                "Managed content disk profile is full",
                            ));
                        }
                        fs::rename(stage.path(), &dest).map_err(ServerError::io)?;
                        if record.source.path.starts_with(stage.path()) {
                            record.source.path = dest.join(relative);
                        }
                        record.managed_directory = Some(dest);
                    }
                    // Cancellation after a committed publication must never
                    // roll back acknowledged catalog state. Check before commit.
                    let publication = ctx
                        .check()
                        .map_err(ServerError::from)
                        .and_then(|()| worker.catalog.publish(record.clone(), &condition));
                    match publication {
                        Ok(r) => Ok(r),
                        Err(e) => {
                            if let Some(path) = &record.managed_directory {
                                let resolved = path.canonicalize().map_err(ServerError::io)?;
                                if resolved.parent()
                                    == Some(worker.catalog.root().join("managed").as_path())
                                {
                                    let _ = fs::remove_dir_all(resolved);
                                }
                            }
                            Err(e)
                        }
                    }
                })
                .await;
            let status = if result.is_ok() {
                OperationStatus::Ready
            } else {
                OperationStatus::Failed
            };
            let _ = state.set_operation(op, status, Some(&result));
        });
        Ok(())
    }
    fn validate_external(&self, path: &Path, format: &str) -> Result<PathBuf> {
        let canonical = path
            .canonicalize()
            .map_err(|_| ServerError::invalid("External path cannot be resolved"))?;
        let roots = self
            .config
            .external_roots
            .iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect::<Vec<_>>();
        if !roots.iter().any(|r| canonical.starts_with(r)) {
            return Err(ServerError::invalid(
                "External path is outside configured roots",
            ));
        }
        if !canonical.is_file() {
            return Err(ServerError::invalid(
                "External source must be a regular file",
            ));
        }
        if format == "shapefile" {
            for ext in ["shx", "dbf", "prj", "cpg"] {
                let side = canonical.with_extension(ext);
                if side.exists() {
                    let side = side.canonicalize().map_err(ServerError::io)?;
                    if !roots.iter().any(|r| side.starts_with(r)) {
                        return Err(ServerError::invalid(
                            "Shapefile sidecar escapes external roots",
                        ));
                    }
                }
            }
        }
        Ok(canonical)
    }
    fn prepare_resource(
        &self,
        key: ResourceKey,
        source: SourceSpec,
        ownership: Ownership,
        ctx: &OperationContext,
    ) -> Result<ResourceRecord> {
        self.prepare_resource_definition(key, source, ownership, None, ctx)
    }
    fn prepare_resource_definition(
        &self,
        key: ResourceKey,
        mut source: SourceSpec,
        ownership: Ownership,
        dataset_body: Option<String>,
        ctx: &OperationContext,
    ) -> Result<ResourceRecord> {
        let p = &self.config.profile;
        let mut store_limits = p.store_limits();
        store_limits.max_disk_bytes = store_limits
            .max_disk_bytes
            .saturating_sub(directory_bytes(&self.catalog.root().join("derived"), ctx)?);
        let source_fingerprint = fingerprint(&source, p.max_source_bytes, ctx)?;
        let data_version = super::catalog::identity(&[
            source_fingerprint.as_bytes(),
            dataset_body.as_deref().unwrap_or("").as_bytes(),
        ]);
        let mut dependencies = vec![];
        let mut derived_version = None;
        let mut derived_path = None;
        let mut class = None;
        let mut effective_crs = None;
        let mut definition = dataset_body;
        let mut type_bindings = vec![];
        let mut definition_holds = vec![];
        match key.kind {
            ResourceKind::Dataset => {
                if source.format == "geotiff"
                    && !matches!(
                        source.sample_kind.as_deref(),
                        Some("continuous" | "categorical")
                    )
                {
                    return Err(ServerError::invalid(
                        "GeoTIFF requires explicit continuous or categorical sample_kind",
                    ));
                }
                if source.format == "toml" {
                    let (bytes, reservations) =
                        memory::read_file(&source.path, p.max_registry_bytes, ctx)?.into_parts();
                    definition_holds.extend(reservations);
                    let text = String::from_utf8(bytes)
                        .map_err(|_| ServerError::invalid("TOML must be UTF-8"))?;
                    let definition = parse_dataset(&text, &source.path)?;
                    if definition.id != key.id {
                        return Err(ServerError::invalid("Dataset body ID differs from route"));
                    }
                    let resolved =
                        definition.source.path.canonicalize().map_err(|_| {
                            ServerError::invalid("Dataset source cannot be resolved")
                        })?;
                    let registered = self.catalog.find_dataset_source(&resolved)?;
                    if ownership == Ownership::Managed && registered.is_none() {
                        return Err(ServerError::invalid(
                            "Managed dataset TOML must reference an already registered source path",
                        ));
                    }
                    source = SourceSpec {
                        path: if registered.is_some() {
                            resolved
                        } else {
                            self.validate_external(
                                &definition.source.path,
                                &definition.source.kind,
                            )?
                        },
                        format: definition.source.kind,
                        encoding: definition.source.encoding,
                        crs: definition.source.crs,
                        sample_kind: definition.source.sample_kind,
                    };
                    let mut result =
                        self.prepare_resource_definition(key, source, ownership, Some(text), ctx)?;
                    if let Some(k) = registered
                        && k != result.key
                    {
                        result.dependencies.push(k);
                    }
                    return Ok(result);
                }
                class = Some(
                    if source.format == "geotiff" {
                        "raster"
                    } else {
                        "vector"
                    }
                    .into(),
                );
                let parent = self.catalog.root().join("derived");
                if source.format == "geotiff" {
                    #[cfg(feature = "read-geotiff")]
                    {
                        let snap = open_geotiff(
                            &source.path,
                            &RasterReadOptions {
                                crs_override: source.crs.as_deref().map(parse_crs).transpose()?,
                                primary_ifd: None,
                                max_source_bytes: p.max_source_bytes,
                                max_decode_bytes: p.max_live_bytes.min(usize::MAX as u64) as usize
                                    / 4,
                                max_tag_bytes: p.max_registry_bytes,
                            },
                            ctx,
                        )?;
                        let m = snap.metadata();
                        effective_crs = Some(m.grid.crs.clone());
                        let store = DerivedRaster::build_base(
                            &parent,
                            &data_version,
                            m.width,
                            m.height,
                            &m.grid,
                            p.block_size,
                            store_limits,
                            ctx,
                            |w, c| {
                                Ok((*snap.read_native_window(
                                    NativeWindow {
                                        column: w.column,
                                        row: w.row,
                                        width: w.width,
                                        height: w.height,
                                    },
                                    c,
                                )?)
                                .clone())
                            },
                        )?;
                        derived_path = Some(parent.join(store.derived_version()));
                        derived_version = Some(store.derived_version().into());
                    }
                    #[cfg(not(feature = "read-geotiff"))]
                    return Err(ServerError::invalid("GeoTIFF reader is not compiled"));
                } else {
                    #[cfg(not(any(feature = "read-geojson", feature = "read-shapefile")))]
                    return Err(ServerError::invalid("No vector reader is compiled"));
                    #[cfg(any(feature = "read-geojson", feature = "read-shapefile"))]
                    {
                        let mut options = VectorReadOptions {
                            data_version: data_version.clone(),
                            crs_override: source.crs.as_deref().map(parse_crs).transpose()?,
                            limits: p.reader_limits(),
                            policy: ReadPolicy::Strict,
                            identity: IdentityPolicy::OriginalOrOrdinal,
                            queryables: vec![],
                        };
                        if let Some(text) = &definition {
                            let dataset = parse_dataset(text, &source.path)?;
                            if let Some(id) = dataset.identity
                                && id.kind == "attribute"
                            {
                                options.identity =
                                    IdentityPolicy::Attribute(id.field.ok_or_else(|| {
                                        ServerError::invalid("Missing identity field")
                                    })?);
                            }
                            options.queryables = dataset
                                .queryables
                                .into_iter()
                                .map(|q| {
                                    Ok(Queryable {
                                        name: q.name,
                                        field: q.field,
                                        kind: match q.kind.as_str() {
                                            "string" => ScalarType::String,
                                            "boolean" => ScalarType::Boolean,
                                            "integer" => ScalarType::Integer,
                                            "number" => ScalarType::Number,
                                            _ => {
                                                return Err(ServerError::invalid(
                                                    "Unknown queryable type",
                                                ));
                                            }
                                        },
                                    })
                                })
                                .collect::<Result<_>>()?;
                        }
                        let snapshot: VectorSnapshot = match source.format.as_str() {
                            #[cfg(feature = "read-geojson")]
                            "geojson" => open_geojson(&source.path, &options, ctx)?,
                            #[cfg(feature = "read-shapefile")]
                            "shapefile" => open_shapefile_with(
                                &source.path,
                                &options,
                                &ShapefileOptions {
                                    encoding: source.encoding.clone(),
                                },
                                ctx,
                            )?,
                            _ => {
                                return Err(ServerError::invalid(
                                    "Dataset reader is not compiled or format is unsupported",
                                ));
                            }
                        };
                        let store = DerivedVector::build(&parent, &snapshot, store_limits, ctx)?;
                        effective_crs = Some(snapshot.metadata().crs.clone());
                        derived_path = Some(parent.join(store.derived_version()));
                        derived_version = Some(store.derived_version().into());
                    }
                }
            }
            ResourceKind::Style | ResourceKind::Map | ResourceKind::Animation => {
                if source.format != "toml" {
                    return Err(ServerError::invalid("Definition resource requires TOML"));
                }
                let (bytes, reservations) =
                    memory::read_file(&source.path, p.max_registry_bytes, ctx)?.into_parts();
                definition_holds.extend(reservations);
                let text = String::from_utf8(bytes)
                    .map_err(|_| ServerError::invalid("TOML must be UTF-8"))?;
                let id = match key.kind {
                    ResourceKind::Style => {
                        let s = parse_style(&text)?;
                        for symbol in s.rules.iter().flat_map(|r| &r.symbols) {
                            if let SymbolDefinition::Label { fonts, .. } = symbol {
                                for font in fonts {
                                    let key = ResourceKey::new(ResourceKind::Font, &font.resource)?;
                                    let record = self.catalog.get_accounted(&key)?;
                                    let bytes = memory::read_file(
                                        &record
                                            .derived_path
                                            .as_ref()
                                            .ok_or_else(|| {
                                                ServerError::invalid(
                                                    "Font requires refresh before use",
                                                )
                                            })?
                                            .join("font.bin"),
                                        p.max_upload_bytes,
                                        ctx,
                                    )?;
                                    crate::text::validate_font(&FontResource {
                                        identity: record.version.clone(),
                                        bytes: Arc::from(bytes.as_slice()),
                                        face_index: font.face_index,
                                    })?;
                                    dependencies.push(key);
                                }
                            }
                        }
                        let raster_count = s
                            .rules
                            .iter()
                            .flat_map(|r| &r.symbols)
                            .filter(|s| matches!(s, SymbolDefinition::Raster { .. }))
                            .count();
                        let total = s.rules.iter().map(|r| r.symbols.len()).sum::<usize>();
                        if raster_count != 0 && raster_count != total {
                            return Err(ServerError::invalid(
                                "A style cannot mix raster and vector symbols",
                            ));
                        }
                        class = Some(
                            if s.rules
                                .iter()
                                .flat_map(|r| &r.symbols)
                                .any(|s| matches!(s, SymbolDefinition::Raster { .. }))
                            {
                                "raster"
                            } else {
                                "vector"
                            }
                            .into(),
                        );
                        s.id
                    }
                    ResourceKind::Map => {
                        let m = parse_map(&text)?;
                        for i in &m.items {
                            dependencies.push(ResourceKey::new(ResourceKind::Dataset, &i.dataset)?);
                            dependencies.push(ResourceKey::new(ResourceKind::Style, &i.style)?);
                            type_bindings.push((
                                ResourceKey::new(ResourceKind::Dataset, &i.dataset)?,
                                ResourceKey::new(ResourceKind::Style, &i.style)?,
                            ));
                            for a in &i.animation_bindings {
                                dependencies
                                    .push(ResourceKey::new(ResourceKind::Animation, &a.animation)?);
                            }
                            for a in &i.geometry_bindings {
                                dependencies
                                    .push(ResourceKey::new(ResourceKind::Animation, &a.animation)?);
                            }
                        }
                        for item in &m.items {
                            let d = self.catalog.get_accounted(&ResourceKey::new(
                                ResourceKind::Dataset,
                                &item.dataset,
                            )?)?;
                            let s = self.catalog.get_accounted(&ResourceKey::new(
                                ResourceKind::Style,
                                &item.style,
                            )?)?;
                            if d.class != s.class {
                                return Err(ServerError::invalid(
                                    "Map dataset/style type mismatch",
                                ));
                            }
                        }
                        m.id
                    }
                    _ => parse_animation(&text)?.id,
                };
                if id != key.id {
                    return Err(ServerError::invalid("Resource body ID differs from route"));
                }
                definition = Some(text);
            }
            ResourceKind::Font => {
                let bytes = memory::read_file(&source.path, p.max_upload_bytes, ctx)?;
                if bytes.len() < 12 || !matches!(&bytes[..4], b"\0\x01\0\0" | b"OTTO" | b"ttcf") {
                    return Err(ServerError::invalid("Unsupported font container"));
                }
                crate::text::validate_font(&FontResource {
                    identity: data_version.clone(),
                    bytes: Arc::from(bytes.as_slice()),
                    face_index: 0,
                })?;
                let parent = self.catalog.root().join("derived");
                let path = parent.join(format!("font-{data_version}"));
                fs::create_dir_all(&path).map_err(ServerError::io)?;
                if directory_bytes(&parent, ctx)?.saturating_add(bytes.len() as u64)
                    > p.max_derived_bytes
                {
                    return Err(ServerError::new(
                        503,
                        "derived-budget",
                        "Font snapshot exceeds derived disk profile",
                    ));
                }
                super::catalog::atomic_write(&path.join("font.bin"), &bytes)?;
                super::catalog::atomic_write(
                    &path.join("manifest.json"),
                    b"{\"kind\":\"font\",\"schema\":1}",
                )?;
                derived_version = Some(format!("font-{data_version}"));
                derived_path = Some(path);
            }
        }
        if fingerprint(&source, p.max_source_bytes, ctx)? != source_fingerprint {
            return Err(GeelloError::SourceChanged.into());
        }
        let version = super::catalog::identity(&[
            key.id.as_bytes(),
            data_version.as_bytes(),
            derived_version.as_deref().unwrap_or("").as_bytes(),
            super::catalog::operation_id().as_bytes(),
        ]);
        Ok(ResourceRecord {
            key,
            ownership,
            version,
            published: true,
            source,
            dependencies,
            data_version,
            derived_version,
            derived_path,
            managed_directory: None,
            class,
            effective_crs,
            definition,
            type_bindings,
        })
    }
    /// Close request/task admission before cancelling workers. Call this when
    /// the listener starts graceful shutdown, not after waiting for HTTP bodies.
    pub fn begin_shutdown(&self) {
        self.activities.close();
        self.uploads.close();
        self.requests.close();
        self.scheduler.begin_shutdown();
    }
    pub fn is_shutting_down(&self) -> bool {
        self.activities.is_closed()
    }
    /// Wait for handlers, background orchestration and detached blocking work;
    /// only then release object-cache reservations and close response storage.
    pub async fn shutdown(&self) -> Result<()> {
        self.begin_shutdown();
        let mut complete = self.shutdown_lock.lock().await;
        if *complete {
            return Ok(());
        }
        self.activities.drained().await;
        self.scheduler.shutdown().await;
        self.objects.invalidate_all();
        self.objects.run_pending_tasks();
        self.responses.close().await.map_err(ServerError::io)?;
        *complete = true;
        Ok(())
    }
    /// Overviews are lazy background work, deduplicated per immutable base.
    /// Serving continues with the pinned base while a complete replacement is
    /// built. Commit rechecks the original ETag; failures retain the base and
    /// leave a persistent operation diagnostic rather than a retry storm.
    fn request_overviews(self: &Arc<Self>, record: &ResourceRecord) {
        if record.class.as_deref() != Some("raster") {
            return;
        }
        let Ok(activity) = self.activities.enter() else {
            return;
        };
        let Ok(bytes) = memory::model_bytes(record) else {
            return;
        };
        let Ok(record_hold) = self.scheduler.memory_context().reserve(bytes) else {
            return;
        };
        let identity = (record.key.clone(), record.version.clone());
        {
            let mut attempts = self.overview_attempts.lock().unwrap();
            attempts.retain(|(key, version), _| self.catalog.is_current_version(key, version));
            if attempts.contains_key(&identity)
                || attempts.len() >= self.config.profile.max_operations
            {
                return;
            }
            let Ok(bytes) = memory::model_bytes(&identity) else {
                return;
            };
            let Ok(hold) = self.scheduler.memory_context().reserve(bytes) else {
                return;
            };
            attempts.insert(identity, hold);
        }
        let state = self.clone();
        let record = record.clone();
        tokio::spawn(async move {
            let _activity = activity;
            let Some(path) = &record.derived_path else {
                return;
            };
            let ctx = state.scheduler.context();
            let Ok(base) =
                DerivedRaster::open_with_context(path, state.config.profile.store_limits(), &ctx)
            else {
                return;
            };
            if base.level_count() != 1 || base.dimensions() == (1, 1) {
                return;
            }
            let op = match state.new_operation(record.key.clone()) {
                Ok(op) => op,
                Err(_) => return,
            };
            let _ = state.set_operation(op.clone(), OperationStatus::Preparing, None);
            let worker = state.clone();
            let opid = op.operation_id.clone();
            let ctx = state.scheduler.context();
            let result = state
                .scheduler
                .blocking(WorkClass::Background, ctx, move |ctx| {
                    let _record_memory = record_hold;
                    let _disk = worker.preparation_commit.lock().unwrap();
                    ctx.check()?;
                    let condition = Precondition::Version(record.version.clone());
                    worker.catalog.check(&record.key, &condition)?;
                    let _pin = worker.catalog.pin(&record.key)?;
                    worker.catalog.collect_garbage()?;
                    let parent = worker.catalog.root().join("derived");
                    let mut limits = worker.config.profile.store_limits();
                    limits.max_disk_bytes = limits
                        .max_disk_bytes
                        .saturating_sub(directory_bytes(&parent, &ctx)?);
                    let base = DerivedRaster::open_with_context(
                        record.derived_path.as_ref().unwrap(),
                        limits,
                        &ctx,
                    )?;
                    let overview = base.build_overviews(
                        &parent,
                        record.source.sample_kind.as_deref() == Some("categorical"),
                        &ctx,
                    )?;
                    let mut replacement = record;
                    replacement.version = opid;
                    replacement.derived_version = Some(overview.derived_version().into());
                    replacement.derived_path = Some(parent.join(overview.derived_version()));
                    ctx.check()?;
                    worker.catalog.publish(replacement, &condition)
                })
                .await;
            let status = if result.is_ok() {
                OperationStatus::Ready
            } else {
                OperationStatus::Failed
            };
            let _ = state.set_operation(op, status, Some(&result));
        });
    }
    /// A catalog may survive removal/loss of disposable derived artifacts.
    /// First use starts one bounded repair and returns a preparing response.
    /// It must reproduce the pinned DataVersion: changed originals require an
    /// explicit administrative refresh rather than becoming a silent update.
    fn request_base_restore(self: &Arc<Self>, record: &ResourceRecord) -> Result<()> {
        let identity = (record.key.clone(), record.version.clone());
        let mut attempts = self.restore_attempts.lock().unwrap();
        attempts.retain(|(key, version), _| self.catalog.is_current_version(key, version));
        let id = if let Some(id) = attempts.get(&identity) {
            (**id).clone()
        } else {
            if attempts.len() >= self.config.profile.max_operations {
                return Err(ServerError::new(
                    503,
                    "preparation-capacity",
                    "Preparation capacity exhausted",
                ));
            }
            let activity = self.activities.enter()?;
            let hold = self.scheduler.memory_context().reserve(
                memory::model_bytes(&identity)?
                    .checked_add(memory::envelope(128)?)
                    .ok_or(GeelloError::SizeOverflow)?,
            )?;
            let record_hold = self
                .scheduler
                .memory_context()
                .reserve(memory::model_bytes(record)?)?;
            let op = self.new_operation(record.key.clone())?;
            let id = op.operation_id.clone();
            attempts.insert(identity, Accounted::new(id.clone(), vec![hold]));
            let state = self.clone();
            let record = record.clone();
            tokio::spawn(async move {
                let _activity = activity;
                let _ = state.set_operation(op.clone(), OperationStatus::Preparing, None);
                let worker = state.clone();
                let opid = op.operation_id.clone();
                let ctx = state.scheduler.context();
                let result = state
                    .scheduler
                    .blocking(WorkClass::Background, ctx, move |ctx| {
                        let _record_memory = record_hold;
                        let _disk = worker.preparation_commit.lock().unwrap();
                        ctx.check()?;
                        let condition = Precondition::Version(record.version.clone());
                        worker.catalog.check(&record.key, &condition)?;
                        let _pin = worker.catalog.pin(&record.key)?;
                        let mut replacement = worker.prepare_resource_definition(
                            record.key.clone(),
                            record.source.clone(),
                            record.ownership,
                            record.definition.clone(),
                            &ctx,
                        )?;
                        if replacement.data_version != record.data_version {
                            return Err(ServerError::conflict(
                                "Original source changed; explicit refresh required",
                            ));
                        }
                        replacement.version = opid;
                        replacement.managed_directory = record.managed_directory;
                        replacement.dependencies = record.dependencies;
                        ctx.check()?;
                        worker.catalog.publish(replacement, &condition)
                    })
                    .await;
                let status = if result.is_ok() {
                    OperationStatus::Ready
                } else {
                    OperationStatus::Failed
                };
                let _ = state.set_operation(op, status, Some(&result));
            });
            id
        };
        let operation = self.operation(&id)?;
        if operation.status == OperationStatus::Failed {
            return Err(ServerError::new(
                503,
                "preparation-failed",
                format!("Derived preparation failed; operation {id}"),
            ));
        }
        Err(ServerError::new(
            503,
            "preparing",
            format!("Derived preparation in progress; operation {id}"),
        ))
    }
    /// A fixed graph identity includes backend, ordered resource versions and
    /// every normalized request field. Cache lookup follows publication checks.
    pub async fn render_map(
        self: &Arc<Self>,
        id: &str,
        view: View,
    ) -> Result<Arc<EncodedResponse>> {
        self.render_map_at(id, view, 0.).await
    }
    /// Render a fixed published graph at finite, nonnegative seconds. Time is
    /// part of cache/singleflight identity, not the resource RenderRevision.
    pub async fn render_map_at(
        self: &Arc<Self>,
        id: &str,
        view: View,
        time: f64,
    ) -> Result<Arc<EncodedResponse>> {
        self.render_request(
            id,
            view,
            None,
            time,
            None,
            (RenderDelivery::Http, crate::encoding::ImageFormat::Png),
        )
        .await
    }
    /// Intermediate animation frames use animation admission and never enter
    /// the HTTP response or rendered-area caches/singleflight tables.
    #[cfg(test)]
    pub(crate) async fn render_animation_at(
        self: &Arc<Self>,
        id: &str,
        view: View,
        time: f64,
    ) -> Result<Arc<EncodedResponse>> {
        self.render_request(
            id,
            view,
            None,
            time,
            None,
            (RenderDelivery::Animation, crate::encoding::ImageFormat::Png),
        )
        .await
    }
    pub(crate) async fn render_animation_format(
        self: &Arc<Self>,
        id: &str,
        view: View,
        time: f64,
        format: crate::encoding::ImageFormat,
    ) -> Result<Arc<EncodedResponse>> {
        self.render_request(
            id,
            view,
            None,
            time,
            None,
            (RenderDelivery::Animation, format),
        )
        .await
    }
    pub async fn render_tile(
        self: &Arc<Self>,
        id: &str,
        z: u32,
        x: u32,
        y: u32,
    ) -> Result<Arc<EncodedResponse>> {
        self.render_tile_at(id, z, x, y, 0.).await
    }
    /// Legacy multi-map requests share one pinned generation, canvas, global
    /// label pass and final encoding. Request background applies exactly once.
    pub(crate) async fn render_protocol_maps(
        self: &Arc<Self>,
        ids: Vec<String>,
        view: View,
        time: f64,
        background: Option<Color>,
        format: crate::encoding::ImageFormat,
    ) -> Result<Arc<EncodedResponse>> {
        let _activity = self.activities.enter()?;
        crate::animation::validate_time(time)?;
        view.validate()?;
        if ids.is_empty()
            || ids.len() > self.config.profile.max_list_items
            || view.width > self.config.profile.max_dimension
            || view.height > self.config.profile.max_dimension
        {
            return Err(ServerError::invalid("Map request exceeds profile"));
        }
        let keys = ids
            .iter()
            .map(|id| ResourceKey::new(ResourceKind::Map, id))
            .collect::<Result<Vec<_>>>()?;
        let pin = self.catalog.pin_many(&keys)?;
        let ctx = self.scheduler.context();
        let identities = pin
            .records
            .values()
            .map(|r| (&r.key, &r.version))
            .collect::<Vec<_>>();
        let bytes = crate::derived::bounded_json(
            &identities,
            self.config.profile.max_registry_bytes,
            &ctx,
        )?;
        let revision =
            super::catalog::identity(&[&bytes, crate::projection::runtime_identity()?.as_bytes()]);
        let worker = self.clone();
        let scene = self
            .scheduler
            .blocking(WorkClass::Http, ctx.clone(), move |c| {
                let mut maps = Vec::new();
                let mut holds = Vec::new();
                for key in keys {
                    let definition = pin.records[&key]
                        .definition
                        .as_deref()
                        .ok_or_else(|| ServerError::io("missing map"))?;
                    holds.push(c.reserve(memory::envelope(definition.len())?)?);
                    maps.push(parse_map(definition)?);
                }
                worker.prepare_maps(&maps, &pin, &view, time, background, &c)
            })
            .await?;
        let incomplete = scene.summary().incompatible_components > 0;
        let pixels = self.execute_pixels(scene, &ctx, WorkClass::Http).await?;
        self.encodes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let max = self.config.profile.max_image_bytes;
        let encoded = self
            .scheduler
            .blocking(WorkClass::Http, ctx, move |c| {
                Ok(crate::encoding::encode(&pixels, format, max, &c)?)
            })
            .await?;
        let etag = format!("\"{}\"", blake3::hash(&encoded).to_hex());
        let (bytes, encoding_holds) = encoded.into_parts();
        let hold = self.scheduler.memory_context().reserve(
            bytes
                .capacity()
                .checked_add(RESPONSE_HEADER_MAX)
                .ok_or(GeelloError::SizeOverflow)?,
        )?;
        drop(encoding_holds);
        Ok(Arc::new(EncodedResponse {
            bytes,
            etag,
            content_type: match format {
                crate::encoding::ImageFormat::Png => "image/png",
                _ => "image/jpeg",
            }
            .into(),
            render_revision: revision,
            backend: self.backend_name.clone(),
            incomplete,
            _memory: vec![hold],
        }))
    }
    pub async fn render_tile_at(
        self: &Arc<Self>,
        id: &str,
        z: u32,
        x: u32,
        y: u32,
        time: f64,
    ) -> Result<Arc<EncodedResponse>> {
        let grid = super::tile_grid::TileGrid::WebMercatorQuad;
        let (view, crop) = grid.layout(z, x, y, &self.config.profile)?;
        self.render_request(
            id,
            view,
            Some(crop),
            time,
            None,
            (RenderDelivery::Http, crate::encoding::ImageFormat::Png),
        )
        .await
    }
    /// Protocol tile request: publication and grid are checked against the same
    /// resource pin used for rendering/cache identity, including warm responses.
    pub(crate) async fn render_protocol_tile_at(
        self: &Arc<Self>,
        id: &str,
        grid: super::tile_grid::TileGrid,
        coordinates: [u32; 3],
        time: f64,
    ) -> Result<Arc<EncodedResponse>> {
        let [z, x, y] = coordinates;
        let (view, crop) = grid.layout(z, x, y, &self.config.profile)?;
        self.render_request(
            id,
            view,
            Some(crop),
            time,
            Some((grid, z)),
            (RenderDelivery::Http, crate::encoding::ImageFormat::Png),
        )
        .await
    }
    async fn render_request(
        self: &Arc<Self>,
        id: &str,
        view: View,
        crop: Option<[u32; 4]>,
        time: f64,
        protocol_tile: Option<(super::tile_grid::TileGrid, u32)>,
        output: (RenderDelivery, crate::encoding::ImageFormat),
    ) -> Result<Arc<EncodedResponse>> {
        let (delivery, image_format) = output;
        let _activity = self.activities.enter()?;
        crate::animation::validate_time(time)
            .map_err(|_| ServerError::invalid("Invalid animation time"))?;
        let time = if time == 0. { 0. } else { time };
        view.validate()?;
        if view.width > self.config.profile.max_dimension
            || view.height > self.config.profile.max_dimension
        {
            return Err(ServerError::invalid("Canvas exceeds profile"));
        }
        let pin = self
            .catalog
            .pin(&ResourceKey::new(ResourceKind::Map, id)?)?;
        let map_key = ResourceKey::new(ResourceKind::Map, id)?;
        let record = &pin.records[&map_key];
        if let Some((grid, z)) = protocol_tile {
            let text = record
                .definition
                .as_deref()
                .ok_or_else(|| ServerError::io("missing map definition"))?;
            let _hold = self
                .scheduler
                .memory_context()
                .reserve(memory::envelope(text.len())?)?;
            let map = parse_map(text).map_err(ServerError::from)?;
            super::tile_grid::validate_publication(&map, grid, z)?;
        }
        for resource in pin.records.values() {
            if resource.key.kind == ResourceKind::Dataset
                && resource
                    .derived_path
                    .as_ref()
                    .is_none_or(|p| !p.join("manifest.json").is_file())
            {
                self.request_base_restore(resource)?;
            }
            self.request_overviews(resource);
        }
        let ctx = self.scheduler.context();
        let _graph_memory = ctx.reserve(memory::envelope(
            pin.records
                .len()
                .checked_mul(64)
                .ok_or(GeelloError::SizeOverflow)?,
        )?)?;
        let versions = pin
            .records
            .values()
            .map(|r| (&r.key, &r.version, &r.data_version, &r.derived_version))
            .collect::<Vec<_>>();
        let version_bytes =
            crate::derived::bounded_json(&versions, self.config.profile.max_registry_bytes, &ctx)?;
        let revision = super::catalog::identity(&[
            &version_bytes,
            crate::projection::runtime_identity()?.as_bytes(),
        ]);
        if matches!(delivery, RenderDelivery::Animation) {
            drop(version_bytes);
            let definition_hold = ctx.reserve(memory::envelope(
                record.definition.as_ref().map_or(0, String::len),
            )?)?;
            let definition = record
                .definition
                .clone()
                .ok_or_else(|| ServerError::io("missing immutable map definition"))?;
            let definition = Accounted::new(definition, vec![definition_hold]);
            let worker = self.clone();
            let scene = self
                .scheduler
                .blocking(WorkClass::Animation, ctx.clone(), move |c| {
                    worker.prepare_definition(&definition, &pin, &view, time, &c)
                })
                .await?;
            let incomplete = scene.summary().incompatible_components > 0;
            let pixels = self
                .execute_pixels(scene, &ctx, WorkClass::Animation)
                .await?;
            self.encodes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let max = self.config.profile.max_image_bytes;
            let encoded = self
                .scheduler
                .blocking(WorkClass::Animation, ctx.clone(), move |c| {
                    Ok(crate::encoding::encode(&pixels, image_format, max, &c)?)
                })
                .await?;
            let etag = format!("\"{}\"", blake3::hash(&encoded).to_hex());
            let (bytes, encoding_holds) = encoded.into_parts();
            let hold = self.scheduler.memory_context().reserve(
                bytes
                    .capacity()
                    .checked_add(RESPONSE_HEADER_MAX)
                    .ok_or(GeelloError::SizeOverflow)?,
            )?;
            drop(encoding_holds);
            return Ok(Arc::new(EncodedResponse {
                bytes,
                etag,
                content_type: match image_format {
                    crate::encoding::ImageFormat::Png => "image/png",
                    crate::encoding::ImageFormat::Jpeg { .. } => "image/jpeg",
                }
                .into(),
                render_revision: revision,
                backend: self.backend_name.clone(),
                incomplete,
                _memory: vec![hold],
            }));
        }
        let view_bytes =
            crate::derived::bounded_json(&view, self.config.profile.max_registry_bytes, &ctx)?;
        let crop_bytes =
            crate::derived::bounded_json(&crop, self.config.profile.max_registry_bytes, &ctx)?;
        let area_key = super::catalog::identity(&[
            // Bump when scene/response semantics change across binary upgrades.
            b"geello-text-render-v5",
            revision.as_bytes(),
            self.backend_name.as_bytes(),
            &view_bytes,
            &time.to_le_bytes(),
        ]);
        let key = super::catalog::identity(&[area_key.as_bytes(), &crop_bytes, b"png"]);
        let cache_limit = response_limit(self.config.profile.max_image_bytes)?;
        let _cache_read = ctx.reserve(cache_limit)?;
        let cached = self.responses.get(&key).await;
        if cached.is_err() {
            self.cache_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let Ok(Some(entry)) = cached {
            match decode_response(
                entry.value(),
                &key,
                self.config.profile.max_image_bytes,
                &ctx,
            ) {
                Ok(value) => return Ok(Arc::new(value)),
                Err(error) if error.status == 503 => return Err(error),
                Err(_) => {}
            }
            self.responses.remove(&key);
            self.cache_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        drop(_cache_read);
        drop(version_bytes);
        drop(view_bytes);
        drop(crop_bytes);
        let mut receiver = {
            let mut flights = self.encode_flights.lock().unwrap();
            if let Some(tx) = flights.get(&key) {
                tx.subscribe()
            } else {
                if flights.len()
                    >= self.config.profile.queued_per_class + self.config.profile.active_http
                {
                    return Err(ServerError::new(
                        503,
                        "queue-full",
                        "Render singleflight capacity exhausted",
                    ));
                }
                let activity = self.activities.enter()?;
                let definition_hold = self.scheduler.memory_context().reserve(memory::envelope(
                    record.definition.as_ref().map_or(0, String::len),
                )?)?;
                let definition = record
                    .definition
                    .clone()
                    .ok_or_else(|| ServerError::io("missing immutable map definition"))?;
                let definition = Accounted::new(definition, vec![definition_hold]);
                // Admission and validation must finish before publishing a flight.
                // Otherwise an early error leaves subscribers waiting forever.
                let (tx, rx) = watch::channel(None);
                flights.insert(key.clone(), tx.clone());
                let state = self.clone();
                let key = key.clone();
                let view = view.clone();
                let revision = revision.clone();
                tokio::spawn(async move {
                    let _activity = activity;
                    let work = async {
                        let pixels = state
                            .area_pixels(area_key, definition, pin, view, time)
                            .await?;
                        let incomplete = pixels.incomplete;
                        let max = state.config.profile.max_image_bytes;
                        let c = state.scheduler.context();
                        state
                            .encodes
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let encoded = state
                            .scheduler
                            .blocking(WorkClass::Http, c, move |c| {
                                let _hold = c.reserve(pixels.pixels().len())?;
                                let image = if let Some([x, y, w, h]) = crop {
                                    let _crop_hold = c.reserve(
                                        (w as usize)
                                            .checked_mul(h as usize)
                                            .and_then(|v| v.checked_mul(8))
                                            .ok_or(GeelloError::SizeOverflow)?,
                                    )?;
                                    let mut bytes = vec![];
                                    for row in y..y + h {
                                        let start = ((row * pixels.width() + x) * 4) as usize;
                                        bytes.extend_from_slice(
                                            &pixels.pixels()[start..start + (w * 4) as usize],
                                        );
                                    }
                                    RgbaImage::from_rgba(w, h, bytes)?
                                } else {
                                    (**pixels).clone()
                                };
                                Ok(crate::encoding::encode(
                                    &image,
                                    crate::encoding::ImageFormat::Png,
                                    max,
                                    &c,
                                )?)
                            })
                            .await?;
                        let etag = format!("\"{}\"", blake3::hash(&encoded).to_hex());
                        let (bytes, encoding_holds) = encoded.into_parts();
                        let hold = state.scheduler.memory_context().reserve(
                            bytes
                                .capacity()
                                .checked_add(RESPONSE_HEADER_MAX)
                                .ok_or(GeelloError::SizeOverflow)?,
                        )?;
                        drop(encoding_holds);
                        Ok(EncodedResponse {
                            bytes,
                            etag,
                            content_type: "image/png".into(),
                            render_revision: revision,
                            backend: state.backend_name.clone(),
                            incomplete,
                            _memory: vec![hold],
                        })
                    };
                    let result = tokio::select! {r=work=>r,_=tx.closed()=>Err(ServerError::new(503,"cancelled","All encoding waiters disconnected"))};
                    if let Ok(response) = &result
                        && let Ok(bytes) = encode_response(
                            response,
                            &key,
                            state.config.profile.max_image_bytes,
                            &state.scheduler.context(),
                        )
                    {
                        let (bytes, _hold) = bytes.into_parts();
                        state.responses.insert(key.clone(), bytes);
                    }
                    tx.send_replace(Some(result.map(Arc::new)));
                    state.encode_flights.lock().unwrap().remove(&key);
                });
                rx
            }
        };
        loop {
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            receiver
                .changed()
                .await
                .map_err(|_| ServerError::new(503, "worker", "Render worker stopped"))?;
        }
    }
    async fn area_pixels(
        self: &Arc<Self>,
        key: String,
        definition: Accounted<String>,
        pin: ResourcePin,
        view: View,
        time: f64,
    ) -> Result<Arc<RenderedArea>> {
        if let Some(CachedObject::Image(image)) = self.objects.get(&key) {
            return Ok(image);
        }
        let mut rx = {
            let mut flights = self.render_flights.lock().unwrap();
            if let Some(tx) = flights.get(&key) {
                tx.subscribe()
            } else {
                if flights.len()
                    >= self.config.profile.queued_per_class + self.config.profile.active_http
                {
                    return Err(ServerError::new(
                        503,
                        "queue-full",
                        "Area render capacity exhausted",
                    ));
                }
                let activity = self.activities.enter()?;
                let (tx, rx) = watch::channel(None);
                flights.insert(key.clone(), tx.clone());
                let state = self.clone();
                tokio::spawn(async move {
                    let _activity = activity;
                    let ctx = state.scheduler.context();
                    let worker = state.clone();
                    let work = async {
                        let scene = state
                            .scheduler
                            .blocking(WorkClass::Http, ctx.clone(), move |c| {
                                worker.prepare_definition(&definition, &pin, &view, time, &c)
                            })
                            .await?;
                        let incomplete = scene.summary().incompatible_components > 0;
                        Ok(RenderedArea {
                            image: state.execute_pixels(scene, &ctx, WorkClass::Http).await?,
                            incomplete,
                        })
                    };
                    let result = tokio::select! {r=work=>r,_=tx.closed()=>{ctx.cancel();Err(ServerError::new(503,"cancelled","All area waiters disconnected"))}};
                    let result = result.map(Arc::new);
                    if let Ok(image) = &result {
                        let object = CachedObject::Image(image.clone());
                        if object.weight(&key)
                            <= state.config.profile.object_cache_bytes.min(u32::MAX as u64)
                        {
                            state.objects.insert(key.clone(), object);
                        }
                    }
                    tx.send_replace(Some(result));
                    state.render_flights.lock().unwrap().remove(&key);
                });
                rx
            }
        };
        loop {
            if let Some(result) = rx.borrow().clone() {
                return result;
            }
            rx.changed()
                .await
                .map_err(|_| ServerError::new(503, "worker", "Area worker stopped"))?;
        }
    }
    fn prepare_definition(
        &self,
        definition: &str,
        pin: &ResourcePin,
        view: &View,
        time: f64,
        ctx: &OperationContext,
    ) -> Result<PreparedScene> {
        let _map_memory = ctx.reserve(memory::envelope(definition.len())?)?;
        let map = parse_map(definition)?;
        if !map.output_crs.contains(&view.crs.definition().to_owned()) {
            return Err(ServerError::invalid("Output CRS is not published by map"));
        }
        crate::projection::validate_backend(map.projection_backend.as_deref())?;
        self.prepare_map(&map, pin, view, time, ctx)
    }
    fn prepare_map(
        &self,
        map: &MapDefinition,
        pin: &ResourcePin,
        view: &View,
        time: f64,
        ctx: &OperationContext,
    ) -> Result<PreparedScene> {
        let background = map.background.as_deref().map(parse_color).transpose()?;
        self.prepare_maps(std::slice::from_ref(map), pin, view, time, background, ctx)
    }
    fn prepare_maps(
        &self,
        maps: &[MapDefinition],
        pin: &ResourcePin,
        view: &View,
        time: f64,
        background: Option<Color>,
        ctx: &OperationContext,
    ) -> Result<PreparedScene> {
        let mut composition = MapComposition {
            items: vec![],
            background,
        };
        let mut missing_geometry_targets = 0usize;
        let mut holds = vec![];
        let mut animations = BTreeMap::new();
        for record in pin
            .records
            .values()
            .filter(|r| r.key.kind == ResourceKind::Animation)
        {
            let text = record
                .definition
                .as_deref()
                .ok_or_else(|| ServerError::io("missing animation definition"))?;
            holds.push(ctx.reserve(memory::envelope(text.len())?)?);
            animations.insert(record.key.id.clone(), parse_animation(text)?);
        }
        for map in maps {
            crate::projection::validate_backend(map.projection_backend.as_deref())?;
            if !map.output_crs.contains(&view.crs.definition().to_owned()) {
                return Err(ServerError::invalid(
                    "Map output CRS/backend is not published",
                ));
            }
            for item in &map.items {
                let dataset =
                    &pin.records[&ResourceKey::new(ResourceKind::Dataset, &item.dataset)?];
                let style_record =
                    &pin.records[&ResourceKey::new(ResourceKind::Style, &item.style)?];
                let _style_memory = ctx.reserve(memory::envelope(
                    style_record.definition.as_ref().map_or(0, String::len),
                )?)?;
                let style = parse_style(
                    style_record
                        .definition
                        .as_deref()
                        .ok_or_else(|| ServerError::io("missing immutable style definition"))?,
                )?;
                let bindings = prepare_style_animations(&style, item, &animations, ctx)?;
                let path = dataset.derived_path.as_ref().ok_or_else(|| {
                    ServerError::new(503, "preparing", "Dataset has no complete derived version")
                })?;
                if dataset.class.as_deref() == Some("raster") {
                    let raster = DerivedRaster::open_with_context(
                        path,
                        self.config.profile.store_limits(),
                        ctx,
                    )?;
                    for rule in &style.rules {
                        for symbol in &rule.symbols {
                            let ranges = raster.ranges();
                            let mut colors = symbol.raster_options(&ranges)?;
                            for binding in bindings.iter() {
                                ctx.checkpoint(1)?;
                                if binding.rule == rule.id && binding.symbol == symbol.id() {
                                    colors.opacity =
                                        binding.track.raster_opacity(colors.opacity, time, ctx)?;
                                }
                            }
                            let kind = if dataset.source.sample_kind.as_deref()
                                == Some("categorical")
                            {
                                crate::raster_processing::RasterKind::Categorical
                            } else if dataset.source.sample_kind.as_deref() == Some("continuous") {
                                crate::raster_processing::RasterKind::Continuous
                            } else {
                                return Err(ServerError::invalid(
                                    "Raster sample_kind must be explicitly continuous or categorical",
                                ));
                            };
                            let SymbolDefinition::Raster { resampling, .. } = symbol else {
                                return Err(ServerError::invalid("Raster style required"));
                            };
                            let parse_method =
                                |s: &str| -> Result<crate::raster_processing::Resampling> {
                                    use crate::raster_processing::Resampling::*;
                                    match s {
                                        "nearest" => Ok(Nearest),
                                        "bilinear" => Ok(Bilinear),
                                        "area" | "area-average" => Ok(AreaAverage),
                                        _ => Err(ServerError::invalid("Unknown resampling method")),
                                    }
                                };
                            let enlarge = parse_method(&resampling.enlarge)?;
                            let reduce = parse_method(&resampling.reduce)?;
                            let compatible_overview = match kind {
                                crate::raster_processing::RasterKind::Categorical => true,
                                crate::raster_processing::RasterKind::Continuous => {
                                    reduce == crate::raster_processing::Resampling::AreaAverage
                                }
                            };
                            let level = raster_level(&raster, view, compatible_overview);
                            let source_grid = raster.grid();
                            let inverse = if source_grid.crs != view.crs {
                                Some(crate::projection::Projection::new(
                                    map.projection_backend.as_deref(),
                                    view.crs.clone(),
                                    source_grid.crs.clone(),
                                )?)
                            } else {
                                None
                            };
                            let [a, b, c, d, e, f] = source_grid.affine;
                            let _ = (a, d);
                            let factor = raster.level_dimensions(level).unwrap().2 as f64;
                            let pixel =
                                ((b * b + e * e).sqrt()).min((c * c + f * f).sqrt()) * factor;
                            let scale = ((view.bounds.max_x - view.bounds.min_x)
                                / view.width as f64)
                                .max((view.bounds.max_y - view.bounds.min_y) / view.height as f64);
                            let method = if inverse.is_some() || scale > pixel {
                                reduce
                            } else {
                                enlarge
                            };
                            let Some(sampled) = raster.resample_view_cached(
                                level,
                                view.width,
                                view.height,
                                RasterGrid {
                                    crs: view.crs.clone(),
                                    affine: [
                                        view.bounds.min_x,
                                        (view.bounds.max_x - view.bounds.min_x) / view.width as f64,
                                        0.,
                                        view.bounds.max_y,
                                        0.,
                                        -(view.bounds.max_y - view.bounds.min_y)
                                            / view.height as f64,
                                    ],
                                    interpretation: PixelInterpretation::Area,
                                },
                                kind,
                                method,
                                |p| inverse.as_ref().map_or(Ok(p), |op| op.transform_xy(p)),
                                ctx,
                                Some(self),
                            )?
                            else {
                                continue;
                            };
                            let image = crate::raster_processing::colorize(&sampled, &colors, ctx)?;
                            holds.push(ctx.reserve(image.image().pixels().len())?);
                            composition.items.push(DrawItem::Image(ImagePlacement {
                                image: Arc::new(image.image().clone()),
                                crs: view.crs.clone(),
                                bounds: view.bounds,
                                opacity: 1.,
                            }));
                        }
                    }
                } else {
                    let store = DerivedVector::open_with_context(
                        path,
                        self.config.profile.store_limits(),
                        ctx,
                    )?;
                    let geometry =
                        prepare_geometry_animations(item, &animations, store.crs(), ctx)?;
                    let target_count = geometry.iter().try_fold(0usize, |n, b| {
                        n.checked_add(b.ids().len())
                            .ok_or(GeelloError::SizeOverflow)
                    })?;
                    let _target_hold = ctx.reserve(
                        target_count
                            .checked_mul(128)
                            .ok_or(GeelloError::SizeOverflow)?,
                    )?;
                    let mut found = std::collections::HashSet::new();
                    let mut cursor = store.query(VectorQuery::default(), ctx)?;
                    let mut batches = vec![];
                    while let Some(batch) = cursor.next_batch(
                        self.config.profile.shard_features,
                        self.config.profile.max_live_bytes.min(usize::MAX as u64) as usize / 4,
                        ctx,
                    )? {
                        for feature in &batch.features {
                            for binding in geometry.iter() {
                                ctx.checkpoint(binding.ids().len() as u64)?;
                                if let Some(id) = &feature.id
                                    && let Some(target) =
                                        binding.ids().iter().find(|target| *target == id)
                                {
                                    found.insert(target);
                                }
                            }
                        }
                        let mut batch = batch;
                        for binding in geometry.iter() {
                            batch = binding.evaluate_batch(&batch, time, ctx)?;
                        }
                        let (batch, reservations) = batch.into_parts();
                        holds.extend(reservations);
                        batches.push(Arc::new(batch));
                    }
                    missing_geometry_targets = missing_geometry_targets
                        .checked_add(target_count - found.len())
                        .ok_or(GeelloError::SizeOverflow)?;
                    let mut fonts = BTreeMap::new();
                    for symbol in style.rules.iter().flat_map(|r| &r.symbols) {
                        if let SymbolDefinition::Label {
                            fonts: definitions, ..
                        } = symbol
                        {
                            for definition in definitions {
                                let key = (definition.resource.clone(), definition.face_index);
                                if fonts.contains_key(&key) {
                                    continue;
                                }
                                let record = &pin.records
                                    [&ResourceKey::new(ResourceKind::Font, &definition.resource)?];
                                let bytes = memory::read_file(
                                    &record
                                        .derived_path
                                        .as_ref()
                                        .ok_or_else(|| {
                                            ServerError::invalid("Font requires refresh before use")
                                        })?
                                        .join("font.bin"),
                                    self.config.profile.max_upload_bytes,
                                    ctx,
                                )?;
                                holds.push(ctx.reserve(bytes.len())?);
                                fonts.insert(
                                    key,
                                    Arc::new(FontResource {
                                        identity: record.version.clone(),
                                        bytes: Arc::from(bytes.as_slice()),
                                        face_index: definition.face_index,
                                    }),
                                );
                            }
                        }
                    }
                    let resolved = resolve_vector_style_with_fonts(
                        &style, &batches, &bindings, time, &fonts, ctx,
                    )?;
                    let (mut resolved, reservations) = resolved.into_parts();
                    holds.extend(reservations);
                    for draw in &mut resolved.items {
                        if let DrawItem::Label(label) = draw {
                            if let Some((index, _)) = &mut label.marker_source {
                                *index += composition.items.len();
                            }
                            if label.style.language == "und" {
                                label.style.language =
                                    map.language.clone().unwrap_or_else(|| "und".into());
                            }
                            if label.crs != view.crs {
                                use geo::MapCoords;
                                let projection = crate::projection::Projection::new(
                                    map.projection_backend.as_deref(),
                                    label.crs.clone(),
                                    view.crs.clone(),
                                )?;
                                label.geometry = label.geometry.try_map_coords(|c| {
                                    ctx.checkpoint(1)?;
                                    let p = projection.transform_xy([c.x, c.y])?;
                                    Ok::<_, GeelloError>(geo_types::Coord { x: p[0], y: p[1] })
                                })?;
                                label.crs = view.crs.clone();
                            }
                        }
                        if let DrawItem::Vector(v) = draw {
                            for batch in &mut v.batches {
                                if batch.crs != view.crs {
                                    let projection = crate::projection::Projection::new(
                                        map.projection_backend.as_deref(),
                                        batch.crs.clone(),
                                        view.crs.clone(),
                                    )?;
                                    let projected = projection.project_batch(batch, ctx)?;
                                    let (projected, reservations) = projected.into_parts();
                                    holds.extend(reservations);
                                    *batch = Arc::new(projected);
                                }
                            }
                        }
                    }
                    composition.items.extend(resolved.items);
                }
            }
        }
        let mut scene = crate::prepare_scene(&composition, view, ctx)?;
        scene.add_missing_geometry_targets(missing_geometry_targets, ctx)?;
        Ok(scene)
    }
    async fn execute_pixels(
        &self,
        scene: PreparedScene,
        ctx: &OperationContext,
        class: WorkClass,
    ) -> Result<Accounted<RgbaImage>> {
        self.renders
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pixels = match &self.backend {
            #[cfg(feature = "render-cpu")]
            Backend::Cpu => {
                self.scheduler
                    .blocking(class, ctx.clone(), move |c| {
                        Ok(CpuRenderer::new(1).render(&scene, &c)?)
                    })
                    .await?
            }
            #[cfg(feature = "render-gpu")]
            Backend::Gpu(renderer) => {
                let renderer = renderer.clone();
                let render_context = ctx.clone();
                self.scheduler
                    .asynchronous(class, ctx.clone(), async move {
                        Ok(renderer
                            .lock()
                            .await
                            .render_pixels(&scene, &render_context)
                            .await?)
                    })
                    .await?
            }
        };
        let hold = ctx.reserve(pixels.pixels().len())?;
        Ok(Accounted::new(pixels, vec![hold]))
    }
}
impl RasterBlockCache for ServerState {
    fn get_or_load(
        &self,
        key: String,
        load: &mut dyn FnMut() -> std::result::Result<Accounted<RasterBlock>, GeelloError>,
    ) -> std::result::Result<Arc<Accounted<RasterBlock>>, GeelloError> {
        let loaded = self.objects.try_get_with(key.clone(), || {
            self.native_decodes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            load().map(|block| CachedObject::Native(Arc::new(block)))
        });
        let value = match loaded {
            Ok(value) => value,
            Err(e)
                if matches!(
                    *e,
                    GeelloError::Cancelled
                        | GeelloError::DeadlineExceeded
                        | GeelloError::BudgetExceeded(_)
                ) =>
            {
                // A shared miss may have run with another request's cancelled
                // or exhausted context. Retry once with this caller's loader;
                // cancellation/budget errors are never retained as cache values.
                self.native_decodes
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let value = CachedObject::Native(Arc::new(load()?));
                if value.weight(&key) <= self.config.profile.object_cache_bytes.min(u32::MAX as u64)
                {
                    self.objects.insert(key.clone(), value.clone());
                }
                value
            }
            Err(e) => return Err((*e).clone()),
        };
        // Moka's weighted admission is eventual. Explicitly invalidate oversized
        // entries so the u32 weigher cannot undercount a larger allocation.
        if value.weight(&key) > self.config.profile.object_cache_bytes.min(u32::MAX as u64) {
            self.objects.invalidate(&key);
        }
        match value {
            CachedObject::Native(block) => Ok(block),
            CachedObject::Image(_) => Err(GeelloError::InvalidSource(
                "native cache key collision".into(),
            )),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseEnvelope<'a> {
    key: std::borrow::Cow<'a, str>,
    etag: std::borrow::Cow<'a, str>,
    revision: std::borrow::Cow<'a, str>,
    backend: std::borrow::Cow<'a, str>,
    incomplete: bool,
    bytes: usize,
}
/// Overview selection is conservative for affine views. Curved views retain
/// the base resolution; bounded footprint planning lives in the library.
fn raster_level(store: &DerivedRaster, view: &View, allow_overview: bool) -> usize {
    let grid = store.grid();
    if grid.crs != view.crs || !allow_overview {
        return 0;
    }
    let [_, b, c, _, e, f] = grid.affine;
    let dx = (view.bounds.max_x - view.bounds.min_x) / view.width as f64;
    let dy = (view.bounds.max_y - view.bounds.min_y) / view.height as f64;
    store.select_level(dx.min(dy) / (b * b + c * c + e * e + f * f).sqrt())
}

const RESPONSE_MAGIC: &[u8; 8] = b"GLRC0002";
const RESPONSE_HEADER_MAX: usize = 4096;
fn response_limit(max: usize) -> Result<usize> {
    max.checked_add(RESPONSE_HEADER_MAX + 44)
        .ok_or_else(|| GeelloError::SizeOverflow.into())
}
fn encode_response(
    v: &EncodedResponse,
    key: &str,
    max: usize,
    ctx: &OperationContext,
) -> Result<Accounted<Vec<u8>>> {
    if v.bytes.len() > max {
        return Err(GeelloError::BudgetExceeded("encoded cache image").into());
    }
    let header = crate::derived::bounded_json(
        &ResponseEnvelope {
            key: key.into(),
            etag: v.etag.as_str().into(),
            revision: v.render_revision.as_str().into(),
            backend: v.backend.as_str().into(),
            incomplete: v.incomplete,
            bytes: v.bytes.len(),
        },
        RESPONSE_HEADER_MAX,
        ctx,
    )?;
    let length = v
        .bytes
        .len()
        .checked_add(header.len())
        .and_then(|v| v.checked_add(44))
        .ok_or(GeelloError::SizeOverflow)?;
    let hold = ctx.reserve(length)?;
    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(RESPONSE_MAGIC);
    bytes.extend_from_slice(&(header.len() as u32).to_le_bytes());
    let mut hash = blake3::Hasher::new();
    hash.update(&header);
    hash.update(&v.bytes);
    bytes.extend_from_slice(hash.finalize().as_bytes());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&v.bytes);
    Ok(Accounted::new(bytes, vec![hold]))
}
fn decode_response(
    bytes: &[u8],
    key: &str,
    max: usize,
    ctx: &OperationContext,
) -> Result<EncodedResponse> {
    if bytes.len() < 44 || bytes.len() > response_limit(max)? || &bytes[..8] != RESPONSE_MAGIC {
        return Err(ServerError::io("invalid response cache framing"));
    }
    let header_length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if header_length > RESPONSE_HEADER_MAX
        || header_length > bytes.len() - 44
        || blake3::hash(&bytes[44..]).as_bytes() != &bytes[12..44]
    {
        return Err(ServerError::io("invalid response cache header length"));
    }
    let _header_memory = ctx.reserve(memory::envelope(header_length)?)?;
    let v: ResponseEnvelope<'_> =
        serde_json::from_slice(&bytes[44..44 + header_length]).map_err(ServerError::io)?;
    let payload = &bytes[44 + header_length..];
    if v.key != key
        || payload.len() > max
        || payload.len() != v.bytes
        || !matches!(v.backend.as_ref(), "cpu" | "gpu")
        || v.etag != format!("\"{}\"", blake3::hash(payload).to_hex())
    {
        return Err(ServerError::io("invalid response cache"));
    }
    let hold = ctx.reserve(
        payload
            .len()
            .checked_add(RESPONSE_HEADER_MAX)
            .ok_or(GeelloError::SizeOverflow)?,
    )?;
    Ok(EncodedResponse {
        bytes: payload.to_vec(),
        etag: v.etag.into_owned(),
        render_revision: v.revision.into_owned(),
        backend: v.backend.into_owned(),
        incomplete: v.incomplete,
        content_type: "image/png".into(),
        _memory: vec![hold],
    })
}
fn cpu_backend() -> Result<Backend> {
    #[cfg(feature = "render-cpu")]
    {
        Ok(Backend::Cpu)
    }
    #[cfg(not(feature = "render-cpu"))]
    {
        Err(ServerError::invalid("CPU backend is not compiled"))
    }
}
pub(crate) fn parse_crs(id: &str) -> Result<Crs> {
    Ok(Crs::new(
        id,
        if id == "EPSG:4326" || id == "OGC:CRS84" {
            CoordinateSpace::GeographicXy
        } else {
            CoordinateSpace::Planar
        },
    )?)
}
fn atomic_operation(
    root: &Path,
    op: &ResourceOperation,
    max: usize,
    ctx: &OperationContext,
) -> Result<()> {
    let bytes = crate::derived::bounded_json(op, max, ctx)?;
    super::catalog::atomic_write(
        &root
            .join("operations")
            .join(format!("{}.json", op.operation_id)),
        &bytes,
    )
}
fn fingerprint(source: &SourceSpec, max: usize, ctx: &OperationContext) -> Result<String> {
    let _hold = ctx.reserve(65536)?;
    let mut hash = blake3::Hasher::new();
    let mut paths = vec![("source", source.path.clone())];
    if source.format == "shapefile" {
        for ext in ["shx", "dbf", "prj", "cpg"] {
            let path = source.path.with_extension(ext);
            if path.exists() {
                paths.push((ext, path));
            }
        }
    }
    let mut total = 0usize;
    let mut buffer = [0u8; 65536];
    for (name, path) in paths {
        hash.update(&(name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        let mut file = File::open(path).map_err(ServerError::io)?;
        let length = file.metadata().map_err(ServerError::io)?.len();
        hash.update(&length.to_le_bytes());
        let mut actual = 0u64;
        loop {
            ctx.checkpoint(1)?;
            let n = file.read(&mut buffer).map_err(ServerError::io)?;
            if n == 0 {
                break;
            }
            total = total.checked_add(n).ok_or(GeelloError::SizeOverflow)?;
            if total > max {
                return Err(ServerError::new(
                    413,
                    "source-limit",
                    "Source and sidecars exceed profile",
                ));
            }
            hash.update(&buffer[..n]);
            actual = actual
                .checked_add(n as u64)
                .ok_or(GeelloError::SizeOverflow)?;
        }
        if actual != length {
            return Err(GeelloError::SourceChanged.into());
        }
    }
    hash.update(&crate::derived::bounded_json(
        &(
            &source.format,
            &source.crs,
            &source.encoding,
            &source.sample_kind,
        ),
        max,
        ctx,
    )?);
    Ok(hash.finalize().to_hex().to_string())
}
fn directory_bytes(root: &Path, ctx: &OperationContext) -> Result<u64> {
    fn open(
        path: &Path,
        ctx: &OperationContext,
    ) -> Result<(fs::ReadDir, crate::operation::Reservation)> {
        // Reserve the path/iterator envelope before opening it. Depth-first
        // traversal retains one iterator per ancestor, never a breadth-sized
        // queue of paths. Work, cancellation and depth all consume admission.
        let bytes = path
            .as_os_str()
            .len()
            .checked_mul(4)
            .and_then(|n| n.checked_add(4096))
            .ok_or(GeelloError::SizeOverflow)?;
        let hold = ctx.reserve(bytes)?;
        Ok((fs::read_dir(path).map_err(ServerError::io)?, hold))
    }
    let mut total = 0u64;
    let mut todo = vec![open(root, ctx)?];
    while let Some((entries, _hold)) = todo.last_mut() {
        ctx.checkpoint(1)?;
        if let Some(entry) = entries.next() {
            let entry = entry.map_err(ServerError::io)?;
            let meta = entry.metadata().map_err(ServerError::io)?;
            if entry.file_type().map_err(ServerError::io)?.is_symlink() {
                return Err(ServerError::io("owned directory contains a link"));
            }
            if meta.is_dir() {
                todo.push(open(&entry.path(), ctx)?);
            } else {
                total = total
                    .checked_add(meta.len())
                    .ok_or_else(|| ServerError::io("disk accounting overflow"))?;
            }
        } else {
            todo.pop();
        }
    }
    Ok(total)
}

#[cfg(all(test, feature = "render-cpu"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn animation_uses_its_own_queue_and_does_not_reuse_or_populate_http_caches() {
        let root = tempfile::tempdir().unwrap();
        let mut config =
            ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
        config.data_directory = root.path().into();
        config.backend = BackendSelection::Cpu;
        config.profile.active_http = 1;
        let state = ServerState::open(config).await.unwrap();
        let key = ResourceKey::new(ResourceKind::Map, "test:animation").unwrap();
        state.catalog.publish(ResourceRecord {
            key: key.clone(), ownership: Ownership::External, version: "v1".into(), published: true,
            source: SourceSpec { path: root.path().join("map.toml"), format: "toml".into(), encoding: None, crs: None, sample_kind: None },
            dependencies: vec![], data_version: "d1".into(), derived_version: None,
            derived_path: None, managed_directory: None, class: None, effective_crs: None,
            definition: Some("schema_version=1\nid='test:animation'\noutput_crs=['OGC:CRS84']\nitems=[]\n".into()),
            type_bindings: vec![],
        }, &Precondition::Absent).unwrap();
        let view = View {
            crs: Crs::new("OGC:CRS84", CoordinateSpace::GeographicXy).unwrap(),
            bounds: Bounds {
                min_x: 0.,
                min_y: 0.,
                max_x: 1.,
                max_y: 1.,
            },
            width: 16,
            height: 16,
            aspect: AspectPolicy::Stretch,
        };
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let scheduler = state.scheduler.clone();
        let occupied = tokio::spawn(async move {
            scheduler
                .asynchronous(WorkClass::Http, scheduler.context(), async move {
                    started.send(()).unwrap();
                    released.await.unwrap();
                    Ok(())
                })
                .await
        });
        ready.await.unwrap();
        let baseline = state.scheduler.live_bytes();
        let animation = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            state.render_animation_at(&key.id, view.clone(), 2.),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(state.execution_counts(), (1, 1));
        assert_eq!(&animation.bytes[..8], b"\x89PNG\r\n\x1a\n");
        drop(animation);
        assert_eq!(state.catalog.stats().pinned_versions, 0);
        assert_eq!(state.scheduler.live_bytes(), baseline);
        release.send(()).unwrap();
        occupied.await.unwrap().unwrap();

        // An identical HTTP request still has to render and encode; the
        // animation frame populated neither the pixel nor encoded response cache.
        let http = state
            .render_map_at(&key.id, view.clone(), 2.)
            .await
            .unwrap();
        assert_eq!(state.execution_counts(), (2, 2));
        let warm = state
            .render_map_at(&key.id, view.clone(), 2.)
            .await
            .unwrap();
        assert_eq!(http.bytes, warm.bytes);
        assert_eq!(state.execution_counts(), (2, 2));
        let animation = state
            .render_animation_at(&key.id, view.clone(), 2.)
            .await
            .unwrap();
        assert_eq!(animation.bytes, http.bytes);
        assert_eq!(animation.render_revision, http.render_revision);
        assert_eq!(state.execution_counts(), (3, 3));
        drop((http, warm, animation));
        let config = state.config.clone();
        state.shutdown().await.unwrap();
        drop(state);

        // Recovery finds the HTTP disk entry, while animation still renders.
        let state = ServerState::open(config).await.unwrap();
        drop(
            state
                .render_map_at(&key.id, view.clone(), 2.)
                .await
                .unwrap(),
        );
        assert_eq!(state.execution_counts(), (0, 0));
        drop(state.render_animation_at(&key.id, view, 2.).await.unwrap());
        assert_eq!(state.execution_counts(), (1, 1));
        state.shutdown().await.unwrap();
    }

    #[cfg(feature = "render-gpu")]
    #[tokio::test]
    #[ignore = "Requires a real GPU; explicitly destroys the test device"]
    async fn runtime_device_loss_never_switches_to_cpu() {
        let temp = tempfile::tempdir().unwrap();
        let mut config =
            ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
        config.data_directory = temp.path().into();
        config.backend = BackendSelection::GpuPreferred;
        let state = ServerState::open(config).await.unwrap();
        assert_eq!(state.backend_name, "gpu");
        let ctx = state.scheduler.context();
        let view = View {
            crs: Crs::new("OGC:CRS84", CoordinateSpace::GeographicXy).unwrap(),
            bounds: Bounds {
                min_x: -1.,
                min_y: -1.,
                max_x: 1.,
                max_y: 1.,
            },
            width: 16,
            height: 16,
            aspect: AspectPolicy::Stretch,
        };
        let scene = crate::prepare_scene(
            &MapComposition {
                items: vec![],
                background: None,
            },
            &view,
            &ctx,
        )
        .unwrap();
        assert!(
            state
                .execute_pixels(scene.clone(), &ctx, WorkClass::Http)
                .await
                .is_ok()
        );
        let Backend::Gpu(renderer) = &state.backend else {
            panic!("Expected real GPU");
        };
        renderer.lock().await.device().destroy();
        for _ in 0..2 {
            let error = state
                .execute_pixels(scene.clone(), &ctx, WorkClass::Http)
                .await
                .err()
                .unwrap();
            assert_eq!(error.status, 503);
            assert_eq!(error.kind, "urn:geello:problem:backend");
        }
        assert_eq!(state.backend_name, "gpu");
        assert!(state.backend_diagnostic.is_none());
        state.shutdown().await.unwrap();
    }

    #[cfg(feature = "render-gpu")]
    #[tokio::test]
    async fn startup_fallback_is_explicit_and_strict_gpu_failure_releases_catalog() {
        let temp = tempfile::tempdir().unwrap();
        let mut config =
            ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
        config.data_directory = temp.path().into();
        config.backend = BackendSelection::Gpu;
        let failure = ServerError::new(503, "backend", "Injected GPU initialization failure");
        let error = ServerState::open_with_initializer(config.clone(), |_| {
            std::future::ready(Err(failure.clone()))
        })
        .await
        .err()
        .unwrap();
        assert_eq!(error.title, failure.title);
        assert_eq!(error.status, 503);

        // Reopening the same root also proves failed startup released its lock.
        config.backend = BackendSelection::GpuPreferred;
        let state = ServerState::open_with_initializer(config.clone(), |_| {
            std::future::ready(Err(failure.clone()))
        })
        .await
        .unwrap();
        assert_eq!(state.backend_name, "cpu");
        assert!(
            state
                .backend_diagnostic
                .as_ref()
                .unwrap()
                .contains(&failure.title)
        );
        state.shutdown().await.unwrap();
        drop(state);

        config.backend = BackendSelection::Cpu;
        let state = ServerState::open_with_initializer(
            config,
            |_| -> std::future::Ready<Result<Backend>> {
                panic!("Explicit CPU selection must not attempt GPU initialization");
            },
        )
        .await
        .unwrap();
        assert_eq!(state.backend_name, "cpu");
        assert!(state.backend_diagnostic.is_none());
        state.shutdown().await.unwrap();
    }

    #[test]
    fn directory_accounting_is_cancellable_and_bounded_by_depth_and_work() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("a/b/c")).unwrap();
        fs::write(temp.path().join("a/b/c/data"), [0; 12]).unwrap();
        let config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
        let limits = config.profile.context().limits();
        let ctx = OperationContext::new(limits, None);
        assert_eq!(directory_bytes(temp.path(), &ctx).unwrap(), 12);
        assert_eq!(ctx.live_bytes(), 0);
        for limits in [
            OperationLimits {
                max_live_bytes: 8192,
                ..limits
            },
            OperationLimits {
                max_work: 2,
                ..limits
            },
        ] {
            let limited = OperationContext::new(limits, None);
            assert_eq!(
                directory_bytes(temp.path(), &limited).unwrap_err().status,
                503
            );
            assert_eq!(limited.live_bytes(), 0);
        }
        ctx.cancel();
        assert!(directory_bytes(temp.path(), &ctx).is_err());
        assert_eq!(ctx.live_bytes(), 0);
    }

    #[test]
    fn encoded_cache_framing_and_budget_fail_before_payload_allocation() {
        let config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
        let ctx = config.profile.context();
        let response = EncodedResponse {
            bytes: vec![123; 65536],
            etag: format!("\"{}\"", blake3::hash(&vec![123; 65536]).to_hex()),
            content_type: "image/png".into(),
            render_revision: "revision".into(),
            backend: "cpu".into(),
            incomplete: false,
            _memory: vec![],
        };
        let encoded = encode_response(&response, "cache-key", response.bytes.len(), &ctx).unwrap();
        assert!(encoded.len() < response.bytes.len() + 512);
        let retained = ctx.live_bytes();
        for index in [0, 8, 12, 44, encoded.len() - 1] {
            let mut damaged = encoded.to_vec();
            damaged[index] ^= 1;
            assert!(decode_response(&damaged, "cache-key", response.bytes.len(), &ctx).is_err());
            assert_eq!(ctx.live_bytes(), retained);
        }
        for length in [0, 7, 43, encoded.len() - 1] {
            assert!(
                decode_response(&encoded[..length], "cache-key", response.bytes.len(), &ctx)
                    .is_err()
            );
        }
        assert!(decode_response(&encoded, "wrong-key", response.bytes.len(), &ctx).is_err());
        assert!(decode_response(&encoded, "cache-key", response.bytes.len() - 1, &ctx).is_err());
        let small = OperationContext::new(
            OperationLimits {
                max_live_bytes: 2048,
                ..ctx.limits()
            },
            None,
        );
        let error = decode_response(&encoded, "cache-key", response.bytes.len(), &small)
            .err()
            .unwrap();
        assert_eq!(error.status, 503);
        assert_eq!(small.live_bytes(), 0);
        let decoded = decode_response(&encoded, "cache-key", response.bytes.len(), &ctx).unwrap();
        assert!(ctx.live_bytes() > retained);
        assert_eq!(decoded.bytes, response.bytes);
        drop(decoded);
        assert_eq!(ctx.live_bytes(), retained);
        drop(encoded);
        assert_eq!(ctx.live_bytes(), 0);
    }

    #[tokio::test]
    async fn encoded_cache_checksums_reject_corruption_and_allow_replacement() {
        let mut config =
            ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
        let temp = tempfile::tempdir().unwrap();
        config.data_directory = temp.path().into();
        config.backend = BackendSelection::Cpu;
        let state = ServerState::open(config).await.unwrap();
        let key = "corrupt-response".to_string();
        let response = EncodedResponse {
            bytes: vec![1, 2, 3],
            etag: format!("\"{}\"", blake3::hash(&[1, 2, 3]).to_hex()),
            content_type: "image/png".into(),
            render_revision: "revision".into(),
            backend: "cpu".into(),
            incomplete: false,
            _memory: vec![],
        };
        let ctx = state.scheduler.context();
        let encoded = encode_response(&response, &key, 1024, &ctx).unwrap();
        let (mut bytes, _) = encoded.into_parts();
        let payload = bytes.len() - response.bytes.len();
        bytes[payload] ^= 1;
        state.responses.insert(key.clone(), bytes);
        // Close and reopen before lookup: this exercises the disk engine and
        // recovery path rather than only the hybrid cache's memory tier.
        let config = state.config.clone();
        state.shutdown().await.unwrap();
        drop(state);
        let state = ServerState::open(config).await.unwrap();
        let ctx = state.scheduler.context();
        assert!(state.responses.memory().get(&key).is_none());
        let entry = state.responses.get(&key).await.unwrap().unwrap();
        assert!(decode_response(entry.value(), &key, 1024, &ctx).is_err());
        state.responses.remove(&key);
        assert!(state.responses.get(&key).await.unwrap().is_none());
        state.responses.insert(
            key.clone(),
            encode_response(&response, &key, 1024, &ctx)
                .unwrap()
                .into_parts()
                .0,
        );
        let entry = state.responses.get(&key).await.unwrap().unwrap();
        assert_eq!(
            decode_response(entry.value(), &key, 1024, &ctx)
                .unwrap()
                .bytes,
            response.bytes
        );
        state.shutdown().await.unwrap();
    }
}
