#[cfg(not(test))]
pub use tau_core::profile::app_data_dir;
#[cfg(not(test))]
#[allow(unused_imports)]
pub use tau_core::profile::current;

#[cfg(test)]
pub fn current() -> Result<&'static StorageProfile, String> {
    // Native unit tests have no app startup and must never select production data.
    static INITIALIZED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    INITIALIZED
        .get_or_init(|| {
            if tau_core::profile::current().is_ok() {
                return Ok(());
            }
            tau_core::profile::initialize(StorageProfile::ephemeral()?)
        })
        .as_ref()
        .map_err(Clone::clone)?;
    tau_core::profile::current()
}

#[cfg(test)]
pub fn app_data_dir() -> std::path::PathBuf {
    current()
        .expect("isolated test profile")
        .data_dir()
        .to_path_buf()
}
pub use tau_core::profile::StorageProfile;

#[cfg(dev)]
pub const ENVIRONMENT_NAME: &str = "development";
#[cfg(not(dev))]
pub const ENVIRONMENT_NAME: &str = "production";

pub fn initialize() -> Result<(), String> {
    #[cfg(dev)]
    let profile = StorageProfile::ephemeral()?;
    #[cfg(not(dev))]
    let profile = StorageProfile::production()?;
    tau_core::profile::initialize(profile)?;
    #[cfg(dev)]
    {
        let profile = current()?;
        crate::dev_workspace::seed(profile)?;
        eprintln!(
            "Disposable development storage: {}",
            profile.data_dir().display()
        );
    }
    Ok(())
}

pub fn cleanup() {
    #[cfg(dev)]
    if let Ok(profile) = current() {
        if let Some(root) = profile.temporary_root() {
            // Tauri exits without dropping statics. Stop Pi writers and telemetry first.
            if let Err(error) = std::fs::remove_dir_all(root) {
                eprintln!("Could not remove temporary development storage: {error}");
            }
        }
    }
}
