# SessionDB

Engineers are doing more of their day-to-day work inside coding-agent harnesses. A busy team can generate hundreds of sessions a day, often returning to the same projects and workflows. However, agents tend to make little use of past work, and often end up having to do more work rediscovering information.

Teams use `AGENTS.md` to give agents build commands, coding conventions, and instructions for recurring tasks. As those workflows change, someone has to keep correcting the file. Until they do, every new session inherits the old instructions. Agents can also write down what they learn in files like `MEMORY.md` for preferences and `PAPERCUTS.md` for recurring problems and fixes. Knowledge graphs can connect those notes, and retrieval-augmented generation (RAG) can bring relevant passages into a later session. Deciding what to remember takes judgment and taste. An agent writing notes has to choose which details will matter before it knows what the next task will require. Today’s agents still struggle to make that choice reliably, and every later search of those notes is limited by what they kept. Tuning a memory system to capture finer detail still leaves the agent guessing what will matter later. It burns more tokens writing the notes and contributes to context rot when those notes bring irrelevant details into a future session.

The harness already keeps that history in its local session files. The original request, the corrections you made, the commands that failed, and the output that confirmed the fix are there in sequence. SessionDB syncs sessions from Codex, Claude Code, and Pi into a shared, searchable database. When an agent has a question, it can find the relevant exchange in its own or a teammate’s past work and bring just the evidence it needs into context.

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
