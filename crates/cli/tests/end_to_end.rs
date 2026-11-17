mod support;

use serde_json::{Value, json};
use sessiondb_client::{Client, Error};
use sessiondb_types::*;
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor, path::PathBuf, time::Duration};
use support::{Server, Services, cli, docker};
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(30);
fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/native")
            .join(name),
    )
    .unwrap()
}
fn descriptor(path: &str, bytes: &[u8], version: Option<String>) -> FileDescriptor {
    FileDescriptor {
        path: path.into(),
        byte_length: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
        media_type: "application/x-ndjson".into(),
        expected_version: version,
        project_ids: vec![],
    }
}
fn client(server: &Server) -> Client {
    Client::new(&server.url, server.api_key.clone(), Duration::from_secs(5)).unwrap()
}
async fn ingest(
    client: &Client,
    source: Uuid,
    path: &str,
    bytes: Vec<u8>,
    version: Option<String>,
) -> Import {
    let manifest = ImportManifest {
        source_id: source,
        files: vec![descriptor(path, &bytes, version)],
    };
    let import = client
        .create_import(&manifest, &Uuid::new_v4().to_string())
        .await
        .unwrap();
    client
        .upload(&import.uploads[0], Cursor::new(bytes))
        .await
        .unwrap();
    client
        .complete_import(import.id, &Uuid::new_v4().to_string())
        .await
        .unwrap();
    client.wait_for_processing(import.id, WAIT).await.unwrap()
}
fn status(error: Error, expected: u16) {
    match error {
        Error::Problem(p) => assert_eq!(p.status, expected, "{}", p.detail),
        other => panic!("expected HTTP {expected}, got {other}"),
    }
}
async fn source(client: &Client, harness: Harness, label: &str) -> Source {
    client
        .create_source(&SourceCreate {
            harness,
            device_label: label.into(),
            root: format!("/synthetic/{label}"),
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker and cargo build --workspace; uses isolated synthetic services"]
async fn authenticated_sync_storage_and_recovery() {
    let root = tempfile::tempdir().unwrap();
    let services = Services::start().await;
    let mut server = Server::start(root.path(), &services).await;
    let owner = client(&server);
    let peer = client(&server);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&services.database_url("postgres"))
        .await
        .unwrap();

    // The same instance key works for every client; there is no login or identity.
    for key in [None, Some("wrong-key"), Some("")] {
        let mut request = reqwest::Client::new().get(format!("{}/v1/info", server.url));
        if let Some(key) = key {
            request = request.bearer_auth(key);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 401);
        assert_eq!(response.headers()["www-authenticate"], "Bearer");
    }
    let invalid = Client::new(&server.url, "wrong-key".into(), WAIT).unwrap();
    status(
        invalid
            .sessions(&SessionFilter::default())
            .await
            .unwrap_err(),
        401,
    );
    for key in [None, Some("")] {
        let mut command = server.command(&services, "postgres");
        match key {
            Some(key) => {
                command.env("SESSIONDB_API_KEY", key);
            }
            None => {
                command.env_remove("SESSIONDB_API_KEY");
            }
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
    }
    let output = cli(&server, root.path(), &["auth", "status", "--json"]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&server.api_key));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&server.api_key));

    // Originals are accepted without project associations or a repository cwd.
    let codex = source(&owner, Harness::Codex, "codex").await;
    let bytes = fixture("codex/home-tools.jsonl");
    let manifest = ImportManifest {
        source_id: codex.id,
        files: vec![descriptor("home.jsonl", &bytes, None)],
    };
    let key = Uuid::new_v4().to_string();
    let first = owner.create_import(&manifest, &key).await.unwrap();
    let repeated = owner.create_import(&manifest, &key).await.unwrap();
    assert_eq!(first.id, repeated.id);
    let mut conflict = manifest.clone();
    conflict.files[0].sha256 = "b".repeat(64);
    status(owner.create_import(&conflict, &key).await.unwrap_err(), 409);
    owner
        .upload(&first.uploads[0], Cursor::new(bytes.clone()))
        .await
        .unwrap();
    let complete_key = Uuid::new_v4().to_string();
    owner
        .complete_import(first.id, &complete_key)
        .await
        .unwrap();
    let accepted = owner.wait_for_acceptance(first.id, WAIT).await.unwrap();
    assert!(accepted.accepted_at.is_some());
    let ready = owner.wait_for_processing(first.id, WAIT).await.unwrap();
    assert_eq!(ready.status, ImportStatus::Ready);
    assert_eq!(
        owner
            .complete_import(first.id, &complete_key)
            .await
            .unwrap()
            .id,
        first.id
    );
    let session = owner
        .sessions(&SessionFilter {
            harness: Some(Harness::Codex),
            ..Default::default()
        })
        .await
        .unwrap()
        .data
        .remove(0);
    assert!(session.project_ids.is_empty());
    assert_eq!(session.recorded_cwd.as_deref(), Some("/home/ada"));
    assert_eq!(session.message_count, 7);
    assert_eq!(peer.session(session.id).await.unwrap().id, session.id);
    assert_eq!(
        peer.messages(session.id, &MessageFilter::default())
            .await
            .unwrap()
            .data
            .len(),
        7
    );
    assert_eq!(peer.import(first.id).await.unwrap().id, first.id);
    assert_eq!(
        peer.job(ready.job_id.unwrap()).await.unwrap().status,
        JobStatus::Succeeded
    );
    let message_page = owner
        .messages(
            session.id,
            &MessageFilter {
                limit: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(message_page.data.len(), 2);
    assert!(message_page.next_cursor.is_some());
    assert_eq!(
        peer.message(message_page.data[0].id).await.unwrap().id,
        message_page.data[0].id
    );
    assert_eq!(
        peer.content(message_page.data[0].id, None)
            .await
            .unwrap()
            .message_id,
        message_page.data[0].id
    );
    let file = owner
        .source_files(codex.id, &ListOptions::default())
        .await
        .unwrap()
        .data
        .remove(0);
    let mut original = Vec::new();
    owner
        .original_to(codex.id, file.id, None, &mut original)
        .await
        .unwrap();
    assert_eq!(original, bytes);
    let range = reqwest::Client::new()
        .get(format!(
            "{}/v1/sources/{}/files/{}/content",
            server.url, codex.id, file.id
        ))
        .bearer_auth(&server.api_key)
        .header("range", "bytes=0-9")
        .send()
        .await
        .unwrap();
    assert_eq!(range.status(), 206);
    assert_eq!(range.bytes().await.unwrap().as_ref(), &bytes[..10]);
    let mut native = Vec::new();
    owner.export_to(session.id, &mut native).await.unwrap();
    let mut zip = zip_bytes(&native);
    let manifest_value: Value =
        serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
    let name = manifest_value["files"][0]["archive_path"].as_str().unwrap();
    let mut exported = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name(name).unwrap(), &mut exported).unwrap();
    assert_eq!(exported, bytes);

    // Staging is mutable, but an accepted copy must remain immutable.
    let staging = url::Url::parse(&first.uploads[0].url).unwrap();
    let staging_key = staging.path().trim_start_matches("/sessiondb-test/");
    services
        .s3_client()
        .put_object()
        .bucket("sessiondb-test")
        .key(staging_key)
        .body(aws_sdk_s3::primitives::ByteStream::from_static(
            b"changed staging",
        ))
        .send()
        .await
        .unwrap();
    let mut original = Vec::new();
    owner
        .original_to(codex.id, file.id, None, &mut original)
        .await
        .unwrap();
    assert_eq!(original, bytes);

    // Append advances the same generation and preserves manual metadata.
    sqlx::query(
        "UPDATE sessions SET document=jsonb_set(document,'{tags}','[\"manual\"]') WHERE id=$1",
    )
    .bind(session.id)
    .execute(&pool)
    .await
    .unwrap();
    let mut appended = bytes.clone();
    appended.extend_from_slice(b"{\"timestamp\":\"2026-11-16T13:00:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"append\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Appended answer.\"}]}}\n");
    let append = ingest(
        &owner,
        codex.id,
        "home.jsonl",
        appended.clone(),
        Some(file.version.clone()),
    )
    .await;
    assert_eq!(append.status, ImportStatus::Ready);
    let after = owner
        .source_files(codex.id, &ListOptions::default())
        .await
        .unwrap()
        .data
        .remove(0);
    assert_eq!(after.generation_id, file.generation_id);
    let session_after = owner.session(session.id).await.unwrap();
    assert_eq!(session_after.message_count, 8);
    assert_eq!(session_after.tags, vec!["manual"]);
    assert_eq!(peer.session(session.id).await.unwrap().message_count, 8);
    status(
        owner
            .messages(
                session.id,
                &MessageFilter {
                    cursor: message_page.next_cursor,
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
        409,
    );
    status(
        owner
            .create_import(&manifest, &Uuid::new_v4().to_string())
            .await
            .unwrap_err(),
        409,
    );

    // Rewrites retain the preceding generation while making the new original readable.
    let rewritten = fixture("codex/precision-unknown.jsonl");
    let rewrite = ingest(
        &owner,
        codex.id,
        "home.jsonl",
        rewritten.clone(),
        Some(after.version.clone()),
    )
    .await;
    assert_eq!(rewrite.status, ImportStatus::Ready);
    let precision = owner
        .messages(rewrite.results[0].session_ids[0], &MessageFilter::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&precision.data[0].content).unwrap()[0]["input"]["value"]["value"]
            .to_string(),
        "18446744073709551617"
    );
    let rewritten_file = owner
        .source_files(codex.id, &ListOptions::default())
        .await
        .unwrap()
        .data
        .remove(0);
    assert_ne!(rewritten_file.generation_id, file.generation_id);
    let mut old = Vec::new();
    owner
        .original_to(codex.id, file.id, Some(file.generation_id), &mut old)
        .await
        .unwrap();
    assert_eq!(old, appended);

    // Atomic acceptance rejects a corrupt batch without committing its valid member.
    let valid = fixture("codex/fork/parent.jsonl");
    let batch = ImportManifest {
        source_id: codex.id,
        files: vec![
            descriptor("valid.jsonl", &valid, None),
            descriptor("corrupt.jsonl", &valid, None),
        ],
    };
    let bad = owner
        .create_import(&batch, &Uuid::new_v4().to_string())
        .await
        .unwrap();
    owner
        .upload(&bad.uploads[0], Cursor::new(valid.clone()))
        .await
        .unwrap();
    let key = url::Url::parse(&bad.uploads[1].url)
        .unwrap()
        .path()
        .trim_start_matches("/sessiondb-test/")
        .to_owned();
    services
        .s3_client()
        .put_object()
        .bucket("sessiondb-test")
        .key(key)
        .body(aws_sdk_s3::primitives::ByteStream::from(vec![
            b'x';
            valid.len()
        ]))
        .send()
        .await
        .unwrap();
    owner
        .complete_import(bad.id, &Uuid::new_v4().to_string())
        .await
        .unwrap();
    let failed = owner.wait_for_processing(bad.id, WAIT).await.unwrap();
    assert_eq!(failed.status, ImportStatus::Failed);
    assert!(failed.accepted_at.is_none());
    assert!(
        !owner
            .source_files(codex.id, &ListOptions::default())
            .await
            .unwrap()
            .data
            .iter()
            .any(|f| f.path == "valid.jsonl")
    );

    // All harnesses work through the same storage/read boundary.
    let claude = source(&owner, Harness::Claude, "claude").await;
    let imported = ingest(
        &owner,
        claude.id,
        "session.jsonl",
        fixture("claude/fragments-tools.jsonl"),
        None,
    )
    .await;
    assert_eq!(imported.status, ImportStatus::Ready);
    let shared = owner
        .sessions(&SessionFilter {
            harness: Some(Harness::Claude),
            ..Default::default()
        })
        .await
        .unwrap()
        .data
        .remove(0);
    assert_eq!(shared.message_count, 4);
    peer.session(shared.id).await.unwrap();
    let pi = source(&owner, Harness::Pi, "pi").await;
    let imported = ingest(
        &owner,
        pi.id,
        "branches.jsonl",
        fixture("pi/branches-context.jsonl"),
        None,
    )
    .await;
    assert_eq!(imported.status, ImportStatus::Ready);
    let branch = owner
        .sessions(&SessionFilter {
            harness: Some(Harness::Pi),
            ..Default::default()
        })
        .await
        .unwrap()
        .data
        .remove(0);
    let messages = owner
        .messages(branch.id, &MessageFilter::default())
        .await
        .unwrap()
        .data;
    assert_eq!(
        messages[1].conversation_parent_id,
        messages[2].conversation_parent_id
    );
    let ancestry = owner
        .messages(
            branch.id,
            &MessageFilter {
                leaf_id: Some(messages[3].id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(ancestry.data.len(), 3);
    let image = ingest(
        &owner,
        pi.id,
        "image.jsonl",
        fixture("pi/artifact.jsonl"),
        None,
    )
    .await;
    assert_eq!(image.status, ImportStatus::Ready);
    let image_session = owner
        .session(image.results[0].session_ids[0])
        .await
        .unwrap();
    let image_messages = owner
        .messages(image_session.id, &MessageFilter::default())
        .await
        .unwrap()
        .data;
    let block = serde_json::to_value(&image_messages[1].content).unwrap();
    let artifact_id: Uuid = block[0]["content"][0]["artifact_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let artifact = owner.artifact(artifact_id).await.unwrap();
    assert_eq!(artifact.byte_length, 68);
    let mut png = Vec::new();
    owner.artifact_to(artifact_id, &mut png).await.unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(peer.artifact(artifact_id).await.unwrap().byte_length, 68);

    // Authentication protects every data surface, not only session listings.
    for path in [
        format!("sources/{}", codex.id),
        format!("sources/{}/files/{}/content", codex.id, file.id),
        format!("imports/{}", first.id),
        format!("jobs/{}", ready.job_id.unwrap()),
        format!("sessions/{}", shared.id),
        format!("sessions/{}/messages", shared.id),
        format!("sessions/{}/export", shared.id),
        format!("messages/{}", message_page.data[0].id),
        format!("messages/{}/content", message_page.data[0].id),
        format!("artifacts/{artifact_id}"),
        format!("artifacts/{artifact_id}/content"),
    ] {
        let response = reqwest::Client::new()
            .get(format!("{}/v1/{path}", server.url))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401, "{path}");
    }

    // Parent history resolves across native stores for every key-holding client.
    let parent_source = source(&owner, Harness::Codex, "archived-parent").await;
    let parent = ingest(
        &owner,
        parent_source.id,
        "parent.jsonl",
        fixture("codex/fork/parent.jsonl"),
        None,
    )
    .await
    .results[0]
        .session_ids[0];
    let child_source = source(&owner, Harness::Codex, "active-child").await;
    let child = ingest(
        &owner,
        child_source.id,
        "child.jsonl",
        fixture("codex/fork/child.jsonl"),
        None,
    )
    .await
    .results[0]
        .session_ids[0];
    let owner_child = owner.session(child).await.unwrap();
    assert_eq!(owner_child.message_count, 2);
    assert!(
        owner_child
            .relations
            .iter()
            .all(|r| r.resolved && r.session_id == Some(parent))
    );
    let reader_child = peer.session(child).await.unwrap();
    assert!(
        reader_child
            .relations
            .iter()
            .all(|r| r.resolved && r.session_id == Some(parent))
    );
    let mut owner_bundle = Vec::new();
    owner.export_to(child, &mut owner_bundle).await.unwrap();
    let mut archive = zip_bytes(&owner_bundle);
    let manifest: Value =
        serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(manifest["files"].as_array().unwrap().len(), 2);
    let mut reader_bundle = Vec::new();
    peer.export_to(child, &mut reader_bundle).await.unwrap();
    let mut archive = zip_bytes(&reader_bundle);
    let manifest: Value =
        serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(manifest["files"].as_array().unwrap().len(), 2);
    assert_eq!(peer.session(parent).await.unwrap().id, parent);

    // Large structured content expands intact with revision protection.
    let large=format!("{}\n{}\n",json!({"type":"session_meta","payload":{"id":"large-session","cwd":"/home/ada"}}),json!({"type":"response_item","payload":{"type":"function_call","id":"large-call","call_id":"large","name":"fixture","arguments":json!({"text":"ü".repeat(40000)}).to_string()}})).into_bytes();
    let large = ingest(&owner, codex.id, "large.jsonl", large, None).await;
    let large_session = large.results[0].session_ids[0];
    let m = owner
        .messages(large_session, &MessageFilter::default())
        .await
        .unwrap()
        .data
        .remove(0);
    assert!(m.truncated);
    assert!(serde_json::to_vec(&m.content).unwrap().len() <= PREVIEW_BYTES + 2);
    let expanded = owner.content(m.id, Some(&m.revision)).await.unwrap();
    assert_eq!(
        serde_json::to_value(&expanded.content).unwrap()[0]["input"]["value"]["text"]
            .as_str()
            .unwrap()
            .len(),
        80000
    );
    status(owner.content(m.id, Some("stale")).await.unwrap_err(), 412);

    // The real CLI captures a complete prefix, resumes after an S3 outage, and exports.
    let local = root.path().join("native");
    fs::create_dir(&local).unwrap();
    let mut captured = fixture("codex/home-tools.jsonl");
    captured.extend_from_slice(b"{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"watch-tail\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"");
    fs::write(local.join("cli.jsonl"), &captured).unwrap();
    let output = cli(&server, root.path(), &["context", "create", "test"]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = cli(
        &server,
        root.path(),
        &[
            "sources",
            "add",
            "--harness",
            "codex",
            "--root",
            local.to_str().unwrap(),
            "--json",
        ],
    )
    .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    docker(&["pause", &services.s3]);
    let output = cli(&server, root.path(), &["--timeout", "1", "sync", "--json"]).await;
    assert!(!output.status.success());
    docker(&["unpause", &services.s3]);
    let output = cli(&server, root.path(), &["sync", "status", "--json"]).await;
    let pending: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(pending["pending_imports"], 1);
    let output = cli(&server, root.path(), &["sync", "--json"]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status_output = cli(&server, root.path(), &["sync", "status", "--json"]).await;
    let synced: Value = serde_json::from_slice(&status_output.stdout).unwrap();
    assert_eq!(synced["pending_imports"], 0);
    assert!(synced["accepted_files"].as_i64().unwrap() > 0);
    let enabled: Value = serde_json::from_slice(&output.stdout).unwrap();
    let source_id: Uuid = enabled["sources"][0]["source_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let cli_file = owner
        .source_files(source_id, &ListOptions::default())
        .await
        .unwrap()
        .data
        .remove(0);
    let mut capture = Vec::new();
    owner
        .original_to(source_id, cli_file.id, None, &mut capture)
        .await
        .unwrap();
    assert_eq!(capture, sessiondb_adapters::complete_prefix(&captured));

    // Foreground watch finds the completed tail and exits conventionally on SIGINT.
    let config_path = root.path().join("client.toml");
    let mut local_config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    local_config["sync"]["reconcile_interval_seconds"] = toml::Value::Integer(1);
    fs::write(&config_path, toml::to_string(&local_config).unwrap()).unwrap();
    let watch_log = fs::File::create(root.path().join("watch.log")).unwrap();
    let mut watch = support::cli_command(
        &server,
        root.path(),
        &["--quiet", "sync", "--watch", "--json"],
    )
    .stdout(watch_log.try_clone().unwrap())
    .stderr(watch_log)
    .spawn()
    .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let competing = cli(&server, root.path(), &["sync"]).await;
    assert!(!competing.status.success());
    captured.extend_from_slice(b"Watched answer.\"}]}}\n");
    fs::write(local.join("cli.jsonl"), &captured).unwrap();
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        let current = owner
            .source_files(source_id, &ListOptions::default())
            .await
            .unwrap()
            .data
            .remove(0);
        if current.byte_length == captured.len() as u64 {
            let local = cli(&server, root.path(), &["sync", "status", "--json"]).await;
            if serde_json::from_slice::<Value>(&local.stdout).unwrap()["pending_imports"] == 0 {
                break;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "watch did not synchronize the completed tail"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    std::process::Command::new("kill")
        .args(["-INT", &watch.id().unwrap().to_string()])
        .status()
        .unwrap();
    assert_eq!(watch.wait().await.unwrap().code(), Some(130));
    let destination = root.path().join("preserved.zip");
    fs::write(&destination, b"existing export").unwrap();
    let missing = Uuid::new_v4().to_string();
    let failed_export = cli(
        &server,
        root.path(),
        &[
            "sessions",
            "export",
            &missing,
            "--output",
            destination.to_str().unwrap(),
            "--force",
        ],
    )
    .await;
    assert!(!failed_export.status.success());
    assert_eq!(fs::read(destination).unwrap(), b"existing export");

    // Complete through an API-only process, then recover an expired worker lease.
    let queued_bytes = fixture("codex/fork/parent.jsonl");
    let queued_manifest = ImportManifest {
        source_id: codex.id,
        files: vec![descriptor("queued.jsonl", &queued_bytes, None)],
    };
    let queued = owner
        .create_import(&queued_manifest, &Uuid::new_v4().to_string())
        .await
        .unwrap();
    owner
        .upload(&queued.uploads[0], Cursor::new(queued_bytes))
        .await
        .unwrap();
    server.stop();
    let configuration: sessiondb_server::config::Config =
        toml::from_str(&fs::read_to_string(&server.config).unwrap()).unwrap();
    let api_state = std::sync::Arc::new(sessiondb_server::AppState {
        auth: sessiondb_server::auth::Authenticator::from_key(&server.api_key).unwrap(),
        config: configuration,
        pool: pool.clone(),
        objects: std::sync::Arc::new(sessiondb_server::objects::S3Store::from_client(
            services.s3_client(),
            "sessiondb-test",
        )),
        requests: std::sync::atomic::AtomicU64::new(0),
    });
    let listener = tokio::net::TcpListener::bind(server.url.trim_start_matches("http://"))
        .await
        .unwrap();
    let (stop, closed) = tokio::sync::oneshot::channel();
    let api_task = tokio::spawn(async move {
        axum::serve(listener, sessiondb_server::api::router(api_state))
            .with_graceful_shutdown(async move {
                let _ = closed.await;
            })
            .await
            .unwrap();
    });
    let scheduled = owner
        .complete_import(queued.id, &Uuid::new_v4().to_string())
        .await
        .unwrap();
    assert_eq!(scheduled.status, ImportStatus::Verifying);
    assert!(scheduled.accepted_at.is_none());
    sqlx::query("UPDATE jobs SET status='running',attempts=1,lease_owner=$2,lease_until=now()-interval '1 second' WHERE id=$1").bind(scheduled.job_id.unwrap()).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    stop.send(()).unwrap();
    api_task.await.unwrap();
    server.restart(&services, "postgres").await;
    let recovered = owner.wait_for_processing(queued.id, WAIT).await.unwrap();
    assert_eq!(recovered.status, ImportStatus::Ready);
    let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM jobs WHERE id=$1")
        .bind(scheduled.job_id.unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(attempts >= 2);

    // Restart both the API process and object service, then restore the catalog.
    server.stop();
    docker(&["restart", &services.s3]);
    server.write_config(&services);
    server.restart(&services, "postgres").await;
    assert_eq!(
        owner
            .complete_import(first.id, &complete_key)
            .await
            .unwrap()
            .id,
        first.id
    );
    let mut persisted = Vec::new();
    owner
        .original_to(codex.id, rewritten_file.id, None, &mut persisted)
        .await
        .unwrap();
    assert_eq!(persisted, rewritten);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    server.stop();
    services.backup_restore();
    server.restart(&services, "restored").await;
    assert_eq!(
        owner
            .sessions(&SessionFilter {
                limit: Some(200),
                ..Default::default()
            })
            .await
            .unwrap()
            .data
            .len() as i64,
        count
    );
    let mut restored = Vec::new();
    owner
        .original_to(codex.id, rewritten_file.id, None, &mut restored)
        .await
        .unwrap();
    assert_eq!(restored, rewritten);
    let output = cli(&server, root.path(), &["sync", "--json"]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["sources"][0]["accepted_files"], 0);

    // Restoring behind another public address must not expose stale content URLs.
    let preview_url = format!("{}/v1/messages/{}", server.url, m.id);
    let before = reqwest::Client::new()
        .get(&preview_url)
        .bearer_auth(&server.api_key)
        .send()
        .await
        .unwrap()
        .headers()["etag"]
        .to_str()
        .unwrap()
        .to_owned();
    server.stop();
    let configuration = fs::read_to_string(&server.config).unwrap().replace(
        &format!("public_url = \"{}\"", server.url),
        "public_url = \"https://moved.example.com\"",
    );
    fs::write(&server.config, configuration).unwrap();
    server.restart(&services, "restored").await;
    let relocated = owner.message(m.id).await.unwrap();
    assert!(
        relocated
            .content_url
            .as_ref()
            .unwrap()
            .starts_with("https://moved.example.com/")
    );
    assert_eq!(relocated.revision, m.revision);
    owner
        .content(m.id, Some(&relocated.revision))
        .await
        .unwrap();
    let after = reqwest::Client::new()
        .get(&preview_url)
        .bearer_auth(&server.api_key)
        .send()
        .await
        .unwrap()
        .headers()["etag"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_ne!(before, after);

    // Replacing the one instance key revokes every previously configured client.
    server.api_key = format!("rotated-{}", Uuid::new_v4());
    server.restart(&services, "restored").await;
    status(owner.session(shared.id).await.unwrap_err(), 401);
    status(peer.artifact(artifact_id).await.unwrap_err(), 401);
    let replacement = client(&server);
    assert_eq!(replacement.session(shared.id).await.unwrap().id, shared.id);
    let mut still_archived = Vec::new();
    replacement
        .original_to(codex.id, rewritten_file.id, None, &mut still_archived)
        .await
        .unwrap();
    assert_eq!(still_archived, rewritten);
    assert!(
        !fs::read_to_string(&server.log)
            .unwrap()
            .contains(&server.api_key)
    );
    server.stop();
    pool.close().await;
}

fn zip_bytes(bytes: &[u8]) -> zip::ZipArchive<Cursor<&[u8]>> {
    zip::ZipArchive::new(Cursor::new(bytes)).unwrap()
}
