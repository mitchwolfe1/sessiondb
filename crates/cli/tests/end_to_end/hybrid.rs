use super::support::{Server, Services, cli};
use super::{WAIT, client, descriptor, ingest, keywords, source, status};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde_json::{Value, json};
use sessiondb_client::Client;
use sessiondb_types::*;
use std::{
    collections::HashMap,
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

#[derive(Default)]
struct Provider {
    fail_docs: AtomicBool,
    fail_queries: AtomicBool,
    documents: Mutex<HashMap<String, usize>>,
}

async fn embed(
    State(f): State<Arc<Provider>>,
    Json(request): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let inputs = request["input"].as_array().unwrap();
    let mut data = Vec::new();
    for (i, text) in inputs.iter().enumerate() {
        let text = text.as_str().unwrap();
        let query = text.starts_with("Q|");
        assert!(query || text.starts_with("D|"));
        if (query && f.fail_queries.load(Ordering::SeqCst))
            || (!query && f.fail_docs.load(Ordering::SeqCst))
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        if !query {
            *f.documents
                .lock()
                .unwrap()
                .entry(text.to_owned())
                .or_default() += 1;
        }
        let bird = ["raptor", "falcon"].iter().any(|word| text.contains(word));
        let plant = text.contains("lavender");
        assert!(text.len() <= 7900);
        let vector = if text == "Q|invalid-dimensions" {
            vec![1.0]
        } else if text == "Q|zero-vector" {
            vec![0.0; 3]
        } else if request["model"] == "two" {
            vec![if bird { 2.0 } else { 0.1 }, 1.0]
        } else {
            vec![
                if bird { 3.0 } else { 0.0 },
                if plant { 3.0 } else { 0.0 },
                1.0,
            ]
        };
        data.push(json!({"index":i,"embedding":vector}));
    }
    data.reverse();
    Ok(Json(json!({"model":request["model"],"data":data})))
}

fn message(id: &str, role: &str, body: &str) -> String {
    format!(
        "{}\n",
        json!({"type":"response_item","timestamp":"2026-02-20T00:00:00Z","payload":{
            "type":"message","id":id,"role":role,"content":[{"type":if role=="user"{"input_text"}else{"output_text"},"text":body}]
        }})
    )
}
fn session(id: &str, question: &str, reply: &str) -> Vec<u8> {
    format!(
        "{}\n{}{}",
        json!({"type":"session_meta","payload":{"id":id}}),
        message("q", "user", question),
        message("a", "assistant", reply)
    )
    .into_bytes()
}

async fn wait_ready(client: &Client, id: Uuid) -> EmbeddingProgress {
    tokio::time::timeout(WAIT, async {
        loop {
            let progress = client
                .embedding_profile(id)
                .await
                .unwrap()
                .progress
                .unwrap();
            if progress.pending_inputs == 0 && progress.ready_inputs > 0 {
                return progress;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    })
    .await
    .expect("embedding backfill did not finish")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker, the pgvector image and built workspace binaries"]
async fn native_vectors_backfill_filter_reuse_and_recover() {
    let fixture = Arc::new(Provider::default());
    fixture.fail_docs.store(true, Ordering::SeqCst);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/v1/embeddings", post(embed))
        .with_state(fixture.clone());
    let mock = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let services = Services::with_postgres("pgvector/pgvector:0.8.2-pg18-trixie").await;
    let pool = sqlx::PgPool::connect(&services.database_url("postgres"))
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut server = Server::start(root.path(), &services).await;
    let api = client(&server);
    let src = source(&api, Harness::Codex, "hybrid-fixture").await;
    let original = session("mixed", "lavender beds", "unique raptor nest");
    let file = ingest(&api, src.id, "first.jsonl", original.clone(), None).await;
    let other = ingest(
        &api,
        src.id,
        "second.jsonl",
        session("second", "raptor inquiry", "other answer"),
        None,
    )
    .await;
    let base: EmbeddingProfileCreate = serde_json::from_value(json!({
        "name": "Synthetic similarity",
        "provider": "openai_compatible",
        "base_url": endpoint,
        "model": "three",
        "model_revision": "fixture-1",
        "dimensions": 3,
        "query_prefix": "Q|",
        "document_prefix": "D|",
        "batch_size": 8
    }))
    .unwrap();
    let mut invalid = base.clone();
    invalid.credential_ref = Some("env:SESSIONDB_DATABASE_URL".into());
    status(
        api.create_embedding_profile(&invalid).await.unwrap_err(),
        400,
    );
    let denied = Client::new(&server.url, "wrong".into(), WAIT).unwrap();
    status(
        denied.create_embedding_profile(&base).await.unwrap_err(),
        401,
    );
    let first = api.create_embedding_profile(&base).await.unwrap();
    let mut query = SearchRequest {
        mode: SearchMode::Hybrid,
        ..keywords("unique")
    };
    status(api.search(&query).await.unwrap_err(), 409);
    let enable = EmbeddingProfilePatch { enabled: true };
    status(
        api.update_embedding_profile(first.id, &enable)
            .await
            .unwrap_err(),
        409,
    );
    sqlx::query("CREATE EXTENSION vector")
        .execute(&pool)
        .await
        .unwrap();
    let migrated = server
        .command(&services, "postgres")
        .arg("migrate")
        .output()
        .unwrap();
    assert!(
        migrated.status.success(),
        "{}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    api.update_embedding_profile(first.id, &enable)
        .await
        .unwrap();
    let fresh = api.search(&query).await.unwrap();
    assert_eq!(fresh.mode, SearchMode::Hybrid);
    assert_eq!(fresh.coverage.embedding_eligible_passages, 2);
    assert_eq!(fresh.coverage.embedded_passages, 0);
    assert_eq!(fresh.data[0].retrieved_by, [SearchMode::Keyword]);
    let conflict = EmbeddingProfileCreate {
        enabled: true,
        ..base.clone()
    };
    status(
        api.create_embedding_profile(&conflict).await.unwrap_err(),
        409,
    );

    fixture.fail_docs.store(false, Ordering::SeqCst);
    let baseline = wait_ready(&api, first.id).await;
    assert_eq!(baseline.ready_inputs, 6);
    let both = api.search(&query).await.unwrap();
    assert_eq!(both.coverage.embedded_passages, 2);
    assert_eq!(both.coverage.embedding_profile_id, Some(first.id));
    assert!(
        both.data
            .iter()
            .any(|h| h.retrieved_by == [SearchMode::Keyword, SearchMode::Semantic])
    );
    query.query = "falcon".into();
    query.filters.roles = vec![Role::User];
    let scoped = api.search(&query).await.unwrap();
    assert_eq!(scoped.data[0].session_id, other.session_ids[0]);
    assert!(
        scoped
            .data
            .iter()
            .all(|h| h.role == Role::User && h.retrieved_by == [SearchMode::Semantic])
    );
    query.filters = SearchFilters::default();
    query.mode = SearchMode::Semantic;
    query.limit = Some(1);
    let hit = api.search(&query).await.unwrap();
    let cursor = hit.next_cursor.unwrap();
    let anchor = &hit.data[0];
    assert!(
        api.read_around(
            anchor.message_id,
            &AroundOptions {
                leaf_id: Some(anchor.leaf_id),
                ..Default::default()
            }
        )
        .await
        .unwrap()
        .data
        .iter()
        .any(|m| m.id == anchor.message_id)
    );
    query.cursor = Some(cursor.clone());
    assert_ne!(
        api.search(&query).await.unwrap().data[0].message_id,
        anchor.message_id
    );
    query.cursor = None;

    for bad in ["invalid-dimensions", "zero-vector"] {
        status(
            api.search(&SearchRequest {
                mode: SearchMode::Semantic,
                ..keywords(bad)
            })
            .await
            .unwrap_err(),
            503,
        );
    }
    let projects = [Uuid::new_v4(), Uuid::new_v4()];
    for (i, id) in projects.into_iter().enumerate() {
        sqlx::query("INSERT INTO projects(id,slug,name) VALUES($1,$2,'classification')")
            .bind(id)
            .bind(format!("p{i}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO session_projects(session_id,project_id) VALUES($1,$2)")
            .bind(other.session_ids[0])
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    for mode in [SearchMode::Semantic, SearchMode::Hybrid] {
        let mut scoped = SearchRequest {
            mode,
            ..keywords("falcon")
        };
        scoped.filters.project_ids = projects.to_vec();
        let hits = api.search(&scoped).await.unwrap();
        assert_eq!(hits.data.len(), 1);
        assert_eq!(hits.data[0].session_id, other.session_ids[0]);
        assert_eq!(hits.coverage.embedded_passages, 1);
        scoped.filters = SearchFilters {
            unassigned: true,
            ..Default::default()
        };
        assert_eq!(
            api.search(&scoped).await.unwrap().data[0].session_id,
            file.session_ids[0]
        );
    }
    let calls = fixture.documents.lock().unwrap().clone();
    let repeat = api
        .upload_file(
            src.id,
            &descriptor("first.jsonl", &original, None),
            Cursor::new(original.clone()),
        )
        .await
        .unwrap();
    assert!(repeat.unchanged);
    server.restart(&services, "postgres").await;
    wait_ready(&api, first.id).await;
    assert_eq!(*fixture.documents.lock().unwrap(), calls);
    let mut appended = original;
    appended.extend_from_slice(message("extra", "assistant", "newly accepted zephyr").as_bytes());
    let append = ingest(
        &api,
        src.id,
        "first.jsonl",
        appended,
        Some(file.file.version),
    )
    .await;
    wait_ready(&api, first.id).await;
    assert_eq!(fixture.documents.lock().unwrap()["D|unique raptor nest"], 1);
    assert_eq!(
        fixture.documents.lock().unwrap()["D|newly accepted zephyr"],
        1
    );
    let rewritten = ingest(
        &api,
        src.id,
        "first.jsonl",
        session("mixed", "fresh source", "lavender only"),
        Some(append.file.version),
    )
    .await;
    assert_eq!(rewritten.session_ids, file.session_ids);
    let current = api
        .messages(file.session_ids[0], &MessageFilter::default())
        .await
        .unwrap()
        .data;
    let after = api
        .search(&SearchRequest {
            mode: SearchMode::Hybrid,
            ..keywords("zephyr")
        })
        .await
        .unwrap();
    assert!(
        after
            .data
            .iter()
            .filter(|h| h.session_id == file.session_ids[0])
            .all(|h| current.iter().any(|m| m.id == h.message_id) && !h.excerpt.contains("zephyr"))
    );
    wait_ready(&api, first.id).await;

    fixture.fail_queries.store(true, Ordering::SeqCst);
    status(api.search(&query).await.unwrap_err(), 503);
    assert!(
        !api.search(&keywords("lavender"))
            .await
            .unwrap()
            .data
            .is_empty()
    );
    fixture.fail_queries.store(false, Ordering::SeqCst);
    ingest(
        &api,
        src.id,
        "tools.jsonl",
        super::fixture("codex/home-tools.jsonl"),
        None,
    )
    .await;
    let tools = SearchRequest {
        mode: SearchMode::Semantic,
        filters: SearchFilters {
            tool_name: Some("exec_command".into()),
            ..Default::default()
        },
        ..keywords("directory")
    };
    assert!(api.search(&tools).await.unwrap().data.is_empty());
    let long = "é".repeat(6000) + " lastpiece";
    ingest(
        &api,
        src.id,
        "long.jsonl",
        session("long", &long, "end"),
        None,
    )
    .await;
    wait_ready(&api, first.id).await;
    assert!(
        fixture
            .documents
            .lock()
            .unwrap()
            .keys()
            .any(|input| input.contains("lastpiece"))
    );
    api.update_embedding_profile(first.id, &EmbeddingProfilePatch { enabled: false })
        .await
        .unwrap();
    let next = api
        .create_embedding_profile(&EmbeddingProfileCreate {
            model: "two".into(),
            dimensions: 2,
            include_tools: true,
            enabled: true,
            ..base
        })
        .await
        .unwrap();
    query.cursor = Some(cursor);
    status(api.search(&query).await.unwrap_err(), 400);
    wait_ready(&api, next.id).await;
    let tool_hits = api.search(&tools).await.unwrap();
    assert!(tool_hits.data.iter().any(|h| h.block_type == "tool_result"));
    assert!(
        tool_hits
            .data
            .iter()
            .all(|h| h.tool_name.as_deref() == Some("exec_command"))
    );
    let output = cli(
        &server,
        root.path(),
        &["search", "falcon", "--mode", "hybrid", "--json"],
    )
    .await;
    assert!(output.status.success());
    let final_result: SearchResponse = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(final_result.coverage.embedding_profile_id, Some(next.id));
    assert_eq!(
        api.embedding_profiles(&ProfileListOptions::default())
            .await
            .unwrap()
            .data
            .len(),
        2
    );
    server.stop();
    mock.abort();
}
