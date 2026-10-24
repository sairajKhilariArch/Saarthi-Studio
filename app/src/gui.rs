//! Native application window for the desktop app.
//!
//! The window is a thin shell around the local UI server: it loads
//! `http://127.0.0.1:<port>` in the system webview (WebKitGTK on Linux,
//! WebView2 on Windows), so the process stays small and light — no bundled
//! Chromium. Closing the window runs the same shutdown path as Ctrl+C: the
//! local server stops and any router this app started is taken down.

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

pub(crate) fn run(
    port: u16,
    shutdown: tokio::sync::watch::Sender<bool>,
    title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("http://127.0.0.1:{port}");
    let target = WebviewUrl::External(
        url.parse::<tauri::Url>()
            .map_err(|_| format!("invalid window URL: {url}"))?,
    );
    let title = title.to_string();

    let app = tauri::Builder::default()
        .setup(move |handle| {
            WebviewWindowBuilder::new(handle, "main", target)
                .title(title)
                .inner_size(1280.0, 840.0)
                .min_inner_size(920.0, 620.0)
                .center()
                .resizable(true)
                .build()?;
            handle.manage(shutdown.clone());
            Ok(())
        })
        .build(tauri::generate_context!())?;

    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            // Let the HTTP server and router cleanup wind down; `main` joins
            // the server right after this loop returns.
            let sender = handle.state::<tokio::sync::watch::Sender<bool>>();
            let _ = sender.send(true);
        }
    });
    Ok(())
}

/// Open the local UI in the platform default browser. Used by `--browser`.
pub(crate) fn open_in_browser(port: u16) {
    let url = format!("http://127.0.0.1:{port}");
    #[cfg(target_os = "linux")]
    let program = ("xdg-open", vec![url.clone()]);
    #[cfg(target_os = "macos")]
    let program = ("open", vec![url.clone()]);
    #[cfg(target_os = "windows")]
    let program = ("cmd", vec!["/c".to_string(), "start".to_string(), url.clone()]);

    match std::process::Command::new(program.0).args(program.1).spawn() {
        Ok(_) => println!("Opened {url} in your browser."),
        Err(error) => println!("Could not open a browser ({error}). Visit {url} manually."),
    }
}
