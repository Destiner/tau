use serde_json::json;
use tau_session::session::{normalize_session_name, Controller, Effect};

#[test]
fn controller_limits_normalized_names_in_utf16_without_splitting_scalars() {
    for input in [
        "x".repeat(300),
        format!("  {}  😀 tail", "x".repeat(239)),
        format!("  hello\n\t{}", "😀".repeat(200)),
    ] {
        let mut c = Controller::new("runtime");
        let effects = c.begin_generation(1);
        let id = effects
            .iter()
            .find_map(|e| match e {
                Effect::Send(v) if v["type"] == "get_state" => Some(v["id"].clone()),
                _ => None,
            })
            .unwrap();
        c.handle(json!({"type":"response","id":id,"command":"get_state","success":true,"data":{"sessionId":"saved","sessionFile":"/saved","sessionName":"Original"}}));
        let effects = c.rename(&input);
        let name = effects
            .iter()
            .find_map(|e| match e {
                Effect::Send(v) if v["type"] == "set_session_name" => v["name"].as_str(),
                _ => None,
            })
            .unwrap();
        assert_eq!(name, normalize_session_name(&input));
        assert!(name.encode_utf16().count() <= 240);
        assert_eq!(c.state().identity.name, name);
    }
    assert_eq!(
        normalize_session_name(&format!("{}😀", "x".repeat(239))),
        "x".repeat(239)
    );
    assert_eq!(normalize_session_name(" a\n\t b "), "a b");
}
