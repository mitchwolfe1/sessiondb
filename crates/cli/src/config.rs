use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::Utc;
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use sessiondb_client::{Client, TokenProvider, Tokens};
use sessiondb_types::{AuthConfig, Harness, ListOptions, Visibility};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub server: Option<String>,
    pub current_context: Option<String>,
    pub device_label: Option<String>,
    pub contexts: BTreeMap<String, ContextConfig>,
    pub sources: Vec<ConfiguredSource>,
    pub project_rules: Vec<ProjectRule>,
    pub sync: SyncOptions,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    pub server: String,
    pub organization: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredSource {
    pub context: String,
    pub harness: Harness,
    pub root: PathBuf,
    #[serde(default)]
    pub default_visibility: Visibility,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRule {
    pub project: String,
    pub cwd_under: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SyncOptions {
    pub reconcile_interval_seconds: u64,
    pub debounce_seconds: u64,
    pub upload_concurrency: usize,
    pub batch_files: usize,
}
impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            reconcile_interval_seconds: 60,
            debounce_seconds: 2,
            upload_concurrency: 4,
            batch_files: 32,
        }
    }
}
pub fn default_config_path() -> PathBuf {
    ProjectDirs::from("", "", "sessiondb")
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from(".sessiondb/config.toml"))
}
pub fn default_state_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(dir).join("sessiondb");
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(dirs) = BaseDirs::new() {
            return dirs.home_dir().join(".local/state/sessiondb");
        }
    }
    ProjectDirs::from("", "", "sessiondb")
        .map(|d| d.data_local_dir().to_owned())
        .unwrap_or_else(|| PathBuf::from(".sessiondb/state"))
}
pub fn load(path: &Path) -> Result<FileConfig> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).context("parse client configuration"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileConfig::default()),
        Err(e) => Err(e).context("read client configuration"),
    }
}
pub fn private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn save(path: &Path, config: &FileConfig) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if parent != Path::new(".") {
        private_dir(parent)?;
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(toml::to_string_pretty(config)?.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn expand(path: &Path) -> Result<PathBuf> {
    let text = path.to_str().context("native paths must be valid UTF-8")?;
    if text == "~" || text.starts_with("~/") {
        let home = BaseDirs::new().context("home directory is unavailable")?;
        Ok(home
            .home_dir()
            .join(text.trim_start_matches('~').trim_start_matches('/')))
    } else {
        Ok(path.to_owned())
    }
}
pub fn device(config: &mut FileConfig) -> String {
    config
        .device_label
        .get_or_insert_with(|| {
            let host = std::env::var("HOSTNAME")
                .ok()
                .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
                .unwrap_or_else(|| "device".into());
            format!("{}-{}", host.trim(), &Uuid::new_v4().to_string()[..8])
        })
        .clone()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StoredCredentials {
    pub server: String,
    pub issuer: String,
    pub client_id: String,
    pub tokens: Tokens,
}
fn entry(server: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("sessiondb", server).context("open OS credential store")
}
pub async fn store(credentials: StoredCredentials) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        entry(&credentials.server)?
            .set_password(&serde_json::to_string(&credentials)?)
            .context("save credentials in the OS credential store")
    })
    .await?
}
pub async fn remove_credentials(server: String) -> Result<()> {
    tokio::task::spawn_blocking(move || match entry(&server)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("remove OS credentials"),
    })
    .await?
}
pub async fn authenticated(server: &str, timeout: Duration) -> Result<Client> {
    let client = Client::new(server, None, timeout)?;
    if let Ok(token) = std::env::var("SESSIONDB_TOKEN") {
        if token.trim().is_empty() {
            bail!("SESSIONDB_TOKEN is empty");
        }
        return Ok(Client::new(server, Some(token), timeout)?);
    }
    let account = client.server().to_owned();
    let stored: StoredCredentials =
        tokio::task::spawn_blocking(move || -> Result<StoredCredentials> {
            let secret = entry(&account)?
                .get_password()
                .context("sign in with sessiondb auth login, or set SESSIONDB_TOKEN")?;
            serde_json::from_str(&secret)
                .context("read saved credentials; sign in again if they are invalid")
        })
        .await??;
    let auth = client.auth_config().await?;
    if stored.server != client.server()
        || stored.issuer != auth.issuer
        || stored.client_id != auth.cli_client_id
    {
        bail!("the server's identity configuration changed; sign in again");
    }
    Ok(client.with_token_provider(Arc::new(RefreshingCredentials {
        auth,
        stored: Mutex::new(stored),
    })))
}
struct RefreshingCredentials {
    auth: AuthConfig,
    stored: Mutex<StoredCredentials>,
}
#[async_trait]
impl TokenProvider for RefreshingCredentials {
    async fn access_token(&self) -> sessiondb_client::Result<Option<String>> {
        let mut stored = self.stored.lock().await;
        if stored
            .tokens
            .expires_at
            .is_some_and(|t| t <= Utc::now() + chrono::Duration::seconds(30))
        {
            stored.tokens = sessiondb_client::auth::refresh(&self.auth, &stored.tokens).await?;
            store(stored.clone()).await.map_err(|_| {
                sessiondb_client::Error::Authentication(
                    "Could not save refreshed credentials in the OS credential store.".into(),
                )
            })?;
        }
        Ok(Some(stored.tokens.access_token.clone()))
    }
}
pub async fn organization(client: &Client, name: &str) -> Result<Uuid> {
    let mut options = ListOptions::default();
    loop {
        let page = client.organizations(&options).await?;
        if let Some(org) = page
            .data
            .iter()
            .find(|o| o.slug == name || o.id.to_string() == name)
        {
            return Ok(org.id);
        }
        if page.next_cursor.is_none() {
            break;
        }
        options.cursor = page.next_cursor;
    }
    bail!("organization {name} was not found among this account's memberships")
}
