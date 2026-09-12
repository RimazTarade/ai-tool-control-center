fn main() {
    tauri_build::build();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is required for Windows resources");
        println!("cargo:rustc-link-search=native={out_dir}");
    }
}
