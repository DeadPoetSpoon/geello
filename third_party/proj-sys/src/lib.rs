#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
//! Fixed dynamic PROJ ABI. No automatic source-build or system fallback.
include!("bundled_bindings.rs");
pub const GEELLO_NATIVE_MANIFEST: &str = env!("GEELLO_NATIVE_MANIFEST");
pub const GEELLO_NATIVE_ID: &str = env!("GEELLO_NATIVE_ID");
