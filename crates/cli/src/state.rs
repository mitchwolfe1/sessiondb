use crate::config::private_dir;
use anyhow::{Context, Result, bail};
use sessiondb_types::{FileUpload, FileUploadResult, Source};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub struct LocalState {
    pub pool: SqlitePool,
    pub directory: PathBuf,
}
pub struct Pending {
    pub source_id: Uuid,
    pub metadata: FileUpload,
    pub snapshot: PathBuf,
}
#[derive(serde::Serialize)]
pub struct RejectedFile {
    pub source_id: String,
    pub path: String,
    pub reason: String,
    pub rejected_at: String,
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
        sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS registrations(
                server TEXT NOT NULL,device TEXT NOT NULL,harness TEXT NOT NULL,
                root TEXT NOT NULL,source_id TEXT NOT NULL,
                PRIMARY KEY(server,device,harness,root)
            );
            CREATE TABLE IF NOT EXISTS pending_files(
                source_id TEXT NOT NULL,path TEXT NOT NULL,
                byte_length INTEGER NOT NULL,sha256 TEXT NOT NULL,
                expected_version TEXT,project_ids TEXT NOT NULL,snapshot TEXT NOT NULL,
                PRIMARY KEY(source_id,path)
            );
            CREATE TABLE IF NOT EXISTS checkpoints(
                source_id TEXT NOT NULL,path TEXT NOT NULL,version TEXT NOT NULL,
                sha256 TEXT NOT NULL,byte_length INTEGER NOT NULL,accepted_at TEXT NOT NULL,
                PRIMARY KEY(source_id,path)
            );
            CREATE TABLE IF NOT EXISTS rejected_files(
                server TEXT NOT NULL,source_id TEXT NOT NULL,path TEXT NOT NULL,
                sha256 TEXT NOT NULL,server_version TEXT NOT NULL,reason TEXT NOT NULL,rejected_at TEXT NOT NULL,
                PRIMARY KEY(server,source_id,path)
            );
            CREATE TABLE IF NOT EXISTS last_watch_pass(
                id INTEGER PRIMARY KEY CHECK(id=1),server TEXT NOT NULL,
                finished_at TEXT NOT NULL,error TEXT
            );",
        )
        .execute(&pool)
        .await?;
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
        let value: Option<String> = sqlx::query_scalar(
            "SELECT source_id FROM registrations WHERE server=? AND device=? AND harness=? AND root=?",
        )
        .bind(server)
        .bind(device)
        .bind(harness)
        .bind(root)
        .fetch_optional(&self.pool)
        .await?;
        value
            .map(|v| v.parse().context("invalid source ID in local state"))
            .transpose()
    }
    pub async fn register(&self, server: &str, device: &str, source: &Source) -> Result<()> {
        sqlx::query("INSERT INTO registrations(server,device,harness,root,source_id) VALUES(?,?,?,?,?) ON CONFLICT(server,device,harness,root) DO UPDATE SET source_id=excluded.source_id")
            .bind(server)
            .bind(device)
            .bind(source.harness.as_str())
            .bind(&source.root)
            .bind(source.id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn pending(&self, source: Uuid) -> Result<Vec<Pending>> {
        let rows = sqlx::query("SELECT * FROM pending_files WHERE source_id=? ORDER BY path")
            .bind(source.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Pending {
                    source_id: source,
                    metadata: FileUpload {
                        path: row.try_get("path")?,
                        byte_length: u64::try_from(row.try_get::<i64, _>("byte_length")?)?,
                        sha256: row.try_get("sha256")?,
                        expected_version: row.try_get("expected_version")?,
                        project_ids: serde_json::from_str(row.try_get("project_ids")?)?,
                    },
                    snapshot: PathBuf::from(row.try_get::<String, _>("snapshot")?),
                })
            })
            .collect()
    }
    pub async fn save_pending(&self, files: &[Pending]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for pending in files {
            sqlx::query("INSERT INTO pending_files(source_id,path,byte_length,sha256,expected_version,project_ids,snapshot) VALUES(?,?,?,?,?,?,?)")
                .bind(pending.source_id.to_string())
                .bind(&pending.metadata.path)
                .bind(i64::try_from(pending.metadata.byte_length)?)
                .bind(&pending.metadata.sha256)
                .bind(&pending.metadata.expected_version)
                .bind(serde_json::to_string(&pending.metadata.project_ids)?)
                .bind(pending.snapshot.to_str().context("snapshot path must be UTF-8")?)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn accept(
        &self,
        server: &str,
        pending: &Pending,
        receipt: &FileUploadResult,
    ) -> Result<()> {
        let file = &receipt.file;
        if file.source_id != pending.source_id
            || file.path != pending.metadata.path
            || file.sha256 != pending.metadata.sha256
            || file.byte_length != pending.metadata.byte_length
        {
            bail!("upload receipt does not match the pending file; retaining its snapshot");
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO checkpoints(source_id,path,version,sha256,byte_length,accepted_at) VALUES(?,?,?,?,?,?) ON CONFLICT(source_id,path) DO UPDATE SET version=excluded.version,sha256=excluded.sha256,byte_length=excluded.byte_length,accepted_at=excluded.accepted_at")
            .bind(file.source_id.to_string())
            .bind(&file.path)
            .bind(&file.version)
            .bind(&file.sha256)
            .bind(i64::try_from(file.byte_length)?)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM pending_files WHERE source_id=? AND path=?")
            .bind(file.source_id.to_string())
            .bind(&file.path)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM rejected_files WHERE server=? AND source_id=? AND path=?")
            .bind(server)
            .bind(file.source_id.to_string())
            .bind(&file.path)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn reject(
        &self,
        pending: &Pending,
        server: &str,
        version: &str,
        reason: &str,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO rejected_files(server,source_id,path,sha256,server_version,reason,rejected_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(server,source_id,path) DO UPDATE SET sha256=excluded.sha256,server_version=excluded.server_version,reason=excluded.reason,rejected_at=excluded.rejected_at")
            .bind(server).bind(pending.source_id.to_string()).bind(&pending.metadata.path)
            .bind(&pending.metadata.sha256).bind(version).bind(reason).bind(chrono::Utc::now().to_rfc3339())
            .execute(&mut *tx).await?;
        sqlx::query("DELETE FROM pending_files WHERE source_id=? AND path=? AND snapshot=?")
            .bind(pending.source_id.to_string())
            .bind(&pending.metadata.path)
            .bind(
                pending
                    .snapshot
                    .to_str()
                    .context("snapshot path must be UTF-8")?,
            )
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn rejection_hashes(
        &self,
        server: &str,
        source: Uuid,
    ) -> Result<HashMap<String, (String, String)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT path,sha256,server_version FROM rejected_files WHERE server=? AND source_id=?",
        )
        .bind(server)
        .bind(source.to_string())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(path, hash, version)| (path, (hash, version)))
            .collect())
    }
    pub async fn forget_rejections(
        &self,
        server: &str,
        source: Uuid,
        paths: Vec<String>,
    ) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        let id = source.to_string();
        for path in paths {
            sqlx::query("DELETE FROM rejected_files WHERE server=? AND source_id=? AND path=?")
                .bind(server)
                .bind(&id)
                .bind(path)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn retry_rejections(&self, server: &str) -> Result<u64> {
        Ok(sqlx::query("DELETE FROM rejected_files WHERE server=?")
            .bind(server)
            .execute(&self.pool)
            .await?
            .rows_affected())
    }
    pub async fn list_rejections(&self, server: &str) -> Result<Vec<RejectedFile>> {
        let rows=sqlx::query("SELECT source_id,path,reason,rejected_at FROM rejected_files WHERE server=? ORDER BY source_id,path")
            .bind(server).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(RejectedFile {
                    source_id: row.try_get("source_id")?,
                    path: row.try_get("path")?,
                    reason: row.try_get("reason")?,
                    rejected_at: row.try_get("rejected_at")?,
                })
            })
            .collect()
    }
    // The watcher holds the sync lock here, so an old worker cannot be writing
    // its previous heartbeat format while the column is removed.
    pub async fn upgrade_watch_state(&self) -> Result<()> {
        let old:bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pragma_table_info('last_watch_pass') WHERE name='context')")
            .fetch_one(&self.pool).await?;
        if old {
            sqlx::query("ALTER TABLE last_watch_pass DROP COLUMN context")
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
    pub async fn watch_finished(&self, server: &str, error: Option<String>) -> Result<()> {
        sqlx::query("INSERT INTO last_watch_pass(id,server,finished_at,error) VALUES(1,?,?,?) ON CONFLICT(id) DO UPDATE SET server=excluded.server,finished_at=excluded.finished_at,error=excluded.error")
            .bind(server).bind(chrono::Utc::now().to_rfc3339()).bind(error).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn status(&self) -> Result<serde_json::Value> {
        let files: i64 = sqlx::query_scalar("SELECT count(*) FROM checkpoints")
            .fetch_one(&self.pool)
            .await?;
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_files")
            .fetch_one(&self.pool)
            .await?;
        let sources: i64 = sqlx::query_scalar("SELECT count(*) FROM registrations")
            .fetch_one(&self.pool)
            .await?;
        let accepted_at: Option<String> =
            sqlx::query_scalar("SELECT max(accepted_at) FROM checkpoints")
                .fetch_one(&self.pool)
                .await?;
        let rejected: i64 = sqlx::query_scalar("SELECT count(*) FROM rejected_files")
            .fetch_one(&self.pool)
            .await?;
        let pass: Option<(String, String, Option<String>)> =
            sqlx::query_as("SELECT server,finished_at,error FROM last_watch_pass WHERE id=1")
                .fetch_optional(&self.pool)
                .await?;
        Ok(serde_json::json!({
            "state_path": self.directory.join("state.sqlite"),
            "sources": sources,
            "accepted_files": files,
            "pending_files": pending,
            "rejected_files": rejected,
            "last_watch_pass": pass.map(|(server,finished_at,error)| serde_json::json!({
                "server":server, "finished_at":finished_at, "error":error
            })),
            "last_accepted_at": accepted_at
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn a_legacy_watch_pass_stays_readable_until_the_new_watcher_upgrades_it() {
        let dir = tempfile::tempdir().unwrap();
        let state = LocalState::open(dir.path()).await.unwrap();
        sqlx::raw_sql("DROP TABLE last_watch_pass; CREATE TABLE last_watch_pass(id INTEGER PRIMARY KEY CHECK(id=1),server TEXT NOT NULL,context TEXT NOT NULL,finished_at TEXT NOT NULL,error TEXT);
          INSERT INTO last_watch_pass VALUES(1,'https://old.example/','previous-name','previous-pass','previous-error');")
            .execute(&state.pool).await.unwrap();
        let previous = state.status().await.unwrap()["last_watch_pass"].clone();
        assert_eq!(
            previous,
            serde_json::json!({"server":"https://old.example/","finished_at":"previous-pass","error":"previous-error"})
        );
        state.upgrade_watch_state().await.unwrap();
        state.upgrade_watch_state().await.unwrap();
        assert_eq!(state.status().await.unwrap()["last_watch_pass"], previous);
        state
            .watch_finished("https://old.example/", None)
            .await
            .unwrap();
        let next = state.status().await.unwrap();
        assert!(next["last_watch_pass"]["error"].is_null());
        assert_ne!(next["last_watch_pass"]["finished_at"], "previous-pass");
    }
}
