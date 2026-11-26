# SessionDB

SessionDB is a database that gives coding agents a shared, searchable memory of past work.

Frontier models work within a finite context window. What an agent can recall about your work depends on what its harness saves and brings back into that window. When a team’s history is spread across sessions, tools, and machines, agents can repeat investigations and people end up explaining decisions they’ve already made.

Session summaries help an agent pick up a long-running task. A `MEMORY.md` file records lessons and preferences or points to more detailed notes. Markdown knowledge graphs link notes so an agent can trace the connections between a project and its decisions. In each case, a person or an agent chooses what to capture before the next question is known. See [Claude Code’s memory files](https://code.claude.com/docs/en/memory#auto-memory) and [Basic Memory’s linked notes](https://docs.basicmemory.com/concepts/observations-and-relations).

SessionDB automatically syncs the original sessions from Codex, Claude Code, and Pi. With the history skill installed, an agent can search across your team’s conversations for a detail that only became important later, read the surrounding exchange, and cite the message or tool result behind its answer.

## Search past work

Keyword search is the default. An enabled embedding profile adds semantic search, which can find an exchange even when you describe the problem in different words. Hybrid search combines the keyword and semantic rankings. See [search and embedding setup](docs/search-and-embeddings.md#configure-semantic-and-hybrid-search).

```sh
sessiondb search --open-first -- "webhook retry"
sessiondb search --mode hybrid --open-first -- "Why did one payment create two invoices?"
```

Each result includes an excerpt and a link to the original message. `--open-first` shows the surrounding messages on the first match’s recorded conversation branch. Add `--include-tools` to search tool calls and results, such as the test output that confirmed a fix.

Search covers the whole synced collection by default. Use `--member ada` for sessions collected under one teammate’s handle, `--mine` for your sessions across devices, or `--local` for your history on this machine. For a worked example, see [how an agent used workstation history to find a browser connection](dev/evals/browser/README.md#native-sessiondb-pilot).

## Sync and query sessions

Install the CLI from [GitHub Releases](https://github.com/mitchwolfe1/sessiondb/releases) or follow the [source installation instructions](docs/cli.md#set-up-a-teammate). From the unpacked client archive or checkout, install the history skill for detected agents and connect to your server. Use your own member handle in place of `ada`.

```sh
./scripts/install-harnesses
sessiondb connect https://sessions.example.com --member ada
sessiondb sources add --detect
sessiondb sync install
sessiondb sync status
```

`connect` prompts for the instance API key. On Linux and macOS, the background service syncs new and updated sessions from the selected stores regardless of their starting directory. It also keeps the CLI and agent integration up to date. Start a fresh agent session to use the skill. See [background sync](docs/sync.md#background-service) for logs and service management.

You can also browse conversations, read their messages, and export the original files directly.

```sh
sessiondb sessions list --harness claude
sessiondb sessions show SESSION_ID
sessiondb sessions messages SESSION_ID --json
sessiondb sessions export SESSION_ID --format native --output session.zip
```

## Features

- **Native history.** SessionDB preserves original files and their earlier versions alongside searchable messages, tool activity, branches, and artifacts. See [native file handling](docs/decisions/0015-strict-native-records-and-simple-reads.md).
- **Filters.** Narrow a search by project, harness, date, or message role. Sessions can belong to multiple projects independently of where they started. See [project associations](docs/project-associations.md).
- **Application access.** Scripts and applications can search, read messages in context, and export native sessions through the same HTTP API that powers the CLI and client SDK. See the [API reference](docs/api.md).

Each instance has one shared collection. Every client with the instance API key has full access. See [access control](docs/access-control.md).

The initial release targets a single server with PostgreSQL and S3-compatible object storage, with sync clients for Linux and macOS. See [implementation status](docs/implementation.md) for available features and remaining milestones.

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

Maintainers [package and publish client releases manually](docs/development.md#release-the-client). Licensing remains undecided.
