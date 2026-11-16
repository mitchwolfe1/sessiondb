use crate::{
    AppState,
    api::imports::staging_key,
    objects::{ObjectError, read_bounded},
};
use chrono::Utc;
use serde_json::json;
use sessiondb_adapters::{NativeSession, parse};
use sessiondb_types::{
    ContentBlock, ContentReference, FileError, FullContentBlock, Harness, ImportFileResult,
    ImportManifest, Message, NativeRecordRef, PREVIEW_BYTES, ProcessingState, ReferenceType,
    Session, SessionEvent, SessionRelation, ToolOutputBlock,
};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

struct Claimed {
    id: Uuid,
    org: Uuid,
    requester: Uuid,
    source: Uuid,
    import: Uuid,
    kind: String,
    owner: Uuid,
    attempts: i32,
}
#[derive(Debug)]
struct WorkError {
    code: &'static str,
    retry: bool,
    path: Option<String>,
}
type WorkResult<T> = Result<T, WorkError>;
impl WorkError {
    fn permanent(code: &'static str) -> Self {
        Self {
            code,
            retry: false,
            path: None,
        }
    }
}
impl From<sqlx::Error> for WorkError {
    fn from(_: sqlx::Error) -> Self {
        Self {
            code: "database_unavailable",
            retry: true,
            path: None,
        }
    }
}
impl From<serde_json::Error> for WorkError {
    fn from(_: serde_json::Error) -> Self {
        Self::permanent("invalid_projection")
    }
}
impl From<ObjectError> for WorkError {
    fn from(e: ObjectError) -> Self {
        match e {
            ObjectError::Unavailable => Self {
                code: "storage_unavailable",
                retry: true,
                path: None,
            },
            ObjectError::Missing => Self::permanent("object_missing"),
            ObjectError::Precondition => Self::permanent("object_changed"),
            _ => Self::permanent("invalid_object"),
        }
    }
}

/// Start bounded workers; the database, rather than process memory, owns pending work.
pub fn spawn(
    state: Arc<AppState>,
    shutdown: watch::Receiver<bool>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut tasks = Vec::new();
    for _ in 0..state.config.workers.parse_concurrency {
        let state = state.clone();
        let shutdown = shutdown.clone();
        tasks.push(tokio::spawn(run(state, shutdown)));
    }
    tasks
}
async fn run(state: Arc<AppState>, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        match claim(&state).await {
            Ok(Some(job)) => {
                let (stop, done) = watch::channel(false);
                let heartbeat = tokio::spawn(heartbeat(state.clone(), job.id, job.owner, done));
                let result = match job.kind.as_str() {
                    "verify_import" => verify(&state, &job).await,
                    "parse" => parse_import(&state, &job).await,
                    _ => Err(WorkError::permanent("unsupported_job")),
                };
                let _ = stop.send(true);
                let _ = heartbeat.await;
                if let Err(error) = result
                    && error.code != "lease_lost"
                    && failure(&state, &job, &error).await.is_err()
                {
                    tracing::warn!(job_id=%job.id,"could not record job failure; lease will permit retry");
                }
            }
            Ok(None) => {
                tokio::select! { _ = tokio::time::sleep(Duration::from_millis(250)) => {}, _ = shutdown.changed() => {} }
            }
            Err(_) => {
                tracing::warn!("job catalog unavailable");
                tokio::select! { _ = tokio::time::sleep(Duration::from_secs(2)) => {}, _ = shutdown.changed() => {} }
            }
        }
    }
}
async fn claim(state: &AppState) -> Result<Option<Claimed>, sqlx::Error> {
    let owner = Uuid::new_v4();
    let row = sqlx::query("WITH candidate AS (SELECT id FROM jobs WHERE (status='queued' AND available_at<=now()) OR (status='running' AND lease_until<=now()) ORDER BY created_at,id FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE jobs j SET status='running',attempts=attempts+1,lease_owner=$1,lease_until=now()+interval '30 seconds',updated_at=now() FROM candidate c WHERE j.id=c.id RETURNING j.*").bind(owner).fetch_optional(&state.pool).await?;
    Ok(row.map(|r| Claimed {
        id: r.get("id"),
        org: r.get("organization_id"),
        requester: r.get("requester_id"),
        source: r.get("source_id"),
        import: r.get("import_id"),
        kind: r.get("kind"),
        owner,
        attempts: r.get("attempts"),
    }))
}
async fn heartbeat(state: Arc<AppState>, id: Uuid, owner: Uuid, mut done: watch::Receiver<bool>) {
    loop {
        tokio::select! { _ = tokio::time::sleep(Duration::from_secs(8)) => {}, _ = done.changed() => { break; } }
        let result = sqlx::query("UPDATE jobs SET lease_until=now()+interval '30 seconds' WHERE id=$1 AND lease_owner=$2 AND status='running' AND lease_until>now()").bind(id).bind(owner).execute(&state.pool).await;
        if result.is_ok_and(|r| r.rows_affected() == 0) {
            break;
        }
    }
}
async fn own_job(tx: &mut Transaction<'_, Postgres>, job: &Claimed) -> WorkResult<()> {
    let valid = sqlx::query("SELECT id FROM jobs WHERE id=$1 AND lease_owner=$2 AND status='running' AND lease_until>clock_timestamp() FOR UPDATE").bind(job.id).bind(job.owner).fetch_optional(&mut **tx).await?;
    if valid.is_some() {
        Ok(())
    } else {
        Err(WorkError::permanent("lease_lost"))
    }
}
async fn lock_source(tx: &mut Transaction<'_, Postgres>, job: &Claimed) -> WorkResult<PgRow> {
    let source = sqlx::query("SELECT * FROM sources WHERE id=$1 FOR UPDATE")
        .bind(job.source)
        .fetch_one(&mut **tx)
        .await?;
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM memberships WHERE organization_id=$1 AND user_id=$2 AND status='active'",
    )
    .bind(job.org)
    .bind(job.requester)
    .fetch_optional(&mut **tx)
    .await?;
    if source.get::<bool, _>("paused")
        || !role.is_some_and(|r| {
            source.get::<Uuid, _>("owner_id") == job.requester || r == "owner" || r == "admin"
        })
    {
        return Err(WorkError::permanent("access_revoked"));
    }
    own_job(tx, job).await?;
    Ok(source)
}
async fn finish_job(
    tx: &mut Transaction<'_, Postgres>,
    job: &Claimed,
    status: &str,
    completed: usize,
) -> WorkResult<()> {
    let rows = sqlx::query("UPDATE jobs SET status=$3,completed_items=$4,lease_until=NULL,lease_owner=NULL,updated_at=now() WHERE id=$1 AND lease_owner=$2 AND status='running' AND lease_until>now()").bind(job.id).bind(job.owner).bind(status).bind(completed as i64).execute(&mut **tx).await?;
    if rows.rows_affected() != 1 {
        return Err(WorkError::permanent("lease_lost"));
    }
    Ok(())
}
async fn failure(state: &AppState, job: &Claimed, error: &WorkError) -> WorkResult<()> {
    let mut tx = state.pool.begin().await?;
    let owned = sqlx::query("SELECT id FROM jobs WHERE id=$1 AND lease_owner=$2 AND status='running' AND lease_until>now() FOR UPDATE").bind(job.id).bind(job.owner).fetch_optional(&mut *tx).await?;
    if owned.is_none() {
        return Ok(());
    }
    if error.retry && job.attempts < 5 {
        sqlx::query("UPDATE jobs SET status='queued',available_at=now()+$3::int*interval '1 second',lease_owner=NULL,lease_until=NULL,error_code=$4,updated_at=now() WHERE id=$1 AND lease_owner=$2").bind(job.id).bind(job.owner).bind(2_i32.pow(job.attempts as u32).min(32)).bind(error.code).execute(&mut *tx).await?;
    } else {
        let errors = vec![FileError {
            path: error.path.clone(),
            code: error.code.into(),
            detail: "The import could not finish. Accepted originals remain available.".into(),
        }];
        sqlx::query("UPDATE jobs SET status='failed',error_code=$3,lease_owner=NULL,lease_until=NULL,updated_at=now() WHERE id=$1 AND lease_owner=$2").bind(job.id).bind(job.owner).bind(error.code).execute(&mut *tx).await?;
        sqlx::query("UPDATE imports SET status='failed',closed_at=now(),errors=$2 WHERE id=$1")
            .bind(job.import)
            .bind(sqlx::types::Json(errors))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

struct Verified {
    path: String,
    file_id: Uuid,
    generation: Uuid,
    version: String,
    key: String,
    unchanged: bool,
    previous_key: Option<String>,
}
async fn verify(state: &AppState, job: &Claimed) -> WorkResult<()> {
    let row = sqlx::query("SELECT manifest FROM imports WHERE id=$1")
        .bind(job.import)
        .fetch_one(&state.pool)
        .await?;
    let manifest: ImportManifest = row
        .get::<sqlx::types::Json<ImportManifest>, _>("manifest")
        .0;
    let mut verified = Vec::new();
    for (index, file) in manifest.files.iter().enumerate() {
        let prior = sqlx::query("SELECT f.*,g.object_key FROM source_files f JOIN file_generations g ON g.id=f.generation_id WHERE f.source_id=$1 AND f.path=$2").bind(job.source).bind(&file.path).fetch_optional(&state.pool).await?;
        let staged = staging_key(job.org, job.import, index);
        let key = format!("native/{}/{}", job.org, Uuid::new_v4());
        sqlx::query("INSERT INTO objects(object_key,import_id,state) VALUES($1,$2,'promotion')")
            .bind(&key)
            .bind(job.import)
            .execute(&state.pool)
            .await?;
        let check = async {
            let head = state.objects.head(&staged).await?;
            if head.byte_length != file.byte_length {
                return Err(WorkError::permanent("length_mismatch"));
            }
            state.objects.copy(&staged, &key, &head.etag).await?;
            let bytes = read_bounded(
                state.objects.as_ref(),
                &key,
                state.config.limits.max_file_bytes,
            )
            .await?;
            if bytes.len() as u64 != file.byte_length
                || hex::encode(Sha256::digest(&bytes)) != file.sha256
            {
                return Err(WorkError::permanent("checksum_mismatch"));
            }
            let unchanged = prior.as_ref().is_some_and(|p| {
                p.get::<String, _>("sha256") == file.sha256
                    && p.get::<i64, _>("byte_length") as u64 == file.byte_length
            });
            let append = prior.as_ref().is_some_and(|p| {
                let len = p.get::<i64, _>("byte_length") as usize;
                len <= bytes.len()
                    && hex::encode(Sha256::digest(&bytes[..len])) == p.get::<String, _>("sha256")
            });
            Ok(Verified {
                path: file.path.clone(),
                file_id: prior.as_ref().map_or_else(Uuid::new_v4, |p| p.get("id")),
                generation: if append {
                    prior
                        .as_ref()
                        .expect("append has prior")
                        .get("generation_id")
                } else {
                    Uuid::new_v4()
                },
                version: if unchanged {
                    prior.as_ref().expect("unchanged has prior").get("version")
                } else {
                    Uuid::new_v4().to_string()
                },
                key: if unchanged {
                    prior
                        .as_ref()
                        .expect("unchanged has prior")
                        .get("object_key")
                } else {
                    key.clone()
                },
                unchanged,
                previous_key: prior.as_ref().map(|p| p.get("object_key")),
            })
        }
        .await;
        verified.push(check.map_err(|mut e: WorkError| {
            e.path = Some(file.path.clone());
            e
        })?);
    }
    let mut tx = state.pool.begin().await?;
    lock_source(&mut tx, job).await?;
    let mut results = Vec::new();
    for (v, f) in verified.iter().zip(&manifest.files) {
        let current: Option<String> =
            sqlx::query_scalar("SELECT version FROM source_files WHERE source_id=$1 AND path=$2")
                .bind(job.source)
                .bind(&f.path)
                .fetch_optional(&mut *tx)
                .await?;
        if current != f.expected_version {
            return Err(WorkError {
                path: Some(f.path.clone()),
                ..WorkError::permanent("version_conflict")
            });
        }
        if !v.unchanged {
            sqlx::query("INSERT INTO source_files(id,source_id,path,generation_id,version,byte_length,sha256,media_type) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(source_id,path) DO UPDATE SET generation_id=EXCLUDED.generation_id,version=EXCLUDED.version,byte_length=EXCLUDED.byte_length,sha256=EXCLUDED.sha256,media_type=EXCLUDED.media_type,updated_at=now()")
                .bind(v.file_id).bind(job.source).bind(&v.path).bind(v.generation).bind(&v.version).bind(f.byte_length as i64).bind(&f.sha256).bind(&f.media_type).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO file_generations(id,file_id,object_key,byte_length,sha256) VALUES($1,$2,$3,$4,$5) ON CONFLICT(id) DO UPDATE SET object_key=EXCLUDED.object_key,byte_length=EXCLUDED.byte_length,sha256=EXCLUDED.sha256").bind(v.generation).bind(v.file_id).bind(&v.key).bind(f.byte_length as i64).bind(&f.sha256).execute(&mut *tx).await?;
            sqlx::query("UPDATE objects SET state='accepted' WHERE object_key=$1")
                .bind(&v.key)
                .execute(&mut *tx)
                .await?;
            if let Some(previous) = &v.previous_key {
                sqlx::query("UPDATE objects SET state='retired',retired_at=now() WHERE object_key=$1 AND NOT EXISTS(SELECT 1 FROM file_generations WHERE object_key=$1)").bind(previous).execute(&mut *tx).await?;
            }
        }
        sqlx::query("INSERT INTO import_files(import_id,file_id,generation_id,version,object_key,path) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING").bind(job.import).bind(v.file_id).bind(v.generation).bind(&v.version).bind(&v.key).bind(&f.path).execute(&mut *tx).await?;
        let unavailable: i64 = sqlx::query_scalar("SELECT count(*) FROM unnest($1::uuid[]) p(id) WHERE NOT EXISTS(SELECT 1 FROM projects WHERE id=p.id AND organization_id=$2 AND NOT archived)").bind(&f.project_ids).bind(job.org).fetch_one(&mut *tx).await?;
        results.push(ImportFileResult {
            path: f.path.clone(),
            status: if v.unchanged { "unchanged" } else { "accepted" }.into(),
            version: Some(v.version.clone()),
            session_ids: vec![],
            warnings: if unavailable > 0 {
                vec!["unavailable_project_labels".into()]
            } else {
                vec![]
            },
        });
    }
    sqlx::query("INSERT INTO jobs(id,organization_id,requester_id,source_id,import_id,kind,total_items) VALUES($1,$2,$3,$4,$5,'parse',$6) ON CONFLICT(import_id,kind) DO NOTHING").bind(Uuid::new_v4()).bind(job.org).bind(job.requester).bind(job.source).bind(job.import).bind(verified.len() as i64).execute(&mut *tx).await?;
    sqlx::query("UPDATE imports SET accepted_at=now(),status='queued',results=$2 WHERE id=$1 AND accepted_at IS NULL").bind(job.import).bind(sqlx::types::Json(results)).execute(&mut *tx).await?;
    sqlx::query("UPDATE sources SET last_synced_at=now() WHERE id=$1")
        .bind(job.source)
        .execute(&mut *tx)
        .await?;
    finish_job(&mut tx, job, "succeeded", verified.len()).await?;
    tx.commit().await?;
    Ok(())
}

async fn parse_import(state: &AppState, job: &Claimed) -> WorkResult<()> {
    let row = sqlx::query("SELECT manifest,results FROM imports WHERE id=$1")
        .bind(job.import)
        .fetch_one(&state.pool)
        .await?;
    let manifest: ImportManifest = row
        .get::<sqlx::types::Json<ImportManifest>, _>("manifest")
        .0;
    let mut results: Vec<ImportFileResult> = row
        .get::<sqlx::types::Json<Vec<ImportFileResult>>, _>("results")
        .0;
    let files = sqlx::query("SELECT i.*,f.sha256 FROM import_files i JOIN source_files f ON f.id=i.file_id WHERE i.import_id=$1 ORDER BY i.path").bind(job.import).fetch_all(&state.pool).await?;
    let harness: String = sqlx::query_scalar("SELECT harness FROM sources WHERE id=$1")
        .bind(job.source)
        .fetch_one(&state.pool)
        .await?;
    let harness: Harness = harness
        .parse()
        .map_err(|_| WorkError::permanent("unsupported_harness"))?;
    {
        let mut tx = state.pool.begin().await?;
        lock_source(&mut tx, job).await?;
        sqlx::query("UPDATE imports SET status='processing' WHERE id=$1 AND status IN ('queued','processing') AND accepted_at IS NOT NULL").bind(job.import).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    let mut errors = Vec::new();
    let mut completed = 0;
    let mut partial = false;
    for file in &files {
        let current: String = sqlx::query_scalar("SELECT version FROM source_files WHERE id=$1")
            .bind(file.get::<Uuid, _>("file_id"))
            .fetch_one(&state.pool)
            .await?;
        if current != file.get::<String, _>("version") {
            completed += 1;
            continue;
        }
        let bytes = read_bounded(
            state.objects.as_ref(),
            file.get("object_key"),
            state.config.limits.max_file_bytes,
        )
        .await?;
        let path: &str = file.get("path");
        let parsed = match parse(harness, path, &bytes) {
            Ok(parsed) => parsed,
            Err(_) => {
                errors.push(FileError {
                    path: Some(path.into()),
                    code: "unsupported_format".into(),
                    detail: "The original was accepted, but its native format could not be parsed."
                        .into(),
                });
                continue;
            }
        };
        partial |= !parsed.warnings.is_empty() || parsed.deferred_bytes > 0;
        let labels = manifest
            .files
            .iter()
            .find(|f| f.path == path)
            .map_or(&[][..], |f| f.project_ids.as_slice());
        let mut ids = Vec::new();
        for session in parsed.sessions {
            partial |= !session.warnings.is_empty();
            if let Some(id) = publish(state, job, file, harness, session, labels).await? {
                ids.push(id);
            }
        }
        if let Some(result) = results.iter_mut().find(|r| r.path == path) {
            result.session_ids = ids;
        }
        completed += 1;
    }
    let mut tx = state.pool.begin().await?;
    lock_source(&mut tx, job).await?;
    let status = if completed == 0 && !errors.is_empty() {
        "failed"
    } else if partial || !errors.is_empty() {
        "partial"
    } else {
        "ready"
    };
    sqlx::query("UPDATE imports SET status=$2,closed_at=now(),results=$3,errors=$4 WHERE id=$1")
        .bind(job.import)
        .bind(status)
        .bind(sqlx::types::Json(results))
        .bind(sqlx::types::Json(errors))
        .execute(&mut *tx)
        .await?;
    finish_job(
        &mut tx,
        job,
        match status {
            "ready" => "succeeded",
            "failed" => "failed",
            _ => "partial",
        },
        completed,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn publish(
    state: &AppState,
    job: &Claimed,
    file: &PgRow,
    harness: Harness,
    mut native: NativeSession,
    labels: &[Uuid],
) -> WorkResult<Option<Uuid>> {
    let mut uploaded = HashMap::new();
    for artifact in &native.artifacts {
        let key = format!("artifacts/{}/{}", job.org, Uuid::new_v4());
        sqlx::query("INSERT INTO objects(object_key,import_id,state) VALUES($1,$2,'promotion')")
            .bind(&key)
            .bind(job.import)
            .execute(&state.pool)
            .await?;
        state
            .objects
            .put(&key, artifact.bytes.clone(), &artifact.media_type)
            .await?;
        uploaded.insert(artifact.id, key);
    }
    let mut tx = state.pool.begin().await?;
    let source = lock_source(&mut tx, job).await?;
    let owner: Uuid = source.get("owner_id");
    let file_id: Uuid = file.get("file_id");
    let generation: Uuid = file.get("generation_id");
    let current: String = sqlx::query_scalar("SELECT version FROM source_files WHERE id=$1")
        .bind(file_id)
        .fetch_one(&mut *tx)
        .await?;
    if current != file.get::<String, _>("version") {
        return Ok(None);
    }
    let existing = sqlx::query("SELECT s.* FROM session_files sf JOIN sessions s ON s.id=sf.session_id WHERE sf.file_id=$1 AND s.native_key=$2 ORDER BY (sf.generation_id=$3) DESC,s.updated_at DESC LIMIT 1").bind(file_id).bind(&native.native_key).bind(generation).fetch_optional(&mut *tx).await?;
    let mut existing = existing;
    if let Some(prior) = &existing {
        let has_current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM session_files WHERE session_id=$1 AND file_id=$2 AND generation_id=$3)").bind(prior.get::<Uuid,_>("id")).bind(file_id).bind(generation).fetch_one(&mut *tx).await?;
        let other_copy: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM session_files WHERE session_id=$1 AND file_id<>$2)",
        )
        .bind(prior.get::<Uuid, _>("id"))
        .bind(file_id)
        .fetch_one(&mut *tx)
        .await?;
        if !has_current && other_copy {
            existing = None;
        }
    }
    if existing.is_none() {
        let duplicate = sqlx::query("SELECT DISTINCT s.id FROM sessions s JOIN session_files sf ON sf.session_id=s.id JOIN file_generations g ON g.id=sf.generation_id WHERE s.organization_id=$1 AND s.owner_id=$2 AND s.harness=$3 AND s.native_key=$4 AND g.sha256=$5 ORDER BY s.id LIMIT 2").bind(job.org).bind(owner).bind(harness.as_str()).bind(&native.native_key).bind(file.get::<String,_>("sha256")).fetch_all(&mut *tx).await?;
        if duplicate.len() == 1 {
            let id: Uuid = duplicate[0].get("id");
            sqlx::query("INSERT INTO session_files(session_id,file_id,generation_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(id).bind(file_id).bind(generation).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok(Some(id));
        }
    }
    let id = existing.as_ref().map_or_else(Uuid::new_v4, |r| r.get("id"));
    let revision: String = file.get("version");
    let mut artifact_ids = HashMap::new();
    for a in &native.artifacts {
        artifact_ids.insert(a.id, Uuid::new_v5(&id, a.id.as_bytes()));
    }
    for m in &mut native.messages {
        remap_artifacts(&mut m.content, &artifact_ids);
    }
    let mut session = if let Some(row) = &existing {
        row.get::<sqlx::types::Json<Session>, _>("document").0
    } else {
        Session {
            id,
            organization_id: job.org,
            author_id: owner,
            owner_id: owner,
            harness,
            native_session_id: native.native_session_id.clone(),
            title: native.title(),
            tags: vec![],
            project_ids: vec![],
            visibility: source
                .get::<String, _>("default_visibility")
                .parse()
                .map_err(|_| WorkError::permanent("invalid_visibility"))?,
            recorded_cwd: native.recorded_cwd.clone(),
            started_at: native.started_at,
            updated_at: Utc::now(),
            message_count: 0,
            processing: ProcessingState {
                parse: "ready".into(),
                keyword: "pending".into(),
                embedding: "disabled".into(),
            },
            relations: vec![],
            warnings: vec![],
        }
    };
    if existing.is_none() {
        session.project_ids = sqlx::query_scalar("SELECT id FROM projects WHERE organization_id=$1 AND id=ANY($2) AND NOT archived ORDER BY id").bind(job.org).bind(labels).fetch_all(&mut *tx).await?;
    }
    session.updated_at = native
        .messages
        .iter()
        .filter_map(|m| m.timestamp)
        .max()
        .or(native.started_at)
        .unwrap_or_else(Utc::now);
    session.message_count = native.messages.len() as u64;
    session.recorded_cwd = native.recorded_cwd;
    session.relations = native
        .relations
        .into_iter()
        .map(|r| SessionRelation {
            relation_type: r.relation_type,
            session_id: None,
            native_reference: Some(r.native_reference),
            resolved: false,
            history_boundary: r.history_boundary,
        })
        .collect();
    session.warnings = native.warnings.into_iter().take(32).collect();
    session.processing.parse = if session.warnings.is_empty() {
        "ready"
    } else {
        "partial"
    }
    .into();
    sqlx::query("INSERT INTO sessions(id,organization_id,source_id,owner_id,native_key,harness,native_session_id,visibility,revision,updated_at,document) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT(id) DO UPDATE SET revision=EXCLUDED.revision,updated_at=EXCLUDED.updated_at,document=EXCLUDED.document").bind(id).bind(job.org).bind(job.source).bind(owner).bind(&native.native_key).bind(harness.as_str()).bind(&native.native_session_id).bind(session.visibility.as_str()).bind(&revision).bind(session.updated_at).bind(sqlx::types::Json(&session)).execute(&mut *tx).await?;
    for project in &session.project_ids {
        sqlx::query("INSERT INTO session_projects(session_id,project_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(id).bind(project).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO session_files(session_id,file_id,generation_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(id).bind(file_id).bind(generation).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM messages WHERE session_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM session_events WHERE session_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let message_ids: HashMap<_, _> = native
        .messages
        .iter()
        .map(|m| (m.key.clone(), Uuid::new_v5(&id, m.key.as_bytes())))
        .collect();
    for (sequence, m) in native.messages.into_iter().enumerate() {
        let message_id = message_ids[&m.key];
        let parent = m
            .parent_key
            .as_ref()
            .and_then(|k| message_ids.get(k))
            .copied();
        let revision = hex::encode(Sha256::digest(serde_json::to_vec(&json!([
            m.content,
            parent,
            m.role,
            m.origin,
            m.provider_usage,
            m.provider_response_id,
            m.model,
            m.timestamp
        ]))?));
        let full = m.content;
        let mut message = Message {
            id: message_id,
            session_id: id,
            role: m.role,
            native_role: Some(m.native_role),
            origin: m.origin,
            sequence: sequence as u64,
            timestamp: m.timestamp,
            conversation_parent_id: parent,
            content: vec![],
            revision,
            truncated: false,
            content_url: None,
            provider_response_id: m.provider_response_id,
            model: m.model,
            provider_usage: m.provider_usage,
            warnings: m.warnings,
            source_refs: m
                .anchors
                .into_iter()
                .map(|a| NativeRecordRef {
                    file_id,
                    generation_id: generation,
                    record_id: Some(a.record_id),
                    line: a.line,
                    byte_offset: a.byte_offset,
                    byte_length: a.byte_length,
                })
                .collect(),
        };
        preview(
            &mut message,
            &full,
            state.config.public_url.as_str(),
            job.org,
        )?;
        sqlx::query("INSERT INTO messages(id,session_id,sequence,document,full_content) VALUES($1,$2,$3,$4,$5)").bind(message.id).bind(id).bind(sequence as i64).bind(sqlx::types::Json(&message)).bind(sqlx::types::Json(&full)).execute(&mut *tx).await?;
    }
    for (sequence, event) in native.events.into_iter().enumerate() {
        let event_id = Uuid::new_v5(&id, format!("event:{}", event.anchor.record_id).as_bytes());
        let event = SessionEvent {
            id: event_id,
            session_id: id,
            sequence: sequence as u64,
            event_type: event.event_type.clone(),
            timestamp: event.timestamp,
            native_type: event.event_type,
            data: event.data,
        };
        sqlx::query(
            "INSERT INTO session_events(id,session_id,sequence,document) VALUES($1,$2,$3,$4)",
        )
        .bind(event_id)
        .bind(id)
        .bind(sequence as i64)
        .bind(sqlx::types::Json(event))
        .execute(&mut *tx)
        .await?;
    }
    for a in native.artifacts {
        let artifact_id = artifact_ids[&a.id];
        let key = &uploaded[&a.id];
        let old: Option<String> =
            sqlx::query_scalar("SELECT object_key FROM artifacts WHERE id=$1")
                .bind(artifact_id)
                .fetch_optional(&mut *tx)
                .await?;
        let kind = match a.kind {
            sessiondb_types::ArtifactKind::Image => "image",
            sessiondb_types::ArtifactKind::File => "file",
            sessiondb_types::ArtifactKind::Audio => "audio",
            sessiondb_types::ArtifactKind::Video => "video",
            sessiondb_types::ArtifactKind::Other => "other",
        };
        sqlx::query("INSERT INTO artifacts(id,session_id,object_key,media_type,kind,byte_length) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(id) DO UPDATE SET object_key=EXCLUDED.object_key,media_type=EXCLUDED.media_type,byte_length=EXCLUDED.byte_length").bind(artifact_id).bind(id).bind(key).bind(&a.media_type).bind(kind).bind(a.bytes.len() as i64).execute(&mut *tx).await?;
        sqlx::query("UPDATE objects SET state='accepted' WHERE object_key=$1")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        if let Some(old) = old {
            sqlx::query("UPDATE objects SET state='retired',retired_at=now() WHERE object_key=$1")
                .bind(old)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(Some(id))
}

fn remap_artifacts(blocks: &mut [FullContentBlock], ids: &HashMap<Uuid, Uuid>) {
    for block in blocks {
        match block {
            FullContentBlock::Artifact { artifact_id, .. } => {
                if let Some(id) = ids.get(artifact_id) {
                    *artifact_id = *id;
                }
            }
            FullContentBlock::ToolResult { content, .. } => {
                for b in content {
                    if let ToolOutputBlock::Artifact { artifact_id, .. } = b
                        && let Some(id) = ids.get(artifact_id)
                    {
                        *artifact_id = *id;
                    }
                }
            }
            _ => {}
        }
    }
}
fn preview(
    message: &mut Message,
    blocks: &[FullContentBlock],
    base: &str,
    org: Uuid,
) -> WorkResult<()> {
    let url = format!(
        "{}/v1/organizations/{org}/messages/{}/content",
        base.trim_end_matches('/'),
        message.id
    );
    let mut used = 0;
    for (index, block) in blocks.iter().enumerate() {
        let len = serde_json::to_vec(block)?.len();
        if used + len <= PREVIEW_BYTES {
            message.content.push(ContentBlock::Full(block.clone()));
            used += len;
        } else {
            message.truncated = true;
            message.content_url = Some(url.clone());
            let reference = ContentReference {
                block_type: ReferenceType::ContentReference,
                content_url: url.clone(),
                revision: message.revision.clone(),
                block_index: index,
                original_type: block.kind().into(),
            };
            let len = serde_json::to_vec(&reference)?.len();
            if used + len > PREVIEW_BYTES {
                break;
            }
            message.content.push(ContentBlock::Reference(reference));
            used += len;
        }
    }
    Ok(())
}

/// Remove only closed staging/orphan objects or retired unreferenced snapshots.
pub async fn cleanup(state: &AppState) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE imports SET status='expired',closed_at=now() WHERE status='awaiting_upload' AND upload_expires_at<=now()").execute(&state.pool).await?;
    let rows = sqlx::query("SELECT o.object_key FROM objects o LEFT JOIN imports i ON i.id=o.import_id WHERE ((o.state IN ('staging','promotion') AND i.closed_at<now()-interval '24 hours') OR (o.state='retired' AND o.retired_at<now()-interval '24 hours')) AND NOT EXISTS(SELECT 1 FROM file_generations g WHERE g.object_key=o.object_key) AND NOT EXISTS(SELECT 1 FROM artifacts a WHERE a.object_key=o.object_key) AND NOT EXISTS(SELECT 1 FROM import_files f JOIN jobs j ON j.import_id=f.import_id WHERE f.object_key=o.object_key AND j.status IN ('queued','running')) ORDER BY o.created_at LIMIT 128").fetch_all(&state.pool).await?;
    for row in rows {
        let key: &str = row.get("object_key");
        if state.objects.delete(key).await.is_ok() {
            sqlx::query("DELETE FROM objects WHERE object_key=$1")
                .bind(key)
                .execute(&state.pool)
                .await?;
        }
    }
    Ok(())
}
