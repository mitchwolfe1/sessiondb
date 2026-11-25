use super::support::{Server, Services, cli};
use super::{client, status};
use serde_json::{Value, json};
use sessiondb_types::{Source, SourceMemberUpdate};
use std::{fs, path::Path};
use uuid::Uuid;

async fn run(server: &Server, client_dir: &Path, args: &[&str]) -> Value {
    let mut flags = vec!["--json"];
    flags.extend_from_slice(args);
    let output = cli(server, client_dir, &flags).await;
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn transcript(id: &str, cwd: &str, text: &str) -> String {
    format!(
        "{}\n{}\n",
        json!({"type":"session_meta","payload":{"id":id,"cwd":cwd}}),
        json!({"type":"response_item","payload":{"type":"message","id":"decision","role":"user",
            "content":[{"type":"input_text","text":text}]}})
    )
}

async fn register(server: &Server, dir: &Path, device: &str, member: Option<&str>) -> Source {
    let store = dir.join("native");
    fs::create_dir_all(&store).unwrap();
    let mut args = vec!["connect", server.url.as_str()];
    if let Some(handle) = member {
        args.extend_from_slice(&["--member", handle]);
    }
    run(server, dir, &args).await;
    let sources = run(
        server,
        dir,
        &[
            "sources",
            "add",
            "--device-label",
            device,
            "--harness",
            "codex",
            "--root",
            store.to_str().unwrap(),
        ],
    )
    .await;
    serde_json::from_value(sources[0].clone()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Docker and built workspace binaries"]
async fn team_sources_share_history_and_scope_personal_lookups() {
    let root = tempfile::tempdir().unwrap();
    let services = Services::start().await;
    let server = Server::start(root.path(), &services).await;
    let api = client(&server);
    let ada = root.path().join("ada");
    let mark = root.path().join("mark");
    let lee = root.path().join("lee");
    let travel = root.path().join("travel");
    let primary = register(&server, &ada, "desk", None).await;
    let mark_source = register(&server, &mark, "desk", Some("Mark")).await;
    register(&server, &lee, "ops", Some("lee")).await;
    register(&server, &travel, "travel", Some("Ada")).await;

    let browser = transcript(
        "ada-browser",
        "/work/reports",
        "My browser uses the cedar connector.",
    );
    fs::write(ada.join("native/browser.jsonl"), &browser).unwrap();
    fs::write(
        mark.join("native/browser.jsonl"),
        transcript("mark-browser", "/work/reports", "My browser uses birch."),
    )
    .unwrap();
    fs::write(
        travel.join("native/browser.jsonl"),
        transcript("ada-travel", "/work/reports", "My browser uses willow."),
    )
    .unwrap();
    for (id, text) in [
        (
            "backlog-one",
            "RabbitMQ backlog on events: I compare ingress and consumer ack rates first.",
        ),
        (
            "backlog-two",
            "RabbitMQ backlog again; checking ingress and ack rates revealed slow consumers.",
        ),
    ] {
        fs::write(
            mark.join("native").join(format!("{id}.jsonl")),
            transcript(id, "/work/events", text),
        )
        .unwrap();
    }
    fs::write(lee.join("native/queue.jsonl"), transcript("dead-events", "/work/events", "Production RabbitMQ dead events queue: preserve the samples, then use the approved replay.")).unwrap();
    for dir in [&ada, &mark, &lee, &travel] {
        run(&server, dir, &["sync"]).await;
    }
    let before = run(&server, &ada, &["search", "cedar"]).await["data"][0].clone();
    assert!(before["member"].is_null());
    assert!(
        !cli(&server, &ada, &["search", "browser", "--local"])
            .await
            .status
            .success()
    );
    run(&server, &ada, &["connect", &server.url, "--member", "Ada"]).await;
    run(&server, &ada, &["sync"]).await;
    let local = run(&server, &ada, &["search", "--local", "browser"]).await;
    let hits = local["data"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit["member"], "ada");
    assert_eq!(hit["device_label"], "desk");
    assert_eq!(hit["source_id"], primary.id.to_string());
    assert_eq!(hit["recorded_cwd"], "/work/reports");
    assert_eq!(before["url"], hit["url"]);
    assert_eq!(before["leaf_id"], hit["leaf_id"]);
    let mine = run(&server, &ada, &["sessions", "list", "--mine"]).await;
    assert_eq!(mine["data"].as_array().unwrap().len(), 2);
    let desk = run(&server, &ada, &["search", "--device", "desk", "browser"]).await;
    assert_eq!(desk["data"].as_array().unwrap().len(), 2);

    let team = run(
        &server,
        &ada,
        &["search", "production RabbitMQ dead events queue"],
    )
    .await;
    let incident = &team["data"][0];
    assert_eq!(incident["member"], "lee");
    let open = run(
        &server,
        &ada,
        &[
            "messages",
            "show",
            incident["message_id"].as_str().unwrap(),
            "-C",
            "2",
        ],
    )
    .await;
    assert_eq!(open["anchor_id"], incident["message_id"]);
    let named = run(
        &server,
        &ada,
        &[
            "search",
            "--member",
            "MARK",
            "--role",
            "user",
            "RabbitMQ backlog",
        ],
    )
    .await;
    let decisions = named["data"].as_array().unwrap();
    assert_eq!(decisions.len(), 2);
    assert_ne!(decisions[0]["session_id"], decisions[1]["session_id"]);
    assert!(
        decisions
            .iter()
            .all(|d| d["member"] == "mark" && d["origin"] == "native_user")
    );
    let page = run(
        &server,
        &ada,
        &["search", "--member", "mark", "--limit", "1", "RabbitMQ"],
    )
    .await;
    let cursor = page["next_cursor"].as_str().unwrap();
    assert!(
        !cli(
            &server,
            &ada,
            &["search", "--member", "lee", "--cursor", cursor, "RabbitMQ"]
        )
        .await
        .status
        .success()
    );

    // Mirroring someone's original bytes is not a second person's decision.
    fs::write(mark.join("native/mirror.jsonl"), browser).unwrap();
    run(&server, &mark, &["sync"]).await;
    assert!(
        run(&server, &ada, &["search", "--member", "mark", "cedar"]).await["data"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mirror = run(&server, &mark, &["search", "cedar"]).await;
    assert_eq!(mirror["data"][0]["url"], hit["url"]);

    let correction = SourceMemberUpdate {
        member: "sam".parse().unwrap(),
        expected_member: Some("other".parse().unwrap()),
    };
    status(
        api.set_source_member(mark_source.id, &correction)
            .await
            .unwrap_err(),
        409,
    );
    status(
        api.set_source_member(Uuid::new_v4(), &correction)
            .await
            .unwrap_err(),
        404,
    );
    for _ in 0..2 {
        let corrected = run(
            &server,
            &lee,
            &[
                "sources",
                "set-member",
                &mark_source.id.to_string(),
                "sam",
                "--from",
                "mark",
            ],
        )
        .await;
        assert_eq!(corrected["member"], "sam");
    }
    let stale = cli(&server, &mark, &["sync"]).await;
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("attributed to sam"));
    assert_eq!(
        api.source(mark_source.id).await.unwrap().member.as_deref(),
        Some("sam")
    );
    let relabelled = run(
        &server,
        &ada,
        &["search", "--member", "sam", "RabbitMQ backlog"],
    )
    .await;
    assert_eq!(relabelled["data"].as_array().unwrap().len(), 2);
}
