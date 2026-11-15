//! Single-writer catalog, immutable shards and an atomically replaced generation
//! pointer. Long preparation runs outside the commit mutex; ETags and references
//! are checked again against the latest complete generation before switching.
use super::*;
use crate::{GeelloError, OperationContext, operation::Reservation};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Dataset,
    Style,
    Map,
    Animation,
    Font,
}
impl ResourceKind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "dataset" => Ok(Self::Dataset),
            "style" => Ok(Self::Style),
            "map" => Ok(Self::Map),
            "animation" => Ok(Self::Animation),
            "font" => Ok(Self::Font),
            _ => Err(ServerError::invalid("Unknown resource kind")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Dataset => "dataset",
            Self::Style => "style",
            Self::Map => "map",
            Self::Animation => "animation",
            Self::Font => "font",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceKey {
    pub kind: ResourceKind,
    pub id: String,
}
impl ResourceKey {
    pub fn new(kind: ResourceKind, id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        if id.len() > 256 || !crate::config::valid_resource_id(&id) {
            return Err(ServerError::invalid("Invalid namespace:local identifier"));
        }
        Ok(Self { kind, id })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ownership {
    Managed,
    External,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    pub path: PathBuf,
    pub format: String,
    pub encoding: Option<String>,
    pub crs: Option<String>,
    pub sample_kind: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRecord {
    pub key: ResourceKey,
    pub ownership: Ownership,
    pub version: String,
    pub published: bool,
    pub source: SourceSpec,
    pub dependencies: Vec<ResourceKey>,
    pub data_version: String,
    pub derived_version: Option<String>,
    pub derived_path: Option<PathBuf>,
    /// Managed immutable directory. External registration never grants ownership.
    pub managed_directory: Option<PathBuf>,
    pub class: Option<String>,
    /// Immutable parsed-definition input. External source files remain outside
    /// Geello ownership; serving never reopens a mutable definition path.
    pub definition: Option<String>,
    pub type_bindings: Vec<(ResourceKey, ResourceKey)>,
}
impl ResourceRecord {
    pub fn etag(&self) -> String {
        format!("\"{}\"", self.version)
    }
}
#[derive(Debug, Clone)]
pub enum Precondition {
    Absent,
    Version(String),
}
impl Precondition {
    pub fn from_headers(h: &axum::http::HeaderMap) -> Result<Self> {
        if h.get_all("if-match").iter().count() > 1 || h.get_all("if-none-match").iter().count() > 1
        {
            return Err(ServerError::invalid("Duplicate precondition header"));
        }
        match (h.get("if-none-match"), h.get("if-match")) {
            (Some(v), None) if v == "*" => Ok(Self::Absent),
            (None, Some(v)) => {
                let v = v
                    .to_str()
                    .map_err(|_| ServerError::invalid("Invalid If-Match"))?;
                if !v.starts_with('"')
                    || !v.ends_with('"')
                    || v.len() < 3
                    || v.contains(',')
                    || v.starts_with("W/")
                {
                    return Err(ServerError::invalid("One strong resource ETag required"));
                }
                Ok(Self::Version(v.trim_matches('"').into()))
            }
            _ => Err(ServerError::new(
                428,
                "precondition-required",
                "Use If-None-Match: * to create or If-Match to mutate",
            )),
        }
    }
    fn check(&self, record: Option<&ResourceRecord>) -> Result<()> {
        match (self, record) {
            (Self::Absent, None) => Ok(()),
            (Self::Version(v), Some(r)) if v == &r.version => Ok(()),
            _ => Err(ServerError::conflict("Resource version changed")),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    name: String,
    hash: String,
    bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    schema: u32,
    number: u64,
    shards: BTreeMap<String, Member>,
    detached: BTreeSet<PathBuf>,
    retired: BTreeSet<PathBuf>,
}
#[derive(Serialize, Deserialize)]
struct DeletionJournal {
    key: ResourceKey,
    version: String,
    original: PathBuf,
    trash: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    schema: u32,
    generation: u64,
    manifest: Member,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Shard {
    records: Vec<ResourceRecord>,
}
#[derive(Debug)]
struct State {
    generation: Generation,
    records: BTreeMap<ResourceKey, ResourceRecord>,
    pins: BTreeMap<ResourceKey, usize>,
    pinned_paths: BTreeMap<PathBuf, usize>,
    _memory: Vec<Reservation>,
    records_bytes: usize,
}
pub struct Catalog {
    root: PathBuf,
    profile: ServerProfile,
    state: Mutex<State>,
    _lock: File,
    memory: OperationContext,
}
#[derive(Debug, Clone, Serialize)]
pub struct CatalogStats {
    pub generation: u64,
    pub resources: usize,
    pub pinned_versions: usize,
    pub detached_content: usize,
}
/// A resource graph is pinned in one short critical section. It can finish after
/// unpublish, while removal/content deletion returns conflict until it drains.
pub struct ResourcePin {
    pub records: BTreeMap<ResourceKey, ResourceRecord>,
    catalog: Arc<Catalog>,
    versions: Vec<ResourceKey>,
    paths: Vec<PathBuf>,
    _memory: Vec<Reservation>,
}
impl Drop for ResourcePin {
    fn drop(&mut self) {
        let mut s = self.catalog.state.lock().unwrap();
        for v in &self.versions {
            if let Some(n) = s.pins.get_mut(v) {
                *n -= 1;
                if *n == 0 {
                    s.pins.remove(v);
                }
            }
        }
        for p in &self.paths {
            if let Some(n) = s.pinned_paths.get_mut(p) {
                *n -= 1;
                if *n == 0 {
                    s.pinned_paths.remove(p);
                }
            }
        }
    }
}
impl Catalog {
    pub fn open(root: &Path, profile: ServerProfile) -> Result<Arc<Self>> {
        let memory = Scheduler::new(profile.clone()).memory_context();
        Self::open_with_context(root, profile, memory)
    }
    pub(crate) fn open_with_context(
        root: &Path,
        profile: ServerProfile,
        memory: OperationContext,
    ) -> Result<Arc<Self>> {
        for p in [
            "managed",
            "external",
            "registry/shards",
            "registry/generations",
            "registry/deletions",
            "staging",
            "operations",
            "derived",
            "response-cache",
        ] {
            fs::create_dir_all(root.join(p)).map_err(ServerError::io)?;
        }
        let root = root.canonicalize().map_err(ServerError::io)?;
        let lock = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("registry/writer.lock"))
            .map_err(ServerError::io)?;
        lock.try_lock()
            .map_err(|_| ServerError::conflict("Resource directory already has a writer"))?;
        let pointer = root.join("registry/catalog.toml");
        let mut records = BTreeMap::new();
        let mut holds = vec![];
        let generation = if pointer.exists() {
            let p_bytes = memory::read_file(&pointer, profile.max_registry_bytes, &memory)?;
            let p: Pointer =
                toml::from_str(std::str::from_utf8(&p_bytes).map_err(ServerError::io)?)
                    .map_err(|_| ServerError::invalid("Invalid resource TOML"))?;
            if p.schema != 1 {
                return Err(ServerError::io("unsupported pointer"));
            }
            let bytes = read_member(
                &root.join("registry/generations"),
                &p.manifest,
                profile.max_registry_bytes,
                &memory,
            )?;
            let g: Generation = serde_json::from_slice(&bytes).map_err(ServerError::io)?;
            if g.schema != 1 || g.number != p.generation {
                return Err(ServerError::io("invalid generation"));
            }
            for member in g.shards.values() {
                let bytes = read_member(
                    &root.join("registry/shards"),
                    member,
                    profile.max_registry_bytes,
                    &memory,
                )?;
                let shard: Shard =
                    toml::from_str(std::str::from_utf8(&bytes).map_err(ServerError::io)?)
                        .map_err(ServerError::io)?;
                for r in shard.records {
                    holds.push(memory.reserve(memory::model_bytes(&r)?)?);
                    ResourceKey::new(r.key.kind, r.key.id.clone())?;
                    if records.insert(r.key.clone(), r).is_some() {
                        return Err(ServerError::io("duplicate catalog record"));
                    }
                    if records.len() > profile.max_resources {
                        return Err(ServerError::new(
                            503,
                            "registry-budget",
                            "Catalog resource count exceeds profile",
                        ));
                    }
                }
            }
            g
        } else {
            Generation {
                schema: 1,
                number: 0,
                shards: BTreeMap::new(),
                detached: BTreeSet::new(),
                retired: BTreeSet::new(),
            }
        };
        holds.push(memory.reserve(memory::model_bytes(&generation)?)?);
        let records_bytes = records_memory(&records)?;
        let _validation = memory.reserve(records_bytes)?;
        let validation_ctx = memory.child(profile.context().limits(), None);
        validate_graph(&records, &validation_ctx)?;
        recover_deletions(&root, &records, profile.max_registry_bytes, &memory)?;
        Ok(Arc::new(Self {
            root,
            profile,
            state: Mutex::new(State {
                generation,
                records,
                pins: BTreeMap::new(),
                pinned_paths: BTreeMap::new(),
                _memory: holds,
                records_bytes,
            }),
            _lock: lock,
            memory,
        }))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn stats(&self) -> CatalogStats {
        let s = self.state.lock().unwrap();
        CatalogStats {
            generation: s.generation.number,
            resources: s.records.len(),
            pinned_versions: s.pins.len(),
            detached_content: s.generation.detached.len(),
        }
    }
    pub fn get(&self, key: &ResourceKey) -> Result<ResourceRecord> {
        self.state
            .lock()
            .unwrap()
            .records
            .get(key)
            .cloned()
            .ok_or_else(|| {
                ServerError::new(404, "resource-not-found", "Resource is not registered")
            })
    }
    pub(crate) fn get_accounted(
        &self,
        key: &ResourceKey,
    ) -> Result<crate::Accounted<ResourceRecord>> {
        let state = self.state.lock().unwrap();
        let record = state.records.get(key).ok_or_else(|| {
            ServerError::new(404, "resource-not-found", "Resource is not registered")
        })?;
        let hold = self.memory.reserve(memory::model_bytes(record)?)?;
        Ok(crate::Accounted::new(record.clone(), vec![hold]))
    }
    /// Retry suppression is useful only while its immutable publication remains
    /// current. This lookup does not copy the resource definition.
    pub(crate) fn is_current_version(&self, key: &ResourceKey, version: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .records
            .get(key)
            .is_some_and(|record| record.version == version)
    }
    pub(crate) fn list_accounted(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<(u64, crate::Accounted<Vec<ResourceRecord>>)> {
        if limit == 0 || limit > self.profile.max_list_items {
            return Err(ServerError::invalid("List limit exceeds profile"));
        }
        let state = self.state.lock().unwrap();
        let mut holds = vec![];
        let mut records = vec![];
        for record in state.records.values().skip(offset).take(limit) {
            holds.push(self.memory.reserve(memory::model_bytes(record)?)?);
            records.push(record.clone());
        }
        // The page's generation must describe these exact records, even if a
        // publication occurs immediately after this critical section ends.
        Ok((
            state.generation.number,
            crate::Accounted::new(records, holds),
        ))
    }
    pub(crate) fn find_dataset_source(&self, path: &Path) -> Result<Option<ResourceKey>> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .records
            .values()
            .find(|r| r.published && r.key.kind == ResourceKind::Dataset && r.source.path == path)
            .map(|r| r.key.clone()))
    }
    pub fn list(&self, offset: usize, limit: usize) -> Result<Vec<ResourceRecord>> {
        if limit == 0 || limit > self.profile.max_list_items {
            return Err(ServerError::invalid("List limit exceeds profile"));
        }
        Ok(self
            .state
            .lock()
            .unwrap()
            .records
            .values()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect())
    }
    pub fn check(&self, key: &ResourceKey, condition: &Precondition) -> Result<()> {
        condition.check(self.state.lock().unwrap().records.get(key))
    }
    pub fn pin(self: &Arc<Self>, key: &ResourceKey) -> Result<ResourcePin> {
        let mut s = self.state.lock().unwrap();
        let mut holds = vec![];
        let mut records = BTreeMap::new();
        let mut todo = vec![key.clone()];
        while let Some(k) = todo.pop() {
            if records.contains_key(&k) {
                continue;
            }
            let r = s.records.get(&k).filter(|r| r.published).ok_or_else(|| {
                ServerError::new(404, "resource-unpublished", "Resource is not published")
            })?;
            holds.push(self.memory.reserve(memory::model_bytes(r)?)?);
            todo.extend(r.dependencies.clone());
            records.insert(k, r.clone());
        }
        let versions = records.keys().cloned().collect::<Vec<_>>();
        for v in &versions {
            *s.pins.entry(v.clone()).or_default() += 1;
        }
        let mut paths = vec![];
        for record in records.values() {
            let managed_source = record
                .source
                .path
                .ancestors()
                .find(|p| p.parent() == Some(self.root.join("managed").as_path()))
                .map(Path::to_path_buf);
            for p in [
                &record.derived_path,
                &record.managed_directory,
                &managed_source,
            ]
            .into_iter()
            .flatten()
            {
                *s.pinned_paths.entry(p.clone()).or_default() += 1;
                paths.push(p.clone());
            }
        }
        Ok(ResourcePin {
            records,
            catalog: self.clone(),
            versions,
            paths,
            _memory: holds,
        })
    }
    pub fn publish(
        &self,
        mut record: ResourceRecord,
        condition: &Precondition,
    ) -> Result<ResourceRecord> {
        let mut s = self.state.lock().unwrap();
        condition.check(s.records.get(&record.key))?;
        if !s.records.contains_key(&record.key) && s.records.len() >= self.profile.max_resources {
            return Err(ServerError::new(503, "registry-budget", "Catalog is full"));
        }
        record.published = true;
        let _workspace = self.workspace(&s, memory::model_bytes(&record)?)?;
        let mut records = s.records.clone();
        records.insert(record.key.clone(), record.clone());
        let validation_ctx = self.memory.child(self.profile.context().limits(), None);
        validate_graph(&records, &validation_ctx)?;
        self.commit(&mut s, records, safe_shard(&record.key), None)?;
        Ok(record)
    }
    pub fn unpublish(&self, key: &ResourceKey, condition: &Precondition) -> Result<ResourceRecord> {
        let mut s = self.state.lock().unwrap();
        condition.check(s.records.get(key))?;
        check_references(&s, key, false)?;
        let _workspace = self.workspace(&s, 0)?;
        let mut records = s.records.clone();
        let r = records.get_mut(key).ok_or_else(|| {
            ServerError::new(404, "resource-not-found", "Resource not registered")
        })?;
        r.published = false;
        r.version = identity(&[r.version.as_bytes(), b"unpublished"]);
        let out = r.clone();
        self.commit(&mut s, records, safe_shard(key), None)?;
        Ok(out)
    }
    pub fn remove(
        &self,
        key: &ResourceKey,
        condition: &Precondition,
        delete_content: bool,
    ) -> Result<ResourceRecord> {
        let mut s = self.state.lock().unwrap();
        condition.check(s.records.get(key))?;
        let _workspace = self.workspace(&s, 0)?;
        let record = s.records.get(key).cloned().ok_or_else(|| {
            ServerError::new(404, "resource-not-found", "Resource not registered")
        })?;
        if delete_content && record.ownership == Ownership::External {
            return Err(ServerError::new(
                409,
                "external-content",
                "External content is not owned",
            ));
        }
        check_references(&s, key, true)?;
        let mut records = s.records.clone();
        records.remove(key);
        // Removal retains content in the generation ledger. Content deletion
        // commits a tombstone first; physical reclaim is separately recoverable.
        let detached = if !delete_content {
            record.managed_directory.clone()
        } else {
            None
        };
        if delete_content {
            let original = record
                .managed_directory
                .as_ref()
                .ok_or_else(|| ServerError::conflict("Managed content directory is missing"))?
                .canonicalize()
                .map_err(ServerError::io)?;
            if original.parent() != Some(self.root.join("managed").as_path()) {
                return Err(ServerError::io("unsafe managed deletion target"));
            }
            let id = operation_id();
            let trash = self.root.join("staging").join(format!("delete-{id}"));
            let journal_path = self
                .root
                .join("registry/deletions")
                .join(format!("{id}.json"));
            let journal = DeletionJournal {
                key: key.clone(),
                version: record.version.clone(),
                original: original.clone(),
                trash: trash.clone(),
            };
            atomic_write(
                &journal_path,
                &serde_json::to_vec(&journal).map_err(ServerError::io)?,
            )?;
            crash_checkpoint("delete-journal", &self.root);
            fs::rename(&original, &trash).map_err(ServerError::io)?;
            crash_checkpoint("delete-renamed", &self.root);
            if let Err(e) = self.commit(&mut s, records, safe_shard(key), None) {
                fs::rename(&trash, &original).map_err(ServerError::io)?;
                let _ = fs::remove_file(journal_path);
                return Err(e);
            }
            // Publication is committed. A purge failure leaves a durable journal
            // for roll-forward recovery; the directory can never become visible.
            crash_checkpoint("delete-committed", &self.root);
            if fs::remove_dir_all(&trash).is_ok() {
                let _ = fs::remove_file(journal_path);
            }
        } else {
            self.commit(&mut s, records, safe_shard(key), detached)?;
        }
        Ok(record)
    }
    /// Reserve before copying the catalog or constructing serializer/graph
    /// workspace. The previous generation remains charged until publication.
    fn workspace(&self, state: &State, extra: usize) -> Result<Reservation> {
        let bytes = state
            .records_bytes
            .checked_add(memory::model_bytes(&state.generation)?)
            .and_then(|v| v.checked_add(extra))
            .and_then(|v| v.checked_mul(2))
            .ok_or(GeelloError::SizeOverflow)?;
        Ok(self.memory.reserve(bytes)?)
    }
    fn retained(&self, records_bytes: usize, generation: &Generation) -> Result<Vec<Reservation>> {
        let bytes = records_bytes
            .checked_add(memory::model_bytes(generation)?)
            .ok_or(GeelloError::SizeOverflow)?;
        Ok(vec![self.memory.reserve(bytes)?])
    }
    fn commit(
        &self,
        s: &mut State,
        records: BTreeMap<ResourceKey, ResourceRecord>,
        shard_id: String,
        detached: Option<PathBuf>,
    ) -> Result<()> {
        let values = records
            .values()
            .filter(|r| safe_shard(&r.key) == shard_id)
            .cloned()
            .collect();
        let bytes = toml::to_string(&Shard { records: values })
            .map_err(ServerError::io)?
            .into_bytes();
        if bytes.len() > self.profile.max_registry_bytes {
            return Err(ServerError::new(
                503,
                "registry-budget",
                "Catalog shard exceeds profile",
            ));
        }
        let shard = immutable_member(
            &self.root.join("registry/shards"),
            &bytes,
            "toml",
            &self.memory,
        )?;
        crash_checkpoint("publish-shard", &self.root);
        let mut generation = s.generation.clone();
        for (key, old) in &s.records {
            if records.get(key).is_none_or(|r| r.version != old.version) {
                if let Some(path) = &old.derived_path
                    && !records
                        .values()
                        .any(|r| r.derived_path.as_ref() == Some(path))
                {
                    generation.retired.insert(path.clone());
                }
                if let Some(path) = &old.managed_directory
                    && detached.as_ref() != Some(path)
                    && !records
                        .values()
                        .any(|r| r.managed_directory.as_ref() == Some(path))
                {
                    generation.retired.insert(path.clone());
                }
            }
        }
        generation.number = generation
            .number
            .checked_add(1)
            .ok_or_else(|| ServerError::io("generation overflow"))?;
        generation.shards.insert(shard_id.clone(), shard);
        if let Some(p) = detached {
            generation.detached.insert(p);
        }
        let mut records_bytes = s.records_bytes;
        for (key, old) in &s.records {
            if safe_shard(key) == shard_id {
                records_bytes = records_bytes
                    .checked_sub(memory::model_bytes(old)?)
                    .ok_or(GeelloError::SizeOverflow)?;
            }
        }
        for (key, record) in &records {
            if safe_shard(key) == shard_id {
                records_bytes = records_bytes
                    .checked_add(memory::model_bytes(record)?)
                    .ok_or(GeelloError::SizeOverflow)?;
            }
        }
        let holds = self.retained(records_bytes, &generation)?;
        self.save_generation(&generation)?;
        s.generation = generation;
        s.records = records;
        s._memory = holds;
        s.records_bytes = records_bytes;
        Ok(())
    }
    fn save_generation(&self, generation: &Generation) -> Result<()> {
        let bytes = crate::derived::bounded_json(
            generation,
            self.profile.max_registry_bytes,
            &self.memory,
        )?;
        let member = immutable_member(
            &self.root.join("registry/generations"),
            &bytes,
            "json",
            &self.memory,
        )?;
        crash_checkpoint("publish-generation", &self.root);
        let pointer = toml::to_string(&Pointer {
            schema: 1,
            generation: generation.number,
            manifest: member,
        })
        .map_err(ServerError::io)?;
        atomic_write(&self.root.join("registry/catalog.toml"), pointer.as_bytes())?;
        crash_checkpoint("publish-pointer", &self.root);
        Ok(())
    }
    /// Reclaim only explicitly retired versions; unknown managed originals and
    /// detached record-only removals are never interpreted as GC permission.
    pub fn collect_garbage(&self) -> Result<usize> {
        let mut s = self.state.lock().unwrap();
        let _workspace = self.workspace(&s, 0)?;
        let mut generation = s.generation.clone();
        let mut removed = 0;
        for path in &s.generation.retired {
            if s.pinned_paths.contains_key(path)
                || generation.detached.contains(path)
                || s.records.values().any(|r| {
                    r.source.path.starts_with(path)
                        || r.derived_path.as_ref() == Some(path)
                        || r.managed_directory.as_ref() == Some(path)
                })
            {
                continue;
            }
            if path.exists() {
                let resolved = path.canonicalize().map_err(ServerError::io)?;
                if ![self.root.join("managed"), self.root.join("derived")]
                    .iter()
                    .any(|base| resolved.parent() == Some(base.as_path()))
                {
                    return Err(ServerError::io("unsafe retired directory"));
                }
                fs::remove_dir_all(&resolved).map_err(ServerError::io)?;
            }
            generation.retired.remove(path);
            removed += 1;
        }
        if removed > 0 {
            generation.number += 1;
            let holds = self.retained(s.records_bytes, &generation)?;
            self.save_generation(&generation)?;
            s.generation = generation;
            s._memory = holds;
        }
        recover_deletions(
            &self.root,
            &s.records,
            self.profile.max_registry_bytes,
            &self.memory,
        )?;
        Ok(removed)
    }
    /// Explicit maintenance scan reports unknown managed directories. It never
    /// imports or removes originals and is not part of request-time lookup.
    pub fn reconcile(&self) -> Result<Vec<String>> {
        let s = self.state.lock().unwrap();
        let _workspace = self.workspace(&s, 0)?;
        let known: BTreeSet<_> = s
            .records
            .values()
            .filter_map(|r| r.managed_directory.clone())
            .chain(s.generation.detached.iter().cloned())
            .collect();
        let mut diagnostics = vec![];
        for entry in fs::read_dir(self.root.join("managed")).map_err(ServerError::io)? {
            let entry = entry.map_err(ServerError::io)?;
            if !known.contains(&entry.path()) {
                diagnostics.push(entry.file_name().to_string_lossy().into_owned());
                if diagnostics.len() >= self.profile.max_list_items {
                    break;
                }
            }
        }
        Ok(diagnostics)
    }
    /// Bounded diagnostics for unregistered derived entries, including missing
    /// or malformed manifests. Reporting never imports or deletes an artifact.
    pub fn reconcile_derived(&self) -> Result<Vec<String>> {
        let state = self.state.lock().unwrap();
        let mut diagnostics = vec![];
        for entry in fs::read_dir(self.root.join("derived")).map_err(ServerError::io)? {
            let entry = entry.map_err(ServerError::io)?;
            let path = entry.path();
            if state.pinned_paths.contains_key(&path)
                || state.generation.retired.contains(&path)
                || state.records.values().any(|r| {
                    r.derived_path.as_ref() == Some(&path) || r.source.path.starts_with(&path)
                })
            {
                continue;
            }
            diagnostics.push(entry.file_name().to_string_lossy().into_owned());
            if diagnostics.len() >= self.profile.max_list_items {
                break;
            }
        }
        Ok(diagnostics)
    }
    /// Rebuild only from the authoritative loaded catalog. Unknown resource
    /// directories never become registered merely because maintenance finds them.
    /// The old pointer remains usable until every rebuilt shard has been synced.
    pub fn rebuild_index(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        let _workspace = self.workspace(&state, 0)?;
        let mut grouped: BTreeMap<String, Vec<ResourceRecord>> = BTreeMap::new();
        for record in state.records.values() {
            grouped
                .entry(safe_shard(&record.key))
                .or_default()
                .push(record.clone());
        }
        let mut generation = state.generation.clone();
        generation.shards.clear();
        for (key, records) in grouped {
            let bytes = toml::to_string(&Shard { records })
                .map_err(ServerError::io)?
                .into_bytes();
            if bytes.len() > self.profile.max_registry_bytes {
                return Err(ServerError::new(
                    503,
                    "registry-budget",
                    "Rebuilt shard exceeds profile",
                ));
            }
            generation.shards.insert(
                key,
                immutable_member(
                    &self.root.join("registry/shards"),
                    &bytes,
                    "toml",
                    &self.memory,
                )?,
            );
        }
        generation.number = generation
            .number
            .checked_add(1)
            .ok_or_else(|| ServerError::io("generation overflow"))?;
        let holds = self.retained(state.records_bytes, &generation)?;
        self.save_generation(&generation)?;
        state.generation = generation;
        state._memory = holds;
        Ok(())
    }
    /// Explicit offline-style maintenance while holding the single-writer
    /// mutex. Snapshot pins own complete records, so no reader needs old index
    /// files. Original/detached content and derived blocks are untouched.
    pub fn compact_index(&self) -> Result<usize> {
        let state = self.state.lock().unwrap();
        let _workspace = self.workspace(&state, 0)?;
        let pointer_bytes = memory::read_file(
            &self.root.join("registry/catalog.toml"),
            self.profile.max_registry_bytes,
            &self.memory,
        )?;
        let pointer: Pointer =
            toml::from_str(std::str::from_utf8(&pointer_bytes).map_err(ServerError::io)?)
                .map_err(|_| ServerError::invalid("Invalid resource TOML"))?;
        let mut removed = 0;
        for (directory, keep) in [
            (
                "registry/shards",
                state
                    .generation
                    .shards
                    .values()
                    .map(|m| m.name.clone())
                    .collect::<BTreeSet<_>>(),
            ),
            (
                "registry/generations",
                BTreeSet::from([pointer.manifest.name]),
            ),
        ] {
            for entry in fs::read_dir(self.root.join(directory)).map_err(ServerError::io)? {
                let entry = entry.map_err(ServerError::io)?;
                if entry.file_type().map_err(ServerError::io)?.is_file()
                    && !keep.contains(&entry.file_name().to_string_lossy().into_owned())
                {
                    fs::remove_file(entry.path()).map_err(ServerError::io)?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
    /// Called only while the runtime preparation mutex excludes all builders.
    /// Derived artifacts are disposable; managed/detached originals are never
    /// candidates. Unknown names and mismatched manifests remain diagnostics.
    pub(crate) fn collect_derived_orphans(&self) -> Result<usize> {
        let state = self.state.lock().unwrap();
        let mut removed = 0;
        let base = self.root.join("derived");
        for entry in fs::read_dir(&base).map_err(ServerError::io)? {
            let entry = entry.map_err(ServerError::io)?;
            if !entry.file_type().map_err(ServerError::io)?.is_dir() {
                continue;
            }
            let path = entry.path();
            if state.pinned_paths.contains_key(&path)
                || state.generation.retired.contains(&path)
                || state.records.values().any(|r| {
                    r.derived_path.as_ref() == Some(&path) || r.source.path.starts_with(&path)
                })
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let disposable = if name.starts_with("preparing-") {
                true
            } else if name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
                // An unidentified directory is not evidence of permission to
                // delete it, nor a reason to reject unrelated preparation.
                // Keep malformed/unreadable artifacts for explicit diagnosis.
                match memory::read_file(
                    &path.join("manifest.json"),
                    self.profile.max_registry_bytes,
                    &self.memory,
                ) {
                    Ok(bytes) => blake3::hash(&bytes).to_hex().as_str() == name,
                    Err(error) if error.status == 503 => return Err(error),
                    Err(_) => false,
                }
            } else {
                false
            };
            if disposable {
                let resolved = path.canonicalize().map_err(ServerError::io)?;
                if resolved.parent() != Some(base.as_path()) {
                    return Err(ServerError::io("unsafe derived orphan target"));
                }
                fs::remove_dir_all(resolved).map_err(ServerError::io)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
    /// Startup-only: the exclusive writer lock is held and deletion recovery
    /// has completed, so no request can still own a private upload directory.
    pub(crate) fn collect_abandoned_uploads(&self) -> Result<usize> {
        let state = self.state.lock().unwrap();
        let base = self.root.join("staging");
        let mut removed = 0;
        for entry in fs::read_dir(&base).map_err(ServerError::io)? {
            let entry = entry.map_err(ServerError::io)?;
            let path = entry.path();
            if !entry.file_name().to_string_lossy().starts_with("upload-")
                || !entry.file_type().map_err(ServerError::io)?.is_dir()
                || state
                    .records
                    .values()
                    .any(|r| r.source.path.starts_with(&path))
            {
                continue;
            }
            let resolved = path.canonicalize().map_err(ServerError::io)?;
            if resolved.parent() != Some(base.as_path()) {
                return Err(ServerError::io("unsafe upload staging target"));
            }
            fs::remove_dir_all(resolved).map_err(ServerError::io)?;
            removed += 1;
        }
        Ok(removed)
    }
}
fn check_references(s: &State, key: &ResourceKey, pins: bool) -> Result<()> {
    let references: Vec<_> = s
        .records
        .values()
        .filter(|r| (pins || r.published) && r.key != *key && r.dependencies.contains(key))
        .take(32)
        .map(|r| r.key.clone())
        .collect();
    let pinned = pins && s.pins.contains_key(key);
    if !references.is_empty() || pinned {
        let mut e = ServerError::new(
            409,
            "resource-in-use",
            if pinned {
                "Resource has in-flight snapshot pins"
            } else {
                "Resource has published dependents"
            },
        );
        e.references = references;
        return Err(e);
    }
    Ok(())
}
fn recover_deletions(
    root: &Path,
    records: &BTreeMap<ResourceKey, ResourceRecord>,
    max: usize,
    ctx: &OperationContext,
) -> Result<()> {
    for entry in fs::read_dir(root.join("registry/deletions")).map_err(ServerError::io)? {
        let entry = entry.map_err(ServerError::io)?;
        let bytes = memory::read_file(&entry.path(), max, ctx)?;
        let j: DeletionJournal = serde_json::from_slice(&bytes).map_err(ServerError::io)?;
        if j.original.parent() != Some(root.join("managed").as_path())
            || j.trash.parent() != Some(root.join("staging").as_path())
        {
            return Err(ServerError::io("unsafe deletion journal"));
        }
        let rollback = records.get(&j.key).is_some_and(|r| r.version == j.version);
        if j.trash.exists() {
            if rollback {
                if j.original.exists() {
                    return Err(ServerError::io("ambiguous deletion recovery"));
                }
                fs::rename(&j.trash, &j.original).map_err(ServerError::io)?;
            } else {
                let resolved = j.trash.canonicalize().map_err(ServerError::io)?;
                if resolved.parent() != Some(root.join("staging").as_path()) {
                    return Err(ServerError::io("unsafe purge target"));
                }
                fs::remove_dir_all(resolved).map_err(ServerError::io)?;
            }
        }
        fs::remove_file(entry.path()).map_err(ServerError::io)?;
    }
    Ok(())
}
fn validate_graph(
    records: &BTreeMap<ResourceKey, ResourceRecord>,
    ctx: &OperationContext,
) -> Result<()> {
    // Iterative DFS keeps a large resource graph off the process call stack.
    // Both published and unpublished definitions must be acyclic.
    let mut done = BTreeSet::new();
    for key in records.keys() {
        let mut active = BTreeSet::new();
        let mut todo = vec![(key.clone(), false)];
        while let Some((key, leaving)) = todo.pop() {
            if leaving {
                active.remove(&key);
                done.insert(key);
                continue;
            }
            if done.contains(&key) {
                continue;
            }
            if !active.insert(key.clone()) {
                return Err(ServerError::conflict("Resource dependency cycle"));
            }
            todo.push((key.clone(), true));
            if let Some(record) = records.get(&key) {
                for dependency in &record.dependencies {
                    todo.push((dependency.clone(), false));
                }
            }
        }
    }
    for r in records.values().filter(|r| r.published) {
        ctx.checkpoint(1)?;
        for dependency in &r.dependencies {
            if dependency == &r.key || !records.get(dependency).is_some_and(|v| v.published) {
                return Err(ServerError::conflict(
                    "Published dependency is missing or unpublished",
                ));
            }
        }
        for (dataset, style) in &r.type_bindings {
            if records.get(dataset).and_then(|r| r.class.as_ref())
                != records.get(style).and_then(|r| r.class.as_ref())
            {
                return Err(ServerError::conflict(
                    "Published map dataset/style type mismatch",
                ));
            }
        }
        // Revalidate against the graph being committed, including when a style
        // or animation changes beneath an existing map. Preparation-time reads
        // alone cannot prevent a concurrent dependency update from racing us.
        if r.key.kind == ResourceKind::Map
            && let Some(text) = r.definition.as_deref()
        {
            let _map_hold = ctx.reserve(memory::envelope(text.len())?)?;
            let map = crate::config::parse_map(text)?;
            for item in map
                .items
                .iter()
                .filter(|i| !i.animation_bindings.is_empty())
            {
                let style_record = records
                    .get(&ResourceKey::new(ResourceKind::Style, &item.style)?)
                    .ok_or_else(|| ServerError::conflict("Missing animation style"))?;
                let text = style_record
                    .definition
                    .as_deref()
                    .ok_or_else(|| ServerError::conflict("Missing animation style definition"))?;
                let _style_hold = ctx.reserve(memory::envelope(text.len())?)?;
                let style = crate::config::parse_style(text)?;
                let mut animations = BTreeMap::new();
                let mut holds = vec![];
                for binding in &item.animation_bindings {
                    if animations.contains_key(&binding.animation) {
                        continue;
                    }
                    let record = records
                        .get(&ResourceKey::new(
                            ResourceKind::Animation,
                            &binding.animation,
                        )?)
                        .ok_or_else(|| ServerError::conflict("Missing animation"))?;
                    let text = record
                        .definition
                        .as_deref()
                        .ok_or_else(|| ServerError::conflict("Missing animation definition"))?;
                    holds.push(ctx.reserve(memory::envelope(text.len())?)?);
                    animations.insert(
                        binding.animation.clone(),
                        crate::config::parse_animation(text)?,
                    );
                }
                crate::config::prepare_style_animations(&style, item, &animations, ctx)?;
            }
        }
    }
    Ok(())
}
/// Crash hooks are compiled only into unit-test binaries. The production
/// binary has no environment-controlled pause or fault injection capability.
fn crash_checkpoint(_name: &str, _root: &Path) {
    #[cfg(test)]
    if std::env::var("GEELLO_CATALOG_CRASH_POINT").as_deref() == Ok(_name)
        && _root.join("crash-armed").exists()
    {
        fs::write(_root.join("crash-reached"), _name).unwrap();
        loop {
            std::thread::park();
        }
    }
}
fn records_memory(records: &BTreeMap<ResourceKey, ResourceRecord>) -> Result<usize> {
    records.values().try_fold(0usize, |total, record| {
        total
            .checked_add(memory::model_bytes(record)?)
            .ok_or_else(|| GeelloError::SizeOverflow.into())
    })
}
fn safe_shard(key: &ResourceKey) -> String {
    let namespace = key.id.split(':').next().unwrap();
    let bucket = blake3::hash(key.id.as_bytes()).as_bytes()[0];
    format!("{}-{namespace}-{bucket:02x}", key.kind.name())
}
pub(crate) fn identity(parts: &[&[u8]]) -> String {
    let mut h = blake3::Hasher::new();
    for p in parts {
        h.update(&(p.len() as u64).to_le_bytes());
        h.update(p);
    }
    h.finalize().to_hex().to_string()
}
pub(crate) fn operation_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "op-{:x}-{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| ServerError::io("missing parent"))?;
    let mut f = tempfile::NamedTempFile::new_in(parent).map_err(ServerError::io)?;
    f.write_all(bytes).map_err(ServerError::io)?;
    f.as_file().sync_all().map_err(ServerError::io)?;
    f.persist(path).map_err(ServerError::io)?;
    Ok(())
}
fn immutable_member(
    root: &Path,
    bytes: &[u8],
    extension: &str,
    ctx: &OperationContext,
) -> Result<Member> {
    let hash = blake3::hash(bytes).to_hex().to_string();
    let name = format!("{hash}.{extension}");
    let member = Member {
        name,
        hash,
        bytes: bytes.len(),
    };
    // Existence alone does not establish integrity. Rebuild/publication has
    // authoritative bytes and can atomically restore a damaged member before
    // committing a pointer to it. Never publish a known-bad reused index file.
    match read_member(root, &member, bytes.len(), ctx) {
        Ok(_) => {}
        Err(error) if error.status == 503 => return Err(error),
        Err(_) => atomic_write(&root.join(&member.name), bytes)?,
    }
    Ok(member)
}
fn read_member(
    root: &Path,
    m: &Member,
    max: usize,
    ctx: &OperationContext,
) -> Result<crate::Accounted<Vec<u8>>> {
    if m.name.contains(['/', '\\', ':']) || m.name.starts_with('.') || m.bytes > max {
        return Err(ServerError::io("invalid shard member"));
    }
    let bytes = memory::read_file(&root.join(&m.name), m.bytes, ctx)?;
    if bytes.len() != m.bytes || blake3::hash(&bytes).to_hex().as_str() != m.hash {
        return Err(ServerError::io("catalog checksum mismatch"));
    }
    Ok(bytes)
}
#[cfg(all(test, feature = "render-cpu"))]
mod tests {
    use super::*;

    fn fixture(root: &Path) -> (Arc<Catalog>, ResourceRecord) {
        let config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
        let catalog = Catalog::open(root, config.profile).unwrap();
        let directory = catalog.root().join("managed/original");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("definition.toml"), b"original bytes").unwrap();
        let record = ResourceRecord {
            key: ResourceKey::new(ResourceKind::Style, "crash:style").unwrap(),
            ownership: Ownership::Managed,
            version: "v1".into(),
            published: true,
            source: SourceSpec {
                path: directory.join("definition.toml"),
                format: "toml".into(),
                encoding: None,
                crs: None,
                sample_kind: None,
            },
            dependencies: vec![],
            data_version: "data-v1".into(),
            derived_version: None,
            derived_path: None,
            managed_directory: Some(directory),
            class: Some("vector".into()),
            definition: Some("immutable definition".into()),
            type_bindings: vec![],
        };
        (catalog, record)
    }

    #[test]
    fn process_kill_at_catalog_commit_boundaries_recovers_complete_state() {
        const ROOT_ENV: &str = "GEELLO_CATALOG_CRASH_ROOT";
        if let Some(root) = std::env::var_os(ROOT_ENV) {
            let (catalog, record) = fixture(Path::new(&root));
            catalog
                .publish(record.clone(), &Precondition::Absent)
                .unwrap();
            fs::write(catalog.root().join("crash-armed"), b"armed").unwrap();
            let point = std::env::var("GEELLO_CATALOG_CRASH_POINT").unwrap();
            if point.starts_with("delete-") {
                catalog
                    .remove(&record.key, &Precondition::Version("v1".into()), true)
                    .unwrap();
            } else {
                let mut replacement = record;
                replacement.version = "v2".into();
                replacement.definition = Some("replacement definition".into());
                catalog
                    .publish(replacement, &Precondition::Version("v1".into()))
                    .unwrap();
            }
            panic!("crash checkpoint was not reached");
        }
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        for point in [
            "publish-shard",
            "publish-generation",
            "publish-pointer",
            "delete-journal",
            "delete-renamed",
            "delete-committed",
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut child = Child(std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "server::catalog::tests::process_kill_at_catalog_commit_boundaries_recovers_complete_state", "--nocapture"])
                .env(ROOT_ENV, root.path()).env("GEELLO_CATALOG_CRASH_POINT", point)
                .stdout(std::process::Stdio::null()).spawn().unwrap());
            let started = std::time::Instant::now();
            while !root.path().join("crash-reached").exists() {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "child exited before {point}"
                );
                assert!(
                    started.elapsed() < std::time::Duration::from_secs(15),
                    "checkpoint timeout: {point}"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            child.0.kill().unwrap();
            child.0.wait().unwrap();
            let config =
                ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
            let catalog = Catalog::open(root.path(), config.profile.clone()).unwrap();
            let key = ResourceKey::new(ResourceKind::Style, "crash:style").unwrap();
            if point == "delete-committed" {
                assert_eq!(catalog.get(&key).unwrap_err().status, 404);
                assert!(!catalog.root().join("managed/original").exists());
            } else {
                let record = catalog.get(&key).unwrap();
                assert_eq!(
                    record.version,
                    if point == "publish-pointer" {
                        "v2"
                    } else {
                        "v1"
                    }
                );
                assert_eq!(fs::read(record.source.path).unwrap(), b"original bytes");
            }
            assert_eq!(
                fs::read_dir(catalog.root().join("registry/deletions"))
                    .unwrap()
                    .count(),
                0
            );
            assert_eq!(
                fs::read_dir(catalog.root().join("staging"))
                    .unwrap()
                    .count(),
                0
            );
            let generation = catalog.stats().generation;
            drop(catalog);
            let reopened = Catalog::open(root.path(), config.profile).unwrap();
            assert_eq!(
                reopened.stats().generation,
                generation,
                "second restart at {point}"
            );
        }
    }
}
