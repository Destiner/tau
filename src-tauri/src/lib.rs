mod models;
mod pi;
mod profile;
mod ssh;
mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(pi::PiState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            pi::send_pi,
            pi::start_pi,
            pi::start_pi_remote,
            pi::start_pi_sdk,
            pi::stop_pi,
            ssh::list_remote_directories,
            ssh::probe_remote_project,
            storage::archive_session,
            storage::import_project,
            storage::import_remote_project,
            storage::load_workspace,
            storage::register_session,
            storage::remove_project,
            storage::set_active_project,
            storage::set_active_session,
            storage::set_project_collapsed,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
