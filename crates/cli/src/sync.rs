use crate::{
    config::{ConfiguredSource, SyncOptions, expand, private_dir},
    state::{LocalState, Pending},
};
use anyhow::{Context, Result, bail};
use futures_util::{StreamExt, TryStreamExt};
use serde_json::json;
use sessiondb_client::Client;
use sessiondb_types::{
    FileDescriptor, Harness, Import, ImportManifest, ImportStatus, ListOptions, Source,
    SourceCreate, SourceFile,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

pub async fn ensure_source(
    client: &Client,
    state: &LocalState,
    org: Uuid,
    device: &str,
    configured: &ConfiguredSource,
) -> Result<Source> {
    let root = expand(&configured.root)?
        .canonicalize()
        .context("native store root does not exist")?;
    if !root.is_dir() {
        bail!("native store root must be a directory");
    }
    let root = root.to_str().context("native store root must be UTF-8")?;
    if let Some(id) = state
        .registration(
            client.server(),
            org,
            device,
            configured.harness.as_str(),
            root,
        )
        .await?
    {
        match client.source(org, id).await {
            Ok(source) => return Ok(source),
            Err(sessiondb_client::Error::Problem(p)) if p.status == 404 => {}
            Err(e) => return Err(e.into()),
        }
    }
    let me = client.me().await?;
    let mut options = ListOptions::default();
    let source = loop {
        let page = client.sources(org, &options).await?;
        if let Some(source) = page.data.into_iter().find(|s| {
            s.owner_id == me.user.id
                && s.harness == configured.harness
                && s.device_label == device
                && s.root == root
        }) {
            break source;
        }
        if page.next_cursor.is_some() {
            options.cursor = page.next_cursor;
        } else {
            break client
                .create_source(
                    org,
                    &SourceCreate {
                        harness: configured.harness,
                        device_label: device.into(),
                        root: root.into(),
                        default_visibility: configured.default_visibility,
                    },
                )
                .await?;
        }
    };
    state
        .register(client.server(), org, device, &source)
        .await?;
    Ok(source)
}
pub fn lock(directory: &Path) -> Result<std::fs::File> {
    private_dir(directory)?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(directory.join("sync.lock"))?;
    file.try_lock()
        .context("another sync process is using this state directory")?;
    Ok(file)
}
async fn remote_files(
    client: &Client,
    org: Uuid,
    source: Uuid,
) -> Result<HashMap<String, SourceFile>> {
    let mut options = ListOptions::default();
    let mut result = HashMap::new();
    loop {
        let page = client.source_files(org, source, &options).await?;
        for file in page.data {
            result.insert(file.path.clone(), file);
        }
        if page.next_cursor.is_none() {
            return Ok(result);
        }
        options.cursor = page.next_cursor;
    }
}

pub async fn source(
    client: &Client,
    state: &LocalState,
    org: Uuid,
    source: &Source,
    options: &SyncOptions,
    quiet: bool,
) -> Result<serde_json::Value> {
    if source.paused {
        bail!("source {} is paused", source.id);
    }
    let limits = client.info().await?.limits;
    let batch = options.batch_files.clamp(1, limits.max_files_per_import);
    let mut accepted = 0;
    if let Some(pending) = state.pending(source.id).await? {
        accepted += reconcile_pending(client, state, org, pending, options, quiet)
            .await?
            .results
            .len();
    }
    let mut deferred = 0;
    loop {
        let remote = remote_files(client, org, source.id).await?;
        let root = PathBuf::from(&source.root);
        let directory = state
            .directory
            .join("pending")
            .join(Uuid::new_v4().to_string());
        let dir = directory.clone();
        let scan = match tokio::task::spawn_blocking(move || {
            scan(&root, &remote, &dir, batch, limits.max_file_bytes)
        })
        .await?
        {
            Ok(scan) => scan,
            Err(error) => {
                std::fs::remove_dir_all(&directory).ok();
                return Err(error);
            }
        };
        deferred = deferred.max(scan.deferred);
        if scan.files.is_empty() {
            std::fs::remove_dir_all(&directory).ok();
            break;
        }
        let pending = Pending {
            manifest: ImportManifest {
                source_id: source.id,
                files: scan.files,
            },
            directory,
            create_key: Uuid::new_v4().to_string(),
            complete_key: Uuid::new_v4().to_string(),
            import_id: None,
        };
        state.save_pending(&pending).await?;
        accepted += reconcile_pending(client, state, org, pending, options, quiet)
            .await?
            .results
            .len();
    }
    Ok(json!({"source_id":source.id,"accepted_files":accepted,"deferred_tail_files":deferred}))
}
async fn reconcile_pending(
    client: &Client,
    state: &LocalState,
    org: Uuid,
    pending: Pending,
    options: &SyncOptions,
    quiet: bool,
) -> Result<Import> {
    let source = pending.manifest.source_id;
    let directory = pending.directory.clone();
    let result = resume(client, state, org, pending, options, quiet).await;
    if let Err(error) = &result
        && matches!(error.downcast_ref::<sessiondb_client::Error>(),Some(sessiondb_client::Error::Problem(p)) if p.status==404 || (p.status==409 && matches!(p.code.as_str(),"version_conflict"|"import_expired")))
    {
        state.clear_pending(source).await?;
        remove_snapshot_dir(state, &directory)?;
    }
    result
}
struct Scan {
    files: Vec<FileDescriptor>,
    deferred: usize,
}
fn scan(
    root: &Path,
    remote: &HashMap<String, SourceFile>,
    directory: &Path,
    batch: usize,
    max_bytes: u64,
) -> Result<Scan> {
    if !root.is_dir() {
        bail!("native store {} is unavailable", root.display());
    }
    private_dir(directory)?;
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.context("scan native session store")?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "jsonl") {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    let mut files = Vec::new();
    let mut deferred = 0;
    for path in paths {
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .context("session path must be UTF-8")?
            .replace(std::path::MAIN_SEPARATOR, "/");
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(&path)
            .with_context(|| format!("read {}", path.display()))?;
        let before = file.metadata()?;
        if before.len() > max_bytes {
            bail!("{} exceeds the server's file limit", path.display());
        }
        let mut bytes = Vec::new();
        (&file).take(before.len()).read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        if before.len() != after.len() || before.modified()? != after.modified()? {
            bail!(
                "{} changed during capture; retry synchronization",
                path.display()
            );
        }
        let prefix = sessiondb_adapters::complete_prefix(&bytes);
        if prefix.len() != bytes.len() {
            deferred += 1;
        }
        if prefix.is_empty() {
            continue;
        }
        let sha256 = hex::encode(Sha256::digest(prefix));
        if remote
            .get(&relative)
            .is_some_and(|f| f.sha256 == sha256 && f.byte_length == prefix.len() as u64)
        {
            continue;
        }
        let mut snapshot = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join(files.len().to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            snapshot.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        snapshot.write_all(prefix)?;
        snapshot.sync_all()?;
        files.push(FileDescriptor {
            path: relative.clone(),
            byte_length: prefix.len() as u64,
            sha256,
            media_type: "application/x-ndjson".into(),
            expected_version: remote.get(&relative).map(|f| f.version.clone()),
            project_ids: vec![],
        });
        if files.len() >= batch {
            break;
        }
    }
    Ok(Scan { files, deferred })
}

async fn resume(
    client: &Client,
    state: &LocalState,
    org: Uuid,
    mut pending: Pending,
    options: &SyncOptions,
    quiet: bool,
) -> Result<Import> {
    let mut import = if let Some(id) = pending.import_id {
        client.import(org, id).await?
    } else {
        let import = client
            .create_import(org, &pending.manifest, &pending.create_key)
            .await?;
        pending.import_id = Some(import.id);
        state.save_pending(&pending).await?;
        import
    };
    if import.accepted_at.is_none()
        && matches!(import.status, ImportStatus::Failed | ImportStatus::Expired)
    {
        state.clear_pending(pending.manifest.source_id).await?;
        remove_snapshot_dir(state, &pending.directory)?;
        bail!(
            "import {} was not accepted ({}); the next sync will reconcile and retry",
            import.id,
            import.status
        );
    }
    if import.accepted_at.is_none() && import.status == ImportStatus::AwaitingUpload {
        if !quiet {
            eprintln!(
                "Uploading {} file(s) for source {}",
                pending.manifest.files.len(),
                pending.manifest.source_id
            );
        }
        futures_util::stream::iter(import.uploads.iter())
            .map(|target| {
                let pending = &pending;
                async move {
                    let index = pending
                        .manifest
                        .files
                        .iter()
                        .position(|f| f.path == target.path)
                        .context("upload target is not in the manifest")?;
                    let file = tokio::fs::File::open(pending.directory.join(index.to_string()))
                        .await
                        .context("pending snapshot is missing")?;
                    client.upload(target, file).await?;
                    Ok::<(), anyhow::Error>(())
                }
            })
            .buffer_unordered(options.upload_concurrency.clamp(1, 16))
            .try_collect::<Vec<_>>()
            .await?;
        import = client
            .complete_import(org, import.id, &pending.complete_key)
            .await?;
    }
    if import.accepted_at.is_none() {
        import = client
            .wait_for_acceptance(org, import.id, Duration::from_secs(900))
            .await?;
    }
    let Some(accepted_at) = import.accepted_at else {
        state.clear_pending(pending.manifest.source_id).await?;
        remove_snapshot_dir(state, &pending.directory)?;
        bail!(
            "import {} failed before acceptance; originals remain local",
            import.id
        );
    };
    if import.results.len() != pending.manifest.files.len() {
        bail!("acceptance receipt is incomplete; retaining pending state");
    }
    state
        .accept(
            pending.manifest.source_id,
            &import.results,
            &pending.manifest,
            accepted_at,
        )
        .await?;
    remove_snapshot_dir(state, &pending.directory)?;
    if !quiet {
        eprintln!("Accepted import {} ({})", import.id, import.status);
    }
    Ok(import)
}
fn remove_snapshot_dir(state: &LocalState, directory: &Path) -> Result<()> {
    let parent = state.directory.join("pending");
    if directory.parent() != Some(parent.as_path()) {
        bail!("pending snapshot path is outside the state directory");
    }
    match std::fs::remove_dir_all(directory) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn detect() -> Vec<(Harness, PathBuf)> {
    let Some(base) = directories::BaseDirs::new() else {
        return vec![];
    };
    let codex = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| base.home_dir().join(".codex"));
    let claude = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| base.home_dir().join(".claude"));
    [
        (Harness::Codex, codex.join("sessions")),
        (Harness::Codex, codex.join("archived_sessions")),
        (Harness::Claude, claude.join("projects")),
        (Harness::Pi, base.home_dir().join(".pi/agent/sessions")),
    ]
    .into_iter()
    .filter(|(_, p)| p.is_dir())
    .collect()
}
