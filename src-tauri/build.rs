use serde_json::{json, Value};
use std::env;

const DEV_ICON_PATHS: [&str; 5] = [
    "icons-dev/32x32.png",
    "icons-dev/128x128.png",
    "icons-dev/128x128@2x.png",
    "icons-dev/icon.icns",
    "icons-dev/icon.ico",
];

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

    let bundle = config
        .entry("bundle")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("TAURI_CONFIG bundle must be a JSON object");
    bundle.insert("icon".into(), json!(DEV_ICON_PATHS));
    for icon in DEV_ICON_PATHS {
        println!("cargo:rerun-if-changed={icon}");
    }

    let config = serde_json::to_string(config).expect("could not encode the Tau dev configuration");
    env::set_var("TAURI_CONFIG", &config);
    println!("cargo:rustc-env=TAURI_CONFIG={config}");
}
