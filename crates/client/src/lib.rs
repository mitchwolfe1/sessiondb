//! Typed SessionDB API access. The CLI and applications share this HTTP boundary.
use reqwest::{
    Method, StatusCode,
    header::{AUTHORIZATION, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};
use sessiondb_types::*;
use std::time::Duration;
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
}
pub type Result<T> = std::result::Result<T, Error>;
fn transport(error: reqwest::Error) -> Error {
    Error::Transport(error.without_url())
}

#[derive(Clone)]
pub struct Client {
    base: Url,
    http: reqwest::Client,
    authorization: HeaderValue,
}
impl Client {
    pub fn new(server: &str, api_key: String, timeout: Duration) -> Result<Self> {
        if api_key.trim().is_empty() {
            return Err(Error::Authentication("An API key is required.".into()));
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| {
                Error::Authentication("The API key contains invalid header characters.".into())
            })?;
        authorization.set_sensitive(true);
        let mut base =
            Url::parse(server).map_err(|_| Error::Configuration("Invalid server URL.".into()))?;
        validate_http_url(&base)?;
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
            authorization,
        })
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
    ) -> Result<T> {
        let response = self.send(method, path, query, body).await?;
        let bytes = bounded_response(response, 16 * 1024 * 1024).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    async fn send(
        &self,
        method: Method,
        path: &str,
        query: Option<&impl Serialize>,
        body: Option<&impl Serialize>,
    ) -> Result<reqwest::Response> {
        let url = self
            .base
            .join(path.trim_start_matches('/'))
            .map_err(|_| Error::Configuration("Invalid API path.".into()))?;
        let safe = method == Method::GET;
        for attempt in 0..3 {
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .header(AUTHORIZATION, self.authorization.clone());
            if let Some(query) = query {
                request = request.query(query);
            }
            if let Some(body) = body {
                request = request.json(body);
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
        self.request(Method::GET, path, None::<&()>, None::<&()>)
            .await
    }
    async fn get_query<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &impl Serialize,
    ) -> Result<T> {
        self.request(Method::GET, path, Some(query), None::<&()>)
            .await
    }

    pub async fn info(&self) -> Result<ServerInfo> {
        self.get("v1/info").await
    }
    pub async fn sources(&self, options: &ListOptions) -> Result<Page<Source>> {
        self.get_query("v1/sources", options).await
    }
    pub async fn create_source(&self, input: &SourceCreate) -> Result<Source> {
        self.request(Method::POST, "v1/sources", None::<&()>, Some(input))
            .await
    }
    pub async fn source(&self, id: Uuid) -> Result<Source> {
        self.get(&format!("v1/sources/{id}")).await
    }
    pub async fn source_files(&self, id: Uuid, options: &ListOptions) -> Result<Page<SourceFile>> {
        self.get_query(&format!("v1/sources/{id}/files"), options)
            .await
    }
    pub async fn sessions(&self, filter: &SessionFilter) -> Result<Page<Session>> {
        self.get_query("v1/sessions", filter).await
    }
    pub async fn session(&self, id: Uuid) -> Result<Session> {
        self.get(&format!("v1/sessions/{id}")).await
    }
    pub async fn messages(&self, id: Uuid, filter: &MessageFilter) -> Result<Page<Message>> {
        self.get_query(&format!("v1/sessions/{id}/messages"), filter)
            .await
    }
    /// Rank matching exchanges and return bounded excerpts anchored to original messages.
    pub async fn search(&self, request: &SearchRequest) -> Result<SearchResponse> {
        self.request(Method::POST, "v1/search", None::<&()>, Some(request))
            .await
    }
    /// Read the anchor, its ancestors and one continuation. Forks expose child IDs.
    pub async fn read_around(&self, id: Uuid, options: &AroundOptions) -> Result<MessageContext> {
        self.get_query(&format!("v1/messages/{id}/around"), options)
            .await
    }
    pub async fn message(&self, id: Uuid) -> Result<Message> {
        self.get(&format!("v1/messages/{id}")).await
    }
    pub async fn content(&self, id: Uuid) -> Result<FullContent> {
        let response = self
            .send(
                Method::GET,
                &format!("v1/messages/{id}/content"),
                None::<&()>,
                None::<&()>,
            )
            .await?;
        Ok(serde_json::from_slice(
            &bounded_response(response, 300 * 1024 * 1024).await?,
        )?)
    }
    pub async fn artifact(&self, id: Uuid) -> Result<Artifact> {
        self.get(&format!("v1/artifacts/{id}")).await
    }

    /// Archive and parse one file through the authenticated API.
    ///
    /// Retry with a fresh reader and the same metadata after a lost response.
    pub async fn upload_file<R: AsyncRead + Send + Sync + 'static>(
        &self,
        source: Uuid,
        metadata: &FileUpload,
        reader: R,
    ) -> Result<FileUploadResult> {
        let metadata_part = reqwest::multipart::Part::text(serde_json::to_string(metadata)?)
            .mime_str("application/json")
            .map_err(transport)?;
        let file_part = reqwest::multipart::Part::stream_with_length(
            reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(reader)),
            metadata.byte_length,
        )
        .file_name("transcript.jsonl")
        .mime_str("application/x-ndjson")
        .map_err(transport)?;
        let form = reqwest::multipart::Form::new()
            .part("metadata", metadata_part)
            .part("file", file_part);
        let url = self
            .base
            .join(&format!("v1/sources/{source}/files"))
            .map_err(|_| Error::Configuration("Invalid API path.".into()))?;
        let response = self
            .http
            .post(url)
            .header(AUTHORIZATION, self.authorization.clone())
            .multipart(form)
            .send()
            .await
            .map_err(transport)?;
        let response = success(response).await?;
        Ok(serde_json::from_slice(
            &bounded_response(response, 16 * 1024 * 1024).await?,
        )?)
    }
    pub async fn export_to<W: AsyncWrite + Unpin>(&self, id: Uuid, writer: &mut W) -> Result<u64> {
        self.download(&format!("v1/sessions/{id}/export"), None::<&()>, writer)
            .await
    }
    pub async fn original_to<W: AsyncWrite + Unpin>(
        &self,
        source: Uuid,
        file: Uuid,
        generation: Option<Uuid>,
        writer: &mut W,
    ) -> Result<u64> {
        self.download(
            &format!("v1/sources/{source}/files/{file}/content"),
            Some(&[("generation_id", generation.map(|v| v.to_string()))]),
            writer,
        )
        .await
    }
    pub async fn artifact_to<W: AsyncWrite + Unpin>(
        &self,
        id: Uuid,
        writer: &mut W,
    ) -> Result<u64> {
        self.download(&format!("v1/artifacts/{id}/content"), None::<&()>, writer)
            .await
    }
    async fn download<W: AsyncWrite + Unpin>(
        &self,
        path: &str,
        query: Option<&impl Serialize>,
        writer: &mut W,
    ) -> Result<u64> {
        let mut response = self.send(Method::GET, path, query, None::<&()>).await?;
        if response.status() == StatusCode::TEMPORARY_REDIRECT {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Error::Configuration("Missing download location.".into()))?;
            let url = Url::parse(location)
                .map_err(|_| Error::Configuration("Invalid download location.".into()))?;
            validate_http_url(&url)?;
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
}

pub(crate) fn validate_http_url(url: &Url) -> Result<()> {
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err(Error::Configuration(
            "URLs must not contain credentials or fragments.".into(),
        ));
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::Configuration(
            "URL scheme must be HTTP or HTTPS.".into(),
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::Client;
    use std::time::Duration;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[tokio::test]
    async fn http_hostname_supports_authenticated_requests() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.ends_with(b"\r\n\r\n") {
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0, "request ended before its headers");
                request.extend_from_slice(&buffer[..count]);
                assert!(request.len() <= 16 * 1024);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /v1/info HTTP/1.1\r\n"));
            assert!(request.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization")
                        && value.trim() == "Bearer tailnet-test-key"
                })
            }));
            let body = r#"{"api_version":"v1","server_version":"tailnet-test","capabilities":["sessions:read"],"limits":{"max_file_bytes":4096}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let mut client = Client::new(
            &format!("http://athena:{}", address.port()),
            "tailnet-test-key".into(),
            Duration::from_secs(3),
        )
        .unwrap();
        client.http = reqwest::Client::builder()
            .no_proxy()
            .resolve("athena", address)
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let info = client.info().await.unwrap();
        assert_eq!(info.server_version, "tailnet-test");
        assert_eq!(info.limits.max_file_bytes, 4096);
        assert_eq!(info.capabilities, vec!["sessions:read"]);
        server.await.unwrap();
    }
}
