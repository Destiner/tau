use super::*;
use tau_core::{profile, storage};

#[test]
fn delayed_registration_and_retry_preserve_authoritative_names_and_predecessors_on_disk() {
    profile::initialize(profile::StorageProfile::ephemeral().unwrap()).unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let project = project_dir
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    storage::import_project(project.clone()).unwrap();
    let directory = profile::current()
        .unwrap()
        .session_dir(&project, std::path::Path::new("/unused"));
    std::fs::create_dir_all(&directory).unwrap();
    let a = directory.join("a.jsonl");
    let b = directory.join("b.jsonl");
    for (file, id) in [(&a, "a"), (&b, "b")] {
        std::fs::write(file, format!("{{\"type\":\"session\",\"id\":\"{id}\"}}\n")).unwrap();
    }
    let a = a.to_string_lossy().into_owned();
    let b = b.to_string_lossy().into_owned();
    let mut names = SessionNames::default();
    names
        .register(project.clone(), "a".into(), a.clone(), "Old A".into(), true)
        .unwrap();
    names
        .store(
            project.clone(),
            "a".into(),
            a.clone(),
            "Authoritative A".into(),
        )
        .unwrap();
    // Captured registrations and their retries must still adopt history, not its old name.
    for _ in 0..2 {
        names
            .register(project.clone(), "a".into(), a.clone(), "Old A".into(), true)
            .unwrap();
    }
    names
        .register(project.clone(), "b".into(), b.clone(), "B".into(), true)
        .unwrap();
    // Replacing the runtime identity does not discard A's registration barrier.
    names
        .register(project.clone(), "a".into(), a.clone(), "Old A".into(), true)
        .unwrap();
    let workspace = storage::load_workspace().unwrap();
    let sessions = &workspace
        .projects
        .iter()
        .find(|p| p.path == project)
        .unwrap()
        .sessions;
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions.iter().find(|s| s.id == "a").unwrap().title,
        "Authoritative A"
    );
    assert_eq!(sessions.iter().find(|s| s.id == "b").unwrap().title, "B");
    assert!(
        !names
            .authoritative
            .contains_key(&(project.clone(), "a".into(), b))
    );
}
