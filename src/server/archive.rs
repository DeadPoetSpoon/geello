//! Restricted Shapefile packages. No member path is accepted as a destination:
//! one flat, consistently named sidecar set is the complete package contract.
use super::*;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub(crate) fn extract(
    path: &Path,
    destination: &Path,
    p: &ServerProfile,
    ctx: &crate::OperationContext,
) -> Result<PathBuf> {
    // Central-directory metadata is parsed before individual entries can be
    // counted. Admit a conservative envelope for the already bounded upload.
    let bytes = std::fs::metadata(path).map_err(ServerError::io)?.len();
    if bytes > p.max_upload_bytes as u64 {
        return Err(ServerError::new(
            413,
            "archive-limit",
            "ZIP input exceeds profile",
        ));
    }
    let _archive_memory = ctx.reserve(
        (bytes as usize)
            .checked_mul(16)
            .ok_or(crate::GeelloError::SizeOverflow)?,
    )?;
    let mut zip = zip::ZipArchive::new(File::open(path).map_err(ServerError::io)?)
        .map_err(|_| ServerError::invalid("Invalid ZIP archive"))?;
    if zip.is_empty() || zip.len() > p.max_archive_files {
        return Err(ServerError::invalid("ZIP member count exceeds profile"));
    }
    let mut names = BTreeSet::new();
    let mut expanded = 0u64;
    let mut stem = None;
    let mut primary = None;
    // Preflight every central-directory entry before writing any member.
    for i in 0..zip.len() {
        ctx.checkpoint(1)?;
        let entry = zip
            .by_index(i)
            .map_err(|_| ServerError::invalid("Invalid ZIP member"))?;
        let name = entry
            .name()
            .map_err(|_| ServerError::invalid("Invalid ZIP name"))?;
        let lower = name.to_ascii_lowercase();
        if name.is_empty()
            || name.len() > 200
            || name.starts_with('.')
            || name.contains(['/', '\\', ':'])
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            || entry.is_dir()
            || entry.encrypted()
        {
            return Err(ServerError::invalid(
                "ZIP requires flat regular unencrypted Shapefile sidecars",
            ));
        }
        if entry
            .unix_mode()
            .is_some_and(|m| m & 0o170000 != 0 && m & 0o170000 != 0o100000)
        {
            return Err(ServerError::invalid(
                "ZIP links and special files are forbidden",
            ));
        }
        if !matches!(
            entry.compression(),
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
        ) {
            return Err(ServerError::invalid("ZIP compression is not allowed"));
        }
        let (base, extension) = lower
            .rsplit_once('.')
            .ok_or_else(|| ServerError::invalid("ZIP member has no extension"))?;
        if !matches!(extension, "shp" | "shx" | "dbf" | "prj" | "cpg")
            || !names.insert(lower.clone())
        {
            return Err(ServerError::invalid(
                "Unknown, nested or duplicate ZIP member",
            ));
        }
        if stem.as_ref().is_some_and(|s| s != base) {
            return Err(ServerError::invalid(
                "ZIP must contain exactly one Shapefile sidecar set",
            ));
        }
        stem = Some(base.to_owned());
        if extension == "shp" {
            primary = Some(destination.join(name.as_ref()));
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or_else(|| ServerError::invalid("ZIP size overflow"))?;
        if entry.size() > p.max_member_bytes as u64
            || expanded > p.max_expanded_bytes as u64
            || entry.size()
                > entry
                    .compressed_size()
                    .max(1)
                    .saturating_mul(p.max_compression_ratio)
        {
            return Err(ServerError::new(
                413,
                "archive-limit",
                "ZIP expansion exceeds profile",
            ));
        }
    }
    let base = stem.unwrap();
    for ext in ["shp", "shx", "dbf"] {
        if !names.contains(&format!("{base}.{ext}")) {
            return Err(ServerError::invalid("ZIP requires SHP, SHX and DBF"));
        }
    }
    // Normalize all destinations to lowercase so sidecar lookup is identical on
    // case-sensitive Linux and Windows. Unsafe aliases were rejected above.
    std::fs::create_dir(destination).map_err(ServerError::io)?;
    let mut actual = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|_| ServerError::invalid("Invalid ZIP member"))?;
        let name = entry
            .name()
            .map_err(|_| ServerError::invalid("Invalid ZIP name"))?
            .to_ascii_lowercase();
        let mut out = File::options()
            .create_new(true)
            .write(true)
            .open(destination.join(&name))
            .map_err(ServerError::io)?;
        let mut bytes = 0usize;
        let mut chunk = [0u8; 65536];
        loop {
            ctx.checkpoint(1)?;
            let n = entry
                .read(&mut chunk)
                .map_err(|_| ServerError::invalid("ZIP decompression or CRC check failed"))?;
            if n == 0 {
                break;
            }
            bytes = bytes
                .checked_add(n)
                .ok_or_else(|| ServerError::invalid("ZIP size overflow"))?;
            actual = actual
                .checked_add(n)
                .ok_or_else(|| ServerError::invalid("ZIP size overflow"))?;
            if bytes > p.max_member_bytes
                || actual > p.max_expanded_bytes
                || bytes as u64 > entry.size()
            {
                return Err(ServerError::new(
                    413,
                    "archive-limit",
                    "ZIP actual expansion exceeds profile",
                ));
            }
            out.write_all(&chunk[..n]).map_err(ServerError::io)?;
        }
        if bytes as u64 != entry.size() {
            return Err(ServerError::invalid("ZIP size mismatch"));
        }
        out.sync_all().map_err(ServerError::io)?;
    }
    let _ = primary;
    Ok(destination.join(format!("{base}.shp")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_encrypted_and_crc_damaged_archives_are_rejected() {
        let config = ServerConfig::load(Path::new("examples/server.experimental.toml")).unwrap();
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        for name in ["roads.shp", "roads.shx", "roads.dbf"] {
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(b"crc-test-payload").unwrap();
        }
        let valid = writer.finish().unwrap().into_inner();
        let mut damaged = valid.clone();
        let payload = damaged
            .windows(16)
            .position(|v| v == b"crc-test-payload")
            .unwrap();
        damaged[payload] ^= 1;
        let mut encrypted = valid.clone();
        // Set the traditional encryption flag in both local and central
        // headers; no encryption support is compiled into the upload reader.
        for (signature, flag_offset) in [(b"PK\x03\x04", 6), (b"PK\x01\x02", 8)] {
            for i in 0..encrypted.len() - flag_offset {
                if encrypted.get(i..i + 4) == Some(signature.as_slice()) {
                    encrypted[i + flag_offset] |= 1;
                }
            }
        }
        for bytes in [
            &valid[..valid.len() - 12],
            damaged.as_slice(),
            encrypted.as_slice(),
        ] {
            let root = tempfile::tempdir().unwrap();
            let input = root.path().join("input.zip");
            let output = root.path().join("unpacked");
            std::fs::write(&input, bytes).unwrap();
            assert!(extract(&input, &output, &config.profile, &config.profile.context()).is_err());
        }
    }
}
