use crate::{
    config::{ConfiguredSource, SyncOptions, expand, private_dir},
    state::{LocalState, Pending},
};
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde::Serialize;
use sessiondb_client::Client;
use sessiondb_types::{FileUpload, Harness, ListOptions, Source, SourceCreate, SourceFile};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub async fn ensure_source(
    client: &Client,
    state: &LocalState,
    device: &str,
    configured: &ConfiguredSource,
) -> Result<Source> {
    let root = expand(&configured.root)?;
    let configured_root = root.to_str().context("native store root must be UTF-8")?;
    if let Some(id) = state
        .registration(
            client.server(),
            device,
            configured.harness.as_str(),
            configured_root,
        )
        .await?
    {
        match client.source(id).await {
            Ok(source) => return Ok(source),
            Err(sessiondb_client::Error::Problem(p)) if p.status == 404 => {}
            Err(e) => return Err(e.into()),
        }
    }
    let root = root
        .canonicalize()
        .context("native store root does not exist")?;
    if !root.is_dir() {
        bail!("native store root must be a directory");
    }
    let root = root.to_str().context("native store root must be UTF-8")?;
    let mut options = ListOptions::default();
    let source =
        loop {
            let page = client.sources(&options).await?;
            if let Some(source) = page.data.into_iter().find(|s| {
                s.harness == configured.harness && s.device_label == device && s.root == root
            }) {
                break source;
            }
            if page.next_cursor.is_some() {
                options.cursor = page.next_cursor;
            } else {
                break client
                    .create_source(&SourceCreate {
                        harness: configured.harness,
                        device_label: device.into(),
                        root: root.into(),
                    })
                    .await?;
            }
        };
    state.register(client.server(), device, &source).await?;
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
async fn remote_files(client: &Client, source: Uuid) -> Result<HashMap<String, SourceFile>> {
    let mut options = ListOptions::default();
    let mut result = HashMap::new();
    loop {
        let page = client.source_files(source, &options).await?;
        for file in page.data {
            result.insert(file.path.clone(), file);
        }
        if page.next_cursor.is_none() {
            return Ok(result);
        }
        options.cursor = page.next_cursor;
    }
}

#[derive(Default, Serialize)]
pub struct SourceReport {
    pub source_id: Uuid,
    pub accepted_files: usize,
    pub deferred_tail_files: usize,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub rejections: BTreeMap<String, String>,
}

pub async fn source(
    client: &Client,
    state: &LocalState,
    source: &Source,
    options: &SyncOptions,
    quiet: bool,
) -> Result<SourceReport> {
    let mut report = SourceReport {
        source_id: source.id,
        ..Default::default()
    };
    // Resolve saved snapshots before later local bytes can replace them.
    resume(
        client,
        state,
        state.pending(source.id).await?,
        options,
        quiet,
        &mut report,
    )
    .await?;
    let limits = client.info().await?.limits;
    let remote = remote_files(client, source.id).await?;
    let root = PathBuf::from(&source.root);
    let source_id = source.id;
    let directory = state
        .directory
        .join("pending")
        .join(Uuid::new_v4().to_string());
    let dir = directory.clone();
    let rejected_paths = report.rejections.keys().cloned().collect();
    let scan = match tokio::task::spawn_blocking(move || {
        scan(
            &root,
            source_id,
            &remote,
            &dir,
            limits.max_file_bytes,
            &rejected_paths,
        )
    })
    .await?
    {
        Ok(scan) => scan,
        Err(error) => {
            remove_snapshot_dir(state, &directory)?;
            return Err(error);
        }
    };
    report.deferred_tail_files = scan.deferred;
    if scan.files.is_empty() {
        remove_snapshot_dir(state, &directory)?;
    } else {
        state.save_pending(&scan.files).await?;
        resume(client, state, scan.files, options, quiet, &mut report).await?;
    }
    Ok(report)
}
struct Scan {
    files: Vec<Pending>,
    deferred: usize,
}
fn scan(
    root: &Path,
    source: Uuid,
    remote: &HashMap<String, SourceFile>,
    directory: &Path,
    max_bytes: u64,
    rejected_paths: &HashSet<String>,
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
        if rejected_paths.contains(&relative) {
            continue;
        }
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
        let snapshot_path = directory.join(files.len().to_string());
        let mut snapshot = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&snapshot_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            snapshot.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        snapshot.write_all(prefix)?;
        snapshot.sync_all()?;
        files.push(Pending {
            source_id: source,
            metadata: FileUpload {
                path: relative.clone(),
                byte_length: prefix.len() as u64,
                sha256,
                expected_version: remote.get(&relative).map(|f| f.version.clone()),
                project_ids: vec![],
            },
            snapshot: snapshot_path,
        });
    }
    // Make the snapshot names durable before SQLite starts referring to them.
    std::fs::File::open(directory)?.sync_all()?;
    std::fs::File::open(
        directory
            .parent()
            .context("snapshot directory has no parent")?,
    )?
    .sync_all()?;
    Ok(Scan { files, deferred })
}

async fn resume(
    client: &Client,
    state: &LocalState,
    pending: Vec<Pending>,
    options: &SyncOptions,
    quiet: bool,
    report: &mut SourceReport,
) -> Result<()> {
    let mut uploads = futures_util::stream::iter(pending)
        .map(|pending| async move {
            let file = tokio::fs::File::open(&pending.snapshot)
                .await
                .context("pending snapshot is missing")?;
            let receipt = match client
                .upload_file(pending.source_id, &pending.metadata, file)
                .await
            {
                Ok(receipt) => receipt,
                Err(sessiondb_client::Error::Problem(problem))
                    if problem.status == 400 && problem.code == "unsupported_native_record" =>
                {
                    state.reject(&pending).await?;
                    remove_snapshot(state, &pending.snapshot)?;
                    let reason = sessiondb_client::Error::Problem(problem).to_string();
                    eprintln!(
                        "Rejected {} for source {}: {reason}",
                        pending.metadata.path, pending.source_id
                    );
                    return Ok(Some((pending.metadata.path, reason)));
                }
                Err(error) => {
                    return Err(error).with_context(|| format!("upload {}", pending.metadata.path));
                }
            };
            state.accept(&pending, &receipt).await?;
            remove_snapshot(state, &pending.snapshot)?;
            if !quiet {
                eprintln!(
                    "Accepted {} for source {}",
                    pending.metadata.path, pending.source_id
                );
            }
            Ok::<_, anyhow::Error>(None)
        })
        .buffer_unordered(options.upload_concurrency.clamp(1, 16));
    while let Some(result) = uploads.next().await {
        match result? {
            Some((path, reason)) => {
                report.rejections.insert(path, reason);
            }
            None => report.accepted_files += 1,
        }
    }
    Ok(())
}
fn remove_snapshot(state: &LocalState, snapshot: &Path) -> Result<()> {
    let directory = snapshot.parent().context("snapshot has no parent")?;
    if directory.parent() != Some(state.directory.join("pending").as_path()) {
        bail!("pending snapshot path is outside the state directory");
    }
    match std::fs::remove_file(snapshot) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    match std::fs::remove_dir(directory) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
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
