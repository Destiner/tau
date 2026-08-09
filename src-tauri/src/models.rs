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
