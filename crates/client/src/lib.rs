//! Typed SessionDB API access. The CLI and applications share this HTTP boundary.
pub mod auth;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use sessiondb_types::*;
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use url::Url;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Configuration(String),
    #[error("{0}")]
    Authentication(String),
    #[error("HTTP transport failed: {0}")]
    Transport(#[source] reqwest::Error),
    #[error("{}: {}", .0.code, .0.detail)]
    Problem(Box<Problem>),
    #[error("server returned HTTP {0}")]
    Http(StatusCode),
    #[error("response does not match the SessionDB contract: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("file transfer failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("timed out waiting for {0}")]
    Timeout(&'static str),
}
pub type Result<T> = std::result::Result<T, Error>;
fn transport(error: reqwest::Error) -> Error {
    Error::Transport(error.without_url())
}

/// Applications can supply a refreshing credential source without coupling it to the CLI.
#[async_trait]
pub trait TokenProvider: Send + Sync {
    async fn access_token(&self) -> Result<Option<String>>;
}
struct StaticToken(Option<String>);
#[async_trait]
impl TokenProvider for StaticToken {
    async fn access_token(&self) -> Result<Option<String>> {
        Ok(self.0.clone())
    }
}

#[derive(Clone)]
pub struct Client {
    base: Url,
    http: reqwest::Client,
    credentials: Arc<dyn TokenProvider>,
}
impl Client {
    pub fn new(server: &str, token: Option<String>, timeout: Duration) -> Result<Self> {
        let mut base =
            Url::parse(server).map_err(|_| Error::Configuration("Invalid server URL.".into()))?;
        secure_url(&base)?;
        if !matches!(base.path(), "" | "/") || base.query().is_some() {
            return Err(Error::Configuration(
                "The server URL must contain only its scheme, host, and port.".into(),
            ));
        }
        base.set_path("/");
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("sessiondb/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(transport)?;
        Ok(Self {
            base,
            http,
            credentials: Arc::new(StaticToken(token)),
        })
    }
    pub fn with_token_provider(mut self, provider: Arc<dyn TokenProvider>) -> Self {
        self.credentials = provider;
        self
    }
    pub fn server(&self) -> &str {
        self.base.as_str()
    }
    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: Option<&impl Serialize>,
        body: Option<&impl Serialize>,
        key: Option<&str>,
    ) -> Result<T> {
        let response = self.send(method, path, query, body, key, None).await?;
        let bytes = bounded_response(response, 16 * 1024 * 1024).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    async fn send(
        &self,
        method: Method,
        path: &str,
        query: Option<&impl Serialize>,
        body: Option<&impl Serialize>,
        key: Option<&str>,
        revision: Option<&str>,
    ) -> Result<reqwest::Response> {
        let url = self
            .base
            .join(path.trim_start_matches('/'))
            .map_err(|_| Error::Configuration("Invalid API path.".into()))?;
        let safe = method == Method::GET || key.is_some();
        for attempt in 0..3 {
            let mut request = self.http.request(method.clone(), url.clone());
            if let Some(token) = self.credentials.access_token().await? {
                request = request.bearer_auth(token);
            }
            if let Some(query) = query {
                request = request.query(query);
            }
            if let Some(body) = body {
                request = request.json(body);
            }
            if let Some(key) = key {
                request = request.header("idempotency-key", key);
            }
            if let Some(revision) = revision {
                request = request.header("if-match", format!("\"{revision}\""));
            }
            match request.send().await {
                Ok(response)
                    if safe
                        && attempt < 2
                        && matches!(response.status().as_u16(), 429 | 502 | 503 | 504) =>
                {
                    let delay = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(1 << attempt)
                        .min(30);
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                }
                Ok(response) => return success(response).await,
                Err(error) if safe && attempt < 2 && (error.is_connect() || error.is_timeout()) => {
                    tokio::time::sleep(Duration::from_millis(250 * (1 << attempt))).await
                }
                Err(error) => return Err(transport(error)),
            }
        }
        unreachable!("bounded retry returns on the final attempt")
    }
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request(Method::GET, path, None::<&()>, None::<&()>, None)
            .await
    }
    async fn get_query<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &impl Serialize,
    ) -> Result<T> {
        self.request(Method::GET, path, Some(query), None::<&()>, None)
            .await
    }
    fn org(org: Uuid, resource: &str) -> String {
        format!("v1/organizations/{org}/{resource}")
    }

    pub async fn info(&self) -> Result<ServerInfo> {
        self.get("v1/info").await
    }
    pub async fn auth_config(&self) -> Result<AuthConfig> {
        self.get("v1/auth/config").await
    }
    pub async fn register_login(&self) -> Result<Me> {
        self.request(
            Method::POST,
            "v1/auth/session",
            None::<&()>,
            None::<&()>,
            None,
        )
        .await
    }
    pub async fn me(&self) -> Result<Me> {
        self.get("v1/me").await
    }
    pub async fn organizations(&self, options: &ListOptions) -> Result<Page<Organization>> {
        self.get_query("v1/organizations", options).await
    }
    pub async fn invite(&self, org: Uuid, input: &InvitationCreate) -> Result<Invitation> {
        self.request(
            Method::POST,
            &Self::org(org, "invitations"),
            None::<&()>,
            Some(input),
            None,
        )
        .await
    }
    pub async fn sources(&self, org: Uuid, options: &ListOptions) -> Result<Page<Source>> {
        self.get_query(&Self::org(org, "sources"), options).await
    }
    pub async fn create_source(&self, org: Uuid, input: &SourceCreate) -> Result<Source> {
        self.request(
            Method::POST,
            &Self::org(org, "sources"),
            None::<&()>,
            Some(input),
            None,
        )
        .await
    }
    pub async fn source(&self, org: Uuid, id: Uuid) -> Result<Source> {
        self.get(&Self::org(org, &format!("sources/{id}"))).await
    }
    pub async fn source_files(
        &self,
        org: Uuid,
        id: Uuid,
        options: &ListOptions,
    ) -> Result<Page<SourceFile>> {
        self.get_query(&Self::org(org, &format!("sources/{id}/files")), options)
            .await
    }
    pub async fn create_import(
        &self,
        org: Uuid,
        manifest: &ImportManifest,
        key: &str,
    ) -> Result<Import> {
        self.request(
            Method::POST,
            &Self::org(org, "imports"),
            None::<&()>,
            Some(manifest),
            Some(key),
        )
        .await
    }
    pub async fn import(&self, org: Uuid, id: Uuid) -> Result<Import> {
        self.get(&Self::org(org, &format!("imports/{id}"))).await
    }
    pub async fn complete_import(&self, org: Uuid, id: Uuid, key: &str) -> Result<Import> {
        self.request(
            Method::POST,
            &Self::org(org, &format!("imports/{id}/complete")),
            None::<&()>,
            None::<&()>,
            Some(key),
        )
        .await
    }
    pub async fn sessions(&self, org: Uuid, filter: &SessionFilter) -> Result<Page<Session>> {
        self.get_query(&Self::org(org, "sessions"), filter).await
    }
    pub async fn session(&self, org: Uuid, id: Uuid) -> Result<Session> {
        self.get(&Self::org(org, &format!("sessions/{id}"))).await
    }
    pub async fn messages(
        &self,
        org: Uuid,
        id: Uuid,
        filter: &MessageFilter,
    ) -> Result<Page<Message>> {
        self.get_query(&Self::org(org, &format!("sessions/{id}/messages")), filter)
            .await
    }
    pub async fn message(&self, org: Uuid, id: Uuid) -> Result<Message> {
        self.get(&Self::org(org, &format!("messages/{id}"))).await
    }
    pub async fn content(
        &self,
        org: Uuid,
        id: Uuid,
        revision: Option<&str>,
    ) -> Result<FullContent> {
        let response = self
            .send(
                Method::GET,
                &Self::org(org, &format!("messages/{id}/content")),
                None::<&()>,
                None::<&()>,
                None,
                revision,
            )
            .await?;
        Ok(serde_json::from_slice(
            &bounded_response(response, 300 * 1024 * 1024).await?,
        )?)
    }
    pub async fn job(&self, org: Uuid, id: Uuid) -> Result<Job> {
        self.get(&Self::org(org, &format!("jobs/{id}"))).await
    }
    pub async fn artifact(&self, org: Uuid, id: Uuid) -> Result<Artifact> {
        self.get(&Self::org(org, &format!("artifacts/{id}"))).await
    }

    /// Upload with only the signed target headers, using a client with no API credentials.
    pub async fn upload<R: AsyncRead + Send + Sync + 'static>(
        &self,
        target: &UploadTarget,
        reader: R,
    ) -> Result<()> {
        if target.method != "PUT" {
            return Err(Error::Configuration(
                "Unsupported signed upload method.".into(),
            ));
        }
        let url = Url::parse(&target.url)
            .map_err(|_| Error::Configuration("Invalid signed upload URL.".into()))?;
        secure_url(&url)?;
        let mut request = self.http.put(url);
        for (key, value) in &target.headers {
            if key.eq_ignore_ascii_case("authorization") || key.eq_ignore_ascii_case("cookie") {
                return Err(Error::Configuration(
                    "Unexpected signed upload credential header.".into(),
                ));
            }
            request = request.header(key, value);
        }
        let response = request
            .body(reqwest::Body::wrap_stream(
                tokio_util::io::ReaderStream::new(reader),
            ))
            .send()
            .await
            .map_err(transport)?;
        if !response.status().is_success() {
            return Err(Error::Http(response.status()));
        }
        Ok(())
    }
    pub async fn export_to<W: AsyncWrite + Unpin>(
        &self,
        org: Uuid,
        id: Uuid,
        writer: &mut W,
    ) -> Result<u64> {
        self.download(
            &Self::org(org, &format!("sessions/{id}/export")),
            None::<&()>,
            writer,
        )
        .await
    }
    pub async fn original_to<W: AsyncWrite + Unpin>(
        &self,
        org: Uuid,
        source: Uuid,
        file: Uuid,
        generation: Option<Uuid>,
        writer: &mut W,
    ) -> Result<u64> {
        self.download(
            &Self::org(org, &format!("sources/{source}/files/{file}/content")),
            Some(&[("generation_id", generation.map(|v| v.to_string()))]),
            writer,
        )
        .await
    }
    pub async fn artifact_to<W: AsyncWrite + Unpin>(
        &self,
        org: Uuid,
        id: Uuid,
        writer: &mut W,
    ) -> Result<u64> {
        self.download(
            &Self::org(org, &format!("artifacts/{id}/content")),
            None::<&()>,
            writer,
        )
        .await
    }
    async fn download<W: AsyncWrite + Unpin>(
        &self,
        path: &str,
        query: Option<&impl Serialize>,
        writer: &mut W,
    ) -> Result<u64> {
        let mut response = self
            .send(Method::GET, path, query, None::<&()>, None, None)
            .await?;
        if response.status() == StatusCode::TEMPORARY_REDIRECT {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Error::Configuration("Missing download location.".into()))?;
            let url = Url::parse(location)
                .map_err(|_| Error::Configuration("Invalid download location.".into()))?;
            secure_url(&url)?;
            response = success(self.http.get(url).send().await.map_err(transport)?).await?;
        }
        let mut bytes = 0;
        while let Some(chunk) = response.chunk().await.map_err(transport)? {
            writer.write_all(&chunk).await?;
            bytes += chunk.len() as u64;
        }
        writer.flush().await?;
        Ok(bytes)
    }
    pub async fn wait_for_acceptance(
        &self,
        org: Uuid,
        id: Uuid,
        timeout: Duration,
    ) -> Result<Import> {
        self.wait_import(org, id, timeout, true).await
    }
    pub async fn wait_for_processing(
        &self,
        org: Uuid,
        id: Uuid,
        timeout: Duration,
    ) -> Result<Import> {
        self.wait_import(org, id, timeout, false).await
    }
    async fn wait_import(
        &self,
        org: Uuid,
        id: Uuid,
        timeout: Duration,
        acceptance: bool,
    ) -> Result<Import> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let import = self.import(org, id).await?;
            if (acceptance && import.accepted_at.is_some()) || import.status.terminal() {
                return Ok(import);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout("import"));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    pub async fn wait_for_job(&self, org: Uuid, id: Uuid, timeout: Duration) -> Result<Job> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let job = self.job(org, id).await?;
            if job.status.terminal() {
                return Ok(job);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Timeout("job"));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

pub(crate) fn secure_url(url: &Url) -> Result<()> {
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(Error::Configuration(
            "URLs must not contain credentials or fragments.".into(),
        ));
    }
    if url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
    {
        Ok(())
    } else {
        Err(Error::Configuration(
            "Use HTTPS, or loopback HTTP for local development.".into(),
        ))
    }
}
async fn success(response: reqwest::Response) -> Result<reqwest::Response> {
    if response.status().is_success() || response.status() == StatusCode::TEMPORARY_REDIRECT {
        return Ok(response);
    }
    let status = response.status();
    let bytes = bounded_response(response, 1024 * 1024).await?;
    if let Ok(problem) = serde_json::from_slice::<Problem>(&bytes) {
        Err(Error::Problem(Box::new(problem)))
    } else {
        Err(Error::Http(status))
    }
}
async fn bounded_response(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    if response.content_length().is_some_and(|n| n > max as u64) {
        return Err(Error::Configuration(
            "Response exceeds the configured client limit.".into(),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if bytes.len() + chunk.len() > max {
            return Err(Error::Configuration(
                "Response exceeds the configured client limit.".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Credentials are serializable for an OS credential store; Debug intentionally omits secrets.
#[derive(Clone, Serialize, serde::Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
}
