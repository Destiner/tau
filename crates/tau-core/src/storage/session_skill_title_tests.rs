use super::*;
use crate::profile::ISOLATED_SESSION_REGISTRY_FILENAME;
use serde_json::json;

const SKILL: &str = "<skill name=\"triage\" location=\"/invented/skills/triage/SKILL.md\">\nInvented instructions.\n</skill>\n\nFix the invented report.";

#[test]
fn saved_skill_titles_survive_registration_reload_and_archive_without_naming_pi() {
    profile::initialize_test_profile();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invented-skill.jsonl");
    let source = format!(
        "{}\n{}\n",
        json!({"type":"session", "id":"invented-skill"}),
        json!({"type":"message", "message":{"role":"user", "timestamp":123, "content":[{"type":"text", "text":SKILL}]}})
    );
    fs::write(&path, &source).unwrap();
    let registered = register_local_session_in(
        directory.path(),
        "invented-skill",
        path.to_str().unwrap(),
        String::new(),
        true,
        None,
        0,
    )
    .unwrap()
    .unwrap();
    assert_eq!(registered.title, "/skill:triage");
    assert!(registered.title_is_derived);
    assert_eq!(registered.title_markdown.as_deref(), Some("/skill:triage"));

    let registry_path = directory.path().join(ISOLATED_SESSION_REGISTRY_FILENAME);
    let mut registry: TauSessionRegistry = read_json_or_default(&registry_path).unwrap();
    assert_eq!(registry.sessions[0].name, None);
    set_local_session_archived(&mut registry, "invented-skill", true).unwrap();
    write_json_atomic(&registry_path, &registry).unwrap();
    for _ in 0..2 {
        let reloaded = list_sessions_in(directory.path()).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].title, "/skill:triage");
        assert!(reloaded[0].archived && reloaded[0].title_is_derived);
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
}

#[test]
fn skill_title_derivation_never_parses_explicit_names_or_guesses_from_prose() {
    for explicit in ["Named in Pi", SKILL] {
        assert_eq!(session_title(explicit, Some("stale"), SKILL), explicit);
        assert_eq!(session_title("", Some(explicit), SKILL), explicit);
    }
    for ordinary in [
        "Ordinary first message",
        "Use /skill:triage please",
        "<skill name=\"triage\" location=\"/invented/SKILL.md\">\nMissing closing tag",
        "<skill name=\"\" location=\"/invented/SKILL.md\">\nBody\n</skill>",
    ] {
        assert_eq!(session_title("", None, ordinary), ordinary);
    }
    assert_eq!(session_title("", None, SKILL), "/skill:triage");
}

#[test]
fn saved_explicit_skill_shaped_name_remains_authoritative_after_reload() {
    profile::initialize_test_profile();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("named.jsonl");
    fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            json!({"type":"session", "id":"named"}),
            json!({"type":"message", "message":{"role":"user", "content":SKILL}}),
            json!({"type":"session_info", "name":SKILL}),
        ),
    )
    .unwrap();
    let summary = register_local_session_in(
        directory.path(),
        "named",
        path.to_str().unwrap(),
        String::new(),
        true,
        None,
        0,
    )
    .unwrap()
    .unwrap();
    assert!(!summary.title_is_derived);
    assert_eq!(summary.title, single_line(SKILL));
    let reloaded = list_sessions_in(directory.path()).unwrap();
    assert_eq!(reloaded[0].title, summary.title);
    assert!(!reloaded[0].title_is_derived);
}
