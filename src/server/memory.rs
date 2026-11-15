//! Conservative admission for persistent metadata and its parser/serializer
//! workspace. These envelopes bound application allocations, not process RSS.
use super::*;
use crate::{GeelloError, OperationContext};
use serde::Serialize;
use std::{
    io::{Read, Write},
    path::Path,
};

pub(crate) fn model_bytes(value: &impl Serialize) -> Result<usize> {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("metadata size overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(ServerError::io)?;
    envelope(counter.0)
}

pub(crate) fn envelope(bytes: usize) -> Result<usize> {
    bytes
        .checked_mul(32)
        .and_then(|v| v.checked_add(1024))
        .ok_or_else(|| GeelloError::SizeOverflow.into())
}

/// Pin the admitted length to the actual read, so a growing external file
/// cannot consume the caller's whole file ceiling under a smaller reservation.
pub(crate) fn read_file(
    path: &Path,
    max: usize,
    ctx: &OperationContext,
) -> Result<crate::Accounted<Vec<u8>>> {
    let file = std::fs::File::open(path).map_err(ServerError::io)?;
    let length = file.metadata().map_err(ServerError::io)?.len();
    if length > max as u64 {
        return Err(ServerError::new(
            413,
            "source-limit",
            "Source exceeds profile",
        ));
    }
    let length = usize::try_from(length).map_err(|_| GeelloError::SizeOverflow)?;
    let capacity = length.checked_add(1).ok_or(GeelloError::SizeOverflow)?;
    let hold = ctx.reserve(envelope(capacity)?)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(capacity as u64)
        .read_to_end(&mut bytes)
        .map_err(ServerError::io)?;
    ctx.check()?;
    if bytes.len() != length {
        return Err(GeelloError::SourceChanged.into());
    }
    Ok(crate::Accounted::new(bytes, vec![hold]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OperationLimits;

    #[test]
    fn file_read_admits_before_allocation_and_releases_on_every_exit() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("definition.toml");
        std::fs::write(&path, vec![b'a'; 8192]).unwrap();
        let limits = OperationLimits {
            max_live_bytes: 1 << 20,
            max_work: 100,
            max_commands: 100,
            max_diagnostics: 10,
        };
        let ctx = OperationContext::new(limits, None);
        assert_eq!(read_file(&path, 8191, &ctx).unwrap_err().status, 413);
        assert_eq!(ctx.live_bytes(), 0);
        let small = OperationContext::new(
            OperationLimits {
                max_live_bytes: 8192,
                ..limits
            },
            None,
        );
        assert_eq!(read_file(&path, 8192, &small).unwrap_err().status, 503);
        assert_eq!(small.live_bytes(), 0);
        let bytes = read_file(&path, 8192, &ctx).unwrap();
        assert_eq!(bytes.len(), 8192);
        assert!(ctx.live_bytes() > bytes.len() as u64);
        drop(bytes);
        assert_eq!(ctx.live_bytes(), 0);
        ctx.cancel();
        assert!(read_file(&path, 8192, &ctx).is_err());
        assert_eq!(ctx.live_bytes(), 0);
    }
}
