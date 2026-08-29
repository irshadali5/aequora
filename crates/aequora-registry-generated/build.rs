use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../../registry");
    let crate_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let root = crate_dir.join("../..");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default()).join("registry.rs");
    let set = aequora_registry_codegen::load_registry(&root)
        .unwrap_or_else(|error| panic!("canonical registry validation failed: {error}"));
    let lock = aequora_registry_codegen::load_lock(&root)
        .unwrap_or_else(|error| panic!("registry lock load failed: {error}"));
    aequora_registry_codegen::verify_lock(&set, &lock)
        .unwrap_or_else(|error| panic!("registry lock verification failed: {error}"));
    fs::write(out, aequora_registry_codegen::generated_rust(&set))
        .unwrap_or_else(|error| panic!("generated registry write failed: {error}"));
}
