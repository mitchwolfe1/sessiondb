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

#[derive(Default, Serialize)]
pub struct FileConfig {
    pub server: Option<String>,
    pub device_label: Option<String>,
    pub sources: Vec<ConfiguredSource>,
    pub project_rules: Vec<ProjectRule>,
    pub sync: SyncOptions,
}
#[derive(PartialEq, Eq, Serialize)]
pub struct ConfiguredSource {
    pub server: String,
    pub harness: Harness,
    pub root: PathBuf,
}

// Only the disk reader knows about names used by older CLI releases.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StoredConfig {
    server: Option<String>,
    device_label: Option<String>,
    sources: Vec<StoredSource>,
    project_rules: Vec<ProjectRule>,
    sync: SyncOptions,
    current_context: Option<String>,
    contexts: BTreeMap<String, LegacyContext>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyContext {
    server: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSource {
    server: Option<String>,
    context: Option<String>,
    harness: Harness,
    root: PathBuf,
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
    pub upload_concurrency: usize,
}
impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            reconcile_interval_seconds: 60,
            upload_concurrency: 4,
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
pub async fn load(
    path: &Path,
    state_dir: &Path,
    explicit_server: Option<&str>,
) -> Result<FileConfig> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileConfig::default()),
        Err(e) => return Err(e).context("read client configuration"),
    };
    let stored: StoredConfig = toml::from_str(&text).context("parse client configuration")?;
    let selected = stored.current_context.as_deref().unwrap_or("default");
    let default_url = stored
        .contexts
        .get(selected)
        .map(|c| c.server.as_str())
        .or(stored.server.as_deref());
    let server = default_url.map(normalize_server).transpose()?;
    let mut sources = Vec::new();
    for source in &stored.sources {
        for url in stored
            .source_urls(source, state_dir, explicit_server)
            .await?
        {
            let entry = ConfiguredSource {
                server: normalize_server(&url)?,
                harness: source.harness,
                root: source.root.clone(),
            };
            if !sources.contains(&entry) {
                sources.push(entry);
            }
        }
    }
    Ok(FileConfig {
        server,
        device_label: stored.device_label,
        sources,
        project_rules: stored.project_rules,
        sync: stored.sync,
    })
}

impl StoredConfig {
    async fn source_urls(
        &self,
        source: &StoredSource,
        state_dir: &Path,
        explicit_server: Option<&str>,
    ) -> Result<Vec<String>> {
        let name = match (&source.server, source.context.as_deref()) {
            (Some(server), None) => return Ok(vec![server.clone()]),
            (None, Some(name)) => name,
            _ => bail!("source {} needs one server URL", source.root.display()),
        };
        if let Some(context) = self.contexts.get(name) {
            return Ok(vec![context.server.clone()]);
        }
        if name == self.current_context.as_deref().unwrap_or("default")
            && let Some(url) = &self.server
        {
            return Ok(vec![url.clone()]);
        }
        // Older clients could register roots using only --server.
        let registered =
            registered_servers(state_dir, self.device_label.as_deref(), source).await?;
        if !registered.is_empty() {
            return Ok(registered);
        }
        if name == "default"
            && self.contexts.is_empty()
            && self.current_context.is_none()
            && let Some(url) = explicit_server
        {
            return Ok(vec![url.to_owned()]);
        }
        bail!(
            "set server to the original URL in the source entry for {} and remove its old context field",
            source.root.display()
        )
    }
}

async fn registered_servers(
    state_dir: &Path,
    device: Option<&str>,
    source: &StoredSource,
) -> Result<Vec<String>> {
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    let path = state_dir.join("state.sqlite");
    if !path.try_exists()? {
        return Ok(Vec::new());
    }
    let mut db =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(path).read_only(true))
            .await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='registrations' AND type='table')",
    )
    .fetch_one(&mut db)
    .await?;
    if !exists {
        return Ok(Vec::new());
    }
    let root = expand(&source.root)?;
    let canonical = root.canonicalize().unwrap_or_else(|_| root.clone());
    let urls = sqlx::query_scalar("SELECT DISTINCT server FROM registrations WHERE harness=? AND root IN (?,?) AND (? IS NULL OR device=?) ORDER BY server")
        .bind(source.harness.as_str()).bind(root.to_str().context("source root must be UTF-8")?)
        .bind(canonical.to_str().context("source root must be UTF-8")?).bind(device).bind(device)
        .fetch_all(&mut db).await?;
    Ok(urls)
}

pub fn write_lock(path: &Path) -> Result<std::fs::File> {
    if let Some(parent) = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
    {
        private_dir(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_added_extension("lock"))?;
    file.lock()
        .context("lock client configuration for update")?;
    Ok(file)
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
        || !matches!(url.scheme(), "http" | "https")
    {
        bail!(
            "use an HTTP or HTTPS server origin without credentials, paths, queries, or fragments"
        );
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
                        "set SESSIONDB_API_KEY or use sessiondb --server {account} auth login --key-stdin"
                    )
                })
            })
            .await??
        }
    };
    Ok(Client::new(&server, api_key, timeout)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn legacy_names_resolve_to_origins_and_duplicate_roots_coalesce() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let old = r#"
server="https://fallback.example"
current_context="work"
device_label="same-device"
[contexts.work]
server="https://A.example:443"
[contexts.alias]
server="https://a.example/"
[contexts.test]
server="http://test.example:8000"
[[sources]]
context="work"
harness="codex"
root="/fixture/codex"
[[sources]]
context="alias"
harness="codex"
root="/fixture/codex"
[[sources]]
context="test"
harness="pi"
root="/fixture/pi"
[sync]
upload_concurrency=2
"#;
        std::fs::write(&path, old).unwrap();
        let config = load(&path, dir.path(), Some("http://override.example"))
            .await
            .unwrap();
        assert_eq!(config.server.as_deref(), Some("https://a.example/"));
        assert_eq!(config.device_label.as_deref(), Some("same-device"));
        assert_eq!(config.sync.upload_concurrency, 2);
        assert_eq!(config.sources.len(), 2);
        assert_eq!(config.sources[0].server, "https://a.example/");
        assert_eq!(config.sources[1].server, "http://test.example:8000/");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        save(&path, &config).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.contains("context"));
        let reread = load(&path, dir.path(), None).await.unwrap();
        assert!(reread.sources == config.sources);
    }

    #[tokio::test]
    async fn override_only_legacy_roots_use_their_recorded_destinations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"device_label="device"
[[sources]]
context="default"
harness="codex"
root="/fixture/codex"
"#,
        )
        .unwrap();
        let state = crate::state::LocalState::open(dir.path()).await.unwrap();
        for (server, device) in [
            ("https://a.example/", "device"),
            ("https://b.example/", "device"),
            ("https://other.example/", "another-device"),
        ] {
            sqlx::query("INSERT INTO registrations(server,device,harness,root,source_id) VALUES(?,?,'codex','/fixture/codex',?)")
                .bind(server).bind(device).bind(Uuid::new_v4().to_string()).execute(&state.pool).await.unwrap();
        }
        let config = load(&path, dir.path(), Some("https://new.example/"))
            .await
            .unwrap();
        assert!(config.server.is_none());
        assert_eq!(
            config
                .sources
                .iter()
                .map(|s| s.server.as_str())
                .collect::<Vec<_>>(),
            ["https://a.example/", "https://b.example/"]
        );
    }

    #[tokio::test]
    async fn conflicting_or_unresolvable_source_bindings_are_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for entry in [
            "context='missing'",
            "context='old'\nserver='https://b.example'",
            "server='https://user:secret@b.example'",
        ] {
            let original = format!("[[sources]]\n{entry}\nharness='pi'\nroot='/fixture/pi'\n");
            std::fs::write(&path, &original).unwrap();
            let result = load(&path, dir.path(), Some("https://new.example/")).await;
            assert!(result.is_err());
            assert!(!result.err().unwrap().to_string().contains("secret"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }
}
