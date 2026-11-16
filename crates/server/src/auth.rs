use crate::{
    AppState,
    config::{Auth, require_secure_url},
    error::{ApiError, ApiResult},
};
use anyhow::{Context, Result};
use axum::{
    Extension,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use sessiondb_types::{OrganizationRole, User};
use sqlx::{PgPool, Row};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub preferred_username: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: bool,
}
#[derive(Clone)]
pub struct Principal {
    pub claims: Claims,
    pub user: Option<User>,
}
impl Principal {
    pub fn user_id(&self) -> ApiResult<Uuid> {
        self.user
            .as_ref()
            .map(|u| u.id)
            .ok_or_else(ApiError::unauthorized)
    }
}
struct CachedKeys {
    keys: JwkSet,
    fetched: Instant,
    unknown_refresh: Option<Instant>,
}
pub struct Authenticator {
    config: Auth,
    http: reqwest::Client,
    cached: RwLock<Option<CachedKeys>>,
    refresh: Mutex<()>,
}
impl Authenticator {
    pub fn new(config: Auth) -> Result<Self> {
        Ok(Self {
            config,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .build()?,
            cached: RwLock::new(None),
            refresh: Mutex::new(()),
        })
    }
    async fn refresh_keys(&self, force: bool) -> ApiResult<()> {
        let _guard = self.refresh.lock().await;
        if self.cached.read().await.as_ref().is_some_and(|c| {
            if force {
                c.unknown_refresh
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(30))
            } else {
                c.fetched.elapsed() < Duration::from_secs(300)
            }
        }) {
            return Ok(());
        }
        let issuer = self.config.issuer.as_str().trim_end_matches('/');
        let response = self
            .http
            .get(format!("{issuer}/.well-known/openid-configuration"))
            .send()
            .await
            .map_err(|_| ApiError::unavailable())?;
        let discovery: Discovery = bounded_json(response).await?;
        if discovery.issuer != self.config.issuer.as_str() {
            return Err(ApiError::unavailable());
        }
        require_secure_url(&discovery.jwks_uri, self.config.allow_insecure_http)
            .map_err(|_| ApiError::unavailable())?;
        let keys: JwkSet = bounded_json(
            self.http
                .get(discovery.jwks_uri)
                .send()
                .await
                .map_err(|_| ApiError::unavailable())?,
        )
        .await?;
        if keys.keys.len() > 64 {
            return Err(ApiError::unavailable());
        }
        *self.cached.write().await = Some(CachedKeys {
            keys,
            fetched: Instant::now(),
            unknown_refresh: force.then(Instant::now),
        });
        Ok(())
    }
    pub async fn validate(&self, token: &str) -> ApiResult<Claims> {
        if token.len() > 32 * 1024 {
            return Err(ApiError::unauthorized());
        }
        let header = decode_header(token).map_err(|_| ApiError::unauthorized())?;
        if header.alg != Algorithm::RS256 {
            return Err(ApiError::unauthorized());
        }
        let kid = header.kid.ok_or_else(ApiError::unauthorized)?;
        self.refresh_keys(false).await?;
        let mut key = self
            .cached
            .read()
            .await
            .as_ref()
            .and_then(|c| c.keys.find(&kid))
            .cloned();
        if key.is_none() {
            self.refresh_keys(true).await?;
            key = self
                .cached
                .read()
                .await
                .as_ref()
                .and_then(|c| c.keys.find(&kid))
                .cloned();
        }
        let key = DecodingKey::from_jwk(&key.ok_or_else(ApiError::unauthorized)?)
            .map_err(|_| ApiError::unauthorized())?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&self.config.audience]);
        validation.set_issuer(&[self.config.issuer.as_str()]);
        validation.set_required_spec_claims(&["iss", "sub", "aud", "exp"]);
        validation.validate_nbf = true;
        validation.leeway = 30;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| ApiError::unauthorized())?
            .claims;
        if claims.sub.is_empty() || claims.iss != self.config.issuer.as_str() {
            return Err(ApiError::unauthorized());
        }
        Ok(claims)
    }
}
#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: Url,
}
async fn bounded_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> ApiResult<T> {
    if !response.status().is_success() || response.content_length().is_some_and(|n| n > 1024 * 1024)
    {
        return Err(ApiError::unavailable());
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::unavailable())?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(ApiError::unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| ApiError::unavailable())
}

pub async fn authenticate(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> ApiResult<Response> {
    let bearer = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token)
        .ok_or_else(ApiError::unauthorized)?;
    let claims = state.auth.validate(bearer).await?;
    let row =
        sqlx::query("SELECT id, display_name, email FROM users WHERE issuer=$1 AND subject=$2")
            .bind(&claims.iss)
            .bind(&claims.sub)
            .fetch_optional(&state.pool)
            .await?;
    let user = row.map(|r| User {
        id: r.get("id"),
        display_name: r.get("display_name"),
        email: r.get("email"),
    });
    request.extensions_mut().insert(Principal { claims, user });
    Ok(next.run(request).await)
}

pub async fn organization_role(
    pool: &PgPool,
    org_id: Uuid,
    principal: &Principal,
) -> ApiResult<OrganizationRole> {
    let value: Option<String> = sqlx::query_scalar(
        "SELECT role FROM memberships WHERE organization_id=$1 AND user_id=$2 AND status='active'",
    )
    .bind(org_id)
    .bind(principal.user_id()?)
    .fetch_optional(pool)
    .await?;
    value
        .ok_or_else(ApiError::not_found)?
        .parse()
        .map_err(ApiError::internal)
}
pub fn is_admin(role: OrganizationRole) -> bool {
    matches!(role, OrganizationRole::Owner | OrganizationRole::Admin)
}

pub async fn bootstrap(pool: &PgPool, config: Option<&crate::config::Bootstrap>) -> Result<()> {
    let Some(config) = config else {
        return Ok(());
    };
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(1936028275)")
        .execute(&mut *tx)
        .await?;
    let done: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bootstrap_state) OR EXISTS(SELECT 1 FROM organizations)",
    )
    .fetch_one(&mut *tx)
    .await?;
    if !done {
        let user_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users(id,issuer,subject,display_name) VALUES($1,$2,$3,'Initial owner')",
        )
        .bind(user_id)
        .bind(&config.owner_issuer)
        .bind(&config.owner_subject)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO organizations(id,slug,name) VALUES($1,$2,$3)")
            .bind(org_id)
            .bind(&config.organization_slug)
            .bind(&config.organization_name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO memberships(organization_id,user_id,role) VALUES($1,$2,'owner')")
            .bind(org_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO bootstrap_state(singleton) VALUES(true)")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await.context("commit initial owner bootstrap")
}

pub type Authenticated = Extension<Principal>;
