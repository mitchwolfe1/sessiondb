# Product specification

SessionDB is a database for coding-agent sessions. It collects sessions from Codex, Claude Code, and Pi, stores their native files, and exposes a consistent model for reading and querying their contents.

A sync client collects new and updated sessions automatically. A server provides session access, keyword search, and optional semantic search through the CLI, HTTP API, and SDK. Applications can use those interfaces to build session browsers, memory tools, analytics, and evaluation workflows.

Status: product direction with proposed v1 behavior. See the [delivery plan](roadmap.md) for implementation milestones.

## Core capabilities

**Collect sessions.** Discover supported harness stores, select sources, backfill existing history, and continue syncing as files change regardless of starting cwd. Collection works across multiple machines and recovers after disconnection.

**Read and query sessions.** List and filter sessions, read messages and tool activity, inspect relationships, update titles/tags and optional project associations, and export original or normalized data.

**Search history.** Search message content by keyword and filter by project, harness, or date. Configure an embedding provider to enable semantic or hybrid retrieval.

**Build on a common interface.** The CLI and SDK use a versioned HTTP API. Harness adapters normalize native differences while retaining the original data for export and future parsing improvements.

**Run the service where it is needed.** The initial deployment uses a server, PostgreSQL, and an S3-compatible object store. One required instance-wide API key gives every correct-key client identical full access to the shared collection.

## Main workflow

1. Connect the client to a configured SessionDB server.
2. Select native session stores and run an initial sync or continuous collection.
3. Continue using Codex, Claude Code, or Pi normally. New sessions and updates appear in SessionDB.
4. List sessions, inspect a conversation, or search previous work through the CLI or API.
5. Optionally associate sessions with projects, export them, or retrieve messages from an application.
6. Configure an embedding profile when semantic search is useful.

The first end-to-end demonstration is a session moving from a native file into SessionDB and becoming readable through both the CLI and API. Subsequent milestones cover all three harnesses, continuous sync, search, shared access, and application integration.

## V1 scope

| ID | Capability | Expected behavior |
| --- | --- | --- |
| P02 | Codex, Claude Code, and Pi collection | Import supported native record shapes, including tool calls/results and later appends. |
| P03 | Automatic sync | Backfill and watch mode recover after restarts or outages without duplicate sessions. |
| P04 | Session access | List, filter, inspect messages and tool activity, edit title/tags, and export sessions. |
| P13 | Optional project associations | Collect home-directory and unassigned sessions; add, replace, or clear multiple project labels without changing instance access. |
| P05 | Keyword search | Search sessions and messages with project, harness, and date filters. |
| P06 | Optional embeddings | Planned semantic/hybrid search with explicit profile and visible coverage; no index-job workflow API. |
| P07 | Conventional interfaces | Versioned REST/OpenAPI, predictable pagination/errors, CLI scripting, and a client SDK. |
| P08 | Native files | Retrieve collected originals even when normalization is incomplete. |
| P10 | Harness adapters | Add another harness through the same discovery/normalization boundary. |
| P12 | Application integration | Applications such as hype-history consume sessions and search through the public API. |
| P09 | Server deployment | Document installation, configuration, migrations, health, backup, and restore. |
| P01 | Shared access | One shared API key protects all data; all key holders have identical full access, independently of project associations. |
| P11 | Administration | Rotate the instance key, configure storage/processing, and delete sessions; there is no account or per-source credential administration. |

See [access control](access-control.md) for shared-server access settings.

## Design approach

Preserve original session files and make the parsed model rebuildable. Native branches, compaction, and context edits remain available even when a normalized view cannot reproduce an exact harness request.

Use familiar interfaces and components: standard HTTP, PostgreSQL, S3-compatible storage, conventional CLI behavior, and operating-system service managers. Extend the service through focused adapter/provider interfaces.

Keep embeddings optional. Session storage, sync, reading, and keyword search remain useful without a model endpoint. Expose collection and indexing state so a pending upload and embedding backlog are distinguishable.

## Initial release boundaries

V1 targets a self-hosted server on Linux and sync clients on Linux/macOS. It includes optional project grouping and instance-key authentication; deployment and credential setup are covered by the server guides.

A browser application, hosted billing, native harness resume, write-back into local transcripts, deterministic execution replay, and model training are later work. Existing applications can add specialized briefing, memory, analytics, or evaluation behavior through the API.

## Evaluation

Evaluate the complete sync-to-query workflow: setup effort, collection freshness, recovery, time to useful search results, and ease of building another API client. Benchmark query and indexing performance on a representative corpus before setting numerical capacity targets.

The [acceptance scenarios](roadmap.md#acceptance-scenarios) cover native format handling, synchronization, retrieval, exports, shared access, and recovery.
