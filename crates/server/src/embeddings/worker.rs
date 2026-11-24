use super::enabled;
use crate::AppState;
use anyhow::Result;
use sqlx::Row;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

async fn batch(state: &AppState, owner: Uuid) -> Result<bool> {
    let Some(profile) = enabled(&state.pool).await? else {
        return Ok(false);
    };
    let rows = sqlx::query(include_str!("claim.sql"))
        .bind(profile.id)
        .bind(profile.batch_size as i32)
        .bind(owner)
        .bind(f64::from(profile.timeout_seconds) + 10.0)
        .bind(profile.include_tools)
        .fetch_all(&state.pool)
        .await?;
    if rows.is_empty() {
        return Ok(false);
    }
    let mut keys = Vec::new();
    let mut bodies = Vec::new();
    for row in &rows {
        if let Some(body) = row.try_get::<Option<&str>, _>("body")? {
            keys.push(row.try_get::<&[u8], _>("digest")?);
            bodies.push(body);
        }
    }
    if !bodies.is_empty() {
        match state.embedding_client.documents(&profile, &bodies).await {
            Ok(vectors) => {
                let mut tx = state.pool.begin().await?;
                for (digest, vector) in keys.into_iter().zip(vectors) {
                    sqlx::query(
                        "UPDATE embedding_cache
                        SET embedding=$4::real[]::vector, owner=NULL, next_attempt=NULL, attempts=0
                        WHERE profile_id=$1 AND digest=$2 AND owner=$3",
                    )
                    .bind(profile.id)
                    .bind(digest)
                    .bind(owner)
                    .bind(vector)
                    .execute(&mut *tx)
                    .await?;
                }
                tx.commit().await?;
            }
            Err(error) => {
                sqlx::query(
                    "UPDATE embedding_cache
                    SET owner=NULL, attempts=least(attempts+1,9),
                        next_attempt=now()+make_interval(secs => least(300,2^least(attempts+1,9)))
                    WHERE profile_id=$1 AND owner=$2",
                )
                .bind(profile.id)
                .bind(owner)
                .execute(&state.pool)
                .await?;
                return Err(error);
            }
        }
    }
    // A rewrite can remove the last reference between enqueue and claim.
    if bodies.len() != rows.len() {
        sqlx::query("UPDATE embedding_cache SET owner=NULL,next_attempt=now()+interval '1 minute' WHERE profile_id=$1 AND owner=$2")
            .bind(profile.id).bind(owner).execute(&state.pool).await?;
    }
    Ok(true)
}

async fn prune(state: &AppState) -> Result<bool> {
    let available: bool = sqlx::query_scalar("SELECT to_regclass('embedding_cache') IS NOT NULL")
        .fetch_one(&state.pool)
        .await?;
    if !available {
        return Ok(false);
    }
    let stale: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT c.digest FROM embedding_cache c JOIN embedding_profiles p ON p.id=c.profile_id
         WHERE NOT EXISTS (SELECT FROM embedding_sources s
                           WHERE s.digest=c.digest AND (p.include_tools OR NOT s.is_tool))
         LIMIT 128",
    )
    .fetch_all(&state.pool)
    .await?;
    if stale.is_empty() {
        return Ok(false);
    }
    let more = stale.len() == 128;
    let mut tx = state.pool.begin().await?;
    sqlx::query("LOCK TABLE embedding_profiles IN EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "DELETE FROM embedding_cache c USING embedding_profiles p
        WHERE c.digest=ANY($1) AND p.id=c.profile_id
          AND NOT EXISTS (SELECT FROM embedding_sources s
                          WHERE s.digest=c.digest AND (p.include_tools OR NOT s.is_tool))",
    )
    .bind(stale)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(more)
}

pub(crate) async fn run(state: Arc<AppState>, mut stop: watch::Receiver<bool>) {
    let owner = Uuid::new_v4();
    let mut next_cleanup = tokio::time::Instant::now() + Duration::from_secs(60);
    let mut pause = Duration::ZERO;
    loop {
        let work = async {
            tokio::time::sleep(pause).await;
            if tokio::time::Instant::now() >= next_cleanup {
                let interval = match prune(&state).await {
                    Ok(true) => 1,
                    Ok(false) => 60,
                    Err(error) => {
                        tracing::warn!(%error, "embedding cache cleanup failed");
                        60
                    }
                };
                next_cleanup = tokio::time::Instant::now() + Duration::from_secs(interval);
            }
            batch(&state, owner).await
        };
        tokio::select! {
            biased;
            _ = stop.changed() => break,
            result = work => {
                pause = match result {
                    Ok(true) => Duration::ZERO,
                    Ok(false) => Duration::from_secs(1),
                    Err(error) => {
                        tracing::warn!(%error, "embedding backfill will retry");
                        Duration::from_secs(2)
                    }
                };
            }
        }
    }
    // A graceful restart releases in-flight work immediately. Crashed leases expire.
    if let Ok(true) =
        sqlx::query_scalar::<_, bool>("SELECT to_regclass('embedding_cache') IS NOT NULL")
            .fetch_one(&state.pool)
            .await
        && let Err(error) =
            sqlx::query("UPDATE embedding_cache SET owner=NULL,next_attempt=now() WHERE owner=$1")
                .bind(owner)
                .execute(&state.pool)
                .await
    {
        tracing::warn!(%error,"embedding lease release failed");
    }
}
