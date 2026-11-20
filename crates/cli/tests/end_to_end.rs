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

struct LegacyTitleCase {
    id: Uuid,
    title: &'static str,
    blocks: Vec<Vec<&'static str>>,
    expected_title: &'static str,
    expected_origins: Vec<Origin>,
}

async fn seed_legacy_titles(pool: &sqlx::PgPool) -> Vec<LegacyTitleCase> {
    const TITLE: &str = "# AGENTS.md instructions for /fixture";
    const INSTRUCTIONS: &str = "# AGENTS.md instructions for /fixture\n\n<INSTRUCTIONS>\nSynthetic harness setup.\n</INSTRUCTIONS>";
    const ENVIRONMENT: &str =
        "<environment_context>\nSynthetic environment.\n</environment_context>";
    const REQUEST: &str = "\n  verify\tthe saved tool output\u{1b}  \nKeep the original detail.";
    const EXPECTED: &str = "verify the saved tool output";
    const LITERAL: &str = "# AGENTS.md instructions for a test fixture";
    let cases = vec![
        LegacyTitleCase {
            id: Uuid::new_v4(),
            title: TITLE,
            blocks: vec![vec![INSTRUCTIONS], vec![ENVIRONMENT], vec![REQUEST]],
            expected_title: EXPECTED,
            expected_origins: vec![Origin::Harness, Origin::Harness, Origin::NativeUser],
        },
        LegacyTitleCase {
            id: Uuid::new_v4(),
            title: "My chosen title",
            blocks: vec![vec![INSTRUCTIONS], vec![REQUEST]],
            expected_title: "My chosen title",
            expected_origins: vec![Origin::Harness, Origin::NativeUser],
        },
        LegacyTitleCase {
            id: Uuid::new_v4(),
            title: LITERAL,
            blocks: vec![vec![LITERAL]],
            expected_title: LITERAL,
            expected_origins: vec![Origin::NativeUser],
        },
        LegacyTitleCase {
            id: Uuid::new_v4(),
            title: TITLE,
            blocks: vec![vec![
                INSTRUCTIONS,
                "A direct human request follows this block.",
            ]],
            expected_title: TITLE,
            expected_origins: vec![Origin::NativeUser],
        },
        LegacyTitleCase {
            id: Uuid::new_v4(),
            title: "",
            blocks: vec![vec![""], vec![REQUEST]],
            expected_title: EXPECTED,
            expected_origins: vec![Origin::NativeUser, Origin::NativeUser],
        },
        LegacyTitleCase {
            id: Uuid::new_v4(),
            title: TITLE,
            blocks: vec![vec![INSTRUCTIONS], vec![ENVIRONMENT]],
            expected_title: "Untitled session",
            expected_origins: vec![Origin::Harness, Origin::Harness],
        },
    ];
    let source = Uuid::new_v4();
    sqlx::query("INSERT INTO sources(id,harness,device_label,root) VALUES($1,'codex','legacy-fixture','/fixture')")
        .bind(source).execute(pool).await.unwrap();
    for case in &cases {
        sqlx::query("INSERT INTO sessions(id,source_id,native_key,harness,native_session_id,title,tags,updated_at,message_count) VALUES($1,$2,$3,'codex',$3,$4,ARRAY['legacy'],'2000-01-01T00:00:00Z',$5)")
            .bind(case.id).bind(source).bind(format!("legacy-{}",case.id))
            .bind(case.title).bind(case.blocks.len() as i64)
            .execute(pool).await.unwrap();
        let mut parent = None;
        for (sequence, blocks) in case.blocks.iter().enumerate() {
            let id = Uuid::new_v5(&case.id, &sequence.to_le_bytes());
            let content: Vec<_> = blocks
                .iter()
                .map(|text| FullContentBlock::text(*text))
                .collect();
            sqlx::query("INSERT INTO messages(id,session_id,sequence,role,native_role,origin,recorded_at,conversation_parent_id,content) VALUES($1,$2,$3,'user','user','native_user','2000-01-01T00:00:00Z',$4,$5)")
                .bind(id).bind(case.id).bind(sequence as i64).bind(parent)
                .bind(sqlx::types::Json(content)).execute(pool).await.unwrap();
            parent = Some(id);
        }
    }
    cases
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker and cargo build --workspace; uses isolated synthetic services"]
async fn authenticated_sync_storage_and_recovery() {
    let root = tempfile::tempdir().unwrap();
    let services = Services::start().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&services.database_url("postgres"))
        .await
        .unwrap();
    let migration_directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../server/migrations");
    sqlx::migrate::Migrator::new(migration_directory.as_path())
        .await
        .unwrap()
        .run_to(1, &pool)
        .await
        .unwrap();
    let legacy_cases = seed_legacy_titles(&pool).await;
    let mut server = Server::start(root.path(), &services).await;
    let owner = client(&server);
    let peer = client(&server);
    sqlx::query("CREATE DATABASE fresh")
        .execute(&pool)
        .await
        .unwrap();
    let migrated = server
        .command(&services, "fresh")
        .arg("migrate")
        .output()
        .unwrap();
    assert!(
        migrated.status.success(),
        "fresh migrations failed: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    server.restart(&services, "fresh").await;
    let fresh = client(&server);
    let fresh_source = source(&fresh, Harness::Codex, "fresh-catalog").await;
    let fresh_bytes = fixture("codex/home-tools.jsonl");
    let fresh_receipt = ingest(
        &fresh,
        fresh_source.id,
        "fresh.jsonl",
        fresh_bytes.clone(),
        None,
    )
    .await;
    assert_eq!(
        fresh
            .session(fresh_receipt.session_ids[0])
            .await
            .unwrap()
            .message_count,
        7
    );
    let mut fresh_original = Vec::new();
    fresh
        .original_to(
            fresh_source.id,
            fresh_receipt.file.id,
            None,
            &mut fresh_original,
        )
        .await
        .unwrap();
    assert_eq!(fresh_original, fresh_bytes);
    server.restart(&services, "postgres").await;
    for case in &legacy_cases {
        let session = owner.session(case.id).await.unwrap();
        assert_eq!(session.title, case.expected_title);
        assert_eq!(session.tags, vec!["legacy"]);
        let messages = owner
            .messages(case.id, &MessageFilter::default())
            .await
            .unwrap()
            .data;
        assert_eq!(messages.len(), case.blocks.len());
        for (sequence, ((message, blocks), origin)) in messages
            .iter()
            .zip(&case.blocks)
            .zip(&case.expected_origins)
            .enumerate()
        {
            assert_eq!(message.id, Uuid::new_v5(&case.id, &sequence.to_le_bytes()));
            assert_eq!(message.origin, *origin);
            assert_eq!(message.native_role.as_deref(), Some("user"));
            assert_eq!(message.content.len(), blocks.len());
            for (block, expected) in message.content.iter().zip(blocks) {
                let ContentBlock::Full(FullContentBlock::Text { text }) = block else {
                    panic!("migration must preserve native text");
                };
                assert_eq!(text, expected);
            }
        }
    }

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

    // Drain the saved batch and still scan for new files when individual uploads reject.
    let config_path = root.path().join("client.toml");
    let mut local_config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    local_config["sync"]["upload_concurrency"] = toml::Value::Integer(1);
    fs::write(&config_path, toml::to_string(&local_config).unwrap()).unwrap();
    fs::write(
        local.join("a-recovery.jsonl"),
        fixture("codex/precision.jsonl"),
    )
    .unwrap();
    fs::write(local.join("rejected.jsonl"), &unknown_bytes).unwrap();
    // Force a per-file publication error in this disposable catalog.
    sqlx::query("ALTER TABLE source_files ADD CONSTRAINT synthetic_retry CHECK (path <> 'b-retryable.jsonl')")
        .execute(&pool).await.unwrap();
    fs::write(
        local.join("b-retryable.jsonl"),
        fixture("codex/precision.jsonl"),
    )
    .unwrap();
    docker(&["pause", &services.s3]);
    let interrupted = cli(&server, root.path(), &["--timeout", "1", "sync", "--json"]).await;
    docker(&["unpause", &services.s3]);
    assert!(!interrupted.status.success());
    let interrupted_status = cli(&server, root.path(), &["sync", "status", "--json"]).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&interrupted_status.stdout).unwrap()["pending_files"],
        3
    );
    fs::write(local.join("second-rejected.jsonl"), &unknown_bytes).unwrap();
    fs::write(
        local.join("z-fresh.jsonl"),
        fixture("codex/fork/parent.jsonl"),
    )
    .unwrap();
    let rejected = cli(&server, root.path(), &["sync", "--json"]).await;
    assert_eq!(rejected.status.code(), Some(1));
    let output: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    let report = &output["sources"][0];
    assert_eq!(report["accepted_files"], 2);
    assert_eq!(report["rejections"].as_object().unwrap().len(), 2);
    assert_eq!(report["retryable_errors"].as_object().unwrap().len(), 1);
    assert!(
        report["retryable_errors"]["b-retryable.jsonl"]
            .as_str()
            .unwrap()
            .contains("internal_error")
    );
    assert_eq!(
        String::from_utf8_lossy(&rejected.stderr)
            .matches("Pending b-retryable.jsonl for source")
            .count(),
        1
    );
    for path in ["rejected.jsonl", "second-rejected.jsonl"] {
        assert!(
            report["rejections"][path]
                .as_str()
                .unwrap()
                .contains("unsupported_native_record")
        );
        assert_eq!(
            String::from_utf8_lossy(&rejected.stderr)
                .matches(&format!("Rejected {path} for source"))
                .count(),
            1
        );
    }
    let status_output = cli(&server, root.path(), &["sync", "status", "--json"]).await;
    let status_value: Value = serde_json::from_slice(&status_output.stdout).unwrap();
    assert_eq!(status_value["pending_files"], 1);
    let accepted = owner
        .source_files(source_id, &ListOptions::default())
        .await
        .unwrap()
        .data;
    assert!(!accepted.iter().any(|file| file.path == "rejected.jsonl"));
    assert!(accepted.iter().any(|file| file.path == "z-fresh.jsonl"));
    assert!(accepted.iter().any(|file| file.path == "a-recovery.jsonl"));
    assert_eq!(
        accepted
            .iter()
            .find(|file| file.id == cli_file.id)
            .unwrap()
            .version,
        cli_file.version
    );
    let repaired = fixture("codex/fork/parent.jsonl");
    sqlx::query("ALTER TABLE source_files DROP CONSTRAINT synthetic_retry")
        .execute(&pool)
        .await
        .unwrap();
    fs::remove_file(local.join("second-rejected.jsonl")).unwrap();
    fs::write(local.join("rejected.jsonl"), &repaired).unwrap();
    let output = cli(&server, root.path(), &["sync", "--json"]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["sources"][0]["accepted_files"], 2);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker and built workspace binaries; uses isolated synthetic services"]
async fn claude_replayed_records_preserve_both_branches_on_append() {
    let root = tempfile::tempdir().unwrap();
    let services = Services::start().await;
    let mut server = Server::start(root.path(), &services).await;
    let client = client(&server);
    let source = source(&client, Harness::Claude, "replayed-claude").await;
    let user = |id: &str| {
        json!({"type":"user","uuid":id,"sessionId":"fixture",
        "message":{"role":"user","content":"A synthetic request"}})
    };
    let bridge = |parent: &str| {
        json!({"type":"attachment","uuid":"bridge","parentUuid":parent,
        "attachment":{"type":"synthetic_context"}})
    };
    let copy = json!({"type":"user","uuid":"copy","parentUuid":"bridge","sessionId":"fixture",
        "message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="}}]}});
    let reply = |id: &str| {
        json!({"type":"assistant","uuid":id,"parentUuid":"copy","sessionId":"fixture",
        "message":{"role":"assistant","content":"Synthetic answer"}})
    };
    let mut bytes = Vec::new();
    for row in [
        user("root-a"),
        bridge("root-a"),
        copy.clone(),
        reply("first"),
    ] {
        serde_json::to_writer(&mut bytes, &row).unwrap();
        bytes.push(b'\n');
    }
    let file = ingest(&client, source.id, "branches.jsonl", bytes.clone(), None).await;
    let session = file.session_ids[0];
    let before = client
        .messages(session, &MessageFilter::default())
        .await
        .unwrap()
        .data;
    for row in [user("root-b"), bridge("root-b"), copy, reply("second")] {
        serde_json::to_writer(&mut bytes, &row).unwrap();
        bytes.push(b'\n');
    }
    let upload = descriptor("branches.jsonl", &bytes, Some(file.file.version));
    client
        .upload_file(source.id, &upload, Cursor::new(bytes.clone()))
        .await
        .unwrap_or_else(|e| {
            let log = fs::read_to_string(&server.log).unwrap();
            panic!(
                "{e}: {:?}",
                log.lines().rfind(|l| l.contains("request failed"))
            );
        });
    let messages = client
        .messages(session, &MessageFilter::default())
        .await
        .unwrap()
        .data;
    assert_eq!(messages.len(), 6);
    assert_eq!(
        messages[..3].iter().map(|m| m.id).collect::<Vec<_>>(),
        before.iter().map(|m| m.id).collect::<Vec<_>>()
    );
    for start in [0, 3] {
        assert_eq!(
            messages[start + 1].conversation_parent_id,
            Some(messages[start].id)
        );
        assert_eq!(
            messages[start + 2].conversation_parent_id,
            Some(messages[start + 1].id)
        );
        assert_eq!(
            messages[start + 1].source_refs[0].record_id.as_deref(),
            Some("copy")
        );
    }
    assert_ne!(messages[1].id, messages[4].id);
    assert_eq!(messages[1].source_refs[0].line, 3);
    assert_eq!(messages[4].source_refs[0].line, 7);
    assert_ne!(messages[1].content, messages[4].content);
    for index in [1, 4] {
        let ContentBlock::Full(FullContentBlock::Artifact { artifact_id, .. }) =
            &messages[index].content[0]
        else {
            panic!("expected image")
        };
        let mut image = Vec::new();
        client.artifact_to(*artifact_id, &mut image).await.unwrap();
        assert_eq!(image, vec![0]);
    }
    let mut original = Vec::new();
    client
        .original_to(source.id, file.file.id, None, &mut original)
        .await
        .unwrap();
    assert_eq!(original, bytes);
    server.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker and built workspace binaries; uses isolated synthetic services"]
async fn codex_message_and_tool_images_round_trip() {
    let root = tempfile::tempdir().unwrap();
    let services = Services::start().await;
    let mut server = Server::start(root.path(), &services).await;
    let client = client(&server);
    let source = source(&client, Harness::Codex, "codex-images").await;
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=";
    let image = json!({"type":"input_image","detail":"high","image_url":format!("data:image/png;base64,{png}")});
    let records = [
        json!({"type":"session_meta","payload":{"id":"fixture-codex-images"}}),
        json!({"type":"response_item","payload":{"type":"configuration_update","reasoning":{"effort":"high"}}}),
        json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"A fixture."},image]}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"lookup","input":"fixture","call_id":"lookup-1"}}),
        json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"lookup-1","output":[image]}}),
    ];
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, &record).unwrap();
        bytes.push(b'\n');
    }
    let receipt = ingest(&client, source.id, "images.jsonl", bytes.clone(), None).await;
    let messages = client
        .messages(receipt.session_ids[0], &MessageFilter::default())
        .await
        .unwrap()
        .data;
    assert_eq!(messages.len(), 3);
    let user = client.content(messages[0].id).await.unwrap();
    let result = client.content(messages[2].id).await.unwrap();
    let FullContentBlock::Artifact {
        artifact_id: first, ..
    } = &user.content[1]
    else {
        panic!("expected message image")
    };
    let FullContentBlock::ToolResult {
        content, call_id, ..
    } = &result.content[0]
    else {
        panic!("expected tool result")
    };
    assert_eq!(call_id.as_deref(), Some("lookup-1"));
    let ToolOutputBlock::Artifact {
        artifact_id: second,
        ..
    } = &content[0]
    else {
        panic!("expected tool result image")
    };
    assert_ne!(first, second);
    for id in [first, second] {
        let meta = client.artifact(*id).await.unwrap();
        assert_eq!(
            (meta.media_type.as_str(), meta.byte_length),
            ("image/png", 68)
        );
        let mut data = Vec::new();
        client.artifact_to(*id, &mut data).await.unwrap();
        assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
    }
    let mut original = Vec::new();
    client
        .original_to(source.id, receipt.file.id, None, &mut original)
        .await
        .unwrap();
    assert_eq!(original, bytes);
    server.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker and built workspace binaries; uses isolated synthetic services"]
async fn native_json_control_characters_round_trip() {
    let root = tempfile::tempdir().unwrap();
    let services = Services::start().await;
    let mut server = Server::start(root.path(), &services).await;
    let client = client(&server);
    let source = source(&client, Harness::Pi, "json-controls").await;
    let text = "before\u{0}after";
    let request = "Preserve\u{0}the native tool output.";
    let records = [
        json!({"type":"session","version":3,"id":"pi-json-controls","cwd":"/fixture","timestamp":"2026-10-24T00:00:00Z"}),
        json!({"type":"message","id":"user","parentId":null,"message":{"role":"user","timestamp":1792800000000i64,"content":[{"type":"text","text":request}]}}),
        json!({"type":"message","id":"assistant","parentId":"user","message":{"role":"assistant","timestamp":1792800000001i64,"usage":{"input":1,"nativeDetail":text},"content":[{"type":"toolCall","id":"call","name":"fixture_tool","arguments":{}}]}}),
        json!({"type":"message","id":"result","parentId":"assistant","message":{"role":"toolResult","toolCallId":"call","isError":false,"timestamp":1792800000002i64,"content":[{"type":"text","text":text}]}}),
        json!({"type":"custom","id":"metadata","parentId":"result","customType":"native-fixture","data":{"text":text}}),
    ];
    let mut bytes = Vec::new();
    for record in records {
        serde_json::to_writer(&mut bytes, &record).unwrap();
        bytes.push(b'\n');
    }
    let receipt = client
        .upload_file(
            source.id,
            &descriptor("controls.jsonl", &bytes, None),
            Cursor::new(bytes.clone()),
        )
        .await
        .unwrap_or_else(|error| {
            panic!(
                "native JSON ingestion failed: {error}\n{}",
                fs::read_to_string(&server.log).unwrap()
            )
        });
    assert_eq!(
        client.session(receipt.session_ids[0]).await.unwrap().title,
        "Preserve the native tool output."
    );
    let messages = client
        .messages(receipt.session_ids[0], &MessageFilter::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&messages.data[0].content).unwrap()[0]["text"],
        request
    );
    assert_eq!(
        serde_json::to_value(&messages.data[2].content).unwrap()[0]["content"][0]["text"],
        text
    );
    assert_eq!(
        messages.data[1].provider_usage.as_ref().unwrap()["nativeDetail"],
        text
    );
    let full = client.content(messages.data[2].id).await.unwrap();
    assert_eq!(
        serde_json::to_value(full.content).unwrap()[0]["content"][0]["text"],
        text
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&services.database_url("postgres"))
        .await
        .unwrap();
    let event: sqlx::types::Json<Value> =
        sqlx::query_scalar("SELECT data FROM session_events WHERE session_id=$1 AND type='custom'")
            .bind(receipt.session_ids[0])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event.0["data"]["text"], text);
    let mut rejected_bytes = bytes.clone();
    rejected_bytes.extend_from_slice(b"{\"type\":\"future_record\",\"id\":\"unsupported\"}\n");
    status(
        client
            .upload_file(
                source.id,
                &descriptor(
                    "controls.jsonl",
                    &rejected_bytes,
                    Some(receipt.file.version.clone()),
                ),
                Cursor::new(rejected_bytes),
            )
            .await
            .unwrap_err(),
        400,
    );
    let retained = client
        .messages(receipt.session_ids[0], &MessageFilter::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&retained.data[2].content).unwrap()[0]["content"][0]["text"],
        text
    );
    let mut original = Vec::new();
    client
        .original_to(source.id, receipt.file.id, None, &mut original)
        .await
        .unwrap();
    assert_eq!(original, bytes);
    server.stop();
    pool.close().await;
}
