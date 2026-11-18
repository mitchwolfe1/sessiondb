# Build and run SessionDB

The initial implementation supports original-file storage, automatic sync, session/message reads, and native export for Codex, Claude Code, and Pi. See [implementation status](implementation.md) for the current boundary.

## Build

Install the toolchain selected by [rust-toolchain.toml](../rust-toolchain.toml), a C/C++ build toolchain, CMake, and pkg-config. Linux CLI builds also need the D-Bus development package for OS credential storage; on Debian/Ubuntu this is `libdbus-1-dev`. The macOS CLI uses the native credential store.

```sh
cargo build --locked --workspace --bins
export PATH="$PWD/target/debug:$PATH"
```

The two executables are `sessiondb-server` and `sessiondb`. There are no published binary releases yet. Both support `--help` and `--version`.

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
sessiondb context create local --server http://127.0.0.1:58080
sessiondb context use local
```

The private `.env` supplies the instance key; do not print or commit it. The server fails at startup if `SESSIONDB_API_KEY` is missing or empty. Environment-key clients need no login or OS credential-store session. Optionally, `sessiondb auth login --key-stdin` validates and saves a key in the OS store without a browser or network credential exchange. Credentials are never written into ordinary client configuration.

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

`sessiondb sync --watch` reconciles periodically until interrupted. Notifications and service installation remain planned. `sessiondb sync status --json` reports accepted checkpoints and `pending_files`. Pending snapshots are persisted before transfer and each successful file receipt advances its checkpoint independently.

The API verifies and fully parses one file before storing originals/artifacts and publishing its catalog state before HTTP 200. Unsupported or malformed native content rejects the entire file with HTTP 400 before storage; there is no background parse job or acceptance wait command.

Stop the development dependencies with:

```sh
docker compose --env-file .env -f dev/compose.yaml down
```

Volumes persist across stops. Removing these volumes resets the development database and object store.

## Existing infrastructure

Use [the server configuration example](examples/server.toml) with PostgreSQL and an existing S3 bucket. Provision the instance key securely. With AWS, omit custom endpoint and explicit credential references to use the SDK chain, including instance/task roles. Custom endpoints can use path-style addressing. Only the server accesses S3; clients need only the API URL.

[Authentication](authentication.md) describes the shared-key boundary. Remote deployments use HTTPS; loopback HTTP is a development option. The only external services required are PostgreSQL and S3.

Environment overrides are `SESSIONDB_LISTEN`, `SESSIONDB_PUBLIC_URL`, and `SESSIONDB_S3_ENDPOINT`. Database/object secrets use configured environment names. Supply container configuration and run `migrate` before serving. Limits contain only `max_file_bytes`; no public S3 endpoint, presign TTL, or worker settings remain.

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

The suite uses isolated loopback PostgreSQL/S3 services and synthetic keys/transcripts, then removes its containers/volumes. It covers binary/SDK authentication, per-file verified acceptance and whole-file rejection, native export, appends/rewrites, lost responses, watch/outage recovery, rejected-file repair, restart persistence, and backup/restore. Historical tolerant-parser results are not proof that strict rejection works.

Validate documentation contracts separately:

```sh
uv run --with pyyaml --with openapi-spec-validator --with jsonschema python scripts/check-contracts.py
npx markdownlint-cli2
```
