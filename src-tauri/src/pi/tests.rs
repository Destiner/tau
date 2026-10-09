use super::*;
use std::{process::Command, sync::mpsc, time::Duration};
use tauri::Listener;

#[cfg(unix)]
#[test]
fn core_events_keep_the_tauri_wire_shape_and_reader_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let telemetry_dir = root.path().join("telemetry");
    let app = tauri::test::mock_builder()
        .manage(Telemetry::for_test(telemetry_dir.clone()))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let (sender, events) = mpsc::channel();
    app.listen("pi-event", move |event| {
        let value: serde_json::Value = serde_json::from_str(event.payload()).unwrap();
        let _ = sender.send(value);
    });
    let state = PiState::default();
    state.claim("owner".into(), 0).unwrap();
    let mut command = Command::new("sh");
    command.args(["-c", "printf 'private-invalid-json\\n'; printf '{\"type\":\"response\",\"secret\":\"private-payload\"}\\n'; exit 0"]);
    let generation = state
        .start_command(
            "owner".into(),
            "runtime".into(),
            command,
            event_sink(app.handle().clone(), None),
        )
        .unwrap();
    let started = events.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        started,
        serde_json::json!({ "runtimeId": "runtime", "generation": generation, "kind": "started" })
    );
    let rpc = events.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(rpc["kind"], "rpc");
    assert!(rpc["line"].as_str().unwrap().contains("private-payload"));
    let exited = events.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(exited["kind"], "exited");
    assert_eq!(exited["code"], 0);
    let telemetry = app.state::<Telemetry>();
    telemetry.force_flush_logs();
    let contents = read_files(&telemetry_dir);
    assert!(contents.contains("pi.reader.line_dropped"));
    assert!(contents.contains("pi.process.started"));
    assert!(contents.contains("pi.process.exited"));
    assert!(!contents.contains("private-payload"));
    assert!(!contents.contains("private-invalid-json"));
}

fn read_files(directory: &std::path::Path) -> String {
    let mut result = String::new();
    for entry in std::fs::read_dir(directory).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            result.push_str(&read_files(&path));
        } else if let Ok(contents) = std::fs::read_to_string(path) {
            result.push_str(&contents);
        }
    }
    result
}
