//! SessionDB API, PostgreSQL catalog, S3 storage, and durable workers.
pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod jobs;
pub mod objects;

use anyhow::{Context, Result, bail};
use auth::Authenticator;
use config::Config;
use objects::BlobStore;
use sqlx::PgPool;
use sqlx::{Row, postgres::PgPoolOptions};
use std::sync::Arc;
use tokio::{net::TcpListener, sync::watch};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub struct AppState {
    pub config: Config,
    pub pool: PgPool,
    pub objects: Arc<dyn BlobStore>,
    pub auth: Authenticator,
    pub requests: std::sync::atomic::AtomicU64,
}

impl AppState {
    pub async fn connect(config: Config) -> Result<Arc<Self>> {
        config.validate()?;
        let pool = PgPoolOptions::new()
            .max_connections(12)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&config.database_url()?)
            .await
            .context("connect to PostgreSQL")?;
        check_migrations(&pool).await?;
        let objects = Arc::new(objects::S3Store::new(&config.objects).await?);
        let auth = Authenticator::new(config.auth.clone())?;
        Ok(Arc::new(Self {
            config,
            pool,
            objects,
            auth,
            requests: std::sync::atomic::AtomicU64::new(0),
        }))
    }
}
pub async fn check_migrations(pool: &PgPool) -> Result<()> {
    let rows =
        sqlx::query("SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .context(
                "catalog schema is not initialized; run sessiondb-server migrate --config PATH",
            )?;
    let migrations: Vec<_> = MIGRATOR
        .iter()
        .filter(|m| !m.migration_type.is_down_migration())
        .collect();
    if rows.len() != migrations.len() {
        bail!(
            "catalog schema differs from this server; run sessiondb-server migrate --config PATH"
        );
    }
    for (row, migration) in rows.iter().zip(migrations) {
        if row.get::<i64, _>("version") != migration.version
            || !row.get::<bool, _>("success")
            || row.get::<Vec<u8>, _>("checksum") != migration.checksum.as_ref()
        {
            bail!("catalog migration version/checksum does not match this server");
        }
    }
    Ok(())
}

/// Serve the API and drain active work on shutdown. Unfinished leases permit recovery.
pub async fn serve(
    state: Arc<AppState>,
    listener: TcpListener,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let workers = jobs::spawn(state.clone(), shutdown.clone());
    let cleanup_state = state.clone();
    let mut cleanup_stop = shutdown.clone();
    let cleanup = tokio::spawn(async move {
        loop {
            tokio::select! { _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}, _ = cleanup_stop.changed() => { break; } }
            if jobs::cleanup(&cleanup_state).await.is_err() {
                tracing::warn!("object cleanup will retry");
            }
        }
    });
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async move {
            if !*shutdown.borrow() {
                let _ = shutdown.changed().await;
            }
        })
        .await?;
    for mut worker in workers {
        if tokio::time::timeout(std::time::Duration::from_secs(60), &mut worker)
            .await
            .is_err()
        {
            worker.abort();
        }
    }
    cleanup.abort();
    Ok(())
}
