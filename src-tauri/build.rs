use serde_json::{json, Value};
use std::env;

fn main() {
    if tauri_build::is_dev() {
        configure_dev_profile();
    }
    tauri_build::build()
}

fn configure_dev_profile() {
    let mut config = env::var("TAURI_CONFIG")
        .ok()
        .map(|value| serde_json::from_str::<Value>(&value).expect("invalid TAURI_CONFIG"))
        .unwrap_or_else(|| json!({}));
    let config = config
        .as_object_mut()
        .expect("TAURI_CONFIG must be a JSON object");
    config.insert("productName".into(), json!("Tau Dev"));
    config.insert("identifier".into(), json!("com.destiner.tau.dev"));

    let config = serde_json::to_string(config).expect("could not encode the Tau dev configuration");
    env::set_var("TAURI_CONFIG", &config);
    println!("cargo:rustc-env=TAURI_CONFIG={config}");
}
