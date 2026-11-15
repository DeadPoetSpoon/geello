use std::{env, fs, path::PathBuf};
use sha2::{Digest, Sha256};

fn main() {
    println!("cargo:rerun-if-env-changed=GEELLO_NATIVE_ROOT");
    let root = PathBuf::from(env::var_os("GEELLO_NATIVE_ROOT").expect(
        "native-proj requires GEELLO_NATIVE_ROOT: run tools/build_native_proj.py; no bundled/system fallback is permitted",
    )).canonicalize().expect("GEELLO_NATIVE_ROOT must exist");
    let manifest = root.join("geello-native.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let bytes = fs::read(&manifest).expect("missing approved native distribution manifest");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("invalid native manifest");
    for (name, version) in [("proj", "9.6.2"), ("sqlite", "3.54.1"), ("tiff", "4.7.2"), ("zlib", "1.4")] {
        assert_eq!(value["versions"][name].as_str(), Some(version), "native version mismatch: {name}");
    }
    assert_eq!(value["target"].as_str(), Some(env::var("TARGET").unwrap().as_str()), "native target mismatch");
    for (relative, digest) in value["files"].as_object().expect("missing native file inventory") {
        let path = root.join(relative);
        assert!(path.canonicalize().expect("missing native file").starts_with(&root), "native path escapes root");
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(path).expect("unreadable native file");
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), digest.as_str().unwrap(), "native file changed: {relative}");
    }
    assert!(value["files"]["share/proj/proj.db"].is_string(), "missing proj.db identity");
    println!("cargo:rustc-link-search=native={}", root.join("lib").display());
    println!("cargo:rustc-link-lib=dylib={}", value["link_name"].as_str().expect("missing import library name"));
    println!("cargo:rustc-env=GEELLO_NATIVE_MANIFEST={}", manifest.display());
    println!("cargo:rustc-env=GEELLO_NATIVE_ID={}", blake3::hash(&bytes).to_hex());
}
