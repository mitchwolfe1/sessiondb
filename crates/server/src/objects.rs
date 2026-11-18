use crate::{AppState, config::Objects};
use anyhow::{Result, bail};
use async_trait::async_trait;
use aws_sdk_s3::{
    Client,
    config::{
        BehaviorVersion, Credentials, Region, RequestChecksumCalculation,
        ResponseChecksumValidation,
    },
    error::ProvideErrorMetadata,
    primitives::ByteStream,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{env, pin::Pin, time::Duration};
use tokio::io::AsyncRead;

#[derive(Debug, thiserror::Error)]
pub enum ObjectError {
    #[error("object is missing")]
    Missing,
    #[error("immutable object key already exists")]
    Precondition,
    #[error("object byte range is invalid")]
    InvalidRange,
    #[error("object storage is unavailable")]
    Unavailable,
}
pub struct ObjectBody {
    pub reader: Pin<Box<dyn AsyncRead + Send>>,
    pub byte_length: u64,
    pub content_range: Option<String>,
}

/// Immutable storage writes and authorized content delivery.
#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn check(&self) -> Result<(), ObjectError>;
    async fn get(&self, key: &str, range: Option<&str>) -> Result<ObjectBody, ObjectError>;
    async fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        media_type: &str,
        sha256: &str,
    ) -> Result<(), ObjectError>;
    async fn delete(&self, key: &str) -> Result<(), ObjectError>;
}

pub struct S3Store {
    client: Client,
    bucket: String,
}
impl S3Store {
    /// Use an explicitly configured SDK client, including for integration environments.
    pub fn from_client(client: Client, bucket: impl Into<String>) -> Self {
        Self {
            client,
            bucket: bucket.into(),
        }
    }
    pub async fn new(config: &Objects) -> Result<Self> {
        let credentials = credentials_from_env()?;
        let shared = aws_config::defaults(BehaviorVersion::v2026_11_10())
            .region(Region::new(config.region.clone()))
            .load()
            .await;
        let mut builder = aws_sdk_s3::config::Builder::from(&shared)
            .behavior_version(BehaviorVersion::v2026_11_10())
            .force_path_style(config.path_style)
            .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
            .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
            .timeout_config(
                aws_sdk_s3::config::timeout::TimeoutConfig::builder()
                    .connect_timeout(Duration::from_secs(10))
                    .operation_timeout(Duration::from_secs(300))
                    .build(),
            );
        if let Some(credentials) = credentials {
            builder = builder.credentials_provider(credentials);
        }
        if let Some(endpoint) = &config.endpoint {
            builder = builder.endpoint_url(endpoint.as_str());
        }
        Ok(Self {
            client: Client::from_conf(builder.build()),
            bucket: config.bucket.clone(),
        })
    }
}

fn credentials_from_env() -> Result<Option<Credentials>> {
    match (
        env::var("SESSIONDB_S3_ACCESS_KEY_ID"),
        env::var("SESSIONDB_S3_SECRET_ACCESS_KEY"),
    ) {
        (Err(env::VarError::NotPresent), Err(env::VarError::NotPresent)) => Ok(None),
        (Ok(access), Ok(secret)) if !access.is_empty() && !secret.is_empty() => Ok(Some(
            Credentials::new(access, secret, None, None, "sessiondb-environment"),
        )),
        _ => bail!(
            "Set both SESSIONDB_S3_ACCESS_KEY_ID and SESSIONDB_S3_SECRET_ACCESS_KEY to nonempty UTF-8 values, or unset both to use the AWS credential chain"
        ),
    }
}

fn object_error(code: Option<&str>) -> ObjectError {
    match code {
        Some("NoSuchKey" | "NotFound" | "NoSuchBucket" | "404") => ObjectError::Missing,
        Some("PreconditionFailed") => ObjectError::Precondition,
        Some("InvalidRange" | "RequestedRangeNotSatisfiable") => ObjectError::InvalidRange,
        _ => ObjectError::Unavailable,
    }
}
#[async_trait]
impl BlobStore for S3Store {
    async fn check(&self) -> Result<(), ObjectError> {
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .map_err(|_| ObjectError::Unavailable)?;
        Ok(())
    }
    async fn get(&self, key: &str, range: Option<&str>) -> Result<ObjectBody, ObjectError> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .set_range(range.map(str::to_owned))
            .send()
            .await
            .map_err(|e| object_error(e.as_service_error().and_then(ProvideErrorMetadata::code)))?;
        let byte_length = response
            .content_length()
            .filter(|v| *v >= 0)
            .ok_or(ObjectError::Unavailable)? as u64;
        let content_range = response.content_range().map(str::to_owned);
        Ok(ObjectBody {
            reader: Box::pin(response.body.into_async_read()),
            byte_length,
            content_range,
        })
    }
    async fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        media_type: &str,
        sha256: &str,
    ) -> Result<(), ObjectError> {
        let checksum = STANDARD.encode(hex::decode(sha256).map_err(|_| ObjectError::Unavailable)?);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(media_type)
            .checksum_sha256(checksum)
            .if_none_match("*")
            .body(ByteStream::from(bytes))
            .send()
            .await
            .map_err(|e| object_error(e.as_service_error().and_then(ProvideErrorMetadata::code)))?;
        Ok(())
    }
    async fn delete(&self, key: &str) -> Result<(), ObjectError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|_| ObjectError::Unavailable)?;
        Ok(())
    }
}

/// Remove old unreferenced catalog entries atomically, then delete their immutable bytes.
pub async fn cleanup(state: &AppState) -> Result<()> {
    let keys: Vec<String> = sqlx::query_scalar(
        "DELETE FROM objects o WHERE o.object_key IN (
            SELECT candidate.object_key FROM objects candidate
            WHERE candidate.created_at < now() - interval '24 hours'
              AND NOT EXISTS (SELECT 1 FROM file_generations g WHERE g.object_key=candidate.object_key)
              AND NOT EXISTS (SELECT 1 FROM artifacts a WHERE a.object_key=candidate.object_key)
            ORDER BY candidate.created_at LIMIT 128 FOR UPDATE SKIP LOCKED
        ) RETURNING o.object_key",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut failed = 0;
    for key in keys {
        if let Err(error) = state.objects.delete(&key).await {
            tracing::error!(object_key=%key, %error, "orphan object deletion failed");
            failed += 1;
        }
    }
    if failed > 0 {
        anyhow::bail!("{failed} orphan objects could not be deleted; keys are logged");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::credentials_from_env;
    use std::process::Command;

    #[test]
    fn s3_credentials_use_fixed_environment_names() {
        const CHILD: &str = "SESSIONDB_S3_CREDENTIALS_TEST_CHILD";
        if let Ok(case) = std::env::var(CHILD) {
            let result = credentials_from_env();
            match case.as_str() {
                "unset" => assert!(result.unwrap().is_none()),
                "pair" => {
                    let credentials = result.unwrap().unwrap();
                    assert_eq!(credentials.access_key_id(), "fixture-access");
                    assert_eq!(credentials.secret_access_key(), "fixture-secret");
                }
                _ => assert!(result.is_err()),
            }
            return;
        }
        for case in ["unset", "pair", "access-only", "secret-only", "empty"] {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "objects::tests::s3_credentials_use_fixed_environment_names",
                ])
                .env(CHILD, case)
                .env_remove("SESSIONDB_S3_ACCESS_KEY_ID")
                .env_remove("SESSIONDB_S3_SECRET_ACCESS_KEY");
            if matches!(case, "pair" | "access-only" | "empty") {
                child.env(
                    "SESSIONDB_S3_ACCESS_KEY_ID",
                    if case == "empty" {
                        ""
                    } else {
                        "fixture-access"
                    },
                );
            }
            if matches!(case, "pair" | "secret-only" | "empty") {
                child.env(
                    "SESSIONDB_S3_SECRET_ACCESS_KEY",
                    if case == "empty" {
                        ""
                    } else {
                        "fixture-secret"
                    },
                );
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "S3 credential case {case}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
