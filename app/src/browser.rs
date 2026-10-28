//! Open the local UI in the platform default browser. Compiled without the
//! gui feature too, so the headless .deb build (the package's desktop entry
//! runs the server with `--browser`) launches the interface the same way.

pub(crate) fn open(port: u16) {
    open_url(&format!("http://127.0.0.1:{port}"));
}

/// Open any local URL (the UI, or the router dashboard) via the platform
/// default handler. The web UI cannot `window.open` reliably from a desktop
/// window without a popup path, so buttons ask the server to open for them.
pub(crate) fn open_url(url: &str) {
    #[cfg(target_os = "linux")]
    let program = ("xdg-open", vec![url.to_string()]);
    #[cfg(target_os = "macos")]
    let program = ("open", vec![url.to_string()]);
    #[cfg(target_os = "windows")]
    let program = (
        "cmd",
        vec!["/c".to_string(), "start".to_string(), url.to_string()],
    );

    match std::process::Command::new(program.0)
        .args(program.1)
        .spawn()
    {
        Ok(_) => println!("Opened {url}."),
        Err(error) => println!("Could not open a browser ({error}). Visit {url} manually."),
    }
}
