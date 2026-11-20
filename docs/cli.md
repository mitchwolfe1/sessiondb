# CLI and client SDK

The `sessiondb` executable syncs, lists, reads, searches, and exports sessions. `sessiondb-server` runs the service. The CLI uses the same client SDK available to applications.

The first implementation follows [the frozen build slice](first-build.md); later command groups use the same target-v1 contract.

Status: target-v1 interface, with the initial command subset implemented. See [implementation status](implementation.md) and `sessiondb --help` for available commands; [development](development.md) provides a runnable setup.

## First use

```sh
# Supply SESSIONDB_API_KEY securely in the environment.
sessiondb context create shared --server https://sessions.example.com
sessiondb context use shared
sessiondb sources add --detect
sessiondb sync --watch
```

A context selects only a server URL. `SESSIONDB_API_KEY` authenticates every client operation; all key holders have identical full access. `--detect` registers selected native store roots for supported harnesses. Use `--harness` and `--root` for an explicit store, or configuration for unattended setup. Starting cwd and project classification do not determine whether a session can be collected. See [automatic sync](sync.md).

Planned `sessiondb sync install` installs the current context and selected source roots as a user-level service. It will use systemd on Linux and launchd on macOS. Planned `sessiondb sync uninstall` removes that service without deleting local harness files or remote history. The current `sync --watch` runs in the foreground using periodic reconciliation.

## Command groups

| Group | Commands |
| --- | --- |
| `sessions` | `list`, `show`, `messages`, `export`; events/update/delete are planned |
| `search` | Planned keyword, semantic, and hybrid queries |
| `sources` | `list`, `add`, `show`; collection starts/stops on the client |
| `sync` | One pass; `--watch`; `status`; install/uninstall are planned |
| `embeddings` | Planned profile management; backfill execution contract is not yet defined |
| `context` | `create`, `list`, `show`, `use`, `remove` |
| `projects` | Planned `list`, `create`, `show`, `update`; no archive lifecycle |
| `auth` | `login --key-stdin`, `logout`, `status` (OS credential-store conveniences only) |
| Top level | `status`, `config show`, `completion`, `version` |

Session and search commands cover everyday use. Connection and project commands configure server URLs and classification. Release sequencing is described in the [delivery plan](roadmap.md).

## Everyday use

Implemented examples:

```sh
sessiondb sessions list --harness codex --limit 20
sessiondb sessions list --unassigned
sessiondb sessions show SESSION_ID
sessiondb sessions messages SESSION_ID --leaf MESSAGE_ID --json
sessiondb sessions export SESSION_ID --format native --output session.zip
sessiondb sync status --json
```

Search, title/tag/project updates, session deletion, and embedding profile commands are planned, not available examples. See `sessiondb --help` and [implementation status](implementation.md).

Session reads present bounded content; full-content and native export endpoints retrieve more data. Planned metadata updates replace supplied label/tag sets while preserving omitted fields. Ordinary append sync preserves existing catalog metadata.

## Familiar behavior

Human output is a stable, readable table or transcript. `--json` writes structured API-shaped JSON to stdout. Export writes bytes to `--output` or stdout with `--output -`; diagnostics and progress stay on stderr. Disable color when redirected and honor `NO_COLOR`. Do not interleave progress with machine-readable output.

Support `--help`, `--version`, generated shell completions, `--server`, `--context`, `--config`, `--timeout`, and quiet/verbose modes consistently. Server-side filters behave the same through the CLI and SDK.

Exit codes distinguish success (`0`), operational failure (`1`), usage (`2`), authentication (`3`), and missing resources (`4`). Interrupted watch exits with the signal-derived status. There are no jobs or wait commands; each successful file response is its acceptance receipt.

## Configuration

Use TOML configuration in the platform configuration directory (`$XDG_CONFIG_HOME/sessiondb/config.toml` on Linux). Resolve settings in order: explicit flags, environment variables, selected context, file defaults, built-in defaults. Environment variables include `SESSIONDB_URL`, `SESSIONDB_API_KEY`, and `SESSIONDB_CONFIG`.

`config show` reports effective nonsecret values and their origin. `SESSIONDB_API_KEY` takes precedence over the OS credential store; an explicitly empty value fails rather than falling back. Keys never belong in the ordinary configuration file or command-line flags. `auth login --key-stdin` requires that flag, reads a nonempty key from stdin, validates protected `/v1/info`, and saves it per normalized server URL in the OS keyring (`sessiondb-api-key`). There is no browser or token exchange. `auth logout` deletes only the local stored copy; `auth status` validates `/v1/info` and reports the server/authentication status without revealing the key. Keep sync state in the platform state/data directory and make `status` report its path.

See the [client configuration example](examples/client.toml). It contains no credentials or unsupported future settings. Preserve existing registrations, pending files, and checkpoints across upgrades; do not reset deployed client state to apply a server fix. Recovery persists pending per-file snapshots and advances checkpoints individually after HTTP 200; `sync status --json` reports `pending_files`.

The status object contains `state_path`, `sources`, `accepted_files`, nullable `last_accepted_at`, and `pending_files`. Local SQLite stores registrations by server/device/harness/root, pending snapshots by source/path, and accepted per-file checkpoints. Only `[sync] reconcile_interval_seconds` and `upload_concurrency` are current sync settings; debounce/batch settings and future matching rules are not runnable configuration.

## Rust client crate

`sessiondb-client` exposes `Client::new(server: &str, api_key: String, timeout: Duration)`, resource reads, pagination, originals/artifacts, native export, and:

```rust,ignore
pub async fn upload_file<R: AsyncRead + Send + Sync + 'static>(
    &self,
    source: Uuid,
    metadata: &FileUpload,
    reader: R,
) -> Result<FileUploadResult>
```

It sends the metadata JSON and raw JSONL as an authenticated multipart API request. The caller provides a fresh reader when retrying; the SDK does not automatically replay a non-replayable stream. Every successful response represents one accepted file, only after every supplied native record is supported and valid. No import, upload-target, completion, or wait-job methods remain.

Reuse HTTP connections and bounded transfer. Errors distinguish transport, HTTP problem details, and decoding failures. Never send the API key to an object-store host: clients only need the API URL. The CLI owns local capture/recovery and presentation; the SDK owns the network contract.

Follow Rust API naming and documentation conventions. Public types live in `sessiondb-types`; server database structs are private. Additional language SDKs are derived from the same OpenAPI contract after the Rust client is proven. hype-history may use such a client or ordinary HTTP before a packaged Python SDK exists.
