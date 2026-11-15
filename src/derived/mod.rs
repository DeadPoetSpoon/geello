//! Immutable layer-two derived storage. This module has no server registry,
//! HTTP, runtime or response-cache dependency.
mod fgb;
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
    ctx: &OperationContext,
) -> Result<String, GeelloError> {
    let bytes = bounded_json(value, limits.max_manifest_bytes, ctx)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    write_file(root, "manifest.json", &bytes, used, limits.max_disk_bytes)?;
    Ok(hash)
}
/// Count without allocating output, then admit the exact serialization buffer.
/// The first pass also enforces cancellation and the caller's byte ceiling.
pub(crate) fn bounded_json<T: Serialize>(
    value: &T,
    max: usize,
    ctx: &OperationContext,
) -> Result<crate::Accounted<Vec<u8>>, GeelloError> {
    struct Counter<'a> {
        count: usize,
        max: usize,
        ctx: &'a OperationContext,
        failure: Option<GeelloError>,
    }
    impl Write for Counter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let count = self
                .ctx
                .checkpoint(bytes.len() as u64)
                .and_then(|()| {
                    self.count
                        .checked_add(bytes.len())
                        .ok_or(GeelloError::SizeOverflow)
                })
                .and_then(|n| {
                    if n <= self.max {
                        Ok(n)
                    } else {
                        Err(GeelloError::BudgetExceeded("derived JSON"))
                    }
                });
            match count {
                Ok(n) => self.count = n,
                Err(e) => {
                    self.failure = Some(e);
                    return Err(std::io::Error::other("derived JSON admission failed"));
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter {
        count: 0,
        max,
        ctx,
        failure: None,
    };
    let serialized = serde_json::to_writer(&mut counter, value);
    if let Some(failure) = counter.failure {
        return Err(failure);
    }
    serialized.map_err(error)?;
    let reservation = ctx.reserve(
        counter
            .count
            .checked_mul(2)
            .ok_or(GeelloError::SizeOverflow)?,
    )?;
    let mut bytes = Vec::with_capacity(counter.count);
    serde_json::to_writer(&mut bytes, value).map_err(error)?;
    Ok(crate::Accounted::new(bytes, vec![reservation]))
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
pub(crate) fn metadata_memory(
    root: &Path,
    limits: StoreLimits,
    ctx: &OperationContext,
) -> Result<crate::operation::Reservation, GeelloError> {
    limits.validate()?;
    let bytes = fs::metadata(root.join("manifest.json"))
        .map_err(error)?
        .len();
    if bytes > limits.max_manifest_bytes as u64 {
        return Err(GeelloError::BudgetExceeded("derived manifest"));
    }
    // Conservative envelope for typed JSON metadata, strings, Vec capacity,
    // canonical serialization for identity verification and parser scratch.
    ctx.reserve(
        usize::try_from(bytes)
            .map_err(|_| GeelloError::SizeOverflow)?
            .checked_mul(32)
            .ok_or(GeelloError::SizeOverflow)?,
    )
}
/// A content-address collision may be a damaged previous artifact. Never
/// return it as a successfully prepared version merely because it exists.
/// Repair requires explicit quarantine/rebuild; published members stay immutable.
pub(crate) fn verify_reused_directory(
    staging: &Path,
    destination: &Path,
    ctx: &OperationContext,
) -> Result<(), GeelloError> {
    if !destination.exists() {
        return Ok(());
    }
    let _hold = ctx.reserve(64 << 10)?;
    let mut buffer = vec![0; 64 << 10];
    for entry in fs::read_dir(staging).map_err(error)? {
        let entry = entry.map_err(error)?;
        let expected = entry.path();
        let actual = destination.join(entry.file_name());
        let mut hashes = Vec::with_capacity(2);
        let mut lengths = Vec::with_capacity(2);
        for path in [expected, actual] {
            let metadata = fs::symlink_metadata(&path).map_err(error)?;
            if !metadata.file_type().is_file() {
                return Err(error("invalid reused member"));
            }
            lengths.push(metadata.len());
            if lengths.len() == 2 && lengths[0] != lengths[1] {
                return Err(error("reused member size mismatch"));
            }
            let mut file = File::open(path).map_err(error)?;
            let mut hash = blake3::Hasher::new();
            let mut read = 0u64;
            while read < metadata.len() {
                ctx.checkpoint(1)?;
                let count = file.read(&mut buffer).map_err(error)?;
                if count == 0 {
                    return Err(error("truncated reused member"));
                }
                read = read
                    .checked_add(count as u64)
                    .ok_or(GeelloError::SizeOverflow)?;
                if read > metadata.len() {
                    return Err(error("changed reused member"));
                }
                hash.update(&buffer[..count]);
            }
            hashes.push(hash.finalize());
        }
        if hashes[0] != hashes[1] || lengths[0] != lengths[1] {
            return Err(error(
                "damaged existing derived version; quarantine before rebuilding",
            ));
        }
    }
    Ok(())
}
/// A failed build owns only its temporary directory. No current version is touched.
pub(crate) fn stage(parent: &Path) -> Result<tempfile::TempDir, GeelloError> {
    fs::create_dir_all(parent).map_err(error)?;
    tempfile::Builder::new()
        .prefix("preparing-")
        .tempdir_in(parent)
        .map_err(error)
}
