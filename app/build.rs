fn main() {
    // Generates the Tauri context (window/asset metadata) at compile time. The
    // GUI itself is behind the default `gui` feature; this keeps the config in
    // sync for both GUI and headless builds.
    tauri_build::build()
}
