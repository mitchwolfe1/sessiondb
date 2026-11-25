use super::*;
use axum::{
    Router,
    body::{Body, Bytes},
    http::{Method, Request, Response},
    routing::any,
};
use flate2::{Compression, write::GzEncoder};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

const TARGET: &str = "x86_64-unknown-linux-gnu";
const TAG: &str = "v1.1.0";
const INSTALLER: &str = include_str!("../../../../scripts/install-harnesses");

#[derive(Default)]
struct Host {
    location: Option<String>,
    assets: HashMap<String, Bytes>,
    requests: Vec<String>,
}

struct Fixture {
    root: TempDir,
    host: Arc<Mutex<Host>>,
    updater: Updater,
}

impl Fixture {
    async fn new() -> Self {
        let host = Arc::new(Mutex::new(Host::default()));
        let state = Arc::clone(&host);
        let app = Router::new().fallback(any(move |req: Request<Body>| {
            let state = Arc::clone(&state);
            async move {
                assert!(req.headers().get("authorization").is_none());
                let mut host = state.lock().unwrap();
                let path = req.uri().path();
                host.requests.push(format!("{} {path}", req.method()));
                let builder = Response::builder();
                if req.method() == Method::HEAD && path == "/releases/latest" {
                    if let Some(location) = &host.location {
                        return builder
                            .status(302)
                            .header(LOCATION, location)
                            .body(Body::empty())
                            .unwrap();
                    }
                } else if req.method() == Method::GET
                    && let Some(asset) = host.assets.get(path)
                {
                    return builder.body(Body::from(asset.clone())).unwrap();
                }
                builder.status(404).body(Body::empty()).unwrap()
            }
        }));
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let repository = Url::parse(&format!("http://{}/", socket.local_addr().unwrap())).unwrap();
        tokio::spawn(async move { axum::serve(socket, app).await.unwrap() });
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("sessiondb");
        fs::write(&executable, version_script("1.0.0")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let updater = Updater {
            client: Client::builder()
                .no_proxy()
                .redirect(Policy::none())
                .build()
                .unwrap(),
            repository,
            executable,
            running: Version::new(1, 0, 0),
            target: TARGET,
        };
        Self {
            root,
            host,
            updater,
        }
    }

    fn publish(&self, packaged_version: &str, skill: &str) {
        let package = format!("sessiondb-{TAG}-{TARGET}");
        let installer = r#"#!/bin/sh
set -eu
scripts=$(dirname "$0")
export HOME=$(cd "$scripts/../../.." && pwd)/home
export PATH=/usr/bin:/bin CODEX_HOME="$HOME/.codex" CLAUDE_CONFIG_DIR="$HOME/.claude" PI_CODING_AGENT_DIR="$HOME/.pi/agent"
exec "$scripts/installer.py"
"#;
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        for (path, mode, data) in [
            (
                "sessiondb",
                0o755,
                version_script(packaged_version).as_bytes(),
            ),
            ("scripts/install-harnesses", 0o755, installer.as_bytes()),
            ("scripts/installer.py", 0o755, INSTALLER.as_bytes()),
            ("skills/sessiondb/SKILL.md", 0o644, skill.as_bytes()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(mode);
            tar.append_data(&mut header, format!("{package}/{path}"), data)
                .unwrap();
        }
        let archive = tar.into_inner().unwrap().finish().unwrap();
        let sum = format!("{:x}  {package}.tar.gz\n", Sha256::digest(&archive));
        let base = format!("/releases/download/{TAG}");
        let mut host = self.host.lock().unwrap();
        host.location = Some(format!("/releases/tag/{TAG}"));
        host.assets.insert(format!("{base}/SHA256SUMS"), sum.into());
        host.assets
            .insert(format!("{base}/{package}.tar.gz"), archive.into());
    }
}

fn version_script(version: &str) -> String {
    format!("#!/bin/sh\ntest \"$1\" = --version || exit 1\necho 'sessiondb {version}'\n")
}

#[tokio::test]
async fn checks_each_pass_and_only_installs_a_newer_stable_target() {
    let f = Fixture::new().await;
    for tag in [
        None,
        Some("v0.9.0"),
        Some("v1.0.0"),
        Some("v1.0.0%2Bother"),
        Some("v2.0.0-rc.1"),
    ] {
        f.host.lock().unwrap().location = tag.map(|s| format!("/releases/tag/{s}"));
        assert_eq!(f.updater.install_if_newer().await.unwrap(), None);
    }
    f.host.lock().unwrap().location = Some("https://other.example/releases/tag/v2.0.0".into());
    assert!(f.updater.latest().await.is_err());
    assert_eq!(
        f.host.lock().unwrap().requests,
        vec!["HEAD /releases/latest"; 6]
    );

    f.publish("1.1.0", "new skill");
    let sums = format!("/releases/download/{TAG}/SHA256SUMS");
    f.host.lock().unwrap().assets.insert(
        sums,
        format!("{}  another-target.tar.gz\n", "0".repeat(64)).into(),
    );
    assert_eq!(f.updater.install_if_newer().await.unwrap(), None);
    assert_eq!(
        binary_version(&f.updater.executable).await.unwrap(),
        f.updater.running
    );
    assert_eq!(f.host.lock().unwrap().requests.len(), 8);
}

#[tokio::test]
async fn retries_verification_and_setup_then_refreshes_detected_harnesses() {
    let f = Fixture::new().await;
    let original = fs::read(&f.updater.executable).unwrap();
    let home = f.root.path().join("home");
    for dir in [".codex", ".claude", ".pi/agent", ".agents/skills/sessiondb"] {
        fs::create_dir_all(home.join(dir)).unwrap();
    }
    let shared = home.join(".agents/skills/sessiondb/SKILL.md");
    let guide = home.join(".codex/AGENTS.md");
    fs::write(&shared, "previous skill").unwrap();
    fs::write(&guide, "User instructions\n").unwrap();
    f.publish("1.1.0", "updated skill");
    f.host.lock().unwrap().assets.insert(
        format!("/releases/download/{TAG}/sessiondb-{TAG}-{TARGET}.tar.gz"),
        Bytes::from_static(b"damaged"),
    );
    assert!(
        f.updater
            .install_if_newer()
            .await
            .unwrap_err()
            .to_string()
            .contains("SHA-256 mismatch")
    );
    f.publish("1.2.0", "updated skill");
    assert!(
        f.updater
            .install_if_newer()
            .await
            .unwrap_err()
            .to_string()
            .contains("differs from its tag")
    );
    f.publish("1.1.0", "updated skill");
    fs::write(&guide, "<!-- sessiondb:start -->\n").unwrap();
    assert!(
        f.updater
            .install_if_newer()
            .await
            .unwrap_err()
            .to_string()
            .contains("installer failed")
    );
    assert_eq!(fs::read_to_string(&shared).unwrap(), "previous skill");
    assert_eq!(fs::read(&f.updater.executable).unwrap(), original);

    fs::write(&guide, "User instructions\n").unwrap();
    let lock = File::create(f.updater.executable.with_added_extension("update.lock")).unwrap();
    lock.lock().unwrap();
    assert_eq!(f.updater.install_if_newer().await.unwrap(), None);
    drop(lock);
    let new = Version::new(1, 1, 0);
    assert_eq!(
        f.updater.install_if_newer().await.unwrap(),
        Some(new.clone())
    );
    assert_eq!(binary_version(&f.updater.executable).await.unwrap(), new);
    for path in [shared, home.join(".claude/skills/sessiondb/SKILL.md")] {
        assert_eq!(fs::read_to_string(path).unwrap(), "updated skill");
    }
    assert!(
        fs::read_to_string(guide)
            .unwrap()
            .starts_with("User instructions\n")
    );
    for path in [
        ".codex/AGENTS.md",
        ".claude/CLAUDE.md",
        ".pi/agent/AGENTS.md",
    ] {
        assert!(
            fs::read_to_string(home.join(path))
                .unwrap()
                .contains("<!-- sessiondb:start -->")
        );
    }
    // A running older watcher can resume an install finished by another process.
    f.host.lock().unwrap().requests.clear();
    assert_eq!(f.updater.install_if_newer().await.unwrap(), Some(new));
    assert!(
        !f.host
            .lock()
            .unwrap()
            .requests
            .iter()
            .any(|r| r.ends_with(".tar.gz"))
    );
}

#[test]
fn rejects_links_and_paths_outside_the_package_before_running_setup() {
    let tmp = tempfile::tempdir().unwrap();
    for (name, kind) in [
        ("elsewhere", tar::EntryType::Regular),
        ("package/link", tar::EntryType::Symlink),
    ] {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_size(0);
        header.set_mode(0o644);
        tar.append_data(&mut header, name, &[][..]).unwrap();
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        assert!(stage_bundle(tmp.path(), "package", &bytes).is_err());
        assert_eq!(tmp.path().read_dir().unwrap().count(), 0);
    }
}
