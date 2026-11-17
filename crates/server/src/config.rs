use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sessiondb_types::Limits;
use std::{env, net::SocketAddr, path::Path};
use url::Url;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub public_url: Url,
    pub database: Database,
    pub objects: Objects,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default)]
    pub workers: Workers,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub telemetry: Telemetry,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Database {
    pub url_env: String,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Objects {
    pub provider: String,
    pub endpoint: Option<Url>,
    pub public_endpoint: Option<Url>,
    pub bucket: String,
    #[serde(default = "region")]
    pub region: String,
    #[serde(default)]
    pub path_style: bool,
    pub access_key_env: Option<String>,
    pub secret_key_env: Option<String>,
    #[serde(default = "upload_ttl")]
    pub upload_url_ttl_seconds: u64,
    #[serde(default = "download_ttl")]
    pub download_url_ttl_seconds: u64,
}
fn region() -> String {
    "us-east-1".into()
}
fn upload_ttl() -> u64 {
    900
}
fn download_ttl() -> u64 {
    60
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    #[serde(default = "api_key_env")]
    pub api_key_env: String,
}
fn api_key_env() -> String {
    "SESSIONDB_API_KEY".into()
}
impl Default for Auth {
    fn default() -> Self {
        Self {
            api_key_env: api_key_env(),
        }
    }
}
impl Auth {
    pub fn api_key(&self) -> Result<String> {
        let key = env::var(&self.api_key_env)
            .context("API key environment variable is not set or is not valid UTF-8")?;
        if key.is_empty() {
            bail!("API key environment variable must not be empty");
        }
        Ok(key)
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Workers {
    pub parse_concurrency: usize,
    pub embedding_concurrency: usize,
}
impl Default for Workers {
    fn default() -> Self {
        Self {
            parse_concurrency: 2,
            embedding_concurrency: 2,
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Telemetry {
    pub log_format: String,
    pub metrics_listen: Option<SocketAddr>,
}
impl Default for Telemetry {
    fn default() -> Self {
        Self {
            log_format: "json".into(),
            metrics_listen: None,
        }
    }
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let mut value: Self =
            toml::from_str(&std::fs::read_to_string(path).context("read server configuration")?)
                .context("parse server configuration")?;
        if let Ok(listen) = env::var("SESSIONDB_LISTEN") {
            value.listen = listen.parse().context("SESSIONDB_LISTEN")?;
        }
        if let Ok(url) = env::var("SESSIONDB_PUBLIC_URL") {
            value.public_url = url.parse().context("SESSIONDB_PUBLIC_URL")?;
        }
        if let Ok(endpoint) = env::var("SESSIONDB_S3_ENDPOINT") {
            value.objects.endpoint = Some(endpoint.parse().context("SESSIONDB_S3_ENDPOINT")?);
        }
        if let Ok(endpoint) = env::var("SESSIONDB_S3_PUBLIC_ENDPOINT") {
            value.objects.public_endpoint =
                Some(endpoint.parse().context("SESSIONDB_S3_PUBLIC_ENDPOINT")?);
        }
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        crate::auth::Authenticator::new(self.auth.clone())?;
        if self.objects.provider != "s3" {
            bail!("objects.provider must be s3");
        }
        if self.objects.bucket.is_empty() {
            bail!("objects.bucket is required");
        }
        if self.objects.access_key_env.is_some() != self.objects.secret_key_env.is_some() {
            bail!(
                "configure both object credential environment names or use the AWS credential chain"
            );
        }
        if self.objects.upload_url_ttl_seconds == 0
            || self.objects.upload_url_ttl_seconds > 900
            || self.objects.download_url_ttl_seconds == 0
            || self.objects.download_url_ttl_seconds > 60
        {
            bail!("upload URL lifetime must be 1..900 seconds and download lifetime 1..60 seconds");
        }
        require_secure_url(&self.public_url, true)?;
        if self.public_url.path() != "/" {
            bail!("public_url must contain only its scheme, host, and port");
        }
        if self.workers.parse_concurrency == 0 || self.workers.parse_concurrency > 16 {
            bail!("parse_concurrency must be 1..16");
        }
        if !(1..=128).contains(&self.limits.max_files_per_import)
            || self.limits.max_manifest_bytes == 0
            || self.limits.max_manifest_bytes > 2 * 1024 * 1024
            || self.limits.max_file_bytes == 0
            || self.limits.max_file_bytes > 256 * 1024 * 1024
        {
            bail!("configured import limits must be within the advertised v1 maximums");
        }
        Ok(())
    }
    pub fn database_url(&self) -> Result<String> {
        env::var(&self.database.url_env).context("database URL environment variable is not set")
    }
}
pub fn require_secure_url(url: &Url, allow_loopback: bool) -> Result<()> {
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("URLs must not contain credentials, query strings, or fragments");
    }
    if url.scheme() == "https" {
        return Ok(());
    }
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() == "http" && allow_loopback && local {
        return Ok(());
    }
    bail!("HTTPS is required; development HTTP must be explicitly enabled on loopback")
}
