use crate::config::Objects;
use anyhow::{Context, Result};
use async_trait::async_trait;
use aws_sdk_s3::{
    Client,
    config::{
        BehaviorVersion, Credentials, Region, RequestChecksumCalculation,
        ResponseChecksumValidation,
    },
    error::ProvideErrorMetadata,
    presigning::PresigningConfig,
    primitives::ByteStream,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use sessiondb_types::{FileDescriptor, UploadTarget};
use sha2::{Digest, Sha256};
use std::{pin::Pin, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};

#[derive(Debug, thiserror::Error)]
pub enum ObjectError {
    #[error("object is missing")]
    Missing,
    #[error("object changed before promotion")]
    Precondition,
    #[error("object byte range is invalid")]
    InvalidRange,
    #[error("object storage is unavailable")]
    Unavailable,
    #[error("object exceeds the configured limit")]
    TooLarge,
}
pub struct ObjectMeta {
    pub byte_length: u64,
    pub etag: String,
}
pub struct ObjectBody {
    pub reader: Pin<Box<dyn AsyncRead + Send>>,
    pub byte_length: u64,
    pub content_range: Option<String>,
}

/// Object storage operations needed by imports and authorized content delivery.
#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn check(&self) -> Result<(), ObjectError>;
    async fn sign_upload(
        &self,
        key: &str,
        file: &FileDescriptor,
        seconds: u64,
    ) -> Result<UploadTarget, ObjectError>;
    async fn head(&self, key: &str) -> Result<ObjectMeta, ObjectError>;
    async fn copy(&self, source: &str, destination: &str, etag: &str) -> Result<(), ObjectError>;
    async fn get(&self, key: &str, range: Option<&str>) -> Result<ObjectBody, ObjectError>;
    async fn put(&self, key: &str, bytes: Vec<u8>, media_type: &str) -> Result<(), ObjectError>;
    async fn delete(&self, key: &str) -> Result<(), ObjectError>;
}

pub struct S3Store {
    client: Client,
    public_client: Client,
    bucket: String,
}
impl S3Store {
    /// Use an explicitly configured SDK client, including for integration environments.
    pub fn from_client(client: Client, bucket: impl Into<String>) -> Self {
        Self {
            public_client: client.clone(),
            client,
            bucket: bucket.into(),
        }
    }
    pub async fn new(config: &Objects) -> Result<Self> {
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
        if let (Some(access), Some(secret)) = (&config.access_key_env, &config.secret_key_env) {
            builder = builder.credentials_provider(Credentials::new(
                std::env::var(access).context("S3 access key environment is not set")?,
                std::env::var(secret).context("S3 secret environment is not set")?,
                None,
                None,
                "sessiondb-config",
            ));
        }
        if let Some(endpoint) = &config.endpoint {
            builder = builder.endpoint_url(endpoint.as_str());
        }
        let client = Client::from_conf(builder.clone().build());
        if let Some(endpoint) = &config.public_endpoint {
            builder = builder.endpoint_url(endpoint.as_str());
        }
        Ok(Self {
            client,
            public_client: Client::from_conf(builder.build()),
            bucket: config.bucket.clone(),
        })
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
    async fn sign_upload(
        &self,
        key: &str,
        file: &FileDescriptor,
        seconds: u64,
    ) -> Result<UploadTarget, ObjectError> {
        let checksum =
            STANDARD.encode(hex::decode(&file.sha256).map_err(|_| ObjectError::Unavailable)?);
        let signed = self
            .public_client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(&file.media_type)
            .content_length(file.byte_length as i64)
            .checksum_sha256(checksum)
            .presigned(
                PresigningConfig::expires_in(Duration::from_secs(seconds))
                    .map_err(|_| ObjectError::Unavailable)?,
            )
            .await
            .map_err(|_| ObjectError::Unavailable)?;
        Ok(UploadTarget {
            path: file.path.clone(),
            method: "PUT".into(),
            url: signed.uri().into(),
            headers: signed
                .headers()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            expires_at: Utc::now() + chrono::Duration::seconds(seconds as i64),
        })
    }
    async fn head(&self, key: &str) -> Result<ObjectMeta, ObjectError> {
        let head = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| object_error(e.as_service_error().and_then(ProvideErrorMetadata::code)))?;
        Ok(ObjectMeta {
            byte_length: head
                .content_length()
                .filter(|v| *v >= 0)
                .ok_or(ObjectError::Unavailable)? as u64,
            etag: head.e_tag().ok_or(ObjectError::Unavailable)?.into(),
        })
    }
    async fn copy(&self, source: &str, destination: &str, etag: &str) -> Result<(), ObjectError> {
        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(destination)
            .copy_source(format!("{}/{source}", self.bucket))
            .copy_source_if_match(etag)
            .send()
            .await
            .map_err(|e| object_error(e.as_service_error().and_then(ProvideErrorMetadata::code)))?;
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
    async fn put(&self, key: &str, bytes: Vec<u8>, media_type: &str) -> Result<(), ObjectError> {
        let checksum = STANDARD.encode(Sha256::digest(&bytes));
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

pub async fn read_bounded(
    store: &dyn BlobStore,
    key: &str,
    max: u64,
) -> Result<Vec<u8>, ObjectError> {
    let body = store.get(key, None).await?;
    if body.byte_length > max {
        return Err(ObjectError::TooLarge);
    }
    let mut bytes = Vec::new();
    body.reader
        .take(max + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ObjectError::Unavailable)?;
    if bytes.len() as u64 > max {
        return Err(ObjectError::TooLarge);
    }
    Ok(bytes)
}
