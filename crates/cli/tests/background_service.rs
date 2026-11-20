// Exercises a disposable user service against loopback PostgreSQL/S3 fixtures.
#![cfg(target_os = "linux")]
#[allow(dead_code)]
mod support;

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use support::{Server, Services, cli};

struct Cleanup {
    root: PathBuf,
    dropin: PathBuf,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = Command::new(env!("CARGO_BIN_EXE_sessiondb"))
            .arg("--config")
            .arg(self.root.join("client.toml"))
            .arg("--state-dir")
            .arg(self.root.join("state"))
            .args(["sync", "uninstall"])
            .output();
        let _ = fs::remove_file(self.dropin.join("test.conf"));
        let _ = fs::remove_dir(&self.dropin);
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
    }
}

async fn run(server: &Server, root: &Path, args: &[&str]) -> Value {
    let out = support::cli_command(server, root, args)
        .current_dir(root)
        .output()
        .await
        .unwrap();
    assert!(
        out.status.success(),
        "command {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

async fn wait_for(server: &Server, root: &Path, ready: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
    loop {
        let status = run(server, root, &["sync", "status", "--json"]).await;
        if ready(&status) {
            return status;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "watcher status {status}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a systemd user manager, Docker, and built workspace binaries"]
async fn user_service_uploads_recovers_and_uninstalls() {
    let temp = tempfile::Builder::new()
        .prefix("sessiondb service $cash %n & ")
        .tempdir()
        .unwrap();
    let root = temp.path();
    let services = Services::start().await;
    let mut server = Server::start(root, &services).await;
    let context = cli(&server, root, &["context", "create", "background"]).await;
    assert!(
        context.status.success(),
        "{}",
        String::from_utf8_lossy(&context.stderr)
    );
    let native = root.join("native");
    fs::create_dir(&native).unwrap();
    let good = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"service-fixture\"}}\n";
    fs::write(native.join("valid.jsonl"), good).unwrap();
    fs::write(native.join("bad.jsonl"), b"{\"type\":\"unknown\"}\n").unwrap();
    run(
        &server,
        root,
        &[
            "sources",
            "add",
            "--harness",
            "codex",
            "--root",
            native.to_str().unwrap(),
            "--json",
        ],
    )
    .await;
    let config_path = root.join("client.toml");
    let mut cfg: toml::Value = toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    cfg["sync"]["reconcile_interval_seconds"] = 1.into();
    cfg["sources"][0]["root"] = "native".into();
    fs::write(&config_path, toml::to_string(&cfg).unwrap()).unwrap();
    let initial = run(&server, root, &["sync", "status", "--json"]).await;
    assert_eq!(initial["service"]["installed"], false);
    let unit = PathBuf::from(initial["service"]["definition"].as_str().unwrap());
    assert!(
        unit.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("sessiondb-sync-")
    );
    let dropin = unit.with_extension("service.d");
    fs::create_dir_all(dropin.parent().unwrap()).unwrap();
    fs::create_dir(&dropin).unwrap();
    let _cleanup = Cleanup {
        root: root.to_owned(),
        dropin: dropin.clone(),
    };
    fs::write(
        dropin.join("test.conf"),
        format!(
            "[Service]\nEnvironment=SESSIONDB_API_KEY={}\n",
            server.api_key
        ),
    )
    .unwrap();
    run(&server, root, &["sync", "install", "--json"]).await;
    assert!(!fs::read_to_string(&unit).unwrap().contains(&server.api_key));
    let first = wait_for(&server, root, |s| {
        s["accepted_files"] == 1 && s["rejected_files"] == 1 && s["last_watch_pass"].is_object()
    })
    .await;
    assert_eq!(first["service"]["state"], "running");
    assert_eq!(first["service"]["enabled_at_login"], true);
    let rejects = run(&server, root, &["sync", "rejected", "--json"]).await;
    let rejected_at = rejects["files"][0]["rejected_at"].clone();
    assert_eq!(rejects["files"][0]["path"], "bad.jsonl");
    wait_for(&server, root, |s| {
        s["last_watch_pass"]["finished_at"] != first["last_watch_pass"]["finished_at"]
            && s["last_watch_pass"]["error"].is_null()
    })
    .await;
    assert_eq!(
        run(&server, root, &["sync", "rejected", "--json"]).await["files"][0]["rejected_at"],
        rejected_at
    );
    let stopped = run(&server, root, &["sync", "stop", "--json"]).await;
    assert_eq!(stopped["state"], "stopped");
    assert_eq!(stopped["enabled_at_login"], true);
    fs::write(native.join("new.jsonl"), good).unwrap();
    run(&server, root, &["sync", "start", "--json"]).await;
    wait_for(&server, root, |s| s["accepted_files"] == 2).await;
    assert_eq!(
        run(&server, root, &["sync", "rejected", "--json"]).await["files"][0]["rejected_at"],
        rejected_at
    );
    assert_eq!(
        run(&server, root, &["sync", "retry", "--json"]).await["cleared"],
        1
    );
    wait_for(&server, root, |s| s["rejected_files"] == 1).await;
    assert_ne!(
        run(&server, root, &["sync", "rejected", "--json"]).await["files"][0]["rejected_at"],
        rejected_at
    );
    fs::write(native.join("bad.jsonl"), good).unwrap();
    wait_for(&server, root, |s| {
        s["rejected_files"] == 0 && s["accepted_files"] == 3
    })
    .await;
    let additional = root.join("another store");
    fs::create_dir(&additional).unwrap();
    fs::write(additional.join("another.jsonl"), good).unwrap();
    run(
        &server,
        root,
        &[
            "sources",
            "add",
            "--harness",
            "codex",
            "--root",
            additional.to_str().unwrap(),
            "--json",
        ],
    )
    .await;
    wait_for(&server, root, |s| s["accepted_files"] == 4).await;
    run(&server, root, &["sync", "install", "--json"]).await;
    fs::write(&config_path, "[broken").unwrap();
    run(&server, root, &["sync", "uninstall", "--json"]).await;
    run(&server, root, &["sync", "uninstall", "--json"]).await;
    let final_state = run(&server, root, &["sync", "status", "--json"]).await;
    assert_eq!(final_state["service"]["state"], "uninstalled");
    assert_eq!(final_state["accepted_files"], 4);
    assert!(!unit.exists());
    assert!(native.join("bad.jsonl").is_file());
    server.stop();
}
