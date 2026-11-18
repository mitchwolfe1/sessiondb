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
fn descriptor(path: &str, bytes: &[u8], version: Option<String>) -> FileUpload {
    FileUpload {
        path: path.into(),
        byte_length: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
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
) -> FileUploadResult {
    let metadata = descriptor(path, &bytes, version);
    client
        .upload_file(source, &metadata, Cursor::new(bytes))
        .await
        .unwrap()
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
    let metadata = descriptor("home.jsonl", &bytes, None);
    let (first, repeated) = tokio::join!(
        owner.upload_file(codex.id, &metadata, Cursor::new(bytes.clone())),
        peer.upload_file(codex.id, &metadata, Cursor::new(bytes.clone())),
    );
    let first = first.unwrap();
    let repeated = repeated.unwrap();
    assert!(first.unchanged || repeated.unchanged);
    assert_eq!(first.file.id, repeated.file.id);
    assert_eq!(first.file.version, repeated.file.version);
    assert_eq!(first.session_ids, repeated.session_ids);
    let mut corrupt = descriptor("corrupt.jsonl", &bytes, None);
    corrupt.sha256 = "b".repeat(64);
    status(
        owner
            .upload_file(codex.id, &corrupt, Cursor::new(bytes.clone()))
            .await
            .unwrap_err(),
        400,
    );
    assert!(
        !owner
            .source_files(codex.id, &ListOptions::default())
            .await
            .unwrap()
            .data
            .iter()
            .any(|file| file.path == "corrupt.jsonl")
    );
    let mut corrupt_update = metadata.clone();
    corrupt_update.expected_version = Some(first.file.version.clone());
    corrupt_update.sha256 = "c".repeat(64);
    status(
        owner
            .upload_file(codex.id, &corrupt_update, Cursor::new(bytes.clone()))
            .await
            .unwrap_err(),
        400,
    );
    let mut bad_length = metadata.clone();
    bad_length.expected_version = Some(first.file.version.clone());
    bad_length.byte_length += 1;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/sources/{}/files", server.url, codex.id))
        .bearer_auth(&server.api_key)
        .multipart(
            reqwest::multipart::Form::new()
                .text("metadata", serde_json::to_string(&bad_length).unwrap())
                .part("file", reqwest::multipart::Part::bytes(bytes.clone())),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let extra = descriptor("extra-part.jsonl", &bytes, None);
    let response = reqwest::Client::new()
        .post(format!("{}/v1/sources/{}/files", server.url, codex.id))
        .bearer_auth(&server.api_key)
        .multipart(
            reqwest::multipart::Form::new()
                .text("metadata", serde_json::to_string(&extra).unwrap())
                .part("file", reqwest::multipart::Part::bytes(bytes.clone()))
                .text("unexpected", "must reject before accepting"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert!(
        !owner
            .source_files(codex.id, &ListOptions::default())
            .await
            .unwrap()
            .data
            .iter()
            .any(|file| file.path == "extra-part.jsonl")
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
        peer.content(message_page.data[0].id)
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

    // Append advances the same generation and preserves manual metadata.
    sqlx::query("UPDATE sessions SET tags=ARRAY['manual'],title='Manual title' WHERE id=$1")
        .bind(session.id)
        .execute(&pool)
        .await
        .unwrap();
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects(id,slug,name) VALUES($1,'manual','Manual project')")
        .bind(project)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO session_projects(session_id,project_id) VALUES($1,$2)")
        .bind(session.id)
        .bind(project)
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
    assert_eq!(append.session_ids, vec![session.id]);
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
    assert_eq!(session_after.title, "Manual title");
    assert_eq!(session_after.project_ids, vec![project]);
    assert_eq!(peer.session(session.id).await.unwrap().message_count, 8);
    let continued = owner
        .messages(
            session.id,
            &MessageFilter {
                cursor: message_page.next_cursor,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(continued.data.len(), 6);
    assert_eq!(
        serde_json::to_value(&continued.data.last().unwrap().content).unwrap()[0]["text"],
        "Appended answer."
    );
    status(
        owner
            .upload_file(codex.id, &metadata, Cursor::new(bytes.clone()))
            .await
            .unwrap_err(),
        409,
    );
    // A rewrite of the same conversation keeps its identity and manual labels.
    let rewind = ingest(
        &owner,
        codex.id,
        "home.jsonl",
        bytes.clone(),
        Some(after.version.clone()),
    )
    .await;
    assert_eq!(rewind.session_ids, vec![session.id]);
    assert_ne!(rewind.file.generation_id, file.generation_id);
    let rewound_session = owner.session(session.id).await.unwrap();
    assert_eq!(rewound_session.message_count, 7);
    assert_eq!(rewound_session.title, "Manual title");
    assert_eq!(rewound_session.tags, vec!["manual"]);
    assert_eq!(rewound_session.project_ids, vec![project]);
    let after = rewind.file;

    // Rewrites retain the preceding generation while making the new original readable.
    let rewritten = fixture("codex/precision.jsonl");
    let rewrite = ingest(
        &owner,
        codex.id,
        "home.jsonl",
        rewritten.clone(),
        Some(after.version.clone()),
    )
    .await;
    let precision = owner
        .messages(rewrite.session_ids[0], &MessageFilter::default())
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

    // Unsupported or malformed input rejects the whole file without replacing accepted data.
    let unknown_bytes = b"{\"unknown_native_record\":true}\n".to_vec();
    let mut malformed = rewritten.clone();
    malformed.extend_from_slice(b"{not-json}\n");
    let mut unsupported = rewritten.clone();
    unsupported.extend_from_slice(b"{\"type\":\"future_native_record\",\"payload\":{}}\n");
    for rejected in [
        unknown_bytes.clone(),
        fixture("codex/precision-unknown.jsonl"),
        malformed,
        unsupported,
    ] {
        for (path, version) in [
            ("rejected.jsonl", None),
            ("home.jsonl", Some(rewritten_file.version.clone())),
        ] {
            let error = owner
                .upload_file(
                    codex.id,
                    &descriptor(path, &rejected, version),
                    Cursor::new(rejected.clone()),
                )
                .await
                .unwrap_err();
            match error {
                Error::Problem(problem) => {
                    assert_eq!(problem.status, 400);
                    assert_eq!(problem.code, "unsupported_native_record");
                }
                other => panic!("expected a rejected native file, got {other}"),
            }
        }
    }
    let mut invalid_project = descriptor("unknown-project.jsonl", &rewritten, None);
    invalid_project.project_ids.push(Uuid::new_v4());
    status(
        owner
            .upload_file(codex.id, &invalid_project, Cursor::new(rewritten.clone()))
            .await
            .unwrap_err(),
        400,
    );
    let accepted = owner
        .source_files(codex.id, &ListOptions::default())
        .await
        .unwrap()
        .data;
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].id, rewritten_file.id);
    assert_eq!(accepted[0].version, rewritten_file.version);
    assert_eq!(accepted[0].generation_id, rewritten_file.generation_id);
    let mut retained = Vec::new();
    owner
        .original_to(codex.id, rewritten_file.id, None, &mut retained)
        .await
        .unwrap();
    assert_eq!(retained, rewritten);
    assert_eq!(
        serde_json::to_value(
            owner
                .messages(rewrite.session_ids[0], &MessageFilter::default())
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&precision).unwrap()
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
    assert_eq!(imported.session_ids, vec![shared.id]);
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
    let branch = owner
        .sessions(&SessionFilter {
            harness: Some(Harness::Pi),
            ..Default::default()
        })
        .await
        .unwrap()
        .data
        .remove(0);
    assert_eq!(imported.session_ids, vec![branch.id]);
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
    let image_session = owner.session(image.session_ids[0]).await.unwrap();
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
    assert_eq!(artifact.media_type, "image/png");
    let mut png = Vec::new();
    owner.artifact_to(artifact_id, &mut png).await.unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(peer.artifact(artifact_id).await.unwrap().byte_length, 68);

    // Authentication protects every data surface, not only session listings.
    for path in [
        format!("sources/{}", codex.id),
        format!("sources/{}/files/{}/content", codex.id, file.id),
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
    assert_eq!(
        reqwest::Client::new()
            .post(format!("{}/v1/sources/{}/files", server.url, codex.id))
            .body(bytes.clone())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );

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

    // Large structured content expands intact without conditional read tokens.
    let large=format!("{}\n{}\n",json!({"type":"session_meta","payload":{"id":"large-session","cwd":"/home/ada"}}),json!({"type":"response_item","payload":{"type":"function_call","id":"large-call","call_id":"large","name":"fixture","arguments":json!({"text":"ü".repeat(40000)}).to_string()}})).into_bytes();
    let large = ingest(&owner, codex.id, "large.jsonl", large, None).await;
    let large_session = large.session_ids[0];
    let m = owner
        .messages(large_session, &MessageFilter::default())
        .await
        .unwrap()
        .data
        .remove(0);
    assert!(m.truncated);
    assert!(serde_json::to_vec(&m.content).unwrap().len() <= PREVIEW_BYTES + 2);
    let expanded = owner.content(m.id).await.unwrap();
    assert_eq!(
        serde_json::to_value(&expanded.content).unwrap()[0]["input"]["value"]["text"]
            .as_str()
            .unwrap()
            .len(),
        80000
    );

    // The real CLI captures a complete prefix, resumes after an S3 outage, and exports.
    let local = root.path().join("native");
    fs::create_dir(&local).unwrap();
    let mut captured = fixture("codex/home-tools.jsonl");
    captured.extend_from_slice(b"{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"id\":\"watch-tail\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"");
    fs::write(local.join("cli.jsonl"), &captured).unwrap();
    fs::write(
        local.join("precision.jsonl"),
        fixture("codex/precision.jsonl"),
    )
    .unwrap();
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
    assert_eq!(pending["pending_files"], 2);
    let output = cli(&server, root.path(), &["sync", "--json"]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status_output = cli(&server, root.path(), &["sync", "status", "--json"]).await;
    let synced: Value = serde_json::from_slice(&status_output.stdout).unwrap();
    assert_eq!(synced["pending_files"], 0);
    assert!(synced["accepted_files"].as_i64().unwrap() > 0);
    let enabled: Value = serde_json::from_slice(&output.stdout).unwrap();
    let source_id: Uuid = enabled["sources"][0]["source_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let cli_files = owner
        .source_files(source_id, &ListOptions::default())
        .await
        .unwrap()
        .data;
    let cli_file = cli_files
        .iter()
        .find(|file| file.path == "cli.jsonl")
        .unwrap();
    let mut capture = Vec::new();
    owner
        .original_to(source_id, cli_file.id, None, &mut capture)
        .await
        .unwrap();
    assert_eq!(capture, sessiondb_adapters::complete_prefix(&captured));
    let unchanged = cli(&server, root.path(), &["sync", "--json"]).await;
    assert!(
        unchanged.status.success(),
        "{}",
        String::from_utf8_lossy(&unchanged.stderr)
    );
    let unchanged: Value = serde_json::from_slice(&unchanged.stdout).unwrap();
    assert_eq!(unchanged["sources"][0]["accepted_files"], 0);

    // Rejected snapshots do not advance checkpoints or block a repaired local file.
    fs::write(local.join("rejected.jsonl"), &unknown_bytes).unwrap();
    let rejected = cli(&server, root.path(), &["sync", "--json"]).await;
    assert!(!rejected.status.success());
    let status_output = cli(&server, root.path(), &["sync", "status", "--json"]).await;
    let status_value: Value = serde_json::from_slice(&status_output.stdout).unwrap();
    assert_eq!(status_value["pending_files"], 0);
    let accepted = owner
        .source_files(source_id, &ListOptions::default())
        .await
        .unwrap()
        .data;
    assert!(!accepted.iter().any(|file| file.path == "rejected.jsonl"));
    assert_eq!(
        accepted
            .iter()
            .find(|file| file.id == cli_file.id)
            .unwrap()
            .version,
        cli_file.version
    );
    let repaired = fixture("codex/fork/parent.jsonl");
    fs::write(local.join("rejected.jsonl"), &repaired).unwrap();
    let output = cli(&server, root.path(), &["sync", "--json"]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["sources"][0]["accepted_files"], 1);
    let repaired_file = owner
        .source_files(source_id, &ListOptions::default())
        .await
        .unwrap()
        .data
        .into_iter()
        .find(|file| file.path == "rejected.jsonl")
        .unwrap();
    let mut repaired_original = Vec::new();
    owner
        .original_to(source_id, repaired_file.id, None, &mut repaired_original)
        .await
        .unwrap();
    assert_eq!(repaired_original, repaired);

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
            .into_iter()
            .find(|file| file.id == cli_file.id)
            .unwrap();
        if current.byte_length == captured.len() as u64 {
            let local = cli(&server, root.path(), &["sync", "status", "--json"]).await;
            if serde_json::from_slice::<Value>(&local.stdout).unwrap()["pending_files"] == 0 {
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

    // Restart both the API process and object service, then restore the catalog.
    server.stop();
    docker(&["restart", &services.s3]);
    server.write_config(&services);
    server.restart(&services, "postgres").await;
    let retry = owner
        .upload_file(
            codex.id,
            &descriptor("home.jsonl", &rewritten, Some(file.version.clone())),
            Cursor::new(rewritten.clone()),
        )
        .await
        .unwrap();
    assert!(retry.unchanged);
    assert_eq!(retry.file.id, rewritten_file.id);
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
    let mut configuration: toml::Value =
        toml::from_str(&fs::read_to_string(&server.config).unwrap()).unwrap();
    for public_url in ["https://moved.example.com", "http://athena:6767"] {
        server.stop();
        configuration["public_url"] = toml::Value::String(public_url.into());
        fs::write(&server.config, toml::to_string(&configuration).unwrap()).unwrap();
        server.restart(&services, "restored").await;
        let relocated = owner.message(m.id).await.unwrap();
        assert!(
            relocated
                .content_url
                .as_ref()
                .unwrap()
                .starts_with(&format!("{public_url}/"))
        );
        let relocated_content = owner.content(m.id).await.unwrap();
        assert_eq!(
            serde_json::to_value(&relocated_content.content).unwrap(),
            serde_json::to_value(&expanded.content).unwrap()
        );
    }

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
