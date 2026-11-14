//! Single-writer catalog, immutable shards and an atomically replaced generation
//! pointer. Long preparation runs outside the commit mutex; ETags and references
//! are checked again against the latest complete generation before switching.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
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
}
pub struct Catalog {
    root: PathBuf,
    profile: ServerProfile,
    state: Mutex<State>,
    _lock: File,
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
        let generation = if pointer.exists() {
            let p: Pointer = read_toml(&pointer, profile.max_registry_bytes)?;
            if p.schema != 1 {
                return Err(ServerError::io("unsupported pointer"));
            }
            let bytes = read_member(
                &root.join("registry/generations"),
                &p.manifest,
                profile.max_registry_bytes,
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
                )?;
                let shard: Shard =
                    toml::from_str(std::str::from_utf8(&bytes).map_err(ServerError::io)?)
                        .map_err(ServerError::io)?;
                for r in shard.records {
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
        validate_graph(&records)?;
        recover_deletions(&root, &records, profile.max_registry_bytes)?;
        Ok(Arc::new(Self {
            root,
            profile,
            state: Mutex::new(State {
                generation,
                records,
                pins: BTreeMap::new(),
                pinned_paths: BTreeMap::new(),
            }),
            _lock: lock,
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
        let mut records = BTreeMap::new();
        let mut todo = vec![key.clone()];
        while let Some(k) = todo.pop() {
            if records.contains_key(&k) {
                continue;
            }
            let r = s.records.get(&k).filter(|r| r.published).ok_or_else(|| {
                ServerError::new(404, "resource-unpublished", "Resource is not published")
            })?;
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
        let mut records = s.records.clone();
        records.insert(record.key.clone(), record.clone());
        validate_graph(&records)?;
        self.commit(&mut s, records, safe_shard(&record.key), None)?;
        Ok(record)
    }
    pub fn unpublish(&self, key: &ResourceKey, condition: &Precondition) -> Result<ResourceRecord> {
        let mut s = self.state.lock().unwrap();
        condition.check(s.records.get(key))?;
        check_references(&s, key, false)?;
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
            fs::rename(&original, &trash).map_err(ServerError::io)?;
            if let Err(e) = self.commit(&mut s, records, safe_shard(key), None) {
                fs::rename(&trash, &original).map_err(ServerError::io)?;
                let _ = fs::remove_file(journal_path);
                return Err(e);
            }
            // Publication is committed. A purge failure leaves a durable journal
            // for roll-forward recovery; the directory can never become visible.
            if fs::remove_dir_all(&trash).is_ok() {
                let _ = fs::remove_file(journal_path);
            }
        } else {
            self.commit(&mut s, records, safe_shard(key), detached)?;
        }
        Ok(record)
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
        let shard = immutable_member(&self.root.join("registry/shards"), &bytes, "toml")?;
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
        generation.shards.insert(shard_id, shard);
        if let Some(p) = detached {
            generation.detached.insert(p);
        }
        self.save_generation(&generation)?;
        s.generation = generation;
        s.records = records;
        Ok(())
    }
    fn save_generation(&self, generation: &Generation) -> Result<()> {
        let bytes = serde_json::to_vec(generation).map_err(ServerError::io)?;
        if bytes.len() > self.profile.max_registry_bytes {
            return Err(ServerError::new(
                503,
                "registry-budget",
                "Generation manifest exceeds profile",
            ));
        }
        let member = immutable_member(&self.root.join("registry/generations"), &bytes, "json")?;
        let pointer = toml::to_string(&Pointer {
            schema: 1,
            generation: generation.number,
            manifest: member,
        })
        .map_err(ServerError::io)?;
        atomic_write(&self.root.join("registry/catalog.toml"), pointer.as_bytes())?;
        Ok(())
    }
    /// Reclaim only explicitly retired versions; unknown managed originals and
    /// detached record-only removals are never interpreted as GC permission.
    pub fn collect_garbage(&self) -> Result<usize> {
        let mut s = self.state.lock().unwrap();
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
            self.save_generation(&generation)?;
            s.generation = generation;
        }
        recover_deletions(&self.root, &s.records, self.profile.max_registry_bytes)?;
        Ok(removed)
    }
    /// Explicit maintenance scan reports unknown managed directories. It never
    /// imports or removes originals and is not part of request-time lookup.
    pub fn reconcile(&self) -> Result<Vec<String>> {
        let s = self.state.lock().unwrap();
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
    /// Rebuild only from the authoritative loaded catalog. Unknown resource
    /// directories never become registered merely because maintenance finds them.
    /// The old pointer remains usable until every rebuilt shard has been synced.
    pub fn rebuild_index(&self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
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
                immutable_member(&self.root.join("registry/shards"), &bytes, "toml")?,
            );
        }
        generation.number = generation
            .number
            .checked_add(1)
            .ok_or_else(|| ServerError::io("generation overflow"))?;
        self.save_generation(&generation)?;
        state.generation = generation;
        Ok(())
    }
    /// Explicit offline-style maintenance while holding the single-writer
    /// mutex. Snapshot pins own complete records, so no reader needs old index
    /// files. Original/detached content and derived blocks are untouched.
    pub fn compact_index(&self) -> Result<usize> {
        let state = self.state.lock().unwrap();
        let pointer: Pointer = read_toml(
            &self.root.join("registry/catalog.toml"),
            self.profile.max_registry_bytes,
        )?;
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
                let file = File::open(path.join("manifest.json")).map_err(ServerError::io)?;
                let mut bytes = vec![];
                file.take(self.profile.max_registry_bytes as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(ServerError::io)?;
                bytes.len() <= self.profile.max_registry_bytes
                    && blake3::hash(&bytes).to_hex().as_str() == name
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
) -> Result<()> {
    for entry in fs::read_dir(root.join("registry/deletions")).map_err(ServerError::io)? {
        let entry = entry.map_err(ServerError::io)?;
        let file = File::open(entry.path()).map_err(ServerError::io)?;
        if file.metadata().map_err(ServerError::io)?.len() > max as u64 {
            return Err(ServerError::io("oversized deletion journal"));
        }
        let mut bytes = vec![];
        file.take(max as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(ServerError::io)?;
        if bytes.len() > max {
            return Err(ServerError::io("oversized deletion journal"));
        }
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
fn validate_graph(records: &BTreeMap<ResourceKey, ResourceRecord>) -> Result<()> {
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
    }
    Ok(())
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
fn immutable_member(root: &Path, bytes: &[u8], extension: &str) -> Result<Member> {
    let hash = blake3::hash(bytes).to_hex().to_string();
    let name = format!("{hash}.{extension}");
    let path = root.join(&name);
    if !path.exists() {
        atomic_write(&path, bytes)?;
    }
    Ok(Member {
        name,
        hash,
        bytes: bytes.len(),
    })
}
fn read_member(root: &Path, m: &Member, max: usize) -> Result<Vec<u8>> {
    if m.name.contains(['/', '\\', ':']) || m.name.starts_with('.') || m.bytes > max {
        return Err(ServerError::io("invalid shard member"));
    }
    let mut bytes = vec![];
    File::open(root.join(&m.name))
        .map_err(ServerError::io)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(ServerError::io)?;
    if bytes.len() != m.bytes || blake3::hash(&bytes).to_hex().as_str() != m.hash {
        return Err(ServerError::io("catalog checksum mismatch"));
    }
    Ok(bytes)
}
pub(crate) fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path, max: usize) -> Result<T> {
    let mut bytes = vec![];
    File::open(path)
        .map_err(ServerError::io)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(ServerError::io)?;
    if bytes.len() > max {
        return Err(ServerError::new(413, "body-limit", "TOML exceeds profile"));
    }
    toml::from_str(std::str::from_utf8(&bytes).map_err(ServerError::io)?)
        .map_err(|_| ServerError::invalid("Invalid resource TOML"))
}
