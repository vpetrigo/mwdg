fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let include_dir = format!("{out_dir}/include");
    let cbindgen_config = format!("{crate_dir}/cbindgen.toml");

    println!("cargo::rerun-if-changed=cbindgen.toml");
    std::fs::create_dir_all(&include_dir).ok();

    let config = cbindgen::Config::from_file(&cbindgen_config).unwrap_or_default();

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file(format!("{include_dir}/mwdg.h"));
}
