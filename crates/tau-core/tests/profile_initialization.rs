use tau_core::{profile, storage};

#[test]
fn services_before_initialization_fail_without_selecting_production() {
    assert!(profile::current().is_err());
    assert!(storage::load_workspace().is_err());
    assert!(profile::session_registry_filename().is_err());

    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    profile::initialize(profile::StorageProfile::isolated(
        data.clone(),
        root.path().join("sessions"),
    ))
    .unwrap();
    assert_eq!(profile::current().unwrap().data_dir(), data);
    assert!(storage::load_workspace().is_ok());
    assert!(profile::initialize(profile::StorageProfile::persistent(
        root.path().join("other"),
    ))
    .is_err());
    assert_eq!(profile::current().unwrap().data_dir(), data);
}
