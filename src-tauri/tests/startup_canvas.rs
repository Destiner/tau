use tauri::utils::config::{Color, Config};

#[test]
fn startup_canvas_covers_the_native_window_and_webview() {
    let config: Config = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    // Tauri's build checks keep the Cargo feature synchronized with this flag.
    // Without it, WKWebView retains its white backing until the first paint.
    assert!(config.app.macos_private_api);
    let window = &config.app.windows[0];
    assert_eq!(window.background_color, Some(Color(16, 20, 28, 255)));
    assert!(!window.visible);
}
