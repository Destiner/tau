use std::{fs, path::PathBuf};
use tau_core::{
    profile::{self, StorageProfile},
    saved_transcript, storage,
};

#[test]
fn isolated_services_share_one_registry_and_preserve_transcript_identity() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let data = root_path.join("data");
    let sessions = root_path.join("sessions");
    profile::initialize(StorageProfile::isolated(data.clone(), sessions.clone())).unwrap();
    assert!(profile::initialize(StorageProfile::isolated(
        root_path.join("other-data"),
        root_path.join("other-sessions")
    ))
    .is_err());
    assert_eq!(
        profile::current().unwrap().session_registry_filename(),
        ".tau-dev.json"
    );
    assert!(storage::load_workspace().unwrap().projects.is_empty());
    assert!(!data.join("projects.json").exists());

    let project = root_path.join("project");
    fs::create_dir(&project).unwrap();
    let summary = storage::import_project(project.to_string_lossy().into_owned()).unwrap();
    let directory = storage::default_session_dir(&summary.path).unwrap();
    assert!(directory.starts_with(&sessions));
    fs::create_dir_all(&directory).unwrap();
    let transcript = directory.join("session.jsonl");
    let body = concat!(
        "{\"type\":\"session\",\"version\":3,\"id\":\"session\"}\n",
        "{\"type\":\"message\",\"id\":\"user\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"Hello\"}}\n"
    );
    fs::write(&transcript, body).unwrap();
    let path = transcript.to_string_lossy().into_owned();
    let register = |revision| {
        storage::register_session_at_revision(
            summary.path.clone(),
            "session".into(),
            path.clone(),
            None,
            None,
            Some(true),
            revision,
        )
    };
    let revision = storage::archive_revision(&summary.path, "session").unwrap();
    assert!(register(revision).unwrap().is_some());
    assert!(directory.join(".tau-dev.json").exists());
    assert!(!directory.join(".tau.json").exists());
    storage::set_active_session(summary.path.clone(), "session".into()).unwrap();
    let stale_revision = storage::archive_revision(&summary.path, "session").unwrap();
    let archived = storage::archive_session(summary.path.clone(), "session".into()).unwrap();
    assert!(archived.archived);
    assert!(archived.active_session_id.is_empty());
    assert!(register(stale_revision).unwrap().unwrap().archived);
    let current_revision = storage::archive_revision(&summary.path, "session").unwrap();
    assert!(!register(current_revision).unwrap().unwrap().archived);
    storage::archive_session(summary.path.clone(), "session".into()).unwrap();
    storage::unarchive_session(summary.path.clone(), "session".into()).unwrap();
    assert!(!storage::list_sessions(&summary.path).unwrap()[0].archived);
    let generation = saved_transcript::begin_saved_transcript_read();
    saved_transcript::cancel_saved_transcript();
    assert!(saved_transcript::read_saved_transcript_at_generation(
        summary.path.clone(),
        "session".into(),
        path.clone(),
        generation
    )
    .messages
    .is_none());
    let preview = saved_transcript::read_saved_transcript(
        summary.path.clone(),
        "session".into(),
        path.clone(),
    );
    assert_eq!(preview.messages.unwrap()[0]["content"], "Hello");
    assert!(saved_transcript::read_saved_transcript(
        summary.path.clone(),
        "other".into(),
        path.clone()
    )
    .messages
    .is_none());

    // An explicit path never falls back to another transcript with the requested id.
    fs::write(
        &transcript,
        body.replace("\"id\":\"session\"", "\"id\":\"wrong\""),
    )
    .unwrap();
    let fallback = directory.join("fallback.jsonl");
    fs::write(&fallback, body).unwrap();
    assert!(storage::list_sessions(&summary.path).unwrap().is_empty());
    assert!(saved_transcript::read_saved_transcript(
        summary.path.clone(),
        "session".into(),
        fallback.to_string_lossy().into_owned(),
    )
    .messages
    .is_none());
    assert!(saved_transcript::read_saved_transcript(
        summary.path.clone(),
        "session".into(),
        path.clone()
    )
    .messages
    .is_none());
    fs::write(&transcript, body).unwrap();
    assert_eq!(storage::list_sessions(&summary.path).unwrap().len(), 1);
    assert_eq!(fs::read_to_string(&transcript).unwrap(), body);

    storage::set_project_collapsed(summary.path.clone(), true).unwrap();
    storage::set_active_project(summary.path.clone()).unwrap();
    storage::reorder_projects(vec![summary.path.clone()]).unwrap();
    let snapshot = storage::load_workspace().unwrap();
    assert!(snapshot.projects[0].collapsed);
    assert_eq!(snapshot.projects[0].sessions.len(), 1);
    assert!(snapshot.projects[0].selected);

    let remote = storage::import_remote_project(
        "ssh fake.invalid".into(),
        "/work".into(),
        "fake.invalid".into(),
    )
    .unwrap();
    storage::register_session(
        remote.path.clone(),
        "remote-session".into(),
        "/remote/session.jsonl".into(),
        Some("Remote".into()),
        Some(1000),
        None,
    )
    .unwrap();
    storage::set_active_session(remote.path.clone(), "remote-session".into()).unwrap();
    assert!(storage::list_sessions(&remote.path).unwrap()[0].selected);
    storage::archive_session(remote.path.clone(), "remote-session".into()).unwrap();
    assert!(storage::list_sessions(&remote.path).unwrap()[0].archived);
    storage::unarchive_session(remote.path.clone(), "remote-session".into()).unwrap();
    assert!(!storage::list_sessions(&remote.path).unwrap()[0].archived);
    storage::reorder_projects(vec![remote.path.clone(), summary.path.clone()]).unwrap();
    assert_eq!(
        storage::load_workspace().unwrap().projects[0].path,
        remote.path
    );
    storage::remove_project(remote.path).unwrap();
    storage::remove_project(summary.path).unwrap();
    assert!(storage::load_workspace().unwrap().projects.is_empty());
    assert_eq!(fs::read_to_string(&transcript).unwrap(), body);
    assert!(PathBuf::from(&path).starts_with(root_path));
}
