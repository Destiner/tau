// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = tau_lib::update_swap_helper_exit_code() {
        std::process::exit(code);
    }
    tau_lib::run()
}
