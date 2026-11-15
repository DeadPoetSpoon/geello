//! Strict startup schema. Every numerical capacity is explicitly supplied by
//! an experimental deployment profile pending G-07 owner confirmation.
use super::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
/// Bootstrap ceiling applied before the TOML profile is available. Runtime
/// resource and metadata limits remain profile-controlled after startup.
const MAX_CONFIG_BYTES: usize = 1 << 20;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendSelection {
    Cpu,
    Gpu,
    GpuPreferred,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    pub public_base_url: String,
    pub data_directory: PathBuf,
    pub external_roots: Vec<PathBuf>,
    pub backend: BackendSelection,
    pub profile: ServerProfile,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerProfile {
    pub max_upload_bytes: usize,
    pub max_expanded_bytes: usize,
    pub max_archive_files: usize,
    pub max_member_bytes: usize,
    pub max_compression_ratio: u64,
    pub max_registry_bytes: usize,
    pub max_resources: usize,
    pub max_operations: usize,
    pub max_source_bytes: usize,
    pub max_record_bytes: usize,
    pub max_features: usize,
    pub max_live_bytes: u64,
    pub max_work: u64,
    pub max_commands: usize,
    pub max_derived_bytes: u64,
    pub max_managed_bytes: u64,
    pub block_size: u32,
    pub shard_features: usize,
    pub max_image_bytes: usize,
    pub max_dimension: u32,
    pub object_cache_bytes: u64,
    pub response_memory_bytes: usize,
    pub response_disk_bytes: usize,
    pub active_http: usize,
    pub active_animation: usize,
    pub active_background: usize,
    pub queued_per_class: usize,
    pub operation_timeout_ms: u64,
    pub max_feature_cursors: usize,
    pub feature_cursor_ttl_ms: u64,
    pub max_animation_fps: u32,
    pub max_list_items: usize,
    pub metatile_size: u32,
    pub metatile_buffer: u32,
}
impl ServerConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(ServerError::io)?;
        let length = file.metadata().map_err(ServerError::io)?.len();
        if length > MAX_CONFIG_BYTES as u64 {
            return Err(ServerError::new(
                413,
                "config-limit",
                "Server TOML exceeds bootstrap limit",
            ));
        }
        let mut bytes = Vec::with_capacity(length as usize + 1);
        file.take(length + 1)
            .read_to_end(&mut bytes)
            .map_err(ServerError::io)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(ServerError::new(
                413,
                "config-limit",
                "Server TOML exceeds bootstrap limit",
            ));
        }
        if bytes.len() as u64 != length {
            return Err(crate::GeelloError::SourceChanged.into());
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| ServerError::invalid("Server TOML must be UTF-8"))?;
        let mut config: Self =
            toml::from_str(&text).map_err(|_| ServerError::invalid("Invalid server TOML"))?;
        let base = path.parent().unwrap_or(Path::new("."));
        if config.data_directory.is_relative() {
            config.data_directory = base.join(&config.data_directory);
        }
        for p in &mut config.external_roots {
            if p.is_relative() {
                *p = base.join(&*p);
            }
        }
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if !(self.public_base_url.starts_with("https://")
            || self.public_base_url.starts_with("http://"))
            || self.public_base_url.contains(['?', '#'])
        {
            return Err(ServerError::invalid(
                "Explicit absolute public_base_url required",
            ));
        }
        self.profile.validate()
    }
}

impl ServerProfile {
    pub fn validate(&self) -> Result<()> {
        let values = [
            self.max_upload_bytes,
            self.max_expanded_bytes,
            self.max_archive_files,
            self.max_member_bytes,
            self.max_registry_bytes,
            self.max_resources,
            self.max_operations,
            self.max_feature_cursors,
            self.max_source_bytes,
            self.max_record_bytes,
            self.max_features,
            self.max_commands,
            self.max_image_bytes,
            self.response_memory_bytes,
            self.response_disk_bytes,
            self.active_http,
            self.active_animation,
            self.active_background,
            self.queued_per_class,
            self.max_list_items,
            self.shard_features,
        ];
        if values.contains(&0)
            || self.max_live_bytes == 0
            || self.max_work == 0
            || self.max_derived_bytes == 0
            || self.max_managed_bytes == 0
            || self.object_cache_bytes == 0
            || self.block_size == 0
            || self.max_dimension == 0
            || self.max_dimension > u16::MAX as u32
            || self.max_compression_ratio == 0
            || self.operation_timeout_ms == 0
            || self.feature_cursor_ttl_ms == 0
            || self.max_animation_fps == 0
            || self.max_animation_fps > 1000
            || Instant::now()
                .checked_add(Duration::from_millis(self.feature_cursor_ttl_ms))
                .is_none()
            || self.metatile_size == 0
        {
            return Err(ServerError::invalid(
                "Positive explicit profile capacities required",
            ));
        }
        if self
            .metatile_size
            .checked_mul(256)
            .and_then(|v| {
                self.metatile_buffer
                    .checked_mul(2)
                    .and_then(|b| v.checked_add(b))
            })
            .is_none_or(|v| v > self.max_dimension)
        {
            return Err(ServerError::invalid(
                "Metatile exceeds configured dimension",
            ));
        }
        if !self.metatile_size.is_power_of_two() {
            return Err(ServerError::invalid("Metatile size must be a power of two"));
        }
        if self.max_live_bytes < 8
            || self
                .active_http
                .checked_add(self.queued_per_class)
                .is_none()
            || self
                .active_animation
                .checked_add(self.queued_per_class)
                .is_none()
            || self
                .active_background
                .checked_add(self.queued_per_class)
                .is_none()
            || [
                self.active_http,
                self.active_animation,
                self.active_background,
                self.queued_per_class,
            ]
            .into_iter()
            .any(|v| v > tokio::sync::Semaphore::MAX_PERMITS / 2)
        {
            return Err(ServerError::invalid(
                "Profile admission capacity overflows runtime limits",
            ));
        }
        Ok(())
    }
    pub fn context(&self) -> crate::OperationContext {
        crate::OperationContext::new(
            crate::OperationLimits {
                max_live_bytes: self.max_live_bytes,
                max_work: self.max_work,
                max_commands: self.max_commands,
                max_diagnostics: 32,
            },
            Instant::now().checked_add(Duration::from_millis(self.operation_timeout_ms)),
        )
    }
    pub fn store_limits(&self) -> crate::derived::StoreLimits {
        crate::derived::StoreLimits {
            max_disk_bytes: self.max_derived_bytes,
            max_manifest_bytes: self.max_registry_bytes,
            max_shard_features: self.shard_features,
            max_record_bytes: self.max_record_bytes,
            max_block_bytes: self.max_live_bytes.min(usize::MAX as u64) as usize / 4,
        }
    }
    pub fn reader_limits(&self) -> crate::readers::ReaderLimits {
        crate::readers::ReaderLimits {
            max_source_bytes: self.max_source_bytes as u64,
            max_record_bytes: self.max_record_bytes,
            max_features: self.max_features,
            max_coordinates_per_feature: self.max_record_bytes / 16,
            max_depth: 48,
            batch_features: self.shard_features,
            batch_bytes: self.max_live_bytes.min(usize::MAX as u64) as usize / 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_startup_toml_is_rejected_before_parsing() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("server.toml");
        std::fs::write(&path, vec![b' '; MAX_CONFIG_BYTES + 1]).unwrap();
        let error = ServerConfig::load(&path).unwrap_err();
        assert_eq!(error.status, 413);
        assert!(error.kind.ends_with(":config-limit"));
    }
}
