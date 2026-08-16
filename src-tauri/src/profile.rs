#[cfg(dev)]
pub const APP_DIRECTORY_NAME: &str = "tau-dev";
#[cfg(not(dev))]
pub const APP_DIRECTORY_NAME: &str = "tau";

#[cfg(dev)]
pub const SESSION_REGISTRY_FILENAME: &str = ".tau-dev.json";
#[cfg(not(dev))]
pub const SESSION_REGISTRY_FILENAME: &str = ".tau.json";

/// The OTel `deployment.environment.name` resource attribute value.
#[cfg(dev)]
pub const ENVIRONMENT_NAME: &str = "development";
#[cfg(not(dev))]
pub const ENVIRONMENT_NAME: &str = "production";
