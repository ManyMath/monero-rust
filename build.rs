fn main() {
    // Regenerate C header via cbindgen at build time.
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let config = cbindgen::Config::from_file(format!("{}/cbindgen.toml", crate_dir))
        .expect("failed to load cbindgen.toml");

    let out = std::path::Path::new(&crate_dir).join("monero-rust.h");

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("unable to generate bindings")
        .write_to_file(out);
}

