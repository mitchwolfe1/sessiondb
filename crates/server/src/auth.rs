use crate::{
    AppState,
    config::Auth,
    error::{ApiError, ApiResult},
};
use anyhow::{Result, bail};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, header},
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;

pub struct Authenticator {
    key_digest: [u8; 32],
}
impl Authenticator {
    pub fn new(config: Auth) -> Result<Self> {
        Self::from_key(&config.api_key()?)
    }

    pub fn from_key(key: &str) -> Result<Self> {
        if key.is_empty()
            || key
                .bytes()
                .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            bail!("API key must be nonempty and contain no whitespace or control characters");
        }
        Ok(Self {
            key_digest: Sha256::digest(key.as_bytes()).into(),
        })
    }

    fn accepts(&self, headers: &HeaderMap) -> bool {
        let mut values = headers.get_all(header::AUTHORIZATION).iter();
        let Some(value) = values.next() else {
            return false;
        };
        if values.next().is_some() {
            return false;
        }
        let Some((scheme, key)) = value.to_str().ok().and_then(|s| s.split_once(' ')) else {
            return false;
        };
        if !scheme.eq_ignore_ascii_case("bearer") {
            return false;
        }
        let digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
        bool::from(self.key_digest.ct_eq(&digest))
    }
}

pub async fn authenticate(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> ApiResult<Response> {
    if !state.auth.accepts(request.headers()) {
        return Err(ApiError::unauthorized());
    }
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_must_be_nonempty_bearer_secrets() {
        for key in ["", " ", "key\n", "a b"] {
            assert!(Authenticator::from_key(key).is_err());
        }
        assert!(Authenticator::from_key("test-instance-key").is_ok());
    }

    #[test]
    fn bearer_boundary_rejects_missing_wrong_and_ambiguous_credentials() {
        let auth = Authenticator::from_key("instance-secret").unwrap();
        let mut headers = HeaderMap::new();
        assert!(!auth.accepts(&headers));
        for value in [
            "Bearer wrong",
            "Bearer instance-secreu",
            "Bearer instance-secret-extra",
            "Basic instance-secret",
            "Bearer",
            "Bearer ",
            "Bearer  instance-secret",
            "Bearer instance-secret ",
        ] {
            headers.insert(header::AUTHORIZATION, value.parse().unwrap());
            assert!(!auth.accepts(&headers));
        }
        for value in ["Bearer instance-secret", "bearer instance-secret"] {
            headers.insert(header::AUTHORIZATION, value.parse().unwrap());
            assert!(auth.accepts(&headers));
        }
        headers.append(
            header::AUTHORIZATION,
            "Bearer instance-secret".parse().unwrap(),
        );
        assert!(!auth.accepts(&headers));
    }
}
