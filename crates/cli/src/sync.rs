use crate::{
    config::{ConfiguredSource, SyncOptions, expand, private_dir},
    state::{LocalState, Pending},
};
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde::Serialize;
use sessiondb_client::Client;
use sessiondb_types::{
    FileUpload, Harness, ListOptions, ServerInfo, Source, SourceCreate, SourceFile,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
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
    let mut options = ListOptions {
        limit: Some(200),
        ..Default::default()
    };
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
    pub deferred_changing_files: usize,
    pub cached_rejections: usize,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub rejections: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub retryable_errors: BTreeMap<String, String>,
}

pub async fn source(
    client: &Client,
    state: &LocalState,
    source: &Source,
    options: &SyncOptions,
    quiet: bool,
    mut watch: Option<&mut WatchFiles>,
    info: &ServerInfo,
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
        &info.server_version,
    )
    .await?;
    let current_catalog = watch.as_ref().and_then(|w| w.catalog.as_ref()).filter(|c| {
        report.accepted_files == 0
            && c.synced_at == source.last_synced_at
            && c.fetched.elapsed() < Duration::from_secs(15 * 60)
    });
    let remote = if let Some(catalog) = current_catalog {
        Arc::clone(&catalog.files)
    } else {
        let files = Arc::new(remote_files(client, source.id).await?);
        if let Some(w) = watch.as_deref_mut() {
            w.catalog = Some(Catalog {
                files: Arc::clone(&files),
                synced_at: source.last_synced_at,
                fetched: Instant::now(),
            });
        }
        files
    };
    let root = PathBuf::from(&source.root);
    let source_id = source.id;
    let pending_root = state.directory.join("pending");
    let attempted_failures = report
        .rejections
        .keys()
        .chain(report.retryable_errors.keys())
        .cloned()
        .collect();
    let index = ScanIndex {
        remote,
        attempted_failures,
        max_bytes: info.limits.max_file_bytes,
        rejected: state.rejection_hashes(client.server(), source_id).await?,
        version: info.server_version.clone(),
        previous: watch.as_deref_mut().map(|w| std::mem::take(&mut w.local)),
    };
    let scan =
        tokio::task::spawn_blocking(move || scan(&root, source_id, &pending_root, index)).await??;
    report.deferred_tail_files = scan.deferred;
    report.deferred_changing_files = scan.changing;
    report.cached_rejections = scan.cached_rejections;
    if let (Some(watch), Some(current)) = (watch, scan.watched) {
        watch.local = current;
    }
    state
        .forget_rejections(client.server(), source_id, scan.forgotten)
        .await?;
    if let Some(staging) = scan.staging {
        // From this point SQLite may commit references even if the caller is interrupted.
        let _ = staging.keep();
        state.save_pending(&scan.files).await?;
        resume(
            client,
            state,
            scan.files,
            options,
            quiet,
            &mut report,
            &info.server_version,
        )
        .await?;
    }
    Ok(report)
}
struct Scan {
    files: Vec<Pending>,
    staging: Option<tempfile::TempDir>,
    deferred: usize,
    changing: usize,
    cached_rejections: usize,
    forgotten: Vec<String>,
    watched: Option<HashMap<String, SeenFile>>,
}
#[derive(Default)]
pub struct WatchFiles {
    local: HashMap<String, SeenFile>,
    catalog: Option<Catalog>,
}
struct Catalog {
    synced_at: Option<chrono::DateTime<chrono::Utc>>,
    fetched: Instant,
    files: Arc<HashMap<String, SourceFile>>,
}

#[derive(PartialEq, Eq)]
struct FileStamp {
    length: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl FileStamp {
    fn read(meta: &std::fs::Metadata) -> Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            length: meta.len(),
            modified: meta.modified()?,
            #[cfg(unix)]
            identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
        })
    }
}
struct SeenFile {
    stamp: FileStamp,
    length: u64,
    digest: String,
}
struct ScanIndex {
    remote: Arc<HashMap<String, SourceFile>>,
    attempted_failures: HashSet<String>,
    rejected: HashMap<String, (String, String)>,
    max_bytes: u64,
    version: String,
    previous: Option<HashMap<String, SeenFile>>,
}
fn scan(root: &Path, source: Uuid, pending_root: &Path, mut index: ScanIndex) -> Result<Scan> {
    if !root.is_dir() {
        bail!("native store {} is unavailable", root.display());
    }
    let mut staging: Option<tempfile::TempDir> = None;
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Err(e)
                if index.previous.is_some()
                    && e.io_error()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                continue;
            }
            entry => entry.context("scan native session store")?,
        };
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "jsonl") {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    let mut files = Vec::new();
    let mut deferred = 0;
    let mut changing = 0;
    let mut cached_rejections = 0;
    let mut watched = index.previous.as_ref().map(|_| HashMap::new());
    for path in paths {
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .context("session path must be UTF-8")?
            .replace(std::path::MAIN_SEPARATOR, "/");
        if index.attempted_failures.contains(&relative) {
            index.rejected.remove(&relative);
            continue;
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = match options.open(&path) {
            Err(e) if watched.is_some() && e.kind() == std::io::ErrorKind::NotFound => continue,
            opened => opened.with_context(|| format!("read {}", path.display()))?,
        };
        let stamp = FileStamp::read(&file.metadata()?)?;
        if stamp.length > index.max_bytes {
            bail!("{} exceeds the server's file limit", path.display());
        }
        let remote = index.remote.get(&relative);
        let accepted = |f: &SeenFile| {
            remote.is_some_and(|r| r.sha256 == f.digest && r.byte_length == f.length)
        };
        let remembered = index
            .rejected
            .get(&relative)
            .filter(|(_, v)| watched.is_some() && v == &index.version);
        let rejected = |f: &SeenFile| remembered.is_some_and(|(hash, _)| hash == &f.digest);
        let cached = index
            .previous
            .as_mut()
            .and_then(|cache| cache.remove(&relative))
            .filter(|f| {
                cfg!(unix) && f.stamp == stamp && (f.length == 0 || accepted(f) || rejected(f))
            });
        let mut bytes = Vec::new();
        let seen = if let Some(cached) = cached {
            cached
        } else {
            (&file).take(stamp.length).read_to_end(&mut bytes)?;
            if bytes.len() as u64 != stamp.length || FileStamp::read(&file.metadata()?)? != stamp {
                if watched.is_some() {
                    changing += 1;
                    index.rejected.remove(&relative);
                    continue;
                }
                bail!(
                    "{} changed during capture; retry synchronization",
                    path.display()
                );
            }
            let length = sessiondb_adapters::complete_prefix(&bytes).len();
            bytes.truncate(length);
            SeenFile {
                stamp,
                length: length as u64,
                digest: hex::encode(Sha256::digest(&bytes)),
            }
        };
        if seen.length != seen.stamp.length {
            deferred += 1;
        }
        let skip = seen.length == 0 || accepted(&seen);
        let cached_rejection = !skip && rejected(&seen);
        let sha256 = seen.digest.clone();
        if let Some(cache) = &mut watched {
            cache.insert(relative.clone(), seen);
        }
        if cached_rejection {
            index.rejected.remove(&relative);
            cached_rejections += 1;
        }
        if skip || cached_rejection {
            continue;
        }
        let directory = match &staging {
            Some(dir) => dir,
            None => {
                private_dir(pending_root)?;
                staging.insert(tempfile::tempdir_in(pending_root)?)
            }
        };
        let snapshot_path = directory.path().join(files.len().to_string());
        let mut snapshot = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&snapshot_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            snapshot.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        snapshot.write_all(&bytes)?;
        snapshot.sync_all()?;
        files.push(Pending {
            source_id: source,
            metadata: FileUpload {
                path: relative.clone(),
                byte_length: bytes.len() as u64,
                sha256,
                expected_version: remote.map(|f| f.version.clone()),
                project_ids: vec![],
            },
            snapshot: snapshot_path,
        });
    }
    // Make the snapshot names durable before SQLite starts referring to them.
    if let Some(dir) = &staging {
        std::fs::File::open(dir.path())?.sync_all()?;
        std::fs::File::open(pending_root)?.sync_all()?;
    }
    Ok(Scan {
        files,
        staging,
        deferred,
        changing,
        cached_rejections,
        forgotten: index.rejected.into_keys().collect(),
        watched,
    })
}

enum UploadOutcome {
    Accepted,
    Rejected(String, String),
    Pending(String, String),
}

async fn resume(
    client: &Client,
    state: &LocalState,
    pending: Vec<Pending>,
    options: &SyncOptions,
    quiet: bool,
    report: &mut SourceReport,
    server_version: &str,
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
                    let reason = sessiondb_client::Error::Problem(problem).to_string();
                    state
                        .reject(&pending, client.server(), server_version, &reason)
                        .await?;
                    remove_snapshot(state, &pending.snapshot)?;
                    eprintln!(
                        "Rejected {} for source {}: {reason}",
                        pending.metadata.path, pending.source_id
                    );
                    return Ok(UploadOutcome::Rejected(pending.metadata.path, reason));
                }
                Err(sessiondb_client::Error::Problem(problem))
                    if problem.status == 500 && problem.code == "internal_error" =>
                {
                    let reason = sessiondb_client::Error::Problem(problem).to_string();
                    eprintln!(
                        "Pending {} for source {}: {reason}",
                        pending.metadata.path, pending.source_id
                    );
                    return Ok(UploadOutcome::Pending(pending.metadata.path, reason));
                }
                Err(error) => {
                    return Err(error).with_context(|| format!("upload {}", pending.metadata.path));
                }
            };
            state.accept(client.server(), &pending, &receipt).await?;
            remove_snapshot(state, &pending.snapshot)?;
            if !quiet {
                eprintln!(
                    "Accepted {} for source {}",
                    pending.metadata.path, pending.source_id
                );
            }
            Ok::<_, anyhow::Error>(UploadOutcome::Accepted)
        })
        .buffer_unordered(options.upload_concurrency.clamp(1, 16));
    while let Some(result) = uploads.next().await {
        match result? {
            UploadOutcome::Rejected(path, reason) => {
                report.rejections.insert(path, reason);
            }
            UploadOutcome::Pending(path, reason) => {
                report.retryable_errors.insert(path, reason);
            }
            UploadOutcome::Accepted => report.accepted_files += 1,
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

#[cfg(test)]
mod watch_tests {
    use super::*;
    #[test]
    fn a_cached_digest_does_not_hide_replacement_or_remote_restore() {
        let root = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let name = "active.jsonl";
        let path = root.path().join(name);
        let bytes = b"one\n";
        std::fs::write(&path, bytes).unwrap();
        let modified = path.metadata().unwrap().modified().unwrap();
        let source = Uuid::new_v4();
        let remote = SourceFile {
            id: Uuid::new_v4(),
            source_id: source,
            path: name.into(),
            generation_id: Uuid::new_v4(),
            byte_length: bytes.len() as u64,
            sha256: hex::encode(Sha256::digest(bytes)),
            version: "accepted".into(),
        };
        let index = |files, previous| ScanIndex {
            remote: Arc::new(files),
            attempted_failures: HashSet::new(),
            rejected: HashMap::new(),
            max_bytes: 1024,
            version: "1".into(),
            previous,
        };
        let accepted = HashMap::from([(name.into(), remote)]);
        let first = scan(
            root.path(),
            source,
            staging.path(),
            index(accepted.clone(), Some(HashMap::new())),
        )
        .unwrap();
        let idle = scan(
            root.path(),
            source,
            staging.path(),
            index(accepted.clone(), first.watched),
        )
        .unwrap();
        assert!(idle.files.is_empty() && idle.staging.is_none());
        let restored = scan(
            root.path(),
            source,
            staging.path(),
            index(HashMap::new(), idle.watched),
        )
        .unwrap();
        assert_eq!(std::fs::read(&restored.files[0].snapshot).unwrap(), bytes);
        assert_eq!(restored.files[0].metadata.expected_version, None);

        let replacement = root.path().join("replacement");
        std::fs::write(&replacement, b"two\n").unwrap();
        OpenOptions::new()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        std::fs::rename(replacement, &path).unwrap();
        let changed = scan(
            root.path(),
            source,
            staging.path(),
            index(accepted, restored.watched),
        )
        .unwrap();
        assert_eq!(std::fs::read(&changed.files[0].snapshot).unwrap(), b"two\n");
        assert_eq!(
            changed.files[0].metadata.expected_version.as_deref(),
            Some("accepted")
        );
    }

    #[test]
    fn unchanged_rejections_are_skipped_only_for_the_recorded_version_in_watch_mode() {
        let root = tempfile::tempdir().unwrap();
        let snapshots = tempfile::tempdir().unwrap();
        let path = "native.jsonl";
        let bytes = b"{\"type\":\"unsupported\"}\n";
        std::fs::write(root.path().join(path), bytes).unwrap();
        let hash = hex::encode(Sha256::digest(bytes));
        let index = |version: &str, watch: bool| ScanIndex {
            remote: Arc::new(HashMap::new()),
            attempted_failures: HashSet::new(),
            max_bytes: 1024,
            rejected: HashMap::from([(path.into(), (hash.clone(), "1".into()))]),
            version: version.into(),
            previous: watch.then(HashMap::new),
        };
        let source = Uuid::new_v4();
        let first = scan(
            root.path(),
            source,
            &snapshots.path().join("first"),
            index("1", true),
        )
        .unwrap();
        assert_eq!(first.cached_rejections, 1);
        assert!(first.files.is_empty() && first.forgotten.is_empty());
        let mut upgrade = index("2", true);
        upgrade.previous = first.watched;
        let retry = scan(
            root.path(),
            source,
            &snapshots.path().join("upgrade"),
            upgrade,
        )
        .unwrap();
        assert_eq!(retry.files.len(), 1);
        assert_eq!(std::fs::read(&retry.files[0].snapshot).unwrap(), bytes);
        assert_eq!(retry.forgotten, [path]);
        let normal = scan(
            root.path(),
            source,
            &snapshots.path().join("explicit"),
            index("1", false),
        )
        .unwrap();
        assert_eq!(normal.files.len(), 1);
        std::fs::remove_file(root.path().join(path)).unwrap();
        let removed = scan(
            root.path(),
            source,
            &snapshots.path().join("removed"),
            index("1", true),
        )
        .unwrap();
        assert_eq!(removed.forgotten, [path]);
    }
}
