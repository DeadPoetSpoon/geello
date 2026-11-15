//! Explicit fixed-version offline native projection. No backend substitution.
use crate::{CoordinateSpace, Crs, GeelloError};
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, CString},
    path::PathBuf,
    sync::OnceLock,
};

fn error(message: impl ToString) -> GeelloError {
    GeelloError::Projection(message.to_string())
}

/// Verify the exact distribution again at runtime before opening any operation.
/// Deploy the distribution at the build-time path; changing it requires a rebuild.
pub fn distribution_identity() -> Result<&'static str, GeelloError> {
    static VERIFIED: OnceLock<Result<(), String>> = OnceLock::new();
    VERIFIED
        .get_or_init(|| {
            let manifest = PathBuf::from(proj_sys::GEELLO_NATIVE_MANIFEST);
            let root = manifest.parent().ok_or("invalid native root")?;
            let bytes = std::fs::read(&manifest).map_err(|e| e.to_string())?;
            if blake3::hash(&bytes).to_hex().as_str() != proj_sys::GEELLO_NATIVE_ID {
                return Err("native distribution manifest changed".into());
            }
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            for (name, digest) in value["files"]
                .as_object()
                .ok_or("missing native inventory")?
            {
                let bytes = std::fs::read(root.join(name)).map_err(|e| e.to_string())?;
                if format!("{:x}", Sha256::digest(bytes))
                    != digest.as_str().ok_or("invalid digest")?
                {
                    return Err(format!("native file changed: {name}"));
                }
            }
            Ok(())
        })
        .as_ref()
        .map_err(error)?;
    Ok(proj_sys::GEELLO_NATIVE_ID)
}

pub struct NativeProjection {
    source: Crs,
    target: Crs,
    operation: proj::Proj,
}

impl NativeProjection {
    pub fn new(source: Crs, target: Crs) -> Result<Self, GeelloError> {
        distribution_identity()?;
        let root = PathBuf::from(proj_sys::GEELLO_NATIVE_MANIFEST)
            .parent()
            .unwrap()
            .join("share/proj");
        let mut builder = proj::ProjBuilder::new();
        builder.set_search_paths(&root).map_err(error)?;
        // Read numeric ABI version only: PROJ's optional searchpath string can
        // contain platform-encoded user directories and is irrelevant here.
        let info = unsafe { proj_sys::proj_info() };
        if (info.major, info.minor, info.patch) != (9, 6, 2) || builder.network_enabled() {
            return Err(error(
                "native PROJ must be exactly 9.6.2 with network disabled (PROJ_NETWORK=OFF)",
            ));
        }
        // The wrapper's ordinary CRS constructor permits ballpark operations.
        // Use its ABI binding solely to select a strict operation, normalize XY,
        // and serialize it; the safe wrapper owns and executes the final pipeline.
        let pipeline = strict_pipeline(&source, &target, &root)?;
        let operation = builder.proj(&pipeline).map_err(error)?;
        Ok(Self {
            source,
            target,
            operation,
        })
    }
    pub fn source(&self) -> &Crs {
        &self.source
    }
    pub fn target(&self) -> &Crs {
        &self.target
    }
    pub fn transform_xy(&self, xy: [f64; 2]) -> Result<[f64; 2], GeelloError> {
        if xy.iter().any(|v| !v.is_finite())
            || (self.source.space() == CoordinateSpace::GeographicXy
                && (xy[0].abs() > 180. || xy[1].abs() > 90.))
        {
            return Err(error("coordinate outside projection domain"));
        }
        let (x, y) = self.operation.convert((xy[0], xy[1])).map_err(error)?;
        if !x.is_finite() || !y.is_finite() {
            return Err(error("nonfinite native result"));
        }
        Ok([x, y])
    }
}

fn strict_pipeline(
    source: &Crs,
    target: &Crs,
    directory: &std::path::Path,
) -> Result<String, GeelloError> {
    use proj_sys::*;
    let source_text = CString::new(source.definition()).map_err(error)?;
    let target_text = CString::new(target.definition()).map_err(error)?;
    let path = CString::new(
        directory
            .to_str()
            .ok_or_else(|| error("native path must be UTF-8"))?,
    )
    .map_err(error)?;
    let options = [
        c"ALLOW_BALLPARK=NO".as_ptr(),
        c"ONLY_BEST=YES".as_ptr(),
        std::ptr::null(),
    ];
    // Every non-null PJ and context is destroyed before returning. Pointers and
    // CStrings live throughout the synchronous calls; the result is copied.
    unsafe {
        let ctx = proj_context_create();
        if ctx.is_null() {
            return Err(error("native context allocation failed"));
        }
        proj_context_set_enable_network(ctx, 0);
        proj_context_set_search_paths(ctx, 1, &path.as_ptr());
        let from = proj_create(ctx, source_text.as_ptr());
        let to = proj_create(ctx, target_text.as_ptr());
        let mut valid = true;
        for (object, crs) in [(from, source), (to, target)] {
            if object.is_null() {
                valid = false;
                continue;
            }
            let cs = proj_crs_get_coordinate_system(ctx, object);
            let expected = match crs.space() {
                CoordinateSpace::GeographicXy => PJ_TYPE_PJ_TYPE_GEOGRAPHIC_2D_CRS,
                CoordinateSpace::Planar => PJ_TYPE_PJ_TYPE_PROJECTED_CRS,
            };
            valid &= !cs.is_null()
                && proj_cs_get_axis_count(ctx, cs) == 2
                && proj_get_type(object) == expected;
            if !cs.is_null() {
                proj_destroy(cs);
            }
        }
        let operation = if !valid {
            std::ptr::null_mut()
        } else {
            proj_create_crs_to_crs_from_pj(ctx, from, to, std::ptr::null_mut(), options.as_ptr())
        };
        let normalized = if operation.is_null() {
            std::ptr::null_mut()
        } else {
            proj_normalize_for_visualization(ctx, operation)
        };
        let text = if normalized.is_null() {
            std::ptr::null()
        } else {
            proj_as_proj_string(
                ctx,
                normalized,
                PJ_PROJ_STRING_TYPE_PJ_PROJ_5,
                std::ptr::null(),
            )
        };
        let result = if text.is_null() {
            Err(error(
                "strict native operation unavailable; check CRS and offline grids",
            ))
        } else {
            CStr::from_ptr(text)
                .to_str()
                .map(str::to_owned)
                .map_err(error)
        };
        for ptr in [normalized, operation, to, from] {
            if !ptr.is_null() {
                proj_destroy(ptr);
            }
        }
        proj_context_destroy(ctx);
        result
    }
}
