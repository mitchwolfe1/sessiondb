use crate::{Error, Result, Tokens, secure_url};
use openidconnect::NonceVerifier;
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, RefreshToken, Scope, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
    reqwest as oidc_http,
};
use sessiondb_types::AuthConfig;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use url::Url;

type OidcClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;
pub struct LoginFlow {
    pub authorization_url: Url,
    listener: TcpListener,
    client: OidcClient,
    http: oidc_http::Client,
    state: CsrfToken,
    nonce: Nonce,
    verifier: PkceCodeVerifier,
}
fn auth_error(message: &str) -> Error {
    Error::Authentication(message.into())
}
async fn provider(config: &AuthConfig) -> Result<(OidcClient, oidc_http::Client)> {
    let issuer =
        IssuerUrl::new(config.issuer.clone()).map_err(|_| auth_error("Invalid OIDC issuer."))?;
    secure_url(issuer.url())?;
    let http = oidc_http::Client::builder()
        .redirect(oidc_http::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| auth_error("Could not create the identity client."))?;
    let metadata = CoreProviderMetadata::discover_async(issuer, &http)
        .await
        .map_err(|_| auth_error("OIDC discovery failed."))?;
    secure_url(metadata.authorization_endpoint().url())?;
    secure_url(metadata.jwks_uri().url())?;
    if let Some(endpoint) = metadata.token_endpoint() {
        secure_url(endpoint.url())?;
    }
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(config.cli_client_id.clone()),
        None,
    );
    Ok((client, http))
}
impl LoginFlow {
    pub async fn start(config: &AuthConfig) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let redirect = format!(
            "http://127.0.0.1:{}/callback",
            listener.local_addr()?.port()
        );
        let (client, http) = provider(config).await?;
        let client = client.set_redirect_uri(
            RedirectUrl::new(redirect).map_err(|_| auth_error("Invalid callback URI."))?,
        );
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (authorization_url, state, nonce) = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("profile".into()))
            .add_scope(Scope::new("email".into()))
            .set_pkce_challenge(challenge)
            .url();
        Ok(Self {
            authorization_url,
            listener,
            client,
            http,
            state,
            nonce,
            verifier,
        })
    }
    pub async fn finish(self) -> Result<Tokens> {
        tokio::time::timeout(Duration::from_secs(300), self.finish_inner())
            .await
            .map_err(|_| Error::Timeout("browser login"))?
    }
    async fn finish_inner(self) -> Result<Tokens> {
        let code = loop {
            let (mut stream, _) = self.listener.accept().await?;
            let mut line = String::new();
            let read = tokio::time::timeout(Duration::from_secs(5), async {
                BufReader::new(&mut stream)
                    .take(8192)
                    .read_line(&mut line)
                    .await
            })
            .await;
            if !matches!(read,Ok(Ok(n)) if n>0) {
                continue;
            }
            let path = line.split_whitespace().nth(1).unwrap_or("");
            let Ok(callback) = Url::parse(&format!("http://127.0.0.1{path}")) else {
                continue;
            };
            if !line.starts_with("GET ") || callback.path() != "/callback" {
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            let params: std::collections::HashMap<_, _> =
                callback.query_pairs().into_owned().collect();
            let state = params
                .get("state")
                .ok_or_else(|| auth_error("The login callback omitted state."))?;
            if state != self.state.secret() {
                let _=stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                return Err(auth_error("The login callback state did not match."));
            }
            if params.contains_key("error") {
                let _=stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                return Err(auth_error("The identity provider declined login."));
            }
            let code = params
                .get("code")
                .ok_or_else(|| auth_error("The login callback omitted its code."))?
                .clone();
            let message = "Login received. Return to your terminal.";
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{message}",message.len()).as_bytes()).await?;
            break code;
        };
        let tokens = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .map_err(|_| auth_error("The provider has no token endpoint."))?
            .set_pkce_verifier(self.verifier)
            .request_async(&self.http)
            .await
            .map_err(|_| auth_error("The authorization code exchange failed."))?;
        let id_token = tokens
            .id_token()
            .ok_or_else(|| auth_error("The provider omitted the ID token."))?;
        let verifier = self.client.id_token_verifier();
        let claims = id_token
            .claims(&verifier, &self.nonce)
            .map_err(|_| auth_error("The ID token failed validation."))?;
        if let Some(expected) = claims.access_token_hash() {
            let actual = AccessTokenHash::from_token(
                tokens.access_token(),
                id_token
                    .signing_alg()
                    .map_err(|_| auth_error("Invalid ID token algorithm."))?,
                id_token
                    .signing_key(&verifier)
                    .map_err(|_| auth_error("Invalid ID token key."))?,
            )
            .map_err(|_| auth_error("Could not validate the access-token hash."))?;
            if &actual != expected {
                return Err(auth_error("The access-token hash did not match."));
            }
        }
        let mut result = tokens_from_response(&tokens, None);
        result.subject = Some(claims.subject().as_str().to_owned());
        result.nonce = Some(self.nonce.secret().clone());
        Ok(result)
    }
}
pub async fn refresh(config: &AuthConfig, tokens: &Tokens) -> Result<Tokens> {
    let refresh = tokens
        .refresh_token
        .as_ref()
        .ok_or_else(|| auth_error("Sign in again; no refresh credential is available."))?;
    let (client, http) = provider(config).await?;
    let response = client
        .exchange_refresh_token(&RefreshToken::new(refresh.clone()))
        .map_err(|_| auth_error("The provider has no token endpoint."))?
        .request_async(&http)
        .await
        .map_err(|_| {
            auth_error("The refresh credential expired or was rejected. Sign in again.")
        })?;
    if let Some(id) = response.id_token() {
        let verifier = client.id_token_verifier();
        let claims = id
            .claims(&verifier, |nonce: Option<&Nonce>| {
                if let Some(nonce) = nonce {
                    let expected = tokens
                        .nonce
                        .as_ref()
                        .ok_or_else(|| "Sign in again to establish the login nonce.".to_owned())?;
                    Nonce::new(expected.clone()).verify(Some(nonce))
                } else {
                    Ok(())
                }
            })
            .map_err(|_| auth_error("The refreshed ID token failed validation."))?;
        if tokens.subject.as_deref() != Some(claims.subject().as_str()) {
            return Err(auth_error("The refreshed identity changed. Sign in again."));
        }
        if let Some(expected) = claims.access_token_hash() {
            let actual = AccessTokenHash::from_token(
                response.access_token(),
                id.signing_alg()
                    .map_err(|_| auth_error("Invalid refresh signing algorithm."))?,
                id.signing_key(&verifier)
                    .map_err(|_| auth_error("Invalid refresh signing key."))?,
            )
            .map_err(|_| auth_error("Could not validate the refreshed access-token hash."))?;
            if &actual != expected {
                return Err(auth_error("The refreshed access-token hash did not match."));
            }
        }
    }
    let mut result = tokens_from_response(&response, Some(refresh));
    result.subject = tokens.subject.clone();
    result.nonce = tokens.nonce.clone();
    Ok(result)
}
fn tokens_from_response(
    response: &openidconnect::core::CoreTokenResponse,
    old_refresh: Option<&String>,
) -> Tokens {
    Tokens {
        access_token: response.access_token().secret().clone(),
        refresh_token: response
            .refresh_token()
            .map(|t| t.secret().clone())
            .or_else(|| old_refresh.cloned()),
        expires_at: response
            .expires_in()
            .and_then(|d| chrono::Duration::from_std(d).ok())
            .map(|d| chrono::Utc::now() + d),
        subject: None,
        nonce: None,
    }
}
