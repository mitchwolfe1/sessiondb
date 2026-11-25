# CLI and client SDK

The `sessiondb` executable syncs, lists, reads, searches, and exports sessions. `sessiondb-server` runs the service. The CLI uses the same client SDK available to applications.

The first implementation follows [the frozen build slice](first-build.md); later command groups use the same target-v1 contract.

Status: target-v1 interface, with the initial command subset implemented. See [implementation status](implementation.md) and `sessiondb --help` for available commands; [development](development.md) provides a runnable setup.

## First use

```sh
sessiondb connect https://sessions.example.com
sessiondb sources add --detect
sessiondb sync --watch
```

`connect SERVER_URL` prompts for the API key on first use, checks it against `/v1/info`, and saves the server URL and verified key. Subsequent commands and the watcher use that connection. Use `sessiondb status` to check access. `sources add --detect` registers native store roots. Use `--harness` and `--root` to select one explicitly. See [automatic sync](sync.md).

`sessiondb sync install` installs and starts a login service using systemd on Linux or launchd on macOS. It pins the server URL, absolute executable, config and state paths, and request timeout. It picks up source and sync settings from that config on each pass. Installing again refreshes and restarts the service. See [background sync](sync.md#background-service) for credentials, logs and lifecycle.

## Set up a teammate

From a checkout of the same SessionDB release, install the CLI with `cargo install --locked --path crates/cli`. Each teammate connects with the same instance API key and a distinct member handle. Use the same handle on that person's other computers. Handles accept 1–64 ASCII letters, digits, hyphens and underscores, starting with a letter or digit.

```sh
sessiondb connect https://sessions.example.com --member ada
sessiondb sources add --detect
sessiondb sync install
sessiondb config show --json
```

The client stores the handle per server in `[members]`. Each native store has a device label, generated on first use or selected with `sources add --device-label LABEL`. During sync, the handle applies to all sources configured for that server on this client, including their earlier sessions. Check the source list before configuring a handle on a machine that also collected someone else's work. To label an offline source, use `sessiondb sources set-member SOURCE_ID ada`. To correct a label, use `sessiondb sources set-member SOURCE_ID ada --from prev-handle` and update the collecting client's configuration to match.

All key holders can read the whole collection, including native exports. To give agents the lookup workflow, install the [history skill](integrations.md#on-demand-agent-lookup) on each teammate's machine.

## Command groups

| Group | Commands |
| --- | --- |
| `sessions` | `list`, `show`, `messages`, `export`; events/update/delete are planned |
| `search` | Keyword, semantic and hybrid excerpts with filters and pagination |
| `messages` | `show` with optional context, `content` |
| `sources` | `list`, `add`, `show`, `set-member`; collection starts/stops on the client |
| `sync` | One pass, `--watch`, `install`, `start`, `stop`, `uninstall`, `status`, `logs [--follow]`, `rejected`, `retry` |
| `embeddings` | `create FILE`, `list`, `show ID`, `enable ID`, `disable ID` |
| `connect SERVER_URL` | Verify and save the server URL and API key |
| `projects` | Planned `list`, `create`, `show`, `update`; no archive lifecycle |
| Top level | `status`, `config show`, `version` |

Session and search commands cover everyday use. Connection and project commands configure server URLs and classification. Release sequencing is described in the [delivery plan](roadmap.md).

## Everyday use

Implemented examples:

```sh
sessiondb sessions list --harness codex --limit 20
sessiondb sessions list --unassigned
sessiondb search --mode hybrid -- "production RabbitMQ dead-letter queue"
sessiondb search --mode hybrid --member mark -- "RabbitMQ backlog"
sessiondb search --mode hybrid --local -- "connect to my running browser"
sessiondb sessions list --mine
sessiondb search "payment retries" --harness claude
sessiondb search "temporary AND failure" --tool-name test_runner --json
sessiondb messages show MESSAGE_ID
sessiondb messages show MESSAGE_ID -C 2
sessiondb messages show MESSAGE_ID -B 3 -A 5 --leaf BRANCH_MESSAGE_ID
sessiondb sessions show SESSION_ID
sessiondb sessions messages SESSION_ID --leaf MESSAGE_ID --json
sessiondb sessions export SESSION_ID --format native --output session.zip
sessiondb sync status --json
```

Search and session lists default to the whole collection. `--member HANDLE` filters by collecting member, `--device LABEL` by exact device, `--mine` by the member configured for the selected server, and `--local` by that member and this device together. If a required local setting is missing, the command reports it. `config show --json` includes the current member and device label.

Search returns three excerpts by default. Use `--limit` and `--cursor` to read more. Use `--include-tools` to include tool input and output. Use `--mode hybrid` with an enabled profile to combine term and vector evidence. Title, tag and project updates and session deletion are planned. See `sessiondb --help` and [implementation status](implementation.md).

Every search hit includes a `show_command` that opens the matching message with `-C 2` on the same connection. The command includes `--leaf` when a descendant identifies the continuation.

`messages show ID` returns the single message. `-C N` (`--context N`) adds up to N messages on each side. `-B N` (`--before N`) and `-A N` (`--after N`) set the two sides independently. A single directional flag defaults the other side to zero. When combined with `-C`, a directional flag overrides its side. Each count accepts 0–50. Passing `--leaf` alone opens two messages of context on each side.

In context output, `>` marks the selected message. With `--json`, a context request returns `anchor_id`, ordered `data`, and `branch_choices`. Use `messages content ID` for full content.

Planned metadata updates replace supplied label/tag sets while preserving omitted fields. Ordinary append sync preserves existing catalog metadata.

## Familiar behavior

Human output is a stable, readable table or transcript. `--json` writes structured API-shaped JSON to stdout. Export writes bytes to `--output` or stdout with `--output -`; diagnostics and progress stay on stderr. Disable color when redirected and honor `NO_COLOR`. Do not interleave progress with machine-readable output.

Commands can be piped into tools such as `head`. An otherwise successful command exits quietly with code `0` when the reader closes stdout early.

Support `--help`, `--version`, `--server`, `--config`, `--timeout`, and quiet/verbose modes consistently. Server-side filters behave the same through the CLI and SDK.

Exit codes distinguish success (`0`), operational failure (`1`), usage (`2`), authentication (`3`), and missing resources (`4`). A sync pass with rejected native files reports their paths and reasons and finishes other uploads. In `sync --json`, affected source reports include a `rejections` object mapping source-relative paths to reasons. One-shot sync exits `1` after reporting these rejections. Interrupted watch exits with the signal-derived status.

A server `internal_error` on a file upload appears in the source report under `retryable_errors`. The snapshot remains pending while the client processes other files. A one-shot pass with retryable errors also exits `1`.

## Configuration

Use TOML configuration in the platform configuration directory (`$XDG_CONFIG_HOME/sessiondb/config.toml` on Linux). Resolve settings in order: explicit flags, environment variables, file defaults, built-in defaults. Environment variables include `SESSIONDB_URL`, `SESSIONDB_API_KEY`, and `SESSIONDB_CONFIG`.

The client saves verified keys in the `[api_keys]` table of its TOML file, indexed by normalized HTTP or HTTPS origin. On Linux the default is `~/.config/sessiondb/config.toml`. Unix config writes are atomic and use owner-only permissions (`0600`). `config show` displays a configured key as `[redacted]`.

For automation, `SESSIONDB_API_KEY` overrides the saved key. An explicitly empty environment key fails authentication. `connect --key-stdin SERVER_URL` takes the key from standard input and takes precedence over the environment. Only a successful `connect` saves the supplied key. For rotation, run `connect` again: a rejected saved key prompts for a replacement in the terminal. To forget a saved key, remove that origin’s entry from `[api_keys]`.

Each configured source stores its destination URL. `--server URL` and `SESSIONDB_URL` select a destination for an invocation. `connect URL` uses its positional URL even with an override present. Switching the default keeps existing roots assigned to their registered servers. See the [connection upgrade notes](decisions/0020-connect-by-url.md) for older configs and watchers.

See the [client configuration example](examples/client.toml). It contains no credentials or unsupported future settings. Preserve existing registrations, pending files, and checkpoints across upgrades; do not reset deployed client state to apply a server fix. Recovery persists pending per-file snapshots and advances checkpoints individually after HTTP 200; `sync status --json` reports `pending_files`.

The status object contains `state_path`, `sources`, `accepted_files`, nullable `last_accepted_at`, and `pending_files`. It also reports `rejected_files`, the last completed watch pass with its server URL, timestamp and error, and the native service name, definition path and state. `sync rejected --json` lists remembered paths and reasons for the selected server. `sync retry` clears those rejection hashes in this state directory so the next watch pass tests the files again. One-shot sync always retries them. Local SQLite stores registrations by server/device/harness/root, pending snapshots by source/path, and accepted per-file checkpoints. Only `[sync] reconcile_interval_seconds` and `upload_concurrency` are current sync settings; debounce/batch settings and future matching rules are not runnable configuration.

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

For retrieval, use `Client::search(&SearchRequest)` and `Client::read_around(message_id, &AroundOptions)`. Both methods return typed responses with the same filters, branch choices and source IDs as the CLI.

The upload method sends the metadata JSON and raw JSONL as an authenticated multipart API request. The caller provides a fresh reader when retrying; the SDK does not automatically replay a non-replayable stream. Every successful response represents one accepted file, only after every supplied native record is supported and valid. No import, upload-target, completion, or wait-job methods remain.

Reuse HTTP connections and bounded transfer. Errors distinguish transport, HTTP problem details, and decoding failures. Never send the API key to an object-store host: clients only need the API URL. The CLI owns local capture/recovery and presentation; the SDK owns the network contract.

Follow Rust API naming and documentation conventions. Public types live in `sessiondb-types`; server database structs are private. Additional language SDKs are derived from the same OpenAPI contract after the Rust client is proven.
