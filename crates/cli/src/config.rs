use anyhow::{Context, Result, bail};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use sessiondb_client::Client;
use sessiondb_types::Harness;
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
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
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredSource {
    pub context: String,
    pub harness: Harness,
    pub root: PathBuf,
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

pub fn normalize_server(server: &str) -> Result<String> {
    let mut url = url::Url::parse(server).map_err(|_| anyhow::anyhow!("invalid server URL"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        || !matches!(url.path(), "" | "/")
        || !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
    {
        bail!("use an HTTPS server origin, or loopback HTTP, without credentials");
    }
    url.set_path("/");
    Ok(url.into())
}

fn entry(server: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("sessiondb-api-key", server)
        .map_err(|_| anyhow::anyhow!("could not open OS credential store"))
}
pub async fn store(server: String, api_key: String) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        entry(&server)?
            .set_password(&api_key)
            .map_err(|_| anyhow::anyhow!("could not save API key in OS credential store"))
    })
    .await?
}
pub async fn remove_credentials(server: String) -> Result<()> {
    tokio::task::spawn_blocking(move || match entry(&server)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => bail!("could not remove API key from OS credential store"),
    })
    .await?
}
pub async fn authenticated(server: &str, timeout: Duration) -> Result<Client> {
    let server = normalize_server(server)?;
    let api_key = match std::env::var("SESSIONDB_API_KEY") {
        Ok(key) => key,
        Err(std::env::VarError::NotUnicode(_)) => bail!("SESSIONDB_API_KEY must be valid UTF-8"),
        Err(std::env::VarError::NotPresent) => {
            let account = server.clone();
            tokio::task::spawn_blocking(move || {
                entry(&account)?.get_password().map_err(|_| {
                    anyhow::anyhow!(
                        "set SESSIONDB_API_KEY or save a key with sessiondb auth login --key-stdin"
                    )
                })
            })
            .await??
        }
    };
    Ok(Client::new(&server, api_key, timeout)?)
}
