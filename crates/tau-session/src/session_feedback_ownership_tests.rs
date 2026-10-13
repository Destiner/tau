use super::*;

#[test]
fn equal_error_text_from_a_new_incident_is_not_extension_ownership() {
    for expire in [false, true] {
        let mut c = Controller::new("fake");
        c.begin_generation(1);
        c.handle(json!({"type":"extension_ui_request","id":"ask","method":"input","timeout":100}));
        c.extension_response("ask", ExtensionResponse::Cancelled);
        c.extension_sent("ask", false);
        let copy = c.state.error.clone().unwrap();
        assert!(c.extension_delivery_error_owned("ask"));
        c.fail(&copy);
        assert!(!c.extension_delivery_error_owned("ask"));
        if expire {
            c.tick(100);
        } else {
            c.extension_response("ask", ExtensionResponse::Cancelled);
            assert_eq!(c.state.error.as_deref(), Some(copy.as_str()));
            assert!(!c.extension_delivery_error_owned("ask"));
            c.extension_sent("ask", true);
        }
        assert_eq!(c.state.error.as_deref(), Some(copy.as_str()));
    }
}

#[test]
fn an_unrelated_existing_incident_is_not_overwritten_by_failed_written() {
    let mut c = Controller::new("fake");
    c.begin_generation(1);
    c.fail("unrelated");
    c.handle(json!({"type":"extension_ui_request","id":"ask","method":"input"}));
    c.extension_response("ask", ExtensionResponse::Cancelled);
    c.extension_sent("ask", false);
    assert_eq!(c.state.error.as_deref(), Some("unrelated"));
    assert!(!c.extension_delivery_error_owned("ask"));
    c.extension_response("ask", ExtensionResponse::Cancelled);
    c.extension_sent("ask", true);
    assert_eq!(c.state.error.as_deref(), Some("unrelated"));
}

#[test]
fn delivery_ownership_requires_current_epoch_revision_request_and_error() {
    let mut c = Controller::new("fake");
    c.begin_generation(1);
    c.handle(json!({"type":"extension_ui_request","id":"ask","method":"input"}));
    c.extension_response("ask", ExtensionResponse::Cancelled);
    c.extension_sent("ask", false);
    let owner = c.extension_error_owner.clone();
    let error = c.state.error.clone();
    let dialogs = c.state.extension_dialogs.clone();
    let revision = c.error_revision;
    let epoch = c.epoch;
    assert!(c.extension_delivery_error_owned("ask"));
    assert!(!c.extension_delivery_error_owned("other"));
    // Reads must not consume the incident or its provenance.
    assert!(c.extension_delivery_error_owned("ask"));
    assert_eq!(c.extension_error_owner, owner);
    assert_eq!(c.state.error, error);

    c.error_revision += 1;
    assert!(!c.extension_delivery_error_owned("ask"));
    c.error_revision = revision;
    c.epoch += 1;
    assert!(!c.extension_delivery_error_owned("ask"));
    c.epoch = epoch;
    c.state.extension_dialogs.clear();
    assert!(!c.extension_delivery_error_owned("ask"));
    c.state.extension_dialogs = dialogs;
    c.state.error = None;
    assert!(!c.extension_delivery_error_owned("ask"));
    c.state.error = error;
    assert!(c.extension_delivery_error_owned("ask"));

    assert!(c
        .extension_response("ask", ExtensionResponse::Confirmed(true))
        .is_empty());
    assert!(c.extension_delivery_error_owned("ask"));
    assert!(!c
        .extension_response("ask", ExtensionResponse::Cancelled)
        .is_empty());
    assert!(!c.extension_delivery_error_owned("ask"));
    assert!(c.state.error.is_none());
}
