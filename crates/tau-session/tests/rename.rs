use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, REQUEST_TIMEOUT_MS};
fn request(e: &[Effect], kind: &str) -> Value {
    e.iter()
        .find_map(|e| match e {
            Effect::Send(v) if v["type"] == kind => Some(v.clone()),
            _ => None,
        })
        .unwrap()
}
fn reply(c: &mut Controller, r: &Value, success: bool, data: Value) -> Vec<Effect> {
    c.handle(
        json!({"type":"response","id":r["id"],"command":r["type"],"success":success,"data":data}),
    )
}
fn state(id: &str, name: &str) -> Value {
    json!({"sessionId":id,"sessionFile":format!("/{id}"),"sessionName":name})
}
fn ready() -> Controller {
    let mut c = Controller::new("a");
    c.bind_source("saved", "/saved");
    let e = c.begin_generation(1);
    let e = reply(
        &mut c,
        &request(&e, "get_state"),
        true,
        state("saved", "Original"),
    );
    reply(
        &mut c,
        &request(&e, "get_messages"),
        true,
        json!({"messages":[]}),
    );
    c.set_draft("sacred");
    c
}
#[test]
fn rename_is_owned_correlated_and_reads_pi_before_registry_effect() {
    let mut c = ready();
    let r = request(&c.rename("  New   title "), "set_session_name");
    assert_eq!(r["name"], "New title");
    assert!(c.rename("duplicate").is_empty());
    assert!(c
        .handle(json!({"type":"response","id":r["id"],"command":"set_model","success":true}))
        .is_empty());
    assert!(c.rename_pending());
    let e = reply(&mut c, &r, true, json!({}));
    assert!(!e
        .iter()
        .any(|e| matches!(e, Effect::SessionNameChanged { .. })));
    let e = reply(
        &mut c,
        &request(&e, "get_state"),
        true,
        state("saved", "Pi echoed"),
    );
    assert!(e.iter().any(
        |e| matches!(e, Effect::SessionNameChanged { identity } if identity.name == "Pi echoed")
    ));
    assert_eq!(c.state().draft, "sacred");
}
#[test]
fn rejected_and_failed_writes_roll_back_allow_retry_and_keep_local_error() {
    for transport in [false, true] {
        let mut c = ready();
        let r = request(&c.rename("New"), "set_session_name");
        let e = if transport {
            c.request_failed(r["id"].as_str().unwrap())
        } else {
            reply(&mut c, &r, false, json!({}))
        };
        assert_eq!(c.state().identity.name, "Original");
        assert!(c.state().error.as_ref().unwrap().contains("renamed"));
        reply(
            &mut c,
            &request(&e, "get_state"),
            true,
            state("saved", "Original"),
        );
        assert!(c.state().error.is_some());
        assert!(!c.rename("Retry").is_empty());
        assert_eq!(c.state().draft, "sacred");
    }
}
#[test]
fn stale_reads_switches_generations_and_timeout_do_not_leak_titles() {
    let mut c = ready();
    let old = request(
        &c.handle(json!({"type":"session_info_changed"})),
        "get_state",
    );
    let r = request(&c.rename("New"), "set_session_name");
    reply(&mut c, &old, true, state("saved", "Stale"));
    assert_eq!(c.state().identity.name, "New");
    c.tick(REQUEST_TIMEOUT_MS + 1);
    assert_eq!(c.state().identity.name, "Original");
    assert!(!c.rename_pending());
    let r2 = request(&c.rename("Other"), "set_session_name");
    let e = c.handle(json!({"type":"session_info_changed"}));
    reply(
        &mut c,
        &request(&e, "get_state"),
        true,
        state("replacement", "Replacement"),
    );
    reply(&mut c, &r2, true, json!({}));
    assert_eq!(c.state().identity.name, "Replacement");
    c.begin_generation(2);
    assert!(c
        .ingest(
            "a",
            1,
            json!({"type":"response","id":r["id"],"command":"set_session_name","success":true})
        )
        .is_empty());
    assert_eq!(c.state().draft, "sacred");
}
