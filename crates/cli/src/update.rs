//! Update the workstation installation at a sync boundary. The release installer
//! owns setup files; committing the executable last makes failed setup retryable.
use anyhow::{Context, Result, anyhow, bail, ensure};
use flate2::read::GzDecoder;
use reqwest::{Client, StatusCode, header::LOCATION, redirect::Policy};
use semver::Version;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{process::Command, time::timeout};
use url::Url;

const REPOSITORY: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/");
const MIB: usize = 1024 * 1024;

pub struct Updater {
    client: Client,
    repository: Url,
    executable: PathBuf,
    running: Version,
    target: &'static str,
}

impl Updater {
    fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .https_only(true)
                .redirect(Policy::none())
                .user_agent(concat!("sessiondb/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(4))
                .build()?,
            repository: Url::parse(REPOSITORY)?,
            executable: std::env::current_exe()?.canonicalize()?,
            running: Version::parse(env!("CARGO_PKG_VERSION"))?,
            target: env!("SESSIONDB_BUILD_TARGET"),
        })
    }

    /// Keep the executable path and HTTP connection pool across watch passes.
    pub async fn before_sync(slot: &mut Option<Self>, server: &str, quiet: bool) -> Result<()> {
        let updater = match slot {
            Some(updater) => updater,
            None => slot.insert(Self::new()?),
        };
        if let Some(version) = updater.install_if_newer().await? {
            if !quiet {
                eprintln!("Resuming sync with SessionDB {version}");
            }
            // exec preserves the service PID, original flags and cwd. Pin the
            // original watch target even if 'connect' changed the default URL.
            let error = std::process::Command::new(&updater.executable)
                .args(std::env::args_os().skip(1))
                .env("SESSIONDB_URL", server)
                .exec();
            return Err(error).context("restart the updated client");
        }
        Ok(())
    }

    async fn latest(&self) -> Result<Option<Version>> {
        // The public latest-release redirect avoids an unauthenticated REST API
        // quota shared by every workstation on the same IP.
        let response = self
            .client
            .head(self.repository.join("releases/latest")?)
            .timeout(Duration::from_secs(6))
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        ensure!(
            response.status().is_redirection(),
            "latest release returned HTTP {}",
            response.status()
        );
        let location = response
            .headers()
            .get(LOCATION)
            .context("missing release location")?
            .to_str()?;
        let url = response.url().join(location)?;
        let prefix = self.repository.join("releases/tag/v")?;
        let tag = url
            .as_str()
            .strip_prefix(prefix.as_str())
            .context("unexpected release tag URL")?;
        let version = Version::parse(&tag.replace("%2B", "+").replace("%2b", "+"))?;
        Ok(
            (version.pre.is_empty() && version.cmp_precedence(&self.running).is_gt())
                .then_some(version),
        )
    }

    async fn download(&self, mut url: Url, limit: usize) -> Result<Vec<u8>> {
        for _ in 0..5 {
            let mut response = self
                .client
                .get(url.clone())
                .timeout(Duration::from_secs(60))
                .send()
                .await
                .map_err(reqwest::Error::without_url)?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .context("missing asset redirect")?
                    .to_str()?;
                url = url.join(location)?;
                ensure!(
                    url.scheme() == "https"
                        && url.host_str().is_some_and(
                            |h| h == "github.com" || h.ends_with(".githubusercontent.com")
                        ),
                    "unexpected release asset redirect"
                );
                continue;
            }
            ensure!(
                response.status().is_success(),
                "release download returned HTTP {}",
                response.status()
            );
            ensure!(
                response.content_length().unwrap_or(0) <= limit as u64,
                "release download too large"
            );
            let mut data = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(reqwest::Error::without_url)?
            {
                ensure!(
                    chunk.len() <= limit - data.len(),
                    "release download too large"
                );
                data.extend_from_slice(&chunk);
            }
            return Ok(data);
        }
        bail!("too many release asset redirects")
    }

    async fn install_if_newer(&self) -> Result<Option<Version>> {
        let Some(version) = self.latest().await? else {
            return Ok(None);
        };
        let tag = format!("v{version}");
        let name = format!("sessiondb-{}-{}", tag.replace('+', "_"), self.target);
        let archive = format!("{name}.tar.gz");
        let base = self.repository.join(&format!("releases/download/{tag}/"))?;
        let sums = self.download(base.join("SHA256SUMS")?, MIB).await?;
        let Some(digest) = checksum(std::str::from_utf8(&sums)?, &archive)? else {
            // Partial-platform releases are valid. A later pass can pick up a newly added asset.
            return Ok(None);
        };

        let bin_dir = self
            .executable
            .parent()
            .context("missing client installation directory")?;
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(self.executable.with_added_extension("update.lock"))
            .context("open update lock in client installation directory")?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(e)) => return Err(e.into()),
        }
        let on_disk = binary_version(&self.executable).await?;
        if on_disk.cmp_precedence(&self.running).is_gt() {
            // Another watcher (or a manual installation) already replaced this binary.
            return Ok(Some(on_disk));
        }

        let bytes = self.download(base.join(&archive)?, 128 * MIB).await?;
        ensure!(
            Sha256::digest(&bytes)[..] == digest,
            "release archive SHA-256 mismatch"
        );
        let dir = bin_dir.to_owned();
        let bundle = name.clone();
        let stage =
            tokio::task::spawn_blocking(move || stage_bundle(&dir, &bundle, &bytes)).await??;
        let package = stage.path().join(name);
        let binary = package.join("sessiondb");
        ensure!(
            binary_version(&binary).await? == version,
            "packaged CLI version differs from its tag"
        );
        let installer = package.join("scripts/install-harnesses");
        let output = timeout(
            Duration::from_secs(60),
            Command::new(installer)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("harness installer timed out")?
        .context("run the release harness installer")?;
        ensure!(
            output.status.success(),
            "harness installer failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        fs::rename(binary, &self.executable).context("replace installed SessionDB executable")?;
        Ok(Some(version))
    }
}

async fn binary_version(path: &Path) -> Result<Version> {
    let output = timeout(
        Duration::from_secs(10),
        Command::new(path)
            .arg("--version")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("client version check timed out")?
    .context("run client version check")?;
    ensure!(output.status.success(), "client version check failed");
    let value = std::str::from_utf8(&output.stdout)?
        .trim()
        .strip_prefix("sessiondb ")
        .context("unexpected client version output")?;
    Ok(Version::parse(value)?)
}

fn checksum(manifest: &str, archive: &str) -> Result<Option<[u8; 32]>> {
    let mut digest = None;
    for line in manifest.lines() {
        let Some((hash, name)) = line.split_once("  ").or_else(|| line.split_once(" *")) else {
            bail!("invalid release checksum manifest");
        };
        if name == archive {
            ensure!(digest.is_none(), "duplicate release archive checksum");
            let bytes = hex::decode(hash)?
                .try_into()
                .map_err(|_| anyhow!("invalid SHA-256 digest"))?;
            digest = Some(bytes);
        }
    }
    Ok(digest)
}

fn stage_bundle(parent: &Path, name: &str, bytes: &[u8]) -> Result<TempDir> {
    let stage = tempfile::Builder::new()
        .prefix(".sessiondb-update-")
        .tempdir_in(parent)?;
    let gzip = GzDecoder::new(bytes).take((256 * MIB + 1) as u64);
    let mut archive = tar::Archive::new(gzip);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let relative = path
            .strip_prefix(name)
            .context("unexpected release archive root")?;
        ensure!(
            relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
            "invalid release archive path"
        );
        let dest = stage.path().join(name).join(relative);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(dest)?;
        } else {
            ensure!(
                kind.is_file() && !relative.as_os_str().is_empty(),
                "expected a regular release file"
            );
            let mode = if entry.header().mode()? & 0o111 != 0 {
                0o755
            } else {
                0o644
            };
            fs::create_dir_all(dest.parent().context("missing release file parent")?)?;
            let mut file = File::create_new(&dest)?;
            let size = entry.size();
            ensure!(
                io::copy(&mut entry, &mut file)? == size,
                "incomplete release file"
            );
            file.set_permissions(fs::Permissions::from_mode(mode))?;
            file.sync_all()?;
        }
    }
    let mut gzip = archive.into_inner();
    io::copy(&mut gzip, &mut io::sink())?;
    ensure!(gzip.limit() > 0, "unpacked release exceeds size limit");
    let package = stage.path().join(name);
    // The bundled installer owns the layout of its resources, including future ones.
    for file in ["sessiondb", "scripts/install-harnesses"] {
        ensure!(package.join(file).is_file(), "release is missing {file}");
    }
    Ok(stage)
}

#[cfg(test)]
mod tests;
