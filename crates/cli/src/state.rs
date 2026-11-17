use crate::config::private_dir;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sessiondb_types::{ImportManifest, Source};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct LocalState {
    pub pool: SqlitePool,
    pub directory: PathBuf,
}
#[derive(Serialize, Deserialize)]
pub struct Pending {
    pub manifest: ImportManifest,
    pub directory: PathBuf,
    pub create_key: String,
    pub complete_key: String,
    pub import_id: Option<Uuid>,
}
impl LocalState {
    pub async fn open(directory: &Path) -> Result<Self> {
        private_dir(directory)?;
        let directory = directory.canonicalize()?;
        let path = directory.join("state.sqlite");
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .busy_timeout(std::time::Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS registrations(server TEXT NOT NULL,device TEXT NOT NULL,harness TEXT NOT NULL,root TEXT NOT NULL,source_id TEXT NOT NULL,PRIMARY KEY(server,device,harness,root,source_id)); CREATE TABLE IF NOT EXISTS pending(source_id TEXT PRIMARY KEY,document TEXT NOT NULL); CREATE TABLE IF NOT EXISTS checkpoints(source_id TEXT NOT NULL,path TEXT NOT NULL,version TEXT NOT NULL,sha256 TEXT NOT NULL,byte_length INTEGER NOT NULL,accepted_at TEXT NOT NULL,PRIMARY KEY(source_id,path));").execute(&pool).await?;
        let columns = sqlx::query("PRAGMA table_info(registrations)")
            .fetch_all(&pool)
            .await?;
        if columns
            .iter()
            .any(|column| column.get::<String, _>("name") == "organization")
        {
            let mut tx = pool.begin().await?;
            sqlx::raw_sql("ALTER TABLE registrations RENAME TO registrations_scoped; CREATE TABLE registrations(server TEXT NOT NULL,device TEXT NOT NULL,harness TEXT NOT NULL,root TEXT NOT NULL,source_id TEXT NOT NULL,PRIMARY KEY(server,device,harness,root,source_id)); INSERT OR IGNORE INTO registrations(server,device,harness,root,source_id) SELECT server,device,harness,root,source_id FROM registrations_scoped; DROP TABLE registrations_scoped;").execute(&mut *tx).await?;
            tx.commit().await?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self { pool, directory })
    }
    pub async fn registration(
        &self,
        server: &str,
        device: &str,
        harness: &str,
        root: &str,
    ) -> Result<Option<Uuid>> {
        let value: Option<String>=sqlx::query_scalar("SELECT source_id FROM registrations WHERE server=? AND device=? AND harness=? AND root=? ORDER BY EXISTS(SELECT 1 FROM pending WHERE pending.source_id=registrations.source_id) DESC,source_id LIMIT 1").bind(server).bind(device).bind(harness).bind(root).fetch_optional(&self.pool).await?;
        value
            .map(|v| v.parse().context("invalid source ID in local state"))
            .transpose()
    }
    pub async fn register(&self, server: &str, device: &str, source: &Source) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO registrations(server,device,harness,root,source_id) VALUES(?,?,?,?,?)").bind(server).bind(device).bind(source.harness.as_str()).bind(&source.root).bind(source.id.to_string()).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn pending(&self, source: Uuid) -> Result<Option<Pending>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT document FROM pending WHERE source_id=?")
                .bind(source.to_string())
                .fetch_optional(&self.pool)
                .await?;
        value
            .map(|v| serde_json::from_str(&v).context("invalid pending import in local state"))
            .transpose()
    }
    pub async fn save_pending(&self, pending: &Pending) -> Result<()> {
        sqlx::query("INSERT INTO pending(source_id,document) VALUES(?,?) ON CONFLICT(source_id) DO UPDATE SET document=excluded.document").bind(pending.manifest.source_id.to_string()).bind(serde_json::to_string(pending)?).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn accept(
        &self,
        source: Uuid,
        results: &[sessiondb_types::ImportFileResult],
        manifest: &ImportManifest,
        accepted_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for result in results {
            let file = manifest
                .files
                .iter()
                .find(|f| f.path == result.path)
                .context("receipt contains an unknown file")?;
            let version = result
                .version
                .as_ref()
                .context("accepted receipt has no file version")?;
            sqlx::query("INSERT INTO checkpoints(source_id,path,version,sha256,byte_length,accepted_at) VALUES(?,?,?,?,?,?) ON CONFLICT(source_id,path) DO UPDATE SET version=excluded.version,sha256=excluded.sha256,byte_length=excluded.byte_length,accepted_at=excluded.accepted_at").bind(source.to_string()).bind(&result.path).bind(version).bind(&file.sha256).bind(file.byte_length as i64).bind(accepted_at.to_rfc3339()).execute(&mut *tx).await?;
        }
        sqlx::query("DELETE FROM pending WHERE source_id=?")
            .bind(source.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn clear_pending(&self, source: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM pending WHERE source_id=?")
            .bind(source.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn status(&self) -> Result<serde_json::Value> {
        let files: i64 = sqlx::query_scalar("SELECT count(*) FROM checkpoints")
            .fetch_one(&self.pool)
            .await?;
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM pending")
            .fetch_one(&self.pool)
            .await?;
        let sources: i64 = sqlx::query_scalar("SELECT count(*) FROM registrations")
            .fetch_one(&self.pool)
            .await?;
        let row = sqlx::query("SELECT max(accepted_at) AS accepted_at FROM checkpoints")
            .fetch_one(&self.pool)
            .await?;
        Ok(
            serde_json::json!({"state_path":self.directory.join("state.sqlite"),"sources":sources,"accepted_files":files,"pending_imports":pending,"last_accepted_at":row.get::<Option<String>,_>("accepted_at")}),
        )
    }
}
