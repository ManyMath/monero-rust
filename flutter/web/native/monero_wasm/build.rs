fn main() {
    // Only generate C headers on native targets (not WASM).
    // build.rs runs on the host, so we check the TARGET env var.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("wasm") {
        return;
    }

    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::path::PathBuf::from(&crate_dir)
        .join("target")
        .join("bridge");
    std::fs::create_dir_all(&out_dir).ok();

    let config = cbindgen::Config::from_file(
        std::path::Path::new(&crate_dir).join("cbindgen.toml"),
    )
    .unwrap_or_default();

    if let Ok(bindings) = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
    {
        bindings.write_to_file(out_dir.join("bindings.h"));
    }
}
