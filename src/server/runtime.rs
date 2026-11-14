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
#[derive(Clone)]
pub struct EncodedResponse {
    pub bytes: Vec<u8>,
    pub etag: String,
    pub content_type: String,
    pub render_revision: String,
    pub backend: String,
    pub incomplete: bool,
}
enum Backend {
    #[cfg(feature = "render-cpu")]
    Cpu,
    #[cfg(feature = "render-gpu")]
    Gpu(Box<tokio::sync::Mutex<GpuRenderer>>),
}
type Flight = watch::Sender<Option<Result<Arc<EncodedResponse>>>>;
type PixelFlight = watch::Sender<Option<Result<Arc<Accounted<RgbaImage>>>>>;
pub struct ServerState {
    pub config: ServerConfig,
    pub catalog: Arc<Catalog>,
    pub scheduler: Arc<Scheduler>,
    backend: Backend,
    pub backend_name: String,
    pub backend_diagnostic: Option<String>,
    operations: Mutex<BTreeMap<String, ResourceOperation>>,
    objects: Cache<String, Arc<Accounted<RgbaImage>>>,
    responses: foyer::HybridCache<String, Vec<u8>>,
    render_flights: Mutex<BTreeMap<String, PixelFlight>>,
    encode_flights: Mutex<BTreeMap<String, Flight>>,
    preparation_commit: Mutex<()>,
    overview_attempts: Mutex<std::collections::BTreeSet<String>>,
    restore_attempts: Mutex<BTreeMap<String, String>>,
    cache_failures: std::sync::atomic::AtomicU64,
    renders: std::sync::atomic::AtomicU64,
    encodes: std::sync::atomic::AtomicU64,
    pub(crate) uploads: Arc<tokio::sync::Semaphore>,
}
impl ServerState {
    pub async fn open(config: ServerConfig) -> Result<Arc<Self>> {
        config.validate()?;
        let catalog = Catalog::open(&config.data_directory, config.profile.clone())?;
        let scheduler = Arc::new(Scheduler::new(config.profile.clone()));
        #[cfg(feature = "render-gpu")]
        let ctx = scheduler.context();
        #[cfg(feature = "render-gpu")]
        let mut diagnostic = None;
        #[cfg(not(feature = "render-gpu"))]
        let diagnostic: Option<String> = None;
        let backend = match config.backend {
            BackendSelection::Cpu => cpu_backend()?,
            BackendSelection::Gpu | BackendSelection::GpuPreferred => {
                #[cfg(feature = "render-gpu")]
                {
                    match GpuRenderer::new(wgpu::Backends::DX12 | wgpu::Backends::VULKAN, &ctx)
                        .await
                    {
                        Ok((r, _)) => Backend::Gpu(Box::new(tokio::sync::Mutex::new(r))),
                        Err(e) if config.backend == BackendSelection::GpuPreferred => {
                            diagnostic = Some(format!("GPU initialization failed: {e}"));
                            cpu_backend()?
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                #[cfg(not(feature = "render-gpu"))]
                {
                    return Err(ServerError::invalid(
                        "GPU backend is not compiled; select cpu explicitly",
                    ));
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
            .with_engine_config(foyer::BlockEngineConfig::new(device))
            .build()
            .await
            .map_err(ServerError::io)?;
        let objects = Cache::builder()
            .max_capacity(p.object_cache_bytes)
            .time_to_live(std::time::Duration::from_millis(p.operation_timeout_ms))
            .weigher(|_: &String, v: &Arc<Accounted<RgbaImage>>| {
                v.pixels().len().min(u32::MAX as usize) as u32
            })
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
                let bytes = read_bounded(&path, p.max_registry_bytes)?;
                let mut op: ResourceOperation =
                    serde_json::from_slice(&bytes).map_err(ServerError::io)?;
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
                    atomic_operation(catalog.root(), &op)?;
                }
                operations.insert(op.operation_id.clone(), op);
            }
        }
        let uploads = Arc::new(tokio::sync::Semaphore::new(p.active_background));
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
            render_flights: Mutex::new(BTreeMap::new()),
            encode_flights: Mutex::new(BTreeMap::new()),
            preparation_commit: Mutex::new(()),
            overview_attempts: Mutex::new(std::collections::BTreeSet::new()),
            restore_attempts: Mutex::new(BTreeMap::new()),
            cache_failures: std::sync::atomic::AtomicU64::new(0),
            renders: std::sync::atomic::AtomicU64::new(0),
            encodes: std::sync::atomic::AtomicU64::new(0),
            uploads,
        }))
    }
    pub fn operation(&self, id: &str) -> Result<ResourceOperation> {
        self.operations
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| {
                ServerError::new(404, "operation-not-found", "Operation is not recorded")
            })
    }
    /// Indexed catalog maintenance is explicit and separate from public lookup.
    /// It does not register or remove detached original content.
    pub fn collect_garbage(&self) -> Result<usize> {
        let _disk = self.preparation_commit.lock().unwrap();
        Ok(self.catalog.collect_garbage()? + self.catalog.collect_derived_orphans()?)
    }
    pub fn execution_counts(&self) -> (u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.renders.load(Ordering::Relaxed),
            self.encodes.load(Ordering::Relaxed),
        )
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
        atomic_operation(self.catalog.root(), &op)?;
        all.insert(op.operation_id.clone(), op.clone());
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
        atomic_operation(self.catalog.root(), &op)?;
        self.operations
            .lock()
            .unwrap()
            .insert(op.operation_id.clone(), op);
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
        staging: Option<tempfile::TempDir>,
    ) {
        let state = self.clone();
        tokio::spawn(async move {
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
                    worker.catalog.check(&key, &condition)?;
                    worker.catalog.collect_garbage()?;
                    worker.catalog.collect_derived_orphans()?;
                    let derived_used = directory_bytes(&worker.catalog.root().join("derived"))?;
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
                        let current = directory_bytes(&worker.catalog.root().join("managed"))?;
                        let extra = directory_bytes(stage.path())?;
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
                    match worker.catalog.publish(record.clone(), &condition) {
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
            .saturating_sub(directory_bytes(&self.catalog.root().join("derived"))?);
        let source_fingerprint = fingerprint(&source, p.max_source_bytes, ctx)?;
        let data_version = super::catalog::identity(&[
            source_fingerprint.as_bytes(),
            dataset_body.as_deref().unwrap_or("").as_bytes(),
        ]);
        let mut dependencies = vec![];
        let mut derived_version = None;
        let mut derived_path = None;
        let mut class = None;
        let mut definition = dataset_body;
        let mut type_bindings = vec![];
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
                    let text = String::from_utf8(read_bounded(&source.path, p.max_registry_bytes)?)
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
                        derived_path = Some(parent.join(store.derived_version()));
                        derived_version = Some(store.derived_version().into());
                    }
                }
            }
            ResourceKind::Style | ResourceKind::Map | ResourceKind::Animation => {
                if source.format != "toml" {
                    return Err(ServerError::invalid("Definition resource requires TOML"));
                }
                let text = String::from_utf8(read_bounded(&source.path, p.max_registry_bytes)?)
                    .map_err(|_| ServerError::invalid("TOML must be UTF-8"))?;
                let id = match key.kind {
                    ResourceKind::Style => {
                        let s = parse_style(&text)?;
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
                        }
                        for item in &m.items {
                            let d = self
                                .catalog
                                .get(&ResourceKey::new(ResourceKind::Dataset, &item.dataset)?)?;
                            let s = self
                                .catalog
                                .get(&ResourceKey::new(ResourceKind::Style, &item.style)?)?;
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
                let bytes = read_bounded(&source.path, p.max_upload_bytes)?;
                if bytes.len() < 12 || !matches!(&bytes[..4], b"\0\x01\0\0" | b"OTTO" | b"ttcf") {
                    return Err(ServerError::invalid("Unsupported font container"));
                }
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
            definition,
            type_bindings,
        })
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.responses.close().await.map_err(ServerError::io)
    }
    /// Overviews are lazy background work, deduplicated per immutable base.
    /// Serving continues with the pinned base while a complete replacement is
    /// built. Commit rechecks the original ETag; failures retain the base and
    /// leave a persistent operation diagnostic rather than a retry storm.
    fn request_overviews(self: &Arc<Self>, record: &ResourceRecord) {
        if record.class.as_deref() != Some("raster") {
            return;
        }
        let identity = format!(
            "{}:{}",
            record.key.id,
            record.derived_version.as_deref().unwrap_or("")
        );
        {
            let mut attempts = self.overview_attempts.lock().unwrap();
            if attempts.contains(&identity) || attempts.len() >= self.config.profile.max_operations
            {
                return;
            }
            attempts.insert(identity);
        }
        let state = self.clone();
        let record = record.clone();
        tokio::spawn(async move {
            let Some(path) = &record.derived_path else {
                return;
            };
            let Ok(base) = DerivedRaster::open(path, state.config.profile.store_limits()) else {
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
                    let _disk = worker.preparation_commit.lock().unwrap();
                    let condition = Precondition::Version(record.version.clone());
                    worker.catalog.check(&record.key, &condition)?;
                    let _pin = worker.catalog.pin(&record.key)?;
                    worker.catalog.collect_garbage()?;
                    let parent = worker.catalog.root().join("derived");
                    let mut limits = worker.config.profile.store_limits();
                    limits.max_disk_bytes = limits
                        .max_disk_bytes
                        .saturating_sub(directory_bytes(&parent)?);
                    let base = DerivedRaster::open(record.derived_path.as_ref().unwrap(), limits)?;
                    let overview = base.build_overviews(
                        &parent,
                        record.source.sample_kind.as_deref() == Some("categorical"),
                        &ctx,
                    )?;
                    let mut replacement = record;
                    replacement.version = opid;
                    replacement.derived_version = Some(overview.derived_version().into());
                    replacement.derived_path = Some(parent.join(overview.derived_version()));
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
        let identity = format!("{}:{}", record.key.id, record.version);
        let mut attempts = self.restore_attempts.lock().unwrap();
        let id = if let Some(id) = attempts.get(&identity) {
            id.clone()
        } else {
            if attempts.len() >= self.config.profile.max_operations {
                return Err(ServerError::new(
                    503,
                    "preparation-capacity",
                    "Preparation capacity exhausted",
                ));
            }
            let op = self.new_operation(record.key.clone())?;
            let id = op.operation_id.clone();
            attempts.insert(identity, id.clone());
            let state = self.clone();
            let record = record.clone();
            tokio::spawn(async move {
                let _ = state.set_operation(op.clone(), OperationStatus::Preparing, None);
                let worker = state.clone();
                let opid = op.operation_id.clone();
                let ctx = state.scheduler.context();
                let result = state
                    .scheduler
                    .blocking(WorkClass::Background, ctx, move |ctx| {
                        let _disk = worker.preparation_commit.lock().unwrap();
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
        self.render_request(id, view, None).await
    }
    pub async fn render_tile(
        self: &Arc<Self>,
        id: &str,
        z: u32,
        x: u32,
        y: u32,
    ) -> Result<Arc<EncodedResponse>> {
        if z > 30 || x >= 1u32 << z || y >= 1u32 << z {
            return Err(ServerError::invalid("Tile is outside WebMercatorQuad"));
        }
        let p = &self.config.profile;
        let count = 1u32 << z;
        let group = p.metatile_size.min(count);
        let gx = x / group * group;
        let gy = y / group * group;
        let size = 40075016.68557849 / count as f64;
        let origin = 20037508.342789244;
        let buffer = p.metatile_buffer as f64 / 256. * size;
        let view = View {
            crs: parse_crs("EPSG:3857")?,
            bounds: Bounds {
                min_x: -origin + gx as f64 * size - buffer,
                max_x: -origin + (gx + group) as f64 * size + buffer,
                max_y: origin - gy as f64 * size + buffer,
                min_y: origin - (gy + group) as f64 * size - buffer,
            },
            width: group * 256 + p.metatile_buffer * 2,
            height: group * 256 + p.metatile_buffer * 2,
            aspect: AspectPolicy::Stretch,
        };
        self.render_request(
            id,
            view,
            Some([
                (x - gx) * 256 + p.metatile_buffer,
                (y - gy) * 256 + p.metatile_buffer,
                256,
                256,
            ]),
        )
        .await
    }
    async fn render_request(
        self: &Arc<Self>,
        id: &str,
        view: View,
        crop: Option<[u32; 4]>,
    ) -> Result<Arc<EncodedResponse>> {
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
        let versions = pin
            .records
            .values()
            .map(|r| (&r.key, &r.version, &r.data_version, &r.derived_version))
            .collect::<Vec<_>>();
        let revision =
            super::catalog::identity(&[&serde_json::to_vec(&versions).map_err(ServerError::io)?]);
        let area_key = super::catalog::identity(&[
            revision.as_bytes(),
            self.backend_name.as_bytes(),
            &serde_json::to_vec(&view).map_err(ServerError::io)?,
        ]);
        let key = super::catalog::identity(&[
            area_key.as_bytes(),
            &serde_json::to_vec(&crop).map_err(ServerError::io)?,
            b"png",
        ]);
        let cached = self.responses.get(&key).await;
        if cached.is_err() {
            self.cache_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let Ok(Some(entry)) = cached {
            if let Ok(value) =
                decode_response(entry.value(), &key, self.config.profile.max_image_bytes)
            {
                return Ok(Arc::new(value));
            }
            self.responses.remove(&key);
            self.cache_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
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
                let (tx, rx) = watch::channel(None);
                flights.insert(key.clone(), tx.clone());
                let state = self.clone();
                let key = key.clone();
                let view = view.clone();
                let definition = record
                    .definition
                    .clone()
                    .ok_or_else(|| ServerError::io("missing immutable map definition"))?;
                let revision = revision.clone();
                tokio::spawn(async move {
                    let work = async {
                        let pixels = state.area_pixels(area_key, definition, pin, view).await?;
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
                                Ok((*crate::encoding::encode(
                                    &image,
                                    crate::encoding::ImageFormat::Png,
                                    max,
                                    &c,
                                )?)
                                .clone())
                            })
                            .await?;
                        let etag = format!("\"{}\"", blake3::hash(&encoded).to_hex());
                        Ok(EncodedResponse {
                            bytes: encoded,
                            etag,
                            content_type: "image/png".into(),
                            render_revision: revision,
                            backend: state.backend_name.clone(),
                            incomplete: false,
                        })
                    };
                    let result = tokio::select! {r=work=>r,_=tx.closed()=>Err(ServerError::new(503,"cancelled","All encoding waiters disconnected"))};
                    if let Ok(response) = &result
                        && let Ok(bytes) = encode_response(response, &key)
                    {
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
        definition: String,
        pin: ResourcePin,
        view: View,
    ) -> Result<Arc<Accounted<RgbaImage>>> {
        if let Some(image) = self.objects.get(&key) {
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
                let (tx, rx) = watch::channel(None);
                flights.insert(key.clone(), tx.clone());
                let state = self.clone();
                tokio::spawn(async move {
                    let ctx = state.scheduler.context();
                    let worker = state.clone();
                    let work = async {
                        let scene = state
                            .scheduler
                            .blocking(WorkClass::Http, ctx.clone(), move |c| {
                                let map = parse_map(&definition)?;
                                if !map.output_crs.contains(&view.crs.definition().to_owned()) {
                                    return Err(ServerError::invalid(
                                        "Output CRS is not published by map",
                                    ));
                                }
                                if map
                                    .projection_backend
                                    .as_deref()
                                    .is_some_and(|p| p != "rust")
                                {
                                    return Err(ServerError::invalid(
                                        "Projection backend is not available",
                                    ));
                                }
                                if map.items.iter().any(|i| !i.animation_bindings.is_empty()) {
                                    return Err(ServerError::new(
                                        422,
                                        "animation-pending",
                                        "Animation execution belongs to M4",
                                    ));
                                }
                                worker.prepare_map(&map, &pin, &view, &c)
                            })
                            .await?;
                        state.execute_pixels(scene, &ctx).await
                    };
                    let result = tokio::select! {r=work=>r,_=tx.closed()=>{ctx.cancel();Err(ServerError::new(503,"cancelled","All area waiters disconnected"))}};
                    let result = result.map(Arc::new);
                    if let Ok(image) = &result {
                        state.objects.insert(key.clone(), image.clone());
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
    fn prepare_map(
        &self,
        map: &MapDefinition,
        pin: &ResourcePin,
        view: &View,
        ctx: &OperationContext,
    ) -> Result<PreparedScene> {
        let mut composition = MapComposition {
            items: vec![],
            background: map.background.as_deref().map(parse_color).transpose()?,
        };
        let mut holds = vec![];
        for item in &map.items {
            let dataset = &pin.records[&ResourceKey::new(ResourceKind::Dataset, &item.dataset)?];
            let style_record = &pin.records[&ResourceKey::new(ResourceKind::Style, &item.style)?];
            let style = parse_style(
                style_record
                    .definition
                    .as_deref()
                    .ok_or_else(|| ServerError::io("missing immutable style definition"))?,
            )?;
            let path = dataset.derived_path.as_ref().ok_or_else(|| {
                ServerError::new(503, "preparing", "Dataset has no complete derived version")
            })?;
            if dataset.class.as_deref() == Some("raster") {
                let raster = DerivedRaster::open(path, self.config.profile.store_limits())?;
                for rule in &style.rules {
                    for symbol in &rule.symbols {
                        let ranges = raster.ranges();
                        let colors = symbol.raster_options(&ranges)?;
                        let kind = if dataset.source.sample_kind.as_deref() == Some("categorical") {
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
                        let Some((level, window)) =
                            raster_window(&raster, view, compatible_overview)?
                        else {
                            continue;
                        };
                        let block = raster.read_window(level, window, ctx)?;
                        let inverse = if block.grid.crs != view.crs {
                            Some(crate::projection::RustProjection::new(
                                view.crs.clone(),
                                block.grid.crs.clone(),
                            )?)
                        } else {
                            None
                        };
                        let [a, b, c, d, e, f] = block.grid.affine;
                        let _ = (a, d);
                        let pixel = ((b * b + e * e).sqrt()).min((c * c + f * f).sqrt());
                        let scale = ((view.bounds.max_x - view.bounds.min_x) / view.width as f64)
                            .max((view.bounds.max_y - view.bounds.min_y) / view.height as f64);
                        let method = if inverse.is_some() || scale > pixel {
                            reduce
                        } else {
                            enlarge
                        };
                        let sampled = crate::raster_processing::resample(
                            &block,
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
                                    -(view.bounds.max_y - view.bounds.min_y) / view.height as f64,
                                ],
                                interpretation: PixelInterpretation::Area,
                            },
                            kind,
                            method,
                            |p| inverse.as_ref().map_or(Ok(p), |op| op.transform_xy(p)),
                            ctx,
                        )?;
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
                let store = DerivedVector::open(path, self.config.profile.store_limits())?;
                let mut cursor = store.query(VectorQuery::default(), ctx)?;
                let mut batches = vec![];
                while let Some(batch) = cursor.next_batch(
                    self.config.profile.shard_features,
                    self.config.profile.max_live_bytes.min(usize::MAX as u64) as usize / 4,
                    ctx,
                )? {
                    let (batch, reservations) = batch.into_parts();
                    holds.extend(reservations);
                    batches.push(Arc::new(batch));
                }
                let resolved = resolve_vector_style(&style, &batches, ctx)?;
                let (mut resolved, reservations) = resolved.into_parts();
                holds.extend(reservations);
                for draw in &mut resolved.items {
                    if let DrawItem::Vector(v) = draw {
                        for batch in &mut v.batches {
                            if batch.crs != view.crs {
                                let projection = crate::projection::RustProjection::new(
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
        Ok(crate::prepare_scene(&composition, view, ctx)?)
    }
    async fn execute_pixels(
        &self,
        scene: PreparedScene,
        ctx: &OperationContext,
    ) -> Result<Accounted<RgbaImage>> {
        self.renders
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pixels = match &self.backend {
            #[cfg(feature = "render-cpu")]
            Backend::Cpu => {
                self.scheduler
                    .blocking(WorkClass::Http, ctx.clone(), move |c| {
                        Ok(CpuRenderer::new(1).render(&scene, &c)?)
                    })
                    .await?
            }
            #[cfg(feature = "render-gpu")]
            Backend::Gpu(renderer) => {
                self.scheduler
                    .asynchronous(WorkClass::Http, ctx.clone(), async {
                        Ok(renderer.lock().await.render_pixels(&scene, ctx).await?)
                    })
                    .await?
            }
        };
        let hold = ctx.reserve(pixels.pixels().len())?;
        Ok(Accounted::new(pixels, vec![hold]))
    }
}
#[derive(Serialize, Deserialize)]
struct ResponseEnvelope {
    key: String,
    etag: String,
    revision: String,
    backend: String,
    incomplete: bool,
    bytes: Vec<u8>,
}
/// Affine views use a conservative native window with a two-pixel kernel halo.
/// Nonlinear projection deliberately retains base-level coverage: bounding a
/// curved footprint by its corners can silently omit visible source samples.
fn raster_window(
    store: &DerivedRaster,
    view: &View,
    allow_overview: bool,
) -> Result<Option<(usize, BlockWindow)>> {
    let grid = store.grid();
    let (width, height) = store.dimensions();
    if grid.crs != view.crs {
        return Ok(Some((
            0,
            BlockWindow {
                column: 0,
                row: 0,
                width,
                height,
            },
        )));
    }
    let [a, b, c, d, e, f] = grid.affine;
    let det = b * f - c * e;
    let dx = (view.bounds.max_x - view.bounds.min_x) / view.width as f64;
    let dy = (view.bounds.max_y - view.bounds.min_y) / view.height as f64;
    // The Frobenius norm bounds the largest native sampling direction;
    // a rotated/sheared grid must not select a coarser level along one axis.
    let sampling = dx.min(dy) / (b * b + c * c + e * e + f * f).sqrt();
    let level = if allow_overview {
        store.select_level(sampling)
    } else {
        0
    };
    let (width, height, factor) = store
        .level_dimensions(level)
        .ok_or_else(|| ServerError::io("missing raster level"))?;
    let factor = factor as f64;
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for (x, y) in [
        (view.bounds.min_x, view.bounds.min_y),
        (view.bounds.max_x, view.bounds.min_y),
        (view.bounds.max_x, view.bounds.max_y),
        (view.bounds.min_x, view.bounds.max_y),
    ] {
        let px = ((x - a) * f - (y - d) * c) / det / factor;
        let py = ((y - d) * b - (x - a) * e) / det / factor;
        bounds[0] = bounds[0].min(px);
        bounds[1] = bounds[1].min(py);
        bounds[2] = bounds[2].max(px);
        bounds[3] = bounds[3].max(py);
    }
    let x0 = (bounds[0].floor() - 2.).clamp(0., width as f64) as u32;
    let y0 = (bounds[1].floor() - 2.).clamp(0., height as f64) as u32;
    let x1 = (bounds[2].ceil() + 2.).clamp(0., width as f64) as u32;
    let y1 = (bounds[3].ceil() + 2.).clamp(0., height as f64) as u32;
    Ok((x1 > x0 && y1 > y0).then_some((
        level,
        BlockWindow {
            column: x0,
            row: y0,
            width: x1 - x0,
            height: y1 - y0,
        },
    )))
}

fn encode_response(v: &EncodedResponse, key: &str) -> Result<Vec<u8>> {
    serde_json::to_vec(&ResponseEnvelope {
        key: key.into(),
        etag: v.etag.clone(),
        revision: v.render_revision.clone(),
        backend: v.backend.clone(),
        incomplete: v.incomplete,
        bytes: v.bytes.clone(),
    })
    .map_err(ServerError::io)
}
fn decode_response(bytes: &[u8], key: &str, max: usize) -> Result<EncodedResponse> {
    let v: ResponseEnvelope = serde_json::from_slice(bytes).map_err(ServerError::io)?;
    if v.key != key
        || v.bytes.len() > max
        || v.etag != format!("\"{}\"", blake3::hash(&v.bytes).to_hex())
    {
        return Err(ServerError::io("invalid response cache"));
    }
    Ok(EncodedResponse {
        bytes: v.bytes,
        etag: v.etag,
        render_revision: v.revision,
        backend: v.backend,
        incomplete: v.incomplete,
        content_type: "image/png".into(),
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
pub(crate) fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>> {
    let f = File::open(path).map_err(ServerError::io)?;
    if f.metadata().map_err(ServerError::io)?.len() > max as u64 {
        return Err(ServerError::new(
            413,
            "source-limit",
            "Source exceeds profile",
        ));
    }
    let mut bytes = vec![];
    f.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(ServerError::io)?;
    if bytes.len() > max {
        return Err(ServerError::new(
            413,
            "source-limit",
            "Source exceeds profile",
        ));
    }
    Ok(bytes)
}
fn atomic_operation(root: &Path, op: &ResourceOperation) -> Result<()> {
    let bytes = serde_json::to_vec(op).map_err(ServerError::io)?;
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
    hash.update(
        &serde_json::to_vec(&(
            &source.format,
            &source.crs,
            &source.encoding,
            &source.sample_kind,
        ))
        .map_err(ServerError::io)?,
    );
    Ok(hash.finalize().to_hex().to_string())
}
fn directory_bytes(root: &Path) -> Result<u64> {
    let mut total = 0u64;
    let mut todo = vec![root.to_owned()];
    while let Some(path) = todo.pop() {
        for entry in fs::read_dir(path).map_err(ServerError::io)? {
            let entry = entry.map_err(ServerError::io)?;
            let meta = entry.metadata().map_err(ServerError::io)?;
            if entry.file_type().map_err(ServerError::io)?.is_symlink() {
                return Err(ServerError::io("owned directory contains a link"));
            }
            if meta.is_dir() {
                todo.push(entry.path());
            } else {
                total = total
                    .checked_add(meta.len())
                    .ok_or_else(|| ServerError::io("disk accounting overflow"))?;
            }
        }
    }
    Ok(total)
}

#[cfg(all(test, feature = "render-cpu"))]
mod tests {
    use super::*;

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
        };
        let mut bytes = encode_response(&response, &key).unwrap();
        let mut envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        envelope["bytes"][0] = serde_json::json!(9);
        bytes = serde_json::to_vec(&envelope).unwrap();
        state.responses.insert(key.clone(), bytes);
        let entry = state.responses.get(&key).await.unwrap().unwrap();
        assert!(decode_response(entry.value(), &key, 1024).is_err());
        state.responses.remove(&key);
        assert!(state.responses.get(&key).await.unwrap().is_none());
        state
            .responses
            .insert(key.clone(), encode_response(&response, &key).unwrap());
        let entry = state.responses.get(&key).await.unwrap().unwrap();
        assert_eq!(
            decode_response(entry.value(), &key, 1024).unwrap().bytes,
            response.bytes
        );
        state.shutdown().await.unwrap();
    }
}
