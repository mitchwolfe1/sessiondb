# SessionDB

SessionDB is a database for coding-agent sessions. It automatically syncs sessions from Codex, Claude Code, and Pi, stores their original files, and makes their contents accessible through a common interface.

Use SessionDB to browse conversations and tool calls, export native history, or build applications on session history. It provides a CLI, an HTTP API, and a client SDK. Search by keyword and open the matching exchange on its recorded branch.

Sessions contain messages, structural events, and artifacts. Sources record native-store provenance; projects are optional labels. Originals and rewrite generations remain preserved. Sync uploads one file through the API and checkpoints its receipt independently—there is no all-or-nothing multi-file batch or background acceptance job.

[ADR 0015](docs/decisions/0015-strict-native-records-and-simple-reads.md) requires strict whole-file native validation before storage/publication. Only accepted valid originals are preserved; unsupported/malformed records reject with HTTP 400. Reads carry no revision, parser-status, or warning metadata and no snapshot guarantees. Upload write-CAS remains separate; per-file recovery is unchanged.

**Status: initial implementation.** Authenticated sync, original storage, session reads, and native exports work across Codex, Claude Code, and Pi. See [implementation status](docs/implementation.md) for available features and remaining milestones.

Build instructions and a local development setup are in [Build and run SessionDB](docs/development.md).

## Sync and query sessions

Connect to a server, select native session stores, and enable automatic sync. Sessions are collected regardless of their starting directory. Project associations can be added or changed separately.

```sh
sessiondb connect https://sessions.example.com
sessiondb sources add --detect
sessiondb sync install
sessiondb sync status

sessiondb sessions list --harness claude
sessiondb search "payment retries"
sessiondb messages show MESSAGE_ID -C 2
sessiondb sessions show SESSION_ID
sessiondb sessions messages SESSION_ID --json
sessiondb sessions export SESSION_ID --format native --output session.zip
```

`connect` prompts for the instance API key and saves it for that server. On Linux and macOS, `sync install` starts a login service that reads the same client config. Use `sessiondb sync logs --follow` to see activity and `sessiondb sync uninstall` to turn off background collection. [Background sync](docs/sync.md#background-service) covers setup and retries.

Run `./scripts/install-harnesses` from the checkout to add the history skill to detected installations of Codex, Claude Code and Pi. For a shared team archive, [connect each teammate with a member handle](docs/cli.md#set-up-a-teammate). Search everyone’s work by default, use `--member mark` for a particular person or `--local` for procedures tied to this workstation.

## Features

- **Automatic sync:** collect new and updated sessions from multiple harnesses and machines.
- **Session access:** list conversations, read messages and tool activity, inspect relationships, and export sessions.
- **Filtering:** browse sessions by harness, date, or optional project associations. Search exchange text with the same filters and expand the matching original.
- **Native storage:** keep original files alongside a consistent, queryable session model.
- **Optional grouping:** sessions can have zero or more project associations, independently of starting directory and collection source. Project administration is planned.
- **Conventional interfaces:** a scriptable CLI, REST/OpenAPI, a client SDK, PostgreSQL, and S3-compatible object storage.
- **Extensibility:** enable an embedding profile for exact semantic and hybrid search alongside keywords.

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
