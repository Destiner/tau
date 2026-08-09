#[cfg(dev)]
pub const APP_DIRECTORY_NAME: &str = "tau-dev";
#[cfg(not(dev))]
pub const APP_DIRECTORY_NAME: &str = "tau";

#[cfg(dev)]
pub const SESSION_REGISTRY_FILENAME: &str = ".tau-dev.json";
#[cfg(not(dev))]
pub const SESSION_REGISTRY_FILENAME: &str = ".tau.json";
