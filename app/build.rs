fn main() {
    // Generates the Tauri context (window/asset metadata) at compile time. The
    // GUI itself is behind the default `gui` feature; this keeps the config in
    // sync for both GUI and headless builds. Headless builds must skip it:
    // tauri-build panics ("missing cargo:dev") when the tauri CLI environment
    // is absent, which broke the headless deb release job.
    if std::env::var_os("CARGO_FEATURE_GUI").is_some() {
        tauri_build::build();
    } else {
        // Nothing Tauri-related to generate; keep rerun behavior stable.
        println!("cargo:rerun-if-changed=build.rs");
    }
}
