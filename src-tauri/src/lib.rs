mod admin;
#[cfg(any(dev, test))]
mod dev_workspace;
mod feedback;
mod file_preview;
mod models;
mod pi;
mod profile;
mod quit;
mod settings;
mod ssh;
mod storage;
mod telemetry;
mod update;

use std::time::Duration;

use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::window::Color;
use tauri::{AppHandle, Emitter, Manager, Runtime, Theme, WebviewWindow, WindowEvent};
#[cfg(not(dev))]
use tauri_plugin_window_state::StateFlags;

const NEW_SESSION_MENU_ID: &str = "tau-new-session";
const NEW_SESSION_EVENT: &str = "tau://new-session";

/// The window paints its own background before the webview has anything to show
/// and in the gaps a live resize opens up, so it has to carry the canvas colour
/// of the theme it is being drawn in.
const LIGHT_CANVAS: Color = Color(252, 252, 252, 255);
const DARK_CANVAS: Color = Color(16, 20, 28, 255);

pub fn update_swap_helper_exit_code() -> Option<i32> {
    update::swap_helper_exit_code()
}

/// How long the window stays hidden waiting for the frontend to show it.
const REVEAL_TIMEOUT: Duration = Duration::from_secs(3);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(error) = profile::initialize() {
        eprintln!("{error}");
        profile::cleanup();
        return;
    }
    let telemetry = telemetry::Telemetry::init();
    telemetry.install_panic_hook();
    telemetry.record_app_started();

    let builder = tauri::Builder::default()
        .manage(pi::PiState::default())
        .manage(file_preview::FilePreviewState::default())
        .manage(quit::QuitState::default())
        .manage(update::UpdateState::default())
        .manage(telemetry)
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(update::plugin());
    // Development windows start fresh; the plugin otherwise writes geometry
    // to one shared app-config file, independently of Tau's workspace store.
    #[cfg(not(dev))]
    let builder = builder.plugin(
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
    );
    #[allow(unused_mut)]
    let mut context = tauri::generate_context!();
    #[cfg(dev)]
    for window in &mut context.config_mut().app.windows {
        window.incognito = true;
    }
    let app = builder
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if event.id() == NEW_SESSION_MENU_ID {
                let _ = app.emit(NEW_SESSION_EVENT, ());
            } else if event.id() == update::CHECK_FOR_UPDATES_MENU_ID {
                let _ = app.emit(update::CHECK_FOR_UPDATES_EVENT, ());
            } else if event.id() == quit::QUIT_MENU_ID {
                quit::request_quit(app);
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::ThemeChanged(theme) = event {
                if let Some(webview) = window.get_webview_window(window.label()) {
                    let _ = webview.set_background_color(Some(canvas_color(*theme)));
                }
            }
        })
        .setup(|app| {
            // Match both native surfaces to the system theme before revealing
            // the window; the config can only provide a single fallback colour.
            if let Some(window) = app.get_webview_window("main") {
                let theme = window.theme().unwrap_or(Theme::Light);
                let _ = window.set_background_color(Some(canvas_color(theme)));
                reveal_window_eventually(window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            admin::read_admin_mode,
            admin::set_admin_mode,
            admin::get_dismissed_update_version,
            admin::set_dismissed_update_version,
            feedback::submit_issue_report,
            file_preview::prepare_file_preview,
            file_preview::release_file_preview,
            pi::claim_pi_frontend,
            pi::read_pi_frontend_revision,
            pi::send_pi,
            pi::start_pi,
            pi::start_pi_remote,
            pi::stop_pi,
            quit::pending_quit_request,
            quit::resolve_quit_request,
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
            storage::unarchive_session,
            telemetry::ingest::ingest_telemetry,
            update::update_snapshot,
            update::check_for_update,
            update::download_update,
            update::request_update_restart,
            update::install_update,
            update::restart_after_update,
        ])
        .build(context)
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // `App::run` calls `std::process::exit` once this closure returns
        // control after `RunEvent::Exit`, which skips `Drop`. The clean-exit
        // marker must therefore be recorded and flushed here, not relied on
        // to happen implicitly when the managed `Telemetry` goes out of scope.
        if let tauri::RunEvent::Exit = event {
            file_preview::cleanup(app_handle);
            app_handle.state::<pi::PiState>().shutdown();
            let telemetry = app_handle.state::<telemetry::Telemetry>();
            telemetry.record_app_exited();
            telemetry.shutdown();
            profile::cleanup();
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
                    &MenuItem::with_id(
                        app,
                        update::CHECK_FOR_UPDATES_MENU_ID,
                        "Check for Updates…",
                        true,
                        None::<&str>,
                    )?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &MenuItem::with_id(
                        app,
                        quit::QUIT_MENU_ID,
                        format!("Quit {}", package_info.name),
                        true,
                        Some("CmdOrCtrl+Q"),
                    )?,
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
