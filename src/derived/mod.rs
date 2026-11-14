//! Immutable layer-two derived storage. This module has no server registry,
//! HTTP, runtime or response-cache dependency.
mod raster;
mod vector;
pub use raster::*;
pub use vector::*;

use crate::{GeelloError, OperationContext};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

/// Explicit limits are experimental caller profiles, never deployment defaults.
#[derive(Debug, Clone, Copy)]
pub struct StoreLimits {
    pub max_disk_bytes: u64,
    pub max_manifest_bytes: usize,
    pub max_shard_features: usize,
    pub max_record_bytes: usize,
    pub max_block_bytes: usize,
}
impl StoreLimits {
    pub fn validate(&self) -> Result<(), GeelloError> {
        if self.max_disk_bytes == 0
            || self.max_manifest_bytes == 0
            || self.max_shard_features == 0
            || self.max_record_bytes == 0
            || self.max_block_bytes == 0
        {
            return Err(error("positive derived-store limits required"));
        }
        Ok(())
    }
}
/// Checksummed relative member of an immutable derived version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredFile {
    pub name: String,
    pub bytes: u64,
    pub hash: String,
}
pub(crate) fn error(message: impl ToString) -> GeelloError {
    GeelloError::InvalidSource(format!("derived storage: {}", message.to_string()))
}
pub(crate) fn write_file(
    root: &Path,
    name: &str,
    bytes: &[u8],
    used: &mut u64,
    limit: u64,
) -> Result<StoredFile, GeelloError> {
    let next = used
        .checked_add(bytes.len() as u64)
        .ok_or(GeelloError::SizeOverflow)?;
    if next > limit {
        return Err(GeelloError::BudgetExceeded("derived disk"));
    }
    let mut f = File::options()
        .create_new(true)
        .write(true)
        .open(root.join(name))
        .map_err(error)?;
    f.write_all(bytes).map_err(error)?;
    f.sync_all().map_err(error)?;
    *used = next;
    Ok(StoredFile {
        name: name.into(),
        bytes: bytes.len() as u64,
        hash: blake3::hash(bytes).to_hex().to_string(),
    })
}
pub(crate) fn read_file(
    root: &Path,
    file: &StoredFile,
    limit: usize,
    ctx: &OperationContext,
) -> Result<crate::Accounted<Vec<u8>>, GeelloError> {
    if file.name.contains(['/', '\\', ':'])
        || file.name.starts_with('.')
        || file.bytes > limit as u64
    {
        return Err(error("unsafe or oversized member"));
    }
    let _memory = ctx.reserve(
        (file.bytes as usize)
            .checked_mul(2)
            .ok_or(GeelloError::SizeOverflow)?,
    )?;
    let mut f = File::open(root.join(&file.name)).map_err(error)?;
    if f.metadata().map_err(error)?.len() != file.bytes {
        return Err(error("member size mismatch"));
    }
    let mut bytes = Vec::with_capacity(file.bytes as usize);
    Read::by_ref(&mut f)
        .take(file.bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    ctx.checkpoint(bytes.len() as u64)?;
    if bytes.len() as u64 != file.bytes || blake3::hash(&bytes).to_hex().as_str() != file.hash {
        return Err(error("member checksum mismatch"));
    }
    Ok(crate::Accounted::new(bytes, vec![_memory]))
}
pub(crate) fn manifest<T: Serialize>(
    root: &Path,
    value: &T,
    used: &mut u64,
    limits: StoreLimits,
) -> Result<String, GeelloError> {
    let bytes = serde_json::to_vec(value).map_err(error)?;
    if bytes.len() > limits.max_manifest_bytes {
        return Err(GeelloError::BudgetExceeded("derived manifest"));
    }
    let hash = blake3::hash(&bytes).to_hex().to_string();
    write_file(root, "manifest.json", &bytes, used, limits.max_disk_bytes)?;
    Ok(hash)
}
pub(crate) fn load_manifest<T: for<'de> Deserialize<'de>>(
    root: &Path,
    max: usize,
) -> Result<T, GeelloError> {
    let f = File::open(root.join("manifest.json")).map_err(error)?;
    if f.metadata().map_err(error)?.len() > max as u64 {
        return Err(GeelloError::BudgetExceeded("derived manifest"));
    }
    serde_json::from_reader(f.take(max as u64 + 1)).map_err(error)
}
/// A failed build owns only its temporary directory. No current version is touched.
pub(crate) fn stage(parent: &Path) -> Result<tempfile::TempDir, GeelloError> {
    fs::create_dir_all(parent).map_err(error)?;
    tempfile::Builder::new()
        .prefix("preparing-")
        .tempdir_in(parent)
        .map_err(error)
}
