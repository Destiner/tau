mod models;
mod pi;
mod profile;
mod settings;
mod ssh;
mod storage;
mod telemetry;

use std::time::Duration;

use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, Runtime, Theme, WebviewWindow, WindowEvent};
use tauri_plugin_window_state::StateFlags;

const NEW_SESSION_MENU_ID: &str = "tau-new-session";
const NEW_SESSION_EVENT: &str = "tau://new-session";

/// The window paints its own background before the webview has anything to show
/// and in the gaps a live resize opens up, so it has to carry the canvas colour
/// of the theme it is being drawn in.
const LIGHT_CANVAS: Color = Color(252, 252, 252, 255);
const DARK_CANVAS: Color = Color(16, 20, 28, 255);

/// How long the window stays hidden waiting for the frontend to show it.
const REVEAL_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let telemetry = telemetry::Telemetry::init();
    telemetry.record_app_started();

    let app = tauri::Builder::default()
        .manage(pi::PiState::default())
        .manage(telemetry)
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // The window is created hidden and shown once the frontend has
                // mounted. Restoring visibility would show it while it is empty,
                // and restoring decorations would undo the overlay title bar.
                .with_state_flags(
                    StateFlags::SIZE
                        | StateFlags::POSITION
                        | StateFlags::MAXIMIZED
                        | StateFlags::FULLSCREEN,
                )
                .build(),
        )
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if event.id() == NEW_SESSION_MENU_ID {
                let _ = app.emit(NEW_SESSION_EVENT, ());
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::ThemeChanged(theme) = event {
                let _ = window.set_background_color(Some(canvas_color(*theme)));
            }
        })
        .setup(|app| {
            // The colour in the config is a single value, so the window opens
            // holding the light one whichever theme it is about to be drawn in.
            if let Some(window) = app.get_webview_window("main") {
                let theme = window.theme().unwrap_or(Theme::Light);
                let _ = window.set_background_color(Some(canvas_color(theme)));
                reveal_window_eventually(window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pi::send_pi,
            pi::start_pi,
            pi::start_pi_remote,
            pi::stop_pi,
            settings::read_model_scope,
            settings::read_remote_model_scope,
            ssh::list_remote_directories,
            ssh::probe_remote_project,
            storage::archive_session,
            storage::import_project,
            storage::import_remote_project,
            storage::load_workspace,
            storage::register_session,
            storage::remove_project,
            storage::reorder_projects,
            storage::set_active_project,
            storage::set_active_session,
            storage::set_project_collapsed,
            telemetry::ingest::ingest_telemetry,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // `App::run` calls `std::process::exit` once this closure returns
        // control after `RunEvent::Exit`, which skips `Drop`. The clean-exit
        // marker must therefore be recorded and flushed here, not relied on
        // to happen implicitly when the managed `Telemetry` goes out of scope.
        if let tauri::RunEvent::Exit = event {
            let telemetry = app_handle.state::<telemetry::Telemetry>();
            telemetry.record_app_exited();
            telemetry.shutdown();
        }
    });
}

/// The frontend shows the window once it has something to draw. One that never
/// gets that far would otherwise leave Tau running with no window at all, so the
/// wait is given an end.
fn reveal_window_eventually<R: Runtime>(window: WebviewWindow<R>) {
    std::thread::spawn(move || {
        std::thread::sleep(REVEAL_TIMEOUT);
        if matches!(window.is_visible(), Ok(false)) {
            let _ = window.show();
        }
    });
}

fn canvas_color(theme: Theme) -> Color {
    match theme {
        Theme::Dark => DARK_CANVAS,
        _ => LIGHT_CANVAS,
    }
}

/// Mirrors the menu Tauri builds by default, which is what carries the standard
/// window and text editing shortcuts, and adds Tau's own actions to it. A
/// shortcut that lives only in a key handler works but cannot be discovered.
fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let package_info = app.package_info();
    let config = app.config();
    let about_metadata = AboutMetadata {
        name: Some(package_info.name.clone()),
        version: Some(package_info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config
            .bundle
            .publisher
            .clone()
            .map(|publisher| vec![publisher]),
        ..Default::default()
    };

    Menu::with_items(
        app,
        &[
            #[cfg(target_os = "macos")]
            &Submenu::with_items(
                app,
                package_info.name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app, None, Some(about_metadata))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "File",
                true,
                &[
                    &MenuItem::with_id(
                        app,
                        NEW_SESSION_MENU_ID,
                        "New Session",
                        true,
                        Some("CmdOrCtrl+N"),
                    )?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::close_window(app, None)?,
                    #[cfg(not(target_os = "macos"))]
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "View",
                true,
                &[&PredefinedMenuItem::fullscreen(app, None)?],
            )?,
            &Submenu::with_items(
                app,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::maximize(app, None)?,
                    #[cfg(target_os = "macos")]
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::close_window(app, None)?,
                ],
            )?,
            #[cfg(not(target_os = "macos"))]
            &Submenu::with_items(
                app,
                "Help",
                true,
                &[&PredefinedMenuItem::about(app, None, Some(about_metadata))?],
            )?,
        ],
    )
}
