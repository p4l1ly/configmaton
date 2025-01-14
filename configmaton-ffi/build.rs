fn main() {
    // Tell Cargo to rerun if any source file changes
    println!("cargo:rerun-if-changed=src/");
    // Rerun if build.rs itself changes
    println!("cargo:rerun-if-changed=build.rs");

    {
        let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let mut config = cbindgen::Config::default();
        config.language = cbindgen::Language::C;
        cbindgen::Builder::new()
            .with_config(config)
            .with_crate(crate_dir)
            .generate()
            .expect("Unable to generate bindings")
            .write_to_file("../target/include/configmaton.h");
    }
}
