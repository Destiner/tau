use super::*;
use serde_json::json;
use tau_session::session::{Controller, Effect};

fn assert_skill(entry: &Entry, name: &str, prompt: &str, text: &str) {
    let projected = row(entry);
    assert_eq!(projected.id, entry.id);
    let TranscriptContent::Activity(ActivityContent::Skill {
        name: actual_name,
        prompt: actual_prompt,
        text: actual_text,
    }) = projected.content
    else {
        panic!("Pi envelope must reach ActivityRow, not a user bubble")
    };
    assert_eq!(
        (
            actual_name.as_str(),
            actual_prompt.as_str(),
            actual_text.as_str()
        ),
        (name, prompt, text)
    );
}

#[test]
fn actual_pi_history_and_live_wire_project_skill_without_changing_source_or_notice_metadata() {
    let source = "<skill name=\"review\" location=\"/tmp/A & B/SKILL.md\">\n# Checks\n```rs\nlet x = 1;\n```\n\n</skill>\n\n  Review the patch. \n";
    for history in [true, false] {
        let mut controller = Controller::new("skill");
        let bootstrap = controller.begin_generation(1);
        let message =
            json!({"role":"user", "timestamp":55, "content":[{"type":"text", "text":source}]});
        if history {
            let request = bootstrap
                .iter()
                .find_map(|effect| match effect {
                    Effect::Send(request) if request["type"] == "get_messages" => Some(request),
                    _ => None,
                })
                .unwrap();
            controller.handle(json!({"type":"response", "id":request["id"], "command":"get_messages", "success":true, "data":{"messages":[message]}}));
        } else {
            controller.handle(json!({"type":"message_start", "message":message}));
        }
        let entry = &controller.state().transcript[0];
        assert_skill(
            entry,
            "review",
            "Review the patch.",
            "# Checks\n```rs\nlet x = 1;\n```\n",
        );
        assert_eq!(entry.text, source);
        assert_eq!(entry.source, "user:55:occ0");
        assert_eq!(entry.skill().unwrap().location, "/tmp/A & B/SKILL.md");
        assert_eq!(entry.notice_type, tau_session::transcript::NoticeType::Info);
        let mut projection = Projection::default();
        projection.changes(&controller.state().transcript, |_| {});
        let id = entry.id.clone();
        let updated = source.replace("Review the patch.", "Review the patch. Check tests.");
        let mut updated_entry = entry.clone();
        updated_entry.text = updated;
        updated_entry.revision += 1;
        projection.changes(&[updated_entry], |change| {
            let Change::Replace(entry) = change else {
                panic!("skill fields are not a prose suffix")
            };
            assert_eq!(entry.id, id);
            assert_skill(
                entry,
                "review",
                "Review the patch. Check tests.",
                "# Checks\n```rs\nlet x = 1;\n```\n",
            );
        });
    }
}

#[test]
fn ordinary_user_wire_is_unchanged_and_envelope_materialization_replaces_bubble() {
    let mut controller = Controller::new("skill");
    controller.begin_generation(1);
    let ordinary =
        "Review <skill name=\"review\" location=\"path\"> as ordinary prose.\r\n  Keep spacing.\n";
    controller.handle(json!({"type":"message_start", "message":{"role":"user", "timestamp":2, "content":ordinary}}));
    let entry = &controller.state().transcript[0];
    assert!(matches!(row(entry).content, TranscriptContent::User(ref text) if text == ordinary));
    let mut controller = Controller::new("ack");
    let bootstrap = controller.begin_generation(1);
    let state_request = bootstrap
        .iter()
        .find_map(|effect| match effect {
            Effect::Send(request) if request["type"] == "get_state" => Some(request),
            _ => None,
        })
        .unwrap();
    controller.handle(json!({"type":"response", "id":state_request["id"], "command":"get_state", "success":true, "data":{}}));
    assert!(!controller.submit("/skill:review").is_empty());
    let id = controller.state().transcript[0].id.clone();
    let mut projection = Projection::default();
    projection.changes(&controller.state().transcript, |_| {});
    controller.handle(json!({"type":"message_start", "message":{"role":"user", "timestamp":2, "content":"<skill name=\"review\" location=\"path\">\nchecks\n</skill>"}}));
    projection.changes(&controller.state().transcript, |change| {
        let Change::Replace(entry) = change else {
            panic!()
        };
        assert_eq!(entry.id, id);
        assert!(!entry.pending);
        assert_eq!(entry.source, "user:2:occ0");
        assert_skill(entry, "review", "", "checks");
    });
}
