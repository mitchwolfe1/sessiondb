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

This creates private local credentials and starts PostgreSQL, an S3-compatible store, and an identity provider. The profile uses SeaweedFS and Keycloak as reference services; SessionDB communicates through S3 and OIDC. Their ports are bound to loopback and their data uses Compose volumes. The profile does not enable collection of any session store.

Start the catalog and service:

```sh
sessiondb-server --config dev/server.toml migrate
sessiondb-server --config dev/server.toml
```

The API listens at `http://127.0.0.1:58080`. `GET /healthz`, `GET /readyz`, and `GET /v1/info` report liveness, dependency readiness, and actual capabilities. The checked-in contract is served at `/openapi.yaml`.

In another terminal, add `target/debug` to PATH and connect:

```sh
sessiondb auth login --server http://127.0.0.1:58080
sessiondb context create local --organization local
sessiondb context use local
```

The development login is `ada`; its generated password is `SESSIONDB_DEV_PASSWORD` in the private `.env` file. Login uses a browser and stores credentials in the OS credential store. `--no-browser` prints the authorization URL for manual opening. On a headless machine, supply an API-audience access token through `SESSIONDB_TOKEN`; member-issued API token provisioning is a later milestone. Credentials are never written into the ordinary client configuration.

## Try synthetic sessions

Enable a fixture directory explicitly:

```sh
sessiondb sources add --harness codex --root "$PWD/fixtures/native/codex"
sessiondb sync
sessiondb sessions list --harness codex
sessiondb sessions show SESSION_ID
sessiondb sessions messages SESSION_ID --json
sessiondb messages content MESSAGE_ID --json
sessiondb sessions export SESSION_ID --format native --output session.zip
```

The `.jsonl.part` fixture deliberately represents an unfinished capture and is excluded from discovery. The regular JSONL fixtures include independent, inherited, and unknown-content examples.

Use `--harness claude --root "$PWD/fixtures/native/claude"` or `--harness pi --root "$PWD/fixtures/native/pi"` to try the other formats. To collect your native stores, use `sessiondb sources add --detect`, or provide a harness and root explicitly. Collection follows enabled stores, independently of starting cwd or project assignment.

`sessiondb sync --watch` reconciles periodically until interrupted. It currently uses polling; filesystem notifications and user-service installation are later work. `sessiondb sync status --json` reports accepted checkpoints and pending imports. Sync saves pending snapshots and request keys locally before transfer, and advances checkpoints after the server's acceptance receipt.

API processing can continue after sync acceptance. Session detail reports parsing state; `sessiondb jobs show JOB_ID` and `sessiondb jobs wait JOB_ID` inspect the verification job. Import state through the API also reports later parsing and bounded errors.

Stop the development dependencies with:

```sh
docker compose --env-file .env -f dev/compose.yaml down
```

Volumes persist across stops. Removing these volumes resets the development database, object store, and identity provider.

## Existing infrastructure

Use [the server configuration example](examples/server.toml) with an existing PostgreSQL database, S3 bucket, and compatible OIDC provider. Create the bucket before starting SessionDB. With AWS, omit the custom endpoint and explicit credential environment names to use the SDK credential chain, including instance/task roles. Custom S3 endpoints can use path-style addressing. `public_endpoint` can differ from the server's internal endpoint so signed upload URLs are reachable by clients.

The initial OIDC access-token profile is RS256 with the configured issuer, subject, API audience, and expiry. [Authentication](authentication.md) describes provider setup and the tested Keycloak reference. Production uses HTTPS; the local profile explicitly enables loopback HTTP for its development issuer.

Environment overrides are `SESSIONDB_LISTEN`, `SESSIONDB_PUBLIC_URL`, `SESSIONDB_OIDC_ISSUER`, `SESSIONDB_S3_ENDPOINT`, and `SESSIONDB_S3_PUBLIC_ENDPOINT`. Database and explicit object credentials use the environment names in the configuration file. The container runs `sessiondb-server`; supply its configuration and run the `migrate` command before serving.

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

It starts isolated services on allocated loopback ports, uses synthetic identities/transcripts, and removes its containers and volumes afterward. It exercises the real binaries and SDK, including access isolation, PKCE login/refresh, signed uploads, atomic acceptance, native export, append/rewrite behavior, lost responses, watch mode, outage recovery, expired job leases, restart persistence, and catalog backup/restore. Existing development services are not used by the suite.

Validate documentation contracts separately:

```sh
uv run --with pyyaml --with openapi-spec-validator --with jsonschema python scripts/check-contracts.py
npx markdownlint-cli2
```
