# SessionDB

SessionDB is a self-hosted session database for engineering organizations. It automatically collects coding-agent sessions across engineers and machines, presents Codex, Claude Code, and Pi through a consistent API, and supports shared search with optional embeddings.

**Status: documentation and contract design.** This repository contains the proposed product and technical specification. There is no server, client, Cargo workspace, or runnable deployment yet. Command examples describe the intended interface.

SessionDB will be a **Rust project**. Its primary interfaces are a `sessiondb` CLI, a `sessiondb-server` service, an HTTP API, and a Rust client SDK. Applications such as hype-history can build research, briefing, and memory workflows on those interfaces.

## Product experience

An organization operates a server and creates shared projects. Engineers connect the client, select which workspaces to collect, and run automatic sync. Authorized colleagues can find sessions by project, engineer, harness, date, or content and inspect conversations and tool activity.

```sh
# Proposed commands, not available software.
sessiondb-server --config /etc/sessiondb/config.toml
sessiondb auth login --server https://sessions.example.com
sessiondb context use acme
sessiondb sources add --project payments --workspace ~/work/payments --detect
sessiondb sync --watch

sessiondb sessions list --project payments --harness claude
sessiondb sessions show SESSION_ID
sessiondb search "payment retries" --project payments
sessiondb sessions export SESSION_ID --format native --output session.zip
```

## Core capabilities

- Automatic collection from Codex, Claude Code, and Pi, including later updates to existing sessions.
- Organizations, users, teams, and project access from the initial product model.
- Original session files alongside a consistent representation of messages, tool activity, and session relationships.
- Keyword search, filtering, and optional semantic or hybrid search through configurable embedding providers.
- Conventional CLI behavior, a versioned REST API described with OpenAPI, and a reusable Rust SDK.
- PostgreSQL for application data and indexes; an existing S3-compatible service for original files and artifacts.

The initial release targets a single server deployment and Linux/macOS sync clients. A hosted SaaS service, billing, a browser application, and native harness resume are outside the first release.

## Specification

Start with the [documentation guide](docs/README.md), then read:

1. [Product specification](docs/product.md) — customers, requirements, scope, and acceptance criteria.
2. [Architecture](docs/architecture.md) — Rust components and deployment boundaries.
3. [Data model](docs/data-model.md) and [access control](docs/access-control.md) — shared sessions across engineers and teams.
4. [Sync protocol](docs/sync.md), [API contract](docs/api.md), [OpenAPI draft](docs/openapi.yaml), and [CLI contract](docs/cli.md).
5. [Search and embeddings](docs/search-and-embeddings.md) and [harness adapters](docs/harness-adapters.md).
6. [Operations](docs/operations.md), [integrations](docs/integrations.md), and [delivery plan](docs/roadmap.md).

[Design decisions](docs/decisions/README.md) distinguish agreed direction from proposed implementation choices. [Open questions](docs/open-questions.md) identify decisions to resolve before their corresponding implementation work begins.

## Development

Documentation comes first. Review the relevant specification and resolve its blocking questions before implementing a feature. Application code and Cargo scaffolding are intentionally absent during this phase.

Contributions follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), such as `docs: clarify project permissions`, `feat: add session import`, or `fix: retry interrupted uploads`. See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md).

The repository is private. Distribution and licensing are undecided; no open-source license is granted by these documents.
