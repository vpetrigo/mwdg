fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    println!("cargo::rerun-if-changed=cbindgen.toml");
    std::fs::create_dir_all(format!("{crate_dir}/include")).ok();

    let config = cbindgen::Config::from_file("cbindgen.toml").unwrap_or_default();

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("Unable to generate C bindings")
        .write_to_file(format!("{crate_dir}/include/mwdg.h"));
}
