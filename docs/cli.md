# CLI and Rust SDK

The `sessiondb` executable syncs, lists, reads, searches, and exports sessions. `sessiondb-server` runs the service. The CLI uses the same Rust HTTP client crate available to applications.

Status: proposed interface. These commands are examples of the intended contract and are not executable in the current repository.

## First use

```sh
sessiondb auth login --server https://sessions.example.com
sessiondb context create acme --organization acme
sessiondb context use acme
sessiondb sources add --detect
sessiondb sync --watch
```

Login authenticates the account; a context selects a server and organization. `--detect` registers selected native store roots for supported harnesses. Use `--harness` and `--root` for an explicit store, or configuration for unattended setup. Starting cwd and project membership do not determine whether a session can be collected. See [automatic sync](sync.md).

`sessiondb sync install` installs the current context and selected source roots as a user-level service. It uses systemd on Linux and launchd on macOS. `sessiondb sync uninstall` removes that service without deleting local harness files or remote history.

## Command groups

| Group | Commands |
| --- | --- |
| `sessions` | `list`, `show`, `messages`, `events`, `update`, `export`, `delete`, `grants list`, `grants add`, `grants remove` |
| `search` | Query with a keyword, semantic, or hybrid mode and filters |
| `sources` | `list`, `add`, `show`, `pause`, `resume` |
| `sync` | One pass; `--watch`; `status`, `install`, `uninstall` |
| `embeddings` | `profiles list`, `profiles create`, `profiles show`, `profiles enable`, `profiles disable`, `backfill` |
| `jobs` | `show`, `wait` |
| `context` | `create`, `list`, `show`, `use`, `remove` |
| `projects` | `list`, `create`, `show`, `update`, `archive` |
| `auth` | `login`, `logout`, `status`, `token create`, `token list`, `token revoke` |
| `organizations` | `list` |
| `members` | `list`, `invite`, `update`, `remove` |
| `teams` | `list`, `create`, `show`, `update`, `members list`, `members add`, `members remove` |
| Top level | `status`, `config show`, `completion`, `version` |

Session and search commands cover everyday use. Connection, project, and account commands configure the server and shared access. Release sequencing is described in the [delivery plan](roadmap.md).

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
sessiondb auth token create --scope sessions:read --expires-in 7d
```

`sessions show` presents a readable overview and bounded preview. Message/content/export commands retrieve more data. On `sessions update`, repeated `--tag` and `--project` options supply replacement sets; omission preserves a set, and `--clear-tags` or `--clear-projects` clears it. `--visibility private|organization` is an explicit owner/admin action separate from labels. Session grants can share with a user/team as viewer or editor. `sessions delete` explains server deletion and source suppression; interactive deletion requires confirmation unless `--yes` is supplied.

For example, `sessiondb sessions update SESSION_ID --project payments` can label a session whose recorded cwd is `/home/ada`. Its file stays in the harness store, subsequent sync keeps updating it, and its readers remain unchanged. See [project associations](project-associations.md).

## Familiar behavior

Human output is a stable, readable table or transcript. `--json` writes structured API-shaped JSON to stdout. Export writes bytes to `--output` or stdout with `--output -`; diagnostics and progress stay on stderr. Disable color when redirected and honor `NO_COLOR`. Do not interleave progress with machine-readable output.

Support `--help`, `--version`, generated shell completions, `--server`, `--organization`, `--context`, `--config`, `--timeout`, and quiet/verbose modes consistently. Server-side filters behave the same through the CLI and SDK.

Proposed exit codes are `0` for success, `1` for an operational/server failure, `2` for invalid usage, `3` for authentication/authorization failure, and `4` for a requested resource not found. An interrupted foreground watch exits conventionally with the signal-derived status. `jobs wait` returns failure for a failed job; partial results are explained and require explicit caller handling.

## Configuration

Use TOML configuration in the platform configuration directory (`$XDG_CONFIG_HOME/sessiondb/config.toml` on Linux). Resolve settings in order: explicit flags, environment variables, selected context, file defaults, built-in defaults. Environment variables include `SESSIONDB_URL`, `SESSIONDB_ORGANIZATION`, `SESSIONDB_TOKEN`, and `SESSIONDB_CONFIG`.

`config show` reports effective nonsecret values and their origin. Tokens and refresh credentials belong in the OS credential store or explicit secret environment, not the ordinary configuration file. Keep sync state in the platform state/data directory and make `status` report its path.

See the [client configuration example](examples/client.toml). It is illustrative and uses no real credentials.

## Rust client crate

`sessiondb-client` exposes a reusable async client configured with URL, credentials, timeout, and default organization. Provide typed methods for resources, page iteration, import upload/completion, job polling, and downloads. Errors distinguish transport failures, HTTP problem details, and decoding/compatibility errors.

Reuse HTTP connections and bounded streaming for file transfer. Retry only safe reads and operations with explicit idempotency semantics. Do not hide server validation or authorization failures behind unlimited retries. Signed upload targets receive only their provided headers; never forward a SessionDB bearer token to an object-store host. The CLI supplies human presentation and local collection behavior; the SDK supplies the shared network contract.

Follow Rust API naming and documentation conventions. Public types live in `sessiondb-types`; server database structs are private. Additional language SDKs are derived from the same OpenAPI contract after the Rust client is proven. hype-history may use such a client or ordinary HTTP before a packaged Python SDK exists.
