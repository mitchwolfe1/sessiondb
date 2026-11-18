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
    pub objects: Objects,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub telemetry: Telemetry,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Objects {
    pub provider: String,
    pub endpoint: Option<Url>,
    pub bucket: String,
    #[serde(default = "region")]
    pub region: String,
    #[serde(default)]
    pub path_style: bool,
}
fn region() -> String {
    "us-east-1".into()
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
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        crate::auth::Authenticator::from_env()?;
        if self.objects.provider != "s3" {
            bail!("objects.provider must be s3");
        }
        if self.objects.bucket.is_empty() {
            bail!("objects.bucket is required");
        }
        validate_http_url(&self.public_url)?;
        if self.public_url.path() != "/" {
            bail!("public_url must contain only its scheme, host, and port");
        }
        if self.limits.max_file_bytes == 0 || self.limits.max_file_bytes > 256 * 1024 * 1024 {
            bail!("max_file_bytes must be within 1..268435456");
        }
        Ok(())
    }
    pub fn database_url(&self) -> Result<String> {
        env::var("SESSIONDB_DATABASE_URL").map_err(|_| {
            anyhow::anyhow!(
                "SESSIONDB_DATABASE_URL must be set to a valid UTF-8 PostgreSQL connection URL"
            )
        })
    }
}
pub fn validate_http_url(url: &Url) -> Result<()> {
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("URLs must not contain credentials, query strings, or fragments");
    }
    if !matches!(url.scheme(), "http" | "https") {
        bail!("URL scheme must be HTTP or HTTPS");
    }
    Ok(())
}
