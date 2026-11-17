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
| `sessions` | `list`, `show`, `messages`, `events`, `update`, `export`, `delete` |
| `search` | Query with a keyword, semantic, or hybrid mode and filters |
| `sources` | `list`, `add`, `show`, `pause`, `resume` |
| `sync` | One pass; `--watch`; `status`, `install`, `uninstall` |
| `embeddings` | `profiles list`, `profiles create`, `profiles show`, `profiles enable`, `profiles disable`, `backfill` |
| `jobs` | `show`, `wait` |
| `context` | `create`, `list`, `show`, `use`, `remove` |
| `projects` | `list`, `create`, `show`, `update`, `archive` |
| `auth` | `login --key-stdin`, `logout`, `status` (OS credential-store conveniences only) |
| Top level | `status`, `config show`, `completion`, `version` |

Session and search commands cover everyday use. Connection and project commands configure server URLs and classification. Release sequencing is described in the [delivery plan](roadmap.md).

## Everyday use

```sh
sessiondb sessions list --project payments --harness codex --limit 20
sessiondb sessions list --unassigned
sessiondb sessions show SESSION_ID
sessiondb sessions messages SESSION_ID --leaf MESSAGE_ID --json
sessiondb search "retry migration" --project payments --after 2026-11-01
sessiondb search "previous retry designs" --project payments --mode hybrid
sessiondb sessions update SESSION_ID --title "Payment retry investigation" --tag retries
sessiondb sessions update SESSION_ID --project payments --project api-client
sessiondb sessions update SESSION_ID --clear-projects
sessiondb sessions export SESSION_ID --format native --output session.zip
sessiondb embeddings backfill --project payments --profile PROFILE_ID
sessiondb jobs wait JOB_ID
```

`sessions show` presents a readable overview and bounded preview. Message/content/export commands retrieve more data. On `sessions update`, repeated `--tag` and `--project` options supply replacement sets; omission preserves a set, and `--clear-tags` or `--clear-projects` clears it. `sessions delete` explains server deletion and source suppression; interactive deletion requires confirmation unless `--yes` is supplied.

For example, `sessiondb sessions update SESSION_ID --project payments` can label a session whose recorded cwd is `/home/ada`. Its file stays in the harness store, subsequent sync keeps updating it, and its readers remain unchanged. See [project associations](project-associations.md).

## Familiar behavior

Human output is a stable, readable table or transcript. `--json` writes structured API-shaped JSON to stdout. Export writes bytes to `--output` or stdout with `--output -`; diagnostics and progress stay on stderr. Disable color when redirected and honor `NO_COLOR`. Do not interleave progress with machine-readable output.

Support `--help`, `--version`, generated shell completions, `--server`, `--context`, `--config`, `--timeout`, and quiet/verbose modes consistently. Server-side filters behave the same through the CLI and SDK.

Proposed exit codes are `0` for success, `1` for an operational/server failure, `2` for invalid usage, `3` for authentication/authorization failure, and `4` for a requested resource not found. An interrupted foreground watch exits conventionally with the signal-derived status. `jobs wait` returns failure for a failed job; partial results are explained and require explicit caller handling.

## Configuration

Use TOML configuration in the platform configuration directory (`$XDG_CONFIG_HOME/sessiondb/config.toml` on Linux). Resolve settings in order: explicit flags, environment variables, selected context, file defaults, built-in defaults. Environment variables include `SESSIONDB_URL`, `SESSIONDB_API_KEY`, and `SESSIONDB_CONFIG`.

`config show` reports effective nonsecret values and their origin. `SESSIONDB_API_KEY` takes precedence over the OS credential store; an explicitly empty value fails rather than falling back. Keys never belong in the ordinary configuration file or command-line flags. `auth login --key-stdin` requires that flag, reads a nonempty key from stdin, validates protected `/v1/info`, and saves it per normalized server URL in the OS keyring (`sessiondb-api-key`). There is no browser or token exchange. `auth logout` deletes only the local stored copy; `auth status` validates `/v1/info` and reports the server/authentication status without revealing the key. Keep sync state in the platform state/data directory and make `status` report its path.

See the [client configuration example](examples/client.toml). It is illustrative and uses no real credentials. During upgrade, remove obsolete `organization` and `default_visibility` configuration fields; they are rejected, not ignored. The SQLite state migration preserves source registrations, IDs, checkpoints, and pending snapshots while removing organization scope. Do not delete recovery state merely to reconnect with the new key.

## Rust client crate

`sessiondb-client` exposes a reusable async client constructed with `Client::new(server: &str, api_key: String, timeout: Duration)`. It requires a nonempty static key; there is no auth module, token provider, refresh, or organization argument. Resource methods take global resource IDs with no organization UUID. Provide typed methods for resources, page iteration, import upload/completion, job polling, and downloads. Errors distinguish transport failures, HTTP problem details, and decoding/compatibility errors.

Reuse HTTP connections and bounded streaming for file transfer. Retry only safe reads and operations with explicit idempotency semantics. Do not hide server validation or authorization failures behind unlimited retries. Signed upload targets receive only their provided headers; never forward a SessionDB API key to an object-store host. The CLI supplies human presentation and local collection behavior; the SDK supplies the shared network contract.

Follow Rust API naming and documentation conventions. Public types live in `sessiondb-types`; server database structs are private. Additional language SDKs are derived from the same OpenAPI contract after the Rust client is proven. hype-history may use such a client or ordinary HTTP before a packaged Python SDK exists.
