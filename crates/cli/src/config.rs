use anyhow::{Context, Result, bail};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use sessiondb_types::{Harness, MemberHandle};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Default, Serialize)]
pub struct FileConfig {
    pub server: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    api_keys: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub members: BTreeMap<String, MemberHandle>,
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
    api_keys: BTreeMap<String, String>,
    members: BTreeMap<String, MemberHandle>,
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
    #[serde(skip_serializing_if = "is_true")]
    pub auto_update: bool,
    pub reconcile_interval_seconds: u64,
    pub upload_concurrency: usize,
}
// Keep default configs readable by clients that predate automatic updates.
fn is_true(value: &bool) -> bool {
    *value
}
impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            auto_update: true,
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
    // TOML's default error display quotes the input line, which can contain a key.
    let stored: StoredConfig = toml::from_str(&text).map_err(|e: toml::de::Error| {
        let line = e
            .span()
            .and_then(|span| text.as_bytes().get(..span.start))
            .map_or(1, |prefix| {
                1 + prefix.iter().filter(|&&b| b == b'\n').count()
            });
        anyhow::anyhow!(
            "invalid client configuration in {} at line {line}",
            path.display()
        )
    })?;
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
        api_keys: normalize_entries(stored.api_keys, "API key")?,
        members: normalize_entries(stored.members, "member")?,
        device_label: stored.device_label,
        sources,
        project_rules: stored.project_rules,
        sync: stored.sync,
    })
}

fn normalize_entries<T>(entries: BTreeMap<String, T>, label: &str) -> Result<BTreeMap<String, T>> {
    let mut result = BTreeMap::new();
    for (url, value) in entries {
        if result.insert(normalize_server(&url)?, value).is_some() {
            bail!("duplicate {label} entries for the same server URL");
        }
    }
    Ok(result)
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
    config_parent(path)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_added_extension("lock"))?;
    file.lock()
        .context("lock client configuration for update")?;
    Ok(file)
}
fn config_parent(path: &Path) -> Result<&Path> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut dirs = std::fs::DirBuilder::new();
    dirs.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        dirs.mode(0o700);
    }
    dirs.create(parent)?;
    Ok(parent)
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
    let mut file = tempfile::NamedTempFile::new_in(config_parent(path)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
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

pub fn environment_key() -> Result<Option<String>> {
    match std::env::var("SESSIONDB_API_KEY") {
        Ok(key) => Ok(Some(key)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => bail!("SESSIONDB_API_KEY must be valid UTF-8"),
    }
}

impl FileConfig {
    pub fn saved_key(&self, origin: &str) -> Option<&str> {
        self.api_keys.get(origin).map(String::as_str)
    }

    pub fn request_key(&self, origin: &str) -> Result<String> {
        environment_key()?
            .or_else(|| self.api_keys.get(origin).cloned())
            .ok_or_else(|| {
                sessiondb_client::Error::Authentication(format!(
                    "run sessiondb connect {origin} or set SESSIONDB_API_KEY"
                ))
                .into()
            })
    }

    pub fn remember(&mut self, origin: String, key: String) {
        self.api_keys.insert(origin.clone(), key);
        self.server = Some(origin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn saved_keys_are_private_and_scoped_to_normalized_origins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "server='https://a.example'\n[api_keys]\n'https://A.example:443'='first-test-key'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let mut config = load(&path, dir.path(), None).await.unwrap();
        config.remember("https://b.example/".into(), "second-test-key".into());
        save(&path, &config).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        let saved = load(&path, dir.path(), None).await.unwrap();
        assert_eq!(saved.server.as_deref(), Some("https://b.example/"));
        assert_eq!(
            saved.saved_key("https://a.example/"),
            Some("first-test-key")
        );
        assert_eq!(
            saved.saved_key("https://b.example/"),
            Some("second-test-key")
        );
        assert!(saved.saved_key("http://a.example/").is_none());
        assert!(saved.saved_key("https://a.example:8443/").is_none());
    }

    #[tokio::test]
    async fn malformed_private_config_does_not_echo_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        for contents in [
            "[api_keys]\n'http://host/'=['private-test-token']",
            "[api_keys]\n'http://host/'='private-test-token",
        ] {
            std::fs::write(&path, contents).unwrap();
            let error = load(&path, dir.path(), None).await.err().unwrap();
            assert!(error.to_string().contains("line"));
            assert!(!format!("{error:#}").contains("private-test-token"));
        }
    }

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
        assert!(config.sync.auto_update);
        assert_eq!(config.sources.len(), 2);
        assert_eq!(config.sources[0].server, "https://a.example/");
        assert_eq!(config.sources[1].server, "http://test.example:8000/");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        save(&path, &config).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(!written.contains("context"));
        assert!(!written.contains("auto_update"));
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
