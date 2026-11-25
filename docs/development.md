# Build and run SessionDB

The initial implementation supports original-file storage, automatic sync, session/message reads, and native export for Codex, Claude Code, and Pi. See [implementation status](implementation.md) for the current boundary.

## Build

Install the toolchain selected by [rust-toolchain.toml](../rust-toolchain.toml), a C/C++ build toolchain, CMake, and pkg-config.

```sh
cargo build --locked --workspace --bins
export PATH="$PWD/target/debug:$PATH"
```

The two executables are `sessiondb-server` and `sessiondb`. Both support `--help` and `--version`. The [local packaging script](#release-the-client) builds a distributable workstation CLI.

## Release the client

Client releases are built locally and uploaded manually to [GitHub Releases](https://github.com/mitchwolfe1/sessiondb/releases). GitHub Actions is disabled for this repository. Run the [local checks](#checks) on the release commit and use a tag matching the Cargo version of `sessiondb-cli`. Bump the client version even for a skill-only release so installed watchers can discover and apply it.

On each Linux or macOS machine you want to support, check out the same release commit and run:

```sh
tag="v$(cargo metadata --locked --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "sessiondb-cli") | .version')"
target="$(rustc -vV | sed -n 's/^host: //p')"
scripts/package-client "$target" "target/dist/$tag" "$tag"
```

The [packager](../scripts/package-client) builds the CLI with the pinned toolchain and locked dependencies, unpacks the archive, checks the executable version and installs the bundled skill into a temporary home. Each build produces a `.tar.gz` archive and a matching `.sha256` file. The archive includes `sessiondb`, `scripts/install-harnesses`, and `skills/sessiondb/SKILL.md`. When adding workstation setup files, include them in the archive and install them from the bundled `scripts/install-harnesses` entry point. Updaters run that release's installer from the extracted bundle before replacing the binary. The installer must tolerate retries after an interruption.

For a new release, collect the intended archives and their checksum files in one directory on the publishing machine. A single platform is sufficient. From the release commit, push its tag and run the [publisher](../scripts/publish-client-release) with an authenticated GitHub CLI:

```sh
git tag -a "$tag" -m "SessionDB client $tag"
git push origin "refs/tags/$tag"
GH_REPO=mitchwolfe1/sessiondb scripts/publish-client-release "$tag" "target/dist/$tag"
```

The publisher checks the remote tag and each archive digest, uploads the archives and `SHA256SUMS` to a draft, then publishes it. Use the same set of archives when resuming an interrupted draft. SemVer prerelease versions are marked as GitHub prereleases.

## Local services

Docker with Compose is required for this development profile:

```sh
./dev/up
set -a
source .env
set +a
```

This creates private local credentials and starts PostgreSQL and an S3-compatible store (SeaweedFS), and supplies the required `SESSIONDB_API_KEY` to the server and clients. No identity provider is needed. Their ports are bound to loopback and their data uses Compose volumes. The profile does not enable collection of any session store.

Start the catalog and service:

```sh
sessiondb-server --config dev/server.toml migrate
sessiondb-server --config dev/server.toml
```

The API listens at `http://127.0.0.1:58080`. `GET /healthz` and `GET /readyz` are public liveness/readiness probes. `GET /v1/info` reports actual capabilities only with the bearer API key. The checked-in contract is served at `/openapi.yaml`.

In another terminal, add `target/debug` to PATH and connect:

```sh
set -a
source .env
set +a
sessiondb connect http://127.0.0.1:58080
```

The private `.env` supplies the instance key for this example. The server requires a nonempty `SESSIONDB_API_KEY`. The client validates the key, then saves it in its private config for subsequent commands and background sync.

## Try synthetic sessions

Enable a fixture directory explicitly:

```sh
example_root="$(mktemp -d)"
cp fixtures/native/codex/home-tools.jsonl "$example_root/"
sessiondb sources add --harness codex --root "$example_root"
sessiondb sync
sessiondb sessions list --harness codex
sessiondb sessions show SESSION_ID
sessiondb sessions messages SESSION_ID --json
sessiondb messages content MESSAGE_ID --json
sessiondb sessions export SESSION_ID --format native --output session.zip
```

Use a directory containing accepted fixtures: the full fixture tree also contains deliberate rejection cases. Unfinished `.jsonl.part` captures are excluded from discovery; malformed or unsupported `.jsonl` files are rejected.

Use `--harness claude` or `--harness pi` with a directory containing accepted fixtures for that format. To collect your native stores, use `sessiondb sources add --detect`, or provide a harness and root explicitly. Collection follows enabled stores, independently of starting cwd or project assignment.

`sessiondb sync --watch` reconciles periodically until interrupted. Use `sessiondb sync install` to run that watcher as a login service and `sessiondb sync uninstall` to remove it. `sessiondb sync status --json` reports accepted checkpoints and `pending_files`. Pending snapshots are persisted before transfer and each successful file receipt advances its checkpoint independently.

The API verifies and fully parses one file before storing originals/artifacts and publishing its catalog state before HTTP 200. Unsupported or malformed native content rejects the entire file with HTTP 400 before storage; there is no background parse job or acceptance wait command.

Stop the development dependencies with:

```sh
docker compose --env-file .env -f dev/compose.yaml down
```

Volumes persist across stops. Removing these volumes resets the development database and object store.

## Local hybrid search

For a disposable development deployment with pgvector, use the matching PostgreSQL major version and initialize the optional extension. Supply the instance environment as in the local services section.

```sh
SESSIONDB_DEV_POSTGRES_IMAGE=pgvector/pgvector:0.8.2-pg18-trixie ./dev/up
docker compose --env-file .env -f dev/compose.yaml exec postgres psql -U sessiondb -d sessiondb -c 'CREATE EXTENSION IF NOT EXISTS vector'
sessiondb-server --config dev/server.toml migrate
```

Start the server, create and enable an [embedding profile](search-and-embeddings.md#configure-semantic-and-hybrid-search), then run `sessiondb search --mode hybrid "a past decision"`. The Docker integration suite tests both stock PostgreSQL and the pgvector-enabled path with a deterministic local provider.

## Existing infrastructure

Use [the server configuration example](examples/server.toml) with PostgreSQL and an existing S3 bucket. Provision the instance key securely. With AWS, omit the custom endpoint and leave both `SESSIONDB_S3_ACCESS_KEY_ID` and `SESSIONDB_S3_SECRET_ACCESS_KEY` unset to use the SDK credential chain, including instance/task roles. Custom endpoints can use path-style addressing. Only the server accesses S3; clients need only the API URL.

[Authentication](authentication.md) describes the shared-key boundary. Both HTTP and HTTPS server URLs are supported; use HTTPS across untrusted networks. The only external services required are PostgreSQL and S3.

For a Tailscale-only host named `athena`, a direct HTTP deployment can use:

```toml
listen = "0.0.0.0:6767"
public_url = "http://athena:6767"
```

Restrict port 6767 to the tailnet with a firewall, or replace the wildcard listen address with the host's Tailscale IP. The wildcard listens on every IPv4 interface. Tailscale encrypts traffic between peers; HTTP itself does not. The API key is still required.

Environment overrides are `SESSIONDB_LISTEN`, `SESSIONDB_PUBLIC_URL`, and `SESSIONDB_S3_ENDPOINT`. Secrets use the fixed `SESSIONDB_DATABASE_URL`, `SESSIONDB_API_KEY`, `SESSIONDB_S3_ACCESS_KEY_ID`, and `SESSIONDB_S3_SECRET_ACCESS_KEY` names; see [configuration and credentials](operations.md#configuration-and-credentials). No `[database]`, `[auth]`, or `*_env` TOML fields are needed. Supply container configuration and run `migrate` before serving. Limits contain only `max_file_bytes`; no public S3 endpoint, presign TTL, or worker settings remain.

For an existing local development `.env`, rename `SESSIONDB_DEV_S3_ACCESS_KEY` to `SESSIONDB_S3_ACCESS_KEY_ID` and `SESSIONDB_DEV_S3_SECRET_KEY` to `SESSIONDB_S3_SECRET_ACCESS_KEY`, preserving their values. Do not regenerate credentials or remove persistent volumes to perform this rename.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo doc --locked --workspace --no-deps
```

Run the Docker integration suite after building the executables:

```sh
cargo build --locked --workspace --bins
cargo test --locked -p sessiondb-cli --test end_to_end -- --ignored
```

The suite uses isolated loopback PostgreSQL/S3 services and synthetic keys/transcripts, then removes its containers/volumes. It covers binary/SDK authentication, per-file verified acceptance and whole-file rejection, native export, appends/rewrites, lost responses, watch/outage recovery, rejected-file repair, restart persistence, and backup/restore. Search checks cover upgrade backfill, matching across a request and reply, excerpts beyond the preview limit, fork selection, tool filters, retries and rewrite cleanup. Historical tolerant-parser results are not proof that strict rejection works.

On a Linux login with a systemd user manager and Docker, run the background service integration separately. It installs a uniquely named watcher for a temporary state directory and loopback server, exercises its lifecycle, and removes the test unit.

```sh
cargo test --locked -p sessiondb-cli --test background_service -- --ignored
```

Run the installer, browser fixture, and documentation checks locally:

```sh
python3 scripts/test_install_harnesses.py
python3 -m unittest discover -s dev/evals/browser
uv run --with pyyaml --with openapi-spec-validator --with jsonschema python scripts/check-contracts.py
npx markdownlint-cli2
```
