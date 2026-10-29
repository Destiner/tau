use super::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
struct FakeHttp {
    requests: Mutex<Vec<Request>>,
    replies: Mutex<VecDeque<Result<Value>>>,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl Http for FakeHttp {
    fn send(&self, request: Request) -> Result<Value> {
        self.requests.lock().unwrap().push(request);
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook();
        }
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(json!({})))
    }
}
impl Secrets for Arc<MemorySecrets> {
    fn get(&self, p: &str) -> Result<Option<String>> {
        self.as_ref().get(p)
    }
    fn set(&self, p: &str, v: &str) -> Result<()> {
        self.as_ref().set(p, v)
    }
    fn delete(&self, p: &str) -> Result<()> {
        self.as_ref().delete(p)
    }
}
struct Fixture {
    root: tempfile::TempDir,
    cloud: CloudState,
    http: Arc<FakeHttp>,
    secrets: Arc<MemorySecrets>,
}
impl Fixture {
    fn new(admin: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let http = Arc::<FakeHttp>::default();
        let secrets = Arc::<MemorySecrets>::default();
        let pipeline = Arc::new(Pipeline::new(http.clone()));
        let cloud = CloudState::new(
            root.path().to_owned(),
            admin,
            pipeline,
            Box::new(secrets.clone()),
            http.clone(),
        );
        Self {
            root,
            cloud,
            http,
            secrets,
        }
    }
    fn reply(&self, value: Value) {
        self.http.replies.lock().unwrap().push_back(Ok(value));
    }
    fn linear(&self) {
        self.reply(options());
        self.cloud
            .save_linear("test-linear-secret".into(), "team".into(), None)
            .unwrap();
    }
    fn grafana(&self) {
        self.cloud
            .save_grafana(
                "https://example.grafana.net/otlp///".into(),
                "123".into(),
                "test-grafana-secret".into(),
            )
            .unwrap();
    }
    fn report(&self) -> Result<IssueReceipt> {
        self.cloud.submit_inner(
            "  Exact first line\n\nUser description  ".into(),
            Some("session-opt-in".into()),
            report_id().into(),
            "1700000000000000000".into(),
        )
    }
}
fn report_id() -> &'static str {
    "a1872ec6-9bdf-40f6-a7f7-a06ba9024432"
}
fn options() -> Value {
    json!({"data":{"teams":{"nodes":[{"id":"team","name":"Tau","projects":{"nodes":[{"id":"project","name":"Desktop"}],"pageInfo":{"hasNextPage":false}}}],"pageInfo":{"hasNextPage":false}}}})
}
fn empty_issues() -> Value {
    json!({"data":{"issues":{"nodes":[],"pageInfo":{"hasNextPage":false}}}})
}
fn issue() -> Value {
    json!({"id":report_id(),"identifier":"TAU-42","url":"https://linear.app/tau/issue/TAU-42"})
}
fn report_body() -> String {
    format!("  Exact first line\n\nUser description  \n\n---\nTau issue report\nReport ID: {}\nOriginal timestamp (Unix ns): 1700000000000000000\nOriginal timestamp (UTC): 2023-11-14T22:13:20Z\nTau version: {}\nEnvironment: {}\nSession ID (opt-in): session-opt-in", report_id(), env!("CARGO_PKG_VERSION"), crate::profile::ENVIRONMENT_NAME)
}
fn existing(body: &str) -> Value {
    let mut issue = issue();
    issue["description"] = json!(body);
    issue["team"] = json!({"id":"team"});
    issue["project"] = Value::Null;
    json!({"data":{"issues":{"nodes":[issue],"pageInfo":{"hasNextPage":false}}}})
}
fn code<T>(result: Result<T>) -> &'static str {
    match result {
        Err(e) => e.code,
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn every_cloud_operation_requires_runtime_admin_not_preferences() {
    let f = Fixture::new(false);
    assert_eq!(code(f.cloud.read()), "admin_required");
    assert_eq!(
        code(f.cloud.validate_linear("key".into())),
        "admin_required"
    );
    assert_eq!(
        code(f.cloud.save_linear("key".into(), "team".into(), None)),
        "admin_required"
    );
    assert_eq!(
        code(
            f.cloud
                .save_grafana("https://example.com".into(), "1".into(), "token".into())
        ),
        "admin_required"
    );
    assert_eq!(code(f.cloud.disconnect("linear")), "admin_required");
    assert_eq!(code(f.report()), "admin_required");
    assert!(f.http.requests.lock().unwrap().is_empty());
    assert!(fs::read_dir(f.root.path()).unwrap().next().is_none());
}
#[test]
fn admin_without_grafana_does_not_collect_or_export() {
    let f = Fixture::new(true);
    assert!(!f.cloud.read().unwrap().telemetry_enabled);
    assert!(!f.cloud.0.pipeline.append(
        crate::telemetry::transport::Signal::Log,
        json!({"resourceLogs":[]})
    ));
    f.linear();
    assert!(!f.cloud.read().unwrap().telemetry_enabled);
    assert_eq!(f.http.requests.lock().unwrap().len(), 1);
}
#[test]
fn validation_never_persists_and_personal_key_is_not_bearer() {
    let f = Fixture::new(true);
    f.reply(options());
    let options = f.cloud.validate_linear("test-key".into()).unwrap();
    assert_eq!(options.teams[0].projects[0].id, "project");
    assert!(f.cloud.read().unwrap().linear.is_none());
    assert!(fs::read_dir(f.root.path()).unwrap().next().is_none());
    let requests = f.http.requests.lock().unwrap();
    assert_eq!(requests[0].url, LINEAR_URL);
    assert!(matches!(&requests[0].authorization, Authorization::Linear(key) if key == "test-key"));
}
#[test]
fn destination_is_verified_and_project_is_optional() {
    let f = Fixture::new(true);
    f.reply(options());
    assert_eq!(
        code(f.cloud.save_linear("key".into(), "other".into(), None)),
        "invalid"
    );
    f.reply(options());
    assert_eq!(
        code(
            f.cloud
                .save_linear("key".into(), "team".into(), Some("other-project".into()))
        ),
        "invalid"
    );
    f.reply(options());
    let snapshot = f
        .cloud
        .save_linear("key".into(), "team".into(), Some("project".into()))
        .unwrap();
    assert_eq!(
        snapshot.linear.unwrap().project_name.as_deref(),
        Some("Desktop")
    );
    f.linear();
    assert!(f.cloud.read().unwrap().linear.unwrap().project_id.is_none());
}
#[test]
fn graphql_errors_and_truncated_destination_lists_fail_explicitly() {
    let f = Fixture::new(true);
    f.reply(json!({"data":{},"errors":[{"message":"private payload"}]}));
    let e = f.cloud.validate_linear("key".into()).err().unwrap();
    assert_eq!(e.code, "unavailable");
    assert!(!serde_json::to_string(&e).unwrap().contains("private"));
    let mut data = options();
    data["data"]["teams"]["pageInfo"]["hasNextPage"] = json!(true);
    f.reply(data);
    assert_eq!(code(f.cloud.validate_linear("key".into())), "unavailable");
    let mut data = options();
    data["data"]["teams"]["nodes"][0]["projects"]["pageInfo"]["hasNextPage"] = json!(true);
    f.reply(data);
    assert_eq!(code(f.cloud.validate_linear("key".into())), "unavailable");
}
#[test]
fn offline_and_authentication_fail_without_saving() {
    for failure in ["network", "authentication"] {
        let f = Fixture::new(true);
        f.http
            .replies
            .lock()
            .unwrap()
            .push_back(Err(error(failure)));
        assert_eq!(
            code(f.cloud.save_grafana(
                "https://example.com/otlp".into(),
                "123".into(),
                "token".into()
            )),
            failure
        );
        assert!(f.cloud.read().unwrap().grafana.is_none());
        assert!(fs::read_dir(f.root.path()).unwrap().next().is_none());
    }
}
#[test]
fn grafana_validates_all_three_signals_and_stores_no_secrets() {
    let f = Fixture::new(true);
    f.grafana();
    let requests = f.http.requests.lock().unwrap();
    for (i, signal) in ["logs", "traces", "metrics"].iter().enumerate() {
        assert_eq!(
            requests[i].url,
            format!("https://example.grafana.net/otlp/v1/{signal}")
        );
        assert!(
            matches!(&requests[i].authorization, Authorization::Basic(id, token) if id == "123" && token == "test-grafana-secret")
        );
    }
    let snapshot = serde_json::to_string(&f.cloud.read().unwrap()).unwrap();
    let file = fs::read_to_string(f.root.path().join(CONFIG_FILE)).unwrap();
    assert!(!snapshot.contains("secret"));
    assert!(!file.contains("test-grafana-secret"));
    assert_eq!(f.secrets.0.lock().unwrap().len(), 1);
    assert!(f.cloud.0.pipeline.is_enabled());
}
#[test]
fn endpoint_validation_and_partial_rejections_fail_closed() {
    for endpoint in [
        "http://example.com/otlp",
        "https://u:p@example.com/otlp",
        "https://example.com/otlp?q=secret",
        "https://example.com/otlp#token",
        "https://example.com/otlp/v1/logs",
    ] {
        assert_eq!(code(normalize_endpoint(endpoint)), "invalid");
    }
    assert_eq!(
        normalize_endpoint("https://example.com/otlp///").unwrap(),
        "https://example.com/otlp"
    );
    for partial in [
        json!({"partialSuccess":{"rejectedLogRecords":"1"}}),
        json!({"partialSuccess":{"rejectedSpans":1}}),
        json!({"partialSuccess":{"errorMessage":"sensitive text"}}),
    ] {
        assert_eq!(code(validate_otlp_response(&partial)), "unavailable");
        let f = Fixture::new(true);
        f.reply(partial);
        assert_eq!(
            code(f.cloud.save_grafana(
                "https://example.com/otlp".into(),
                "1".into(),
                "token".into()
            )),
            "unavailable"
        );
        assert!(!f.cloud.0.pipeline.is_enabled());
    }
    validate_otlp_response(&json!({"partialSuccess":{"rejectedSpans":"0"}})).unwrap();
}
#[test]
fn disable_reenable_and_disconnect_invalidate_slow_validation_and_saves() {
    for operation in ["toggle", "disconnect"] {
        let f = Fixture::new(true);
        let cloud = f.cloud.clone();
        *f.http.hook.lock().unwrap() = Some(Box::new(move || {
            if operation == "toggle" {
                cloud.set_admin(false);
                cloud.set_admin(true);
            } else {
                cloud.disconnect("linear").unwrap();
            }
        }));
        f.reply(options());
        assert_eq!(
            code(f.cloud.save_linear("key".into(), "team".into(), None)),
            "unavailable"
        );
        assert!(f.cloud.read().unwrap().linear.is_none());
    }
    let f = Fixture::new(true);
    let cloud = f.cloud.clone();
    *f.http.hook.lock().unwrap() = Some(Box::new(move || {
        cloud.set_admin(false);
        cloud.set_admin(true);
    }));
    f.reply(options());
    assert_eq!(code(f.cloud.validate_linear("key".into())), "unavailable");
}
#[test]
fn stale_grafana_setup_stops_before_next_network_request() {
    let f = Fixture::new(true);
    let cloud = f.cloud.clone();
    *f.http.hook.lock().unwrap() = Some(Box::new(move || {
        cloud.set_admin(false);
        cloud.set_admin(true);
    }));
    assert_eq!(
        code(f.cloud.save_grafana(
            "https://example.com/otlp".into(),
            "1".into(),
            "token".into()
        )),
        "unavailable"
    );
    assert_eq!(f.http.requests.lock().unwrap().len(), 1);
    assert!(!f.cloud.0.pipeline.is_enabled());
}
#[test]
fn disconnect_keeps_the_other_provider_and_missing_credentials_are_not_success() {
    let f = Fixture::new(true);
    f.linear();
    f.grafana();
    let snapshot = f.cloud.disconnect("linear").unwrap();
    assert!(snapshot.linear.is_none());
    assert!(snapshot.telemetry_enabled);
    f.linear();
    let snapshot = f.cloud.disconnect("grafana").unwrap();
    assert!(snapshot.linear.is_some());
    assert!(snapshot.grafana.is_none());
    assert!(!snapshot.telemetry_enabled);
    f.secrets.0.lock().unwrap().clear();
    assert!(f.cloud.read().unwrap().linear.is_none());
}
#[test]
fn failed_atomic_save_preserves_previous_config_credential_pairing() {
    let f = Fixture::new(true);
    f.linear();
    let previous = fs::read(f.root.path().join(CONFIG_FILE)).unwrap();
    fs::remove_file(f.root.path().join(CONFIG_FILE)).unwrap();
    fs::create_dir(f.root.path().join(CONFIG_FILE)).unwrap();
    f.reply(options());
    assert_eq!(
        code(
            f.cloud
                .save_linear("replacement-key".into(), "team".into(), None)
        ),
        "storage"
    );
    let inner = f.cloud.0.inner.lock().unwrap();
    assert_eq!(
        f.cloud
            .credential(&inner.config, "linear")
            .unwrap()
            .as_deref(),
        Some("test-linear-secret")
    );
    assert_eq!(f.secrets.0.lock().unwrap().len(), 1);
    fs::remove_dir(f.root.path().join(CONFIG_FILE)).unwrap();
    fs::write(f.root.path().join(CONFIG_FILE), previous).unwrap();
}
#[test]
fn reports_preserve_exact_description_use_client_uuid_and_never_write_content() {
    let f = Fixture::new(true);
    f.linear();
    f.reply(empty_issues());
    f.reply(json!({"data":{"issueCreate":{"success":true,"issue":issue()}}}));
    let receipt = f.report().unwrap();
    assert_eq!(receipt.identifier, "TAU-42");
    let requests = f.http.requests.lock().unwrap();
    let input = &requests[2].body["variables"]["input"];
    assert_eq!(input["id"], report_id());
    assert_eq!(input["description"], report_body());
    assert_eq!(input["title"], "Exact first line");
    assert!(input["projectId"].is_null());
    let files = fs::read_dir(f.root.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(files, vec![std::ffi::OsString::from(CONFIG_FILE)]);
    let saved = fs::read_to_string(f.root.path().join(CONFIG_FILE)).unwrap();
    assert!(!saved.contains("description"));
    assert!(!saved.contains("session-opt-in"));
}
#[test]
fn retry_recognizes_server_uuid_but_never_an_unrelated_report() {
    let f = Fixture::new(true);
    f.linear();
    f.reply(existing(&report_body()));
    f.report().unwrap();
    assert_eq!(f.http.requests.lock().unwrap().len(), 2);
    f.reply(existing("unrelated body"));
    assert_eq!(code(f.report()), "invalid");
    assert_eq!(f.http.requests.lock().unwrap().len(), 3);
}
#[test]
fn duplicate_metadata_survives_upgrade_but_rejects_changed_timestamp_or_session() {
    let expected = report_body();
    let previous = expected.replace(
        concat!("Tau version: ", env!("CARGO_PKG_VERSION")),
        "Tau version: 0.1.0",
    );
    assert!(report_matches(&previous, &expected));
    assert!(!report_matches(
        &expected.replace("1700000000000000000", "1700000000000000001"),
        &expected
    ));
    assert!(!report_matches(
        &expected.replace("session-opt-in", "another-session"),
        &expected
    ));
    assert!(!report_matches(
        &expected.replace("Tau issue report", "Other issue report"),
        &expected
    ));
}
#[test]
fn admin_disabled_during_issue_lookup_prevents_creation() {
    let f = Fixture::new(true);
    f.linear();
    f.reply(empty_issues());
    let cloud = f.cloud.clone();
    *f.http.hook.lock().unwrap() = Some(Box::new(move || cloud.set_admin(false)));
    assert_eq!(code(f.report()), "admin_required");
    assert_eq!(f.http.requests.lock().unwrap().len(), 2);
}
#[test]
fn report_validation_rejects_missing_ids_empty_content_and_session_paths() {
    let f = Fixture::new(true);
    f.linear();
    for (description, session, id, time) in [
        ("", None, report_id(), "1"),
        ("text", None, "not-a-uuid", "1"),
        ("text", None, report_id(), "01"),
        (
            "text",
            Some("/local/private/path".to_string()),
            report_id(),
            "1",
        ),
    ] {
        assert_eq!(
            code(
                f.cloud
                    .submit_inner(description.into(), session, id.into(), time.into())
            ),
            "invalid"
        );
    }
    assert_eq!(f.http.requests.lock().unwrap().len(), 1);
}
#[test]
fn https_only_client_rejects_plain_http_without_sending_credentials() {
    let response = Network.send(Request {
        url: "http://127.0.0.1:1/".into(),
        authorization: Authorization::Linear("test-only-not-real".into()),
        body: json!({}),
    });
    assert_eq!(code(response), "network");
}
#[test]
fn ambiguous_create_looks_up_uuid_without_repeating_mutation() {
    let f = Fixture::new(true);
    f.linear();
    f.reply(empty_issues());
    f.http
        .replies
        .lock()
        .unwrap()
        .push_back(Err(error("network")));
    f.reply(existing(&report_body()));
    f.report().unwrap();
    let requests = f.http.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.body["query"].as_str().unwrap().starts_with("mutation"))
            .count(),
        1
    );
    assert_eq!(requests.len(), 4);
}
#[test]
fn failed_submission_leaves_no_native_outbox() {
    let f = Fixture::new(true);
    f.linear();
    f.reply(empty_issues());
    f.http
        .replies
        .lock()
        .unwrap()
        .push_back(Err(error("network")));
    f.reply(empty_issues());
    assert_eq!(code(f.report()), "network");
    assert_eq!(fs::read_dir(f.root.path()).unwrap().count(), 1);
}
#[test]
fn historical_local_reports_and_markers_are_not_migrated_or_deleted() {
    let f = Fixture::new(true);
    for name in ["feedback", "telemetry"] {
        fs::create_dir(f.root.path().join(name)).unwrap();
    }
    fs::write(
        f.root.path().join("feedback/issues.jsonl"),
        "old private description",
    )
    .unwrap();
    fs::write(f.root.path().join("telemetry/run.marker"), "running").unwrap();
    f.grafana();
    f.cloud.set_admin(false);
    f.cloud.set_admin(true);
    f.cloud.disconnect("grafana").unwrap();
    assert_eq!(
        fs::read_to_string(f.root.path().join("feedback/issues.jsonl")).unwrap(),
        "old private description"
    );
    assert_eq!(
        fs::read_to_string(f.root.path().join("telemetry/run.marker")).unwrap(),
        "running"
    );
}
#[derive(Default)]
struct FailingSecrets {
    fail: AtomicBool,
    fail_grafana: AtomicBool,
    data: MemorySecrets,
}
impl Secrets for Arc<FailingSecrets> {
    fn get(&self, p: &str) -> Result<Option<String>> {
        if self.fail.load(Ordering::Relaxed)
            || (p.starts_with("grafana-") && self.fail_grafana.load(Ordering::Relaxed))
        {
            Err(error("storage"))
        } else {
            self.data.get(p)
        }
    }
    fn set(&self, p: &str, v: &str) -> Result<()> {
        if self.fail.load(Ordering::Relaxed) {
            Err(error("storage"))
        } else {
            self.data.set(p, v)
        }
    }
    fn delete(&self, p: &str) -> Result<()> {
        if self.fail.load(Ordering::Relaxed) {
            Err(error("storage"))
        } else {
            self.data.delete(p)
        }
    }
}
#[test]
fn keychain_failure_is_actionable_and_stops_export() {
    let f = Fixture::new(true);
    let secrets = Arc::<FailingSecrets>::default();
    let cloud = CloudState::new(
        f.root.path().to_owned(),
        true,
        f.cloud.0.pipeline.clone(),
        Box::new(secrets.clone()),
        f.http.clone(),
    );
    cloud
        .save_grafana(
            "https://example.com/otlp".into(),
            "1".into(),
            "token".into(),
        )
        .unwrap();
    secrets.fail.store(true, Ordering::Relaxed);
    assert!(cloud.read().unwrap().grafana.is_none());
    assert!(!cloud.0.pipeline.is_enabled());
    secrets.fail.store(false, Ordering::Relaxed);
    assert!(cloud.read().unwrap().telemetry_enabled);
    cloud.disconnect("grafana").unwrap();
    secrets.fail.store(true, Ordering::Relaxed);
    assert_eq!(
        code(cloud.save_grafana(
            "https://example.com/otlp".into(),
            "1".into(),
            "token".into()
        )),
        "storage"
    );
    assert!(!cloud.0.pipeline.is_enabled());
}
#[test]
fn inaccessible_grafana_credentials_do_not_block_linear_reports() {
    let f = Fixture::new(true);
    let secrets = Arc::<FailingSecrets>::default();
    let cloud = CloudState::new(
        f.root.path().to_owned(),
        true,
        f.cloud.0.pipeline.clone(),
        Box::new(secrets.clone()),
        f.http.clone(),
    );
    f.reply(options());
    cloud
        .save_linear("key".into(), "team".into(), None)
        .unwrap();
    cloud
        .save_grafana(
            "https://example.com/otlp".into(),
            "1".into(),
            "token".into(),
        )
        .unwrap();
    secrets.fail_grafana.store(true, Ordering::Relaxed);
    let snapshot = cloud.read().unwrap();
    assert!(snapshot.linear.is_some());
    assert!(snapshot.grafana.is_none());
    assert!(!snapshot.telemetry_enabled);
    f.reply(empty_issues());
    f.reply(json!({"data":{"issueCreate":{"success":true,"issue":issue()}}}));
    assert!(cloud
        .submit_inner(
            "Report".into(),
            None,
            report_id().into(),
            "1700000000000000000".into()
        )
        .is_ok());
}

#[test]
fn development_restart_without_memory_credentials_is_disconnected() {
    let f = Fixture::new(true);
    f.linear();
    f.grafana();
    let restarted = CloudState::new(
        f.root.path().to_owned(),
        true,
        Arc::new(Pipeline::new(f.http.clone())),
        Box::<MemorySecrets>::default(),
        f.http.clone(),
    );
    let snapshot = restarted.read().unwrap();
    assert!(snapshot.linear.is_none());
    assert!(snapshot.grafana.is_none());
    assert!(!snapshot.telemetry_enabled);
}
