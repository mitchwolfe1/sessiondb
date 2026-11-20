# SessionDB

SessionDB is a database for coding-agent sessions. It automatically syncs sessions from Codex, Claude Code, and Pi, stores their original files, and makes their contents accessible through a common interface.

Use SessionDB to browse conversations and tool calls, export native history, or build applications on session history. It provides a CLI, an HTTP API, and a client SDK. Keyword search and configurable embedding providers are planned.

Sessions contain messages, structural events, and artifacts. Sources record native-store provenance; projects are optional labels. Originals and rewrite generations remain preserved. Sync uploads one file through the API and checkpoints its receipt independently—there is no all-or-nothing multi-file batch or background acceptance job.

[ADR 0015](docs/decisions/0015-strict-native-records-and-simple-reads.md) requires strict whole-file native validation before storage/publication. Only accepted valid originals are preserved; unsupported/malformed records reject with HTTP 400. Reads carry no revision, parser-status, or warning metadata and no snapshot guarantees. Upload write-CAS remains separate; per-file recovery is unchanged.

**Status: initial implementation.** Authenticated sync, original storage, session reads, and native exports work across Codex, Claude Code, and Pi. See [implementation status](docs/implementation.md) for available features and remaining milestones.

Build instructions and a local development setup are in [Build and run SessionDB](docs/development.md).

## Sync and query sessions

Connect to a server, select native session stores, and enable automatic sync. Sessions are collected regardless of their starting directory. Project associations can be added or changed separately.

```sh
# Provision SESSIONDB_API_KEY securely in the server and client environments first.
sessiondb-server --config /etc/sessiondb/config.toml
sessiondb context create shared --server https://sessions.example.com
sessiondb context use shared
printf '%s' "$SESSIONDB_API_KEY" | sessiondb auth login --key-stdin
sessiondb sources add --detect
sessiondb sync install
sessiondb sync status

sessiondb sessions list --harness claude
sessiondb sessions show SESSION_ID
sessiondb sessions messages SESSION_ID --json
sessiondb sessions export SESSION_ID --format native --output session.zip
```

On Linux and macOS, `sync install` starts a user-login service for the selected context. Use `sessiondb sync logs --follow` to see activity and `sessiondb sync uninstall` to turn off background collection. The watcher can use the API key saved with `sessiondb auth login --key-stdin`. [Background sync](docs/sync.md#background-service) covers setup and retries.

## Features

- **Automatic sync:** collect new and updated sessions from multiple harnesses and machines.
- **Session access:** list conversations, read messages and tool activity, inspect relationships, and export sessions.
- **Filtering:** browse sessions by harness, date, or optional project associations. Keyword and semantic search are planned.
- **Native storage:** keep original files alongside a consistent, queryable session model.
- **Optional grouping:** sessions can have zero or more project associations, independently of starting directory and collection source. Project administration is planned.
- **Conventional interfaces:** a scriptable CLI, REST/OpenAPI, a client SDK, PostgreSQL, and S3-compatible object storage.
- **Extensibility:** native adapters and the shared API support additional integrations; embedding-provider support is a later milestone.

Each instance is one shared collection protected by the required `SESSIONDB_API_KEY`. All clients with the key have identical full access; use separate instances for isolated collections. See [project associations](docs/project-associations.md) and [access control](docs/access-control.md).

The initial release targets a single server deployment and Linux/macOS sync clients.

## Documentation

Start with the [documentation guide](docs/README.md), then read:

1. [Product specification](docs/product.md) — capabilities and release scope.
2. [CLI](docs/cli.md) and [automatic sync](docs/sync.md) — collect and work with sessions.
3. [API](docs/api.md), [OpenAPI draft](docs/openapi.yaml), and [data model](docs/data-model.md) — access sessions programmatically.
4. [Search and embeddings](docs/search-and-embeddings.md) — retrieve relevant history.
5. [Harness adapters](docs/harness-adapters.md) and [integrations](docs/integrations.md) — formats and application support.
6. [Architecture](docs/architecture.md), [operations](docs/operations.md), and [access control](docs/access-control.md) — build and run the service.

[The delivery plan](docs/roadmap.md), [design decisions](docs/decisions/README.md), and [open questions](docs/open-questions.md) track implementation planning.

## Development

Development starts with the specification. See [CONTRIBUTING.md](CONTRIBUTING.md) for the documentation workflow and required checks.

The [first-build plan](docs/first-build.md) selects the initial working path. [Reference checks](docs/validation/reference-stack.md) and [synthetic native fixtures](fixtures/README.md) support the storage and message contracts; the earlier OIDC validation is historical and does not verify the instance-key cutover.

Contributions follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), such as `docs: describe session queries`, `feat: add file upload`, or `fix: retry interrupted uploads`.

The repository is private. Distribution and licensing are undecided.
