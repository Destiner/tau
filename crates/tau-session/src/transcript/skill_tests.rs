use super::*;
use crate::session::{Controller, Effect};
use serde_json::json;

const ENVELOPE: &str = "<skill name=\"review:changes\" location=\"/skills/A & B/SKILL.md\">\n  # Instructions\r\nKeep  spaces.\n\n</skill>\n\n  Review **this**.  \n";

#[test]
fn exact_pi_envelope_preserves_metadata_and_instruction_bytes() {
    let parsed = parse_skill_envelope(ENVELOPE).unwrap();
    assert_eq!(parsed.name, "review:changes");
    assert_eq!(parsed.location, "/skills/A & B/SKILL.md");
    assert_eq!(parsed.content, "  # Instructions\r\nKeep  spaces.\n");
    assert_eq!(parsed.prompt, "Review **this**.");
    let nested =
        "<skill name=\"review\" location=\"path\">\nbody\n</skill> inside instructions\n</skill>";
    assert_eq!(
        parse_skill_envelope(nested).unwrap().content,
        "body\n</skill> inside instructions"
    );
    let prompt =
        "<skill name=\"review\" location=\"path\">\nbody\n</skill>\n\n\u{feff} prompt \u{feff}";
    assert_eq!(parse_skill_envelope(prompt).unwrap().prompt, "prompt");
    for ordinary in [
        "Use the review skill",
        "<skill name='review' location='path'>\nbody\n</skill>",
        " <skill name=\"review\" location=\"path\">\nbody\n</skill>",
        "<skill location=\"path\" name=\"review\">\nbody\n</skill>",
        "<skill name=\"\" location=\"path\">\nbody\n</skill>",
        "<skill name=\"review\" location=\"path\">\r\nbody\r\n</skill>",
        "<skill name=\"review\" location=\"path\">\nbody\n</skill>\nextra",
    ] {
        assert!(parse_skill_envelope(ordinary).is_none(), "{ordinary}");
        let row = project(&json!({"role":"user", "content":ordinary}), "user:1").remove(0);
        assert_eq!(row.text, ordinary);
        assert!(row.skill().is_none());
    }
}

#[test]
fn saved_history_live_and_optimistic_ack_keep_pi_owned_source() {
    for history in [true, false] {
        let mut controller = Controller::new("skill");
        let bootstrap = controller.begin_generation(1);
        let message =
            json!({"role":"user", "timestamp":71, "content":[{"type":"text", "text":ENVELOPE}]});
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
        let row = &controller.state().transcript[0];
        assert_eq!(row.text, ENVELOPE);
        assert_eq!(row.source, "user:71:occ0");
        assert_eq!(
            row.skill().unwrap(),
            parse_skill_envelope(ENVELOPE).unwrap()
        );
        assert_eq!(row.notice_type, NoticeType::Info);
    }
    // The controller acknowledges optimistic user rows through replace_text,
    // rather than project(). Parsing the source accessor covers that path too.
    let mut row = Entry::new(
        "pending".into(),
        "user",
        "draft".into(),
        "optimistic:1".into(),
    );
    row.pending = true;
    row.replace_text(ENVELOPE.into());
    assert_eq!(
        row.skill().unwrap(),
        parse_skill_envelope(ENVELOPE).unwrap()
    );
    assert_eq!(row.text, ENVELOPE);
}
