# SessionDB

SessionDB is a database for coding-agent sessions. It automatically syncs sessions from Codex, Claude Code, and Pi, stores their original files, and makes their contents accessible through a common interface.

Use SessionDB to browse conversations and tool calls, search previous work, or build applications on session history. It provides a CLI, an HTTP API, and a client SDK, with configurable embedding providers for semantic search.

**Status: design phase.** This repository currently contains specifications and documentation; the commands and API describe the planned interface.

## Sync and query sessions

Connect to a server, select native session stores, and enable automatic sync. Sessions are collected regardless of their starting directory. Project associations can be added or changed separately.

```sh
sessiondb-server --config /etc/sessiondb/config.toml
sessiondb auth login --server https://sessions.example.com
sessiondb context create acme --organization acme
sessiondb context use acme
sessiondb sources add --detect
sessiondb sync --watch

sessiondb sessions list --harness claude
sessiondb sessions show SESSION_ID
sessiondb sessions update SESSION_ID --project payments
sessiondb search "payment retries" --project payments
sessiondb sessions export SESSION_ID --format native --output session.zip
```

## Features

- **Automatic sync:** collect new and updated sessions from multiple harnesses and machines.
- **Session access:** list conversations, read messages and tool activity, inspect relationships, and export sessions.
- **Search:** filter by project, harness, author, or date; use keyword search or enable semantic and hybrid retrieval.
- **Native storage:** keep original files alongside a consistent, queryable session model.
- **Optional grouping:** associate a session with zero or more projects, independently of its starting directory and collection source.
- **Conventional interfaces:** a scriptable CLI, REST/OpenAPI, a client SDK, PostgreSQL, and S3-compatible object storage.
- **Extensibility:** add harness adapters, embedding providers, and applications without changing the collection workflow.

SessionDB can also serve a shared collection, with users, teams, and explicit session access. Project labels organize history; sharing settings control visibility. See [project associations](docs/project-associations.md) and [access control](docs/access-control.md).

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

Development starts with the specification. See [CONTRIBUTING.md](CONTRIBUTING.md) for the documentation workflow and planned development checks.

Contributions follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), such as `docs: describe session queries`, `feat: add session import`, or `fix: retry interrupted uploads`.

The repository is private. Distribution and licensing are undecided.
