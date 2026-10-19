use std::fs;
use tau_core::profile::StorageProfile;
use tau_observability::telemetry::{
    ingest,
    native::{Lifecycle, Operation, Outcome},
    Environment,
};
use tau_observability::Telemetry;

fn profile(root: &std::path::Path) -> StorageProfile {
    StorageProfile::isolated(root.join("tau"), root.join("sessions"))
}

#[test]
fn root_init_requires_explicit_profile() {
    assert!(Telemetry::init().is_err());
}

#[test]
fn switch_and_reports_share_the_existing_profile_and_format() {
    let root = tempfile::tempdir().unwrap();
    let profile = profile(root.path());
    let service = Telemetry::init_for_profile(&profile, Environment::Development).unwrap();
    service.record_lifecycle(Lifecycle::Started);
    assert!(!service.read_admin_mode());
    assert!(!profile.data_dir().exists());
    assert!(service
        .submit_issue_report("Private description".into(), None)
        .is_err());
    assert!(!profile.data_dir().exists());
    service.set_admin_mode(true).unwrap();
    assert!(service.read_admin_mode());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(profile.data_dir().join("preferences.json")).unwrap()
        )
        .unwrap()["adminMode"],
        true
    );
    service
        .submit_issue_report("  Private description  ".into(), None)
        .unwrap();
    service
        .submit_issue_report("Another issue".into(), Some("opted-in-session".into()))
        .unwrap();
    let lines = fs::read_to_string(profile.data_dir().join("feedback/issues.jsonl")).unwrap();
    let reports: Vec<serde_json::Value> = lines
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0]["description"], "Private description");
    assert!(reports[0].get("sessionId").is_none());
    assert_eq!(reports[1]["sessionId"], "opted-in-session");
    assert!(reports[0]["timeUnixNano"]
        .as_str()
        .unwrap()
        .parse::<u128>()
        .is_ok());
    let log = fs::read_to_string(profile.data_dir().join("telemetry/logs.jsonl")).unwrap();
    assert!(!log.contains("Private description"));
    assert!(!log.contains("opted-in-session"));
    service.set_admin_mode(false).unwrap();
    assert!(service.submit_issue_report("Blocked".into(), None).is_err());
    assert_eq!(
        fs::read_to_string(profile.data_dir().join("feedback/issues.jsonl")).unwrap(),
        lines
    );
    service.shutdown();
    let restarted = Telemetry::init_for_profile(&profile, Environment::Development).unwrap();
    assert!(!restarted.read_admin_mode());
    restarted.shutdown();
}

#[test]
fn validation_and_isolation_do_not_create_feedback() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let first_profile = profile(first.path());
    let second_profile = profile(second.path());
    let a = Telemetry::init_for_profile(&first_profile, Environment::Development).unwrap();
    let b = Telemetry::init_for_profile(&second_profile, Environment::Development).unwrap();
    a.set_admin_mode(true).unwrap();
    assert!(!b.read_admin_mode());
    assert!(!second_profile.data_dir().exists());
    assert!(a.submit_issue_report("  ".into(), None).is_err());
    assert!(a.submit_issue_report("x".repeat(10_001), None).is_err());
    assert!(a
        .submit_issue_report("Valid".into(), Some("s".repeat(257)))
        .is_err());
    assert!(!first_profile.data_dir().join("feedback").exists());
    a.submit_issue_report("é".repeat(10_000), Some("s".repeat(256)))
        .unwrap();
    a.record_app_exited();
    a.shutdown();
    b.shutdown();
}

#[test]
fn native_operations_use_catalog_and_invalidate_pending_timers() {
    let root = tempfile::tempdir().unwrap();
    let profile = profile(root.path());
    let service = Telemetry::init_for_profile(&profile, Environment::Development).unwrap();
    assert!(Operation::command("/private/path").is_none());
    assert!(Operation::action("prompt contents").is_none());
    let op = Operation::command("submit_issue_report").unwrap();
    service.start_operation(op).finish(Outcome::Success);
    assert!(!profile.data_dir().exists());
    service.set_admin_mode(true).unwrap();
    service.start_operation(op).finish(Outcome::Success);
    let trace_path = profile.data_dir().join("telemetry/traces.jsonl");
    let before = fs::read_to_string(&trace_path).unwrap();
    assert!(before.contains("tauri.invoke"));
    assert!(before.contains("submit_issue_report"));
    let stale = service.start_operation(op);
    service.set_admin_mode(false).unwrap();
    service.set_admin_mode(true).unwrap();
    stale.finish(Outcome::Error);
    assert_eq!(fs::read_to_string(trace_path).unwrap(), before);
    assert_eq!(
        ingest::ingest_records(
            &service,
            vec![serde_json::json!({"family":"frontend.error", "path":"/private/path"})]
        )
        .rejected,
        1
    );
    service.record_app_exited();
    service.shutdown();
}
