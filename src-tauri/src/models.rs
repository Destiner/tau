use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRegistry {
    #[serde(default = "registry_version")]
    pub version: u8,
    #[serde(default)]
    pub active_project_path: String,
    #[serde(default)]
    pub projects: Vec<ProjectRecord>,
}

impl Default for ProjectRegistry {
    fn default() -> Self {
        Self {
            version: registry_version(),
            active_project_path: String::new(),
            projects: Vec::new(),
        }
    }
}

fn registry_version() -> u8 {
    1
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub path: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteProjectRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProjectRecord {
    pub connection_string: String,
    pub working_directory: String,
    pub host: String,
    #[serde(default)]
    pub active_session_id: String,
    #[serde(default)]
    pub sessions: Vec<RemoteSessionRecord>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirectoryEntry {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDirectoryListing {
    pub connection_string: String,
    pub working_directory: String,
    pub host: String,
    pub directories: Vec<RemoteDirectoryEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSessionRecord {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub last_active: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub active_project_path: String,
    pub pi_path: Option<String>,
    pub projects: Vec<ProjectSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub path: String,
    pub name: String,
    pub working_directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_string: Option<String>,
    pub collapsed: bool,
    pub selected: bool,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub path: String,
    pub title: String,
    pub last_active: String,
    pub last_user_message_at: u64,
    pub archived: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TauSessionRegistry {
    #[serde(default = "registry_version")]
    pub version: u8,
    #[serde(default)]
    pub active_session_id: String,
    #[serde(default)]
    pub sessions: Vec<TauSessionRecord>,
}

impl Default for TauSessionRegistry {
    fn default() -> Self {
        Self {
            version: registry_version(),
            active_session_id: String::new(),
            sessions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TauSessionRecord {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub archived: bool,
}
