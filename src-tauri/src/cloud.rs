//! Admin-only direct cloud connections. Only nonsecret configuration is stored on disk.
#[cfg(test)]
mod tests;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::telemetry::{trace_context::TraceContext, transport::Pipeline};

const LINEAR_URL: &str = "https://api.linear.app/graphql";
const MAX_RESPONSE: u64 = 1024 * 1024;
const CONFIG_FILE: &str = "cloud-connections.json";

#[derive(Clone, Debug, Serialize)]
pub struct CloudError {
    pub code: &'static str,
    pub message: &'static str,
}
pub type Result<T> = std::result::Result<T, CloudError>;
pub fn error(code: &'static str) -> CloudError {
    let message = match code {
        "admin_required" => "Enable admin mode to use cloud connections.",
        "authentication" => {
            "The service rejected these credentials. Check the key and its permissions."
        }
        "network" => "The service could not be reached. Check your connection and try again.",
        "invalid" => "The connection or report is invalid. Check the values and try again.",
        "storage" => "The connection could not be stored securely. Reconnect and try again.",
        "not_configured" => "Connect the cloud service before continuing.",
        _ => "The service could not complete this operation. Try again.",
    };
    CloudError { code, message }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinearConnection {
    pub team_id: String,
    pub team_name: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GrafanaConnection {
    pub endpoint: String,
    pub instance_id: String,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    linear: Option<LinearConnection>,
    grafana: Option<GrafanaConnection>,
    #[serde(default)]
    linear_credential: Option<String>,
    #[serde(default)]
    grafana_credential: Option<String>,
}
impl Config {
    fn credential(&self, provider: &str) -> Option<&str> {
        if provider == "linear" {
            self.linear_credential.as_deref()
        } else {
            self.grafana_credential.as_deref()
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudConnections {
    linear: Option<LinearConnection>,
    grafana: Option<GrafanaConnection>,
    telemetry_enabled: bool,
}
#[derive(Deserialize, Serialize)]
pub struct ProjectOption {
    id: String,
    name: String,
}
#[derive(Serialize)]
pub struct TeamOption {
    id: String,
    name: String,
    projects: Vec<ProjectOption>,
}
#[derive(Serialize)]
pub struct LinearOptions {
    teams: Vec<TeamOption>,
}

// Never derive Debug on credential-bearing types.
#[derive(Clone)]
pub struct Target {
    pub endpoint: String,
    pub instance_id: String,
    pub token: String,
}
pub struct Request {
    pub url: String,
    pub authorization: Authorization,
    pub body: Value,
}
pub enum Authorization {
    Linear(String),
    Basic(String, String),
}
impl Request {
    pub fn otlp(target: &Target, signal: &str, body: Value) -> Self {
        Self {
            url: format!("{}/v1/{signal}", target.endpoint),
            authorization: Authorization::Basic(target.instance_id.clone(), target.token.clone()),
            body,
        }
    }
}
pub trait Http: Send + Sync {
    fn send(&self, request: Request) -> Result<Value>;
}
pub struct Network;
impl Http for Network {
    fn send(&self, request: Request) -> Result<Value> {
        // Initialized only on background threads; reuse TLS connections across small batches.
        static CLIENT: OnceLock<std::result::Result<reqwest::blocking::Client, ()>> =
            OnceLock::new();
        let client = CLIENT
            .get_or_init(|| {
                reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(8))
                    .connect_timeout(Duration::from_secs(3))
                    .redirect(reqwest::redirect::Policy::none())
                    .https_only(true)
                    .build()
                    .map_err(|_| ())
            })
            .as_ref()
            .map_err(|_| error("network"))?;
        let builder = client.post(request.url).json(&request.body);
        let builder = match request.authorization {
            Authorization::Linear(key) => builder.header(reqwest::header::AUTHORIZATION, key),
            Authorization::Basic(id, token) => builder.basic_auth(id, Some(token)),
        };
        let response = builder.send().map_err(|_| error("network"))?;
        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(error("authentication"));
        }
        if !status.is_success() {
            return Err(error("unavailable"));
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error("network"))?;
        if bytes.len() as u64 > MAX_RESPONSE {
            return Err(error("unavailable"));
        }
        if bytes.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_slice(&bytes).map_err(|_| error("unavailable"))
    }
}

trait Secrets: Send + Sync {
    fn get(&self, provider: &str) -> Result<Option<String>>;
    fn set(&self, provider: &str, value: &str) -> Result<()>;
    fn delete(&self, provider: &str) -> Result<()>;
}
#[cfg(any(dev, test))]
#[derive(Default)]
struct MemorySecrets(Mutex<std::collections::HashMap<String, String>>);
#[cfg(any(dev, test))]
impl Secrets for MemorySecrets {
    fn get(&self, provider: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| error("storage"))?
            .get(provider)
            .cloned())
    }
    fn set(&self, provider: &str, value: &str) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| error("storage"))?
            .insert(provider.into(), value.into());
        Ok(())
    }
    fn delete(&self, provider: &str) -> Result<()> {
        self.0
            .lock()
            .map_err(|_| error("storage"))?
            .remove(provider);
        Ok(())
    }
}
#[cfg(not(any(dev, test)))]
struct NativeSecrets;
#[cfg(all(target_os = "macos", not(any(dev, test))))]
impl Secrets for NativeSecrets {
    fn get(&self, provider: &str) -> Result<Option<String>> {
        match security_framework::passwords::get_generic_password("app.tau.cloud", provider) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| error("storage")),
            Err(e) if e.code() == -25300 => Ok(None),
            Err(_) => Err(error("storage")),
        }
    }
    fn set(&self, provider: &str, value: &str) -> Result<()> {
        security_framework::passwords::set_generic_password(
            "app.tau.cloud",
            provider,
            value.as_bytes(),
        )
        .map_err(|_| error("storage"))
    }
    fn delete(&self, provider: &str) -> Result<()> {
        match security_framework::passwords::delete_generic_password("app.tau.cloud", provider) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(_) => Err(error("storage")),
        }
    }
}
#[cfg(all(not(target_os = "macos"), not(any(dev, test))))]
impl Secrets for NativeSecrets {
    fn get(&self, _: &str) -> Result<Option<String>> {
        Err(error("storage"))
    }
    fn set(&self, _: &str, _: &str) -> Result<()> {
        Err(error("storage"))
    }
    fn delete(&self, _: &str) -> Result<()> {
        Err(error("storage"))
    }
}

struct Inner {
    admin: bool,
    epoch: u64,
    config: Config,
    storage_failed: bool,
}
struct Shared {
    inner: Mutex<Inner>,
    secrets: Box<dyn Secrets>,
    http: Arc<dyn Http>,
    pipeline: Arc<Pipeline>,
    directory: PathBuf,
}
#[derive(Clone)]
pub struct CloudState(Arc<Shared>);
impl CloudState {
    pub fn init(pipeline: Arc<Pipeline>) -> Self {
        #[cfg(any(dev, test))]
        let secrets: Box<dyn Secrets> = Box::<MemorySecrets>::default();
        #[cfg(not(any(dev, test)))]
        let secrets: Box<dyn Secrets> = Box::new(NativeSecrets);
        Self::new(
            crate::profile::app_data_dir(),
            crate::admin::admin_mode_enabled(),
            pipeline,
            secrets,
            Arc::new(Network),
        )
    }
    fn new(
        directory: PathBuf,
        admin: bool,
        pipeline: Arc<Pipeline>,
        secrets: Box<dyn Secrets>,
        http: Arc<dyn Http>,
    ) -> Self {
        let (config, storage_failed) = match fs::read(directory.join(CONFIG_FILE)) {
            Ok(bytes) if bytes.len() <= 64 * 1024 => match serde_json::from_slice(&bytes) {
                Ok(config) => (config, false),
                Err(_) => (Config::default(), true),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), false),
            _ => (Config::default(), true),
        };
        let state = Self(Arc::new(Shared {
            inner: Mutex::new(Inner {
                admin,
                epoch: 0,
                config,
                storage_failed,
            }),
            secrets,
            http,
            pipeline,
            directory,
        }));
        if let Ok(mut inner) = state.0.inner.lock() {
            let _ = state.refresh(&mut inner);
        }
        state
    }
    pub fn admin_enabled(&self) -> bool {
        self.0.inner.lock().is_ok_and(|inner| inner.admin)
    }
    pub fn set_admin(&self, enabled: bool) {
        if let Ok(mut inner) = self.0.inner.lock() {
            inner.admin = enabled;
            inner.epoch = inner.epoch.wrapping_add(1);
            self.0.pipeline.configure(None);
            let _ = self.refresh(&mut inner);
        } else {
            self.0.pipeline.configure(None);
        }
    }
    fn gate(inner: &Inner) -> Result<()> {
        if !inner.admin {
            return Err(error("admin_required"));
        }
        if inner.storage_failed {
            return Err(error("storage"));
        }
        Ok(())
    }
    fn begin(&self) -> Result<u64> {
        let inner = self.0.inner.lock().map_err(|_| error("storage"))?;
        Self::gate(&inner)?;
        Ok(inner.epoch)
    }
    fn current(inner: &Inner, epoch: u64) -> Result<()> {
        Self::gate(inner)?;
        if inner.epoch != epoch {
            return Err(error("unavailable"));
        }
        Ok(())
    }
    fn refresh(&self, inner: &mut Inner) -> Result<()> {
        let mut target = None;
        if inner.admin && !inner.storage_failed {
            if let Some(config) = &inner.config.grafana {
                match self.credential(&inner.config, "grafana") {
                    Ok(Some(token)) => {
                        let endpoint = normalize_endpoint(&config.endpoint)?;
                        validate_instance(&config.instance_id)?;
                        validate_key(&token)?;
                        target = Some(Target {
                            endpoint,
                            instance_id: config.instance_id.clone(),
                            token,
                        });
                    }
                    Ok(None) => inner.config.grafana = None,
                    Err(e) => {
                        self.0.pipeline.configure(None);
                        return Err(e);
                    }
                }
            }
        }
        self.0.pipeline.configure(target);
        Ok(())
    }
    fn snapshot(&self, inner: &mut Inner) -> Result<CloudConnections> {
        Self::gate(inner)?;
        // An inaccessible provider must not block the other connection.
        let mut linear = inner.config.linear.clone();
        let mut grafana = inner.config.grafana.clone();
        for provider in ["linear", "grafana"] {
            let configured = if provider == "linear" {
                inner.config.linear.is_some()
            } else {
                inner.config.grafana.is_some()
            };
            if !configured {
                continue;
            }
            match self.credential(&inner.config, provider) {
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {
                    if provider == "linear" {
                        linear = None;
                    } else {
                        grafana = None;
                        self.0.pipeline.configure(None);
                    }
                }
            }
        }
        if grafana.is_some() && !self.0.pipeline.is_enabled() && self.refresh(inner).is_err() {
            grafana = None;
        }
        Ok(CloudConnections {
            linear,
            grafana,
            telemetry_enabled: self.0.pipeline.is_enabled(),
        })
    }
    fn credential(&self, config: &Config, provider: &str) -> Result<Option<String>> {
        match config.credential(provider) {
            Some(account) => self.0.secrets.get(account),
            None => Ok(None),
        }
    }
    fn persist(&self, config: &Config) -> Result<()> {
        fs::create_dir_all(&self.0.directory).map_err(|_| error("storage"))?;
        let mut file =
            tempfile::NamedTempFile::new_in(&self.0.directory).map_err(|_| error("storage"))?;
        serde_json::to_writer(file.as_file_mut(), config).map_err(|_| error("storage"))?;
        file.as_file().sync_all().map_err(|_| error("storage"))?;
        file.persist(self.0.directory.join(CONFIG_FILE))
            .map_err(|_| error("storage"))?;
        Ok(())
    }
    fn save(
        &self,
        epoch: u64,
        provider: &str,
        config: Config,
        key: &str,
    ) -> Result<CloudConnections> {
        let mut inner = self.0.inner.lock().map_err(|_| error("storage"))?;
        Self::current(&inner, epoch)?;
        let mut next = inner.config.clone();
        // Immutable credential accounts make the atomic config rename the commit point.
        // A failed write never changes the credential paired with the previous config.
        let account = format!("{provider}-{}", uuid::Uuid::new_v4());
        let old_account = next.credential(provider).map(str::to_owned);
        if provider == "linear" {
            next.linear = config.linear;
            next.linear_credential = Some(account.clone());
        } else {
            next.grafana = config.grafana;
            next.grafana_credential = Some(account.clone());
        }
        self.0.secrets.set(&account, key)?;
        if self.persist(&next).is_err() {
            let _ = self.0.secrets.delete(&account);
            return Err(error("storage"));
        }
        inner.config = next;
        if let Some(old) = old_account {
            let _ = self.0.secrets.delete(&old);
        }
        inner.epoch = inner.epoch.wrapping_add(1);
        if provider == "grafana" {
            self.refresh(&mut inner)?;
        }
        self.snapshot(&mut inner)
    }
    fn read(&self) -> Result<CloudConnections> {
        let mut inner = self.0.inner.lock().map_err(|_| error("storage"))?;
        self.snapshot(&mut inner)
    }
    fn disconnect(&self, provider: &str) -> Result<CloudConnections> {
        let mut inner = self.0.inner.lock().map_err(|_| error("storage"))?;
        Self::gate(&inner)?;
        if provider != "linear" && provider != "grafana" {
            return Err(error("invalid"));
        }
        inner.epoch = inner.epoch.wrapping_add(1);
        if provider == "grafana" {
            self.0.pipeline.configure(None);
        }
        let mut next = inner.config.clone();
        let old_account = next.credential(provider).map(str::to_owned);
        if provider == "linear" {
            next.linear = None;
            next.linear_credential = None;
        } else {
            next.grafana = None;
            next.grafana_credential = None;
        }
        // Clear config first: if deleting a credential fails, it cannot be used at next launch.
        self.persist(&next)?;
        inner.config = next;
        if let Some(account) = old_account {
            self.0.secrets.delete(&account)?;
        }
        self.snapshot(&mut inner)
    }
    fn graphql(&self, key: &str, query: &str, variables: Value) -> Result<Value> {
        let response = self.0.http.send(Request {
            url: LINEAR_URL.into(),
            authorization: Authorization::Linear(key.into()),
            body: json!({"query":query,"variables":variables}),
        })?;
        if response
            .get("errors")
            .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
        {
            let authentication = response["errors"].as_array().is_some_and(|errors| {
                errors.iter().any(|e| {
                    matches!(
                        e["extensions"]["code"].as_str(),
                        Some("AUTHENTICATION_ERROR" | "UNAUTHENTICATED" | "FORBIDDEN")
                    )
                })
            });
            return Err(error(if authentication {
                "authentication"
            } else {
                "unavailable"
            }));
        }
        response
            .get("data")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or_else(|| error("unavailable"))
    }
    fn options(&self, key: &str) -> Result<LinearOptions> {
        validate_key(key)?;
        let data = self.graphql(key, "query { teams(first: 100) { nodes { id name projects(first: 100) { nodes { id name } pageInfo { hasNextPage } } } pageInfo { hasNextPage } } }", json!({}))?;
        let teams = connection_nodes(&data["teams"])?;
        let mut options = Vec::new();
        for team in teams {
            let projects = connection_nodes(&team["projects"])?;
            let projects = projects
                .iter()
                .map(|p| {
                    Ok(ProjectOption {
                        id: field(p, "id")?,
                        name: field(p, "name")?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            options.push(TeamOption {
                id: field(team, "id")?,
                name: field(team, "name")?,
                projects,
            });
        }
        Ok(LinearOptions { teams: options })
    }
    fn validate_linear(&self, key: String) -> Result<LinearOptions> {
        let epoch = self.begin()?;
        let options = self.options(&key)?;
        let inner = self.0.inner.lock().map_err(|_| error("storage"))?;
        Self::current(&inner, epoch)?;
        Ok(options)
    }
    fn save_linear(
        &self,
        key: String,
        team_id: String,
        project_id: Option<String>,
    ) -> Result<CloudConnections> {
        let epoch = self.begin()?;
        let options = self.options(&key)?;
        let team = options
            .teams
            .into_iter()
            .find(|t| t.id == team_id)
            .ok_or_else(|| error("invalid"))?;
        let project_name = match &project_id {
            Some(id) => Some(
                team.projects
                    .iter()
                    .find(|p| &p.id == id)
                    .ok_or_else(|| error("invalid"))?
                    .name
                    .clone(),
            ),
            None => None,
        };
        self.save(
            epoch,
            "linear",
            Config {
                linear: Some(LinearConnection {
                    team_id,
                    team_name: team.name,
                    project_id,
                    project_name,
                }),
                ..Config::default()
            },
            &key,
        )
    }
    fn save_grafana(
        &self,
        endpoint: String,
        instance_id: String,
        token: String,
    ) -> Result<CloudConnections> {
        let epoch = self.begin()?;
        let endpoint = normalize_endpoint(&endpoint)?;
        validate_instance(&instance_id)?;
        validate_key(&token)?;
        let target = Target {
            endpoint: endpoint.clone(),
            instance_id: instance_id.clone(),
            token: token.clone(),
        };
        for (signal, resource) in [
            ("logs", "resourceLogs"),
            ("traces", "resourceSpans"),
            ("metrics", "resourceMetrics"),
        ] {
            self.begin_current(epoch)?;
            let response =
                self.0
                    .http
                    .send(Request::otlp(&target, signal, json!({resource: []})))?;
            validate_otlp_response(&response)?;
        }
        self.save(
            epoch,
            "grafana",
            Config {
                linear: None,
                grafana: Some(GrafanaConnection {
                    endpoint,
                    instance_id,
                }),
                ..Config::default()
            },
            &token,
        )
    }
    fn begin_current(&self, epoch: u64) -> Result<()> {
        let inner = self.0.inner.lock().map_err(|_| error("storage"))?;
        Self::current(&inner, epoch)
    }
}
fn connection_nodes(value: &Value) -> Result<&Vec<Value>> {
    // Explicitly fail rather than silently omitting destinations in large accounts.
    if value["pageInfo"]["hasNextPage"] != false {
        return Err(error("unavailable"));
    }
    value["nodes"]
        .as_array()
        .ok_or_else(|| error("unavailable"))
}
fn field(value: &Value, key: &str) -> Result<String> {
    let text = value[key].as_str().ok_or_else(|| error("unavailable"))?;
    if text.is_empty() || text.len() > 4096 {
        return Err(error("unavailable"));
    }
    Ok(text.into())
}
fn validate_key(key: &str) -> Result<()> {
    if key.is_empty()
        || key.len() > 8192
        || key
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return Err(error("invalid"));
    }
    Ok(())
}
fn validate_instance(instance: &str) -> Result<()> {
    if instance.is_empty() || instance.len() > 128 || !instance.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(error("invalid"));
    }
    Ok(())
}
pub fn normalize_endpoint(endpoint: &str) -> Result<String> {
    if endpoint.len() > 2048 {
        return Err(error("invalid"));
    }
    let url = reqwest::Url::parse(endpoint).map_err(|_| error("invalid"))?;
    let has_userinfo = endpoint.split_once("://").is_some_and(|(_, rest)| {
        rest.split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    });
    if has_userinfo
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(error("invalid"));
    }
    let path = url.path().trim_end_matches('/');
    if path.ends_with("/v1/logs") || path.ends_with("/v1/traces") || path.ends_with("/v1/metrics") {
        return Err(error("invalid"));
    }
    Ok(url.as_str().trim_end_matches('/').into())
}
pub fn validate_otlp_response(response: &Value) -> Result<()> {
    if !response.is_object() {
        return Err(error("unavailable"));
    }
    if let Some(partial) = response.get("partialSuccess") {
        if !partial.is_object() {
            return Err(error("unavailable"));
        }
        for (key, value) in partial.as_object().ok_or_else(|| error("unavailable"))? {
            if key == "errorMessage" {
                if value.as_str() != Some("") {
                    return Err(error("unavailable"));
                }
            } else if !matches!(value.as_str(), Some("0")) && value.as_u64() != Some(0) {
                return Err(error("unavailable"));
            }
        }
    }
    Ok(())
}

async fn background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| error("unavailable"))?
}
#[tauri::command]
pub async fn read_cloud_connections(
    cloud: State<'_, CloudState>,
    telemetry_context: Option<TraceContext>,
) -> Result<CloudConnections> {
    let _ = telemetry_context;
    let cloud = cloud.inner().clone();
    background(move || cloud.read()).await
}
#[tauri::command]
pub async fn validate_linear_connection(
    cloud: State<'_, CloudState>,
    api_key: String,
    telemetry_context: Option<TraceContext>,
) -> Result<LinearOptions> {
    let _ = telemetry_context;
    let cloud = cloud.inner().clone();
    background(move || cloud.validate_linear(api_key)).await
}
#[tauri::command]
pub async fn save_linear_connection(
    cloud: State<'_, CloudState>,
    api_key: String,
    team_id: String,
    project_id: Option<String>,
    telemetry_context: Option<TraceContext>,
) -> Result<CloudConnections> {
    let _ = telemetry_context;
    let cloud = cloud.inner().clone();
    background(move || cloud.save_linear(api_key, team_id, project_id)).await
}
#[tauri::command]
pub async fn save_grafana_connection(
    cloud: State<'_, CloudState>,
    endpoint: String,
    instance_id: String,
    token: String,
    telemetry_context: Option<TraceContext>,
) -> Result<CloudConnections> {
    let _ = telemetry_context;
    let cloud = cloud.inner().clone();
    background(move || cloud.save_grafana(endpoint, instance_id, token)).await
}
#[tauri::command]
pub async fn disconnect_cloud_connection(
    cloud: State<'_, CloudState>,
    provider: String,
    telemetry_context: Option<TraceContext>,
) -> Result<CloudConnections> {
    let _ = telemetry_context;
    let cloud = cloud.inner().clone();
    background(move || cloud.disconnect(&provider)).await
}

#[derive(Serialize)]
pub struct IssueReceipt {
    pub id: String,
    pub identifier: String,
    pub url: String,
}
impl CloudState {
    pub async fn submit(
        &self,
        description: String,
        session_id: Option<String>,
        report_id: String,
        time_unix_nano: String,
    ) -> Result<IssueReceipt> {
        let cloud = self.clone();
        background(move || cloud.submit_inner(description, session_id, report_id, time_unix_nano))
            .await
    }
    fn submit_inner(
        &self,
        description: String,
        session_id: Option<String>,
        report_id: String,
        time_unix_nano: String,
    ) -> Result<IssueReceipt> {
        let epoch = self.begin()?;
        let uuid = uuid::Uuid::parse_str(&report_id).map_err(|_| error("invalid"))?;
        if uuid.get_version_num() != 4
            || uuid.to_string() != report_id
            || description.trim().is_empty()
            || description.chars().count() > 10_000
            || session_id.as_ref().is_some_and(|s| {
                s.is_empty()
                    || s.len() > 256
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
        {
            return Err(error("invalid"));
        }
        let nanos = time_unix_nano
            .parse::<u64>()
            .map_err(|_| error("invalid"))?;
        if nanos == 0 || nanos.to_string() != time_unix_nano {
            return Err(error("invalid"));
        }
        let timestamp_utc = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(nanos))
            .map_err(|_| error("invalid"))?
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| error("invalid"))?;
        let (config, key) = {
            let inner = self.0.inner.lock().map_err(|_| error("storage"))?;
            Self::current(&inner, epoch)?;
            (
                inner
                    .config
                    .linear
                    .clone()
                    .ok_or_else(|| error("not_configured"))?,
                self.credential(&inner.config, "linear")?
                    .ok_or_else(|| error("not_configured"))?,
            )
        };
        let first_line = description.lines().next().unwrap_or_default().trim();
        let title = if first_line.is_empty() {
            "Tau issue report"
        } else {
            first_line
        }
        .chars()
        .take(160)
        .collect::<String>();
        let session = session_id
            .as_ref()
            .map(|id| format!("\nSession ID (opt-in): {id}"))
            .unwrap_or_default();
        let body = format!("{description}\n\n---\nTau issue report\nReport ID: {report_id}\nOriginal timestamp (Unix ns): {time_unix_nano}\nOriginal timestamp (UTC): {timestamp_utc}\nTau version: {}\nEnvironment: {}{session}", env!("CARGO_PKG_VERSION"), crate::profile::ENVIRONMENT_NAME);
        // Verified in @linear/sdk 100.0.0 dist/index-BbHxGaKh.d.mts: IssueCreateInput.id
        // accepts UUID v4, and IssueFilter.id uses IdComparator (GraphQL ID).
        // Query before every attempt; report metadata + destination prevent unrelated reuse.
        if let Some(receipt) = self.existing_issue(&key, &report_id, &body, &config)? {
            self.begin_current(epoch)?;
            return Ok(receipt);
        }
        self.begin_current(epoch)?;
        let input = json!({"id":report_id,"title":title,"description":body,"teamId":config.team_id,"projectId":config.project_id});
        let created = self.graphql(&key, "mutation($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { id identifier url } } }", json!({"input":input}));
        self.begin_current(epoch)?;
        match created {
            Ok(data) if data["issueCreate"]["success"] == true => {
                receipt(&data["issueCreate"]["issue"], &report_id)
            }
            result => {
                // A timed out create may have succeeded. Never retry the mutation blindly.
                if let Some(receipt) = self.existing_issue(&key, &report_id, &body, &config)? {
                    self.begin_current(epoch)?;
                    return Ok(receipt);
                }
                Err(result.err().unwrap_or_else(|| error("unavailable")))
            }
        }
    }
    fn existing_issue(
        &self,
        key: &str,
        report_id: &str,
        body: &str,
        config: &LinearConnection,
    ) -> Result<Option<IssueReceipt>> {
        // A filter returns an empty connection when the UUID does not exist (issue(id:) can error).
        let data = self.graphql(key, "query($id: ID!) { issues(first: 2, includeArchived: true, filter: { id: { eq: $id } }) { nodes { id identifier url description team { id } project { id } } pageInfo { hasNextPage } } }", json!({"id":report_id}))?;
        let nodes = connection_nodes(&data["issues"])?;
        if nodes.is_empty() {
            return Ok(None);
        }
        if nodes.len() != 1 {
            return Err(error("invalid"));
        }
        let issue = &nodes[0];
        let project = issue["project"]["id"].as_str();
        if !issue["description"]
            .as_str()
            .is_some_and(|actual| report_matches(actual, body))
            || issue["team"]["id"].as_str() != Some(config.team_id.as_str())
            || project != config.project_id.as_deref()
        {
            return Err(error("invalid"));
        }
        Ok(Some(receipt(issue, report_id)?))
    }
}
fn report_matches(actual: &str, expected: &str) -> bool {
    fn metadata(body: &str) -> Option<(&str, &str)> {
        if body.len() > 64 * 1024 {
            return None;
        }
        let (stable, runtime) = body.rsplit_once("\nTau version: ")?;
        let (version, remainder) = runtime.split_once("\nEnvironment: ")?;
        if version.is_empty()
            || version.len() > 128
            || !version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
        {
            return None;
        }
        let (environment, session) = remainder.split_once('\n').unwrap_or((remainder, ""));
        if !matches!(environment, "production" | "development") {
            return None;
        }
        Some((stable, session))
    }
    // Upgrading or moving the draft between environments must not create another issue.
    matches!((metadata(actual), metadata(expected)), (Some(a), Some(b)) if a == b)
}
fn receipt(issue: &Value, report_id: &str) -> Result<IssueReceipt> {
    let id = field(issue, "id")?;
    if id != report_id {
        return Err(error("invalid"));
    }
    let url = field(issue, "url")?;
    let parsed = reqwest::Url::parse(&url).map_err(|_| error("unavailable"))?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("linear.app")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(error("unavailable"));
    }
    Ok(IssueReceipt {
        id,
        identifier: field(issue, "identifier")?,
        url,
    })
}
