# Delivery plan

Delivery proceeds through reviewable vertical slices. Phase zero and the first storage-to-CLI implementation are complete; [implementation status](implementation.md) tracks the current boundary.

## Instance-key cutover

[ADR 0013](decisions/0013-instance-api-key.md) defines the instance-wide key boundary across the server, types, SDK, CLI, and docs. The abandoned identity model was replaced before deployment; Athena now has accepted data. Preserve its initial migration and validate forward upgrades as well as fresh catalog initialization before recording new results.

## Sessions-first synchronous cutover

[ADR 0014](decisions/0014-sessions-and-synchronous-files.md) removes batch/job state and accepts one file through the API before replying. Each successful file checkpoints independently; multi-file atomicity is deliberately not promised. Preserve the initial catalog and add forward migrations for deployed repairs. Integrated checks must verify per-file retries/rejections, retained native behavior, and upgrades without losing accepted history.

## Phase zero

Review session workflows, the workspace layout, sync semantics, query/search interfaces, and server configuration. Use the [resolved first-build decisions](open-questions.md), [selected scope](first-build.md), and [OpenAPI draft](openapi.yaml), with request/response examples checked against its schemas.

The [first-build plan](first-build.md) now defines that implementation path, backed by reference-stack checks and native fixtures.

## First vertical slice

Create the Cargo workspace and a local server with PostgreSQL and S3-compatible storage. Import one native session without a project label, list it, read its messages, and export its original data through the API and CLI. Include required instance-key authentication for that path.

Prove synchronous per-file acceptance before more retrieval features. Readiness, fresh migrations, pending-snapshot recovery, and a restore drill belong to this slice; no durable job queue is required.

## Automatic collection and all harnesses

Add Codex, Claude Code, and Pi adapters, initial backfill, continuous sync, local state recovery, supporting artifacts, and Linux/macOS service installation. Verify branch/subagent/context-event handling and strict rejection before publication.

## Session search

The first search milestone implements two operations agents can call at the point of need. See the [keyword retrieval contract](search-and-embeddings.md#keyword-search).

1. **Search history.** Implement `POST /v1/search` through the Rust SDK and `sessiondb search`. Rank exchanges across the eligible collection. Index the full eligible text of a user request and its same-branch assistant response, retaining anchors into the original messages. Index standalone messages independently and split oversized content at message or block boundaries. Start with PostgreSQL full-text search and the project, harness, role and date filters in the draft contract. Keep tools identifiable and explicitly selectable.
2. **Open a hit with context.** Start the agent view with up to three short excerpts and a cursor for more. Each excerpt identifies the matching message, speaker, time when available and source link. Use `sessiondb messages show MESSAGE_ID -C 2` to open the exchange and neighboring turns, selecting a branch when the continuation forks. Preserve existing single-message reads and full-content expansion.

The first implementation PR should cover the contract, a forward migration, backfill of accepted messages and one working CLI-to-API search and expansion. Verify that a newly accepted append becomes searchable and that retries and rewrites keep the derived index consistent with canonical reads. The agent chooses when to look up history.

## Optional semantic search

The native hybrid slice adds an explicitly configured embedding endpoint, restartable background backfill and pgvector exact similarity search. Reuse vectors for unchanged exchange text within the same embedding profile. Hybrid search combines keyword and vector ranks, then selects a short source passage inside each hit. Newly accepted text participates through keyword search while embedding catches up. Expose actual retrieval mode and coverage through the existing search proposal.

Run the unrelated-session browser task through the native API with history off, keyword search and hybrid search. Record task success, whether the verified instruction was retrieved before browser access, returned tokens and latency. Use a fixed corpus and matched browser state across conditions.

## Broad questions about past work

Deliver cross-session research through SessionDB’s own agent tools. For questions about recurring preferences or practices, gather examples and exceptions from independent sessions, projects and dates. Open the original decisions and later corrections so the calling agent can give a compact synthesis with citations and identify which conclusions are inferred.

For requests about frequency or the whole archive, add a client-side batched scan of a declared project and time range using paginated session and message reads. Track the unit being counted, duplicates and how much of the selected corpus was examined. Use a frozen corpus when a reproducible count is required. This is a later delivery slice.

## Replace hype-history

Move the remaining history workflows, saved references and reviewed memories to SessionDB. Switch harness registrations and workspace instructions after validating the native workflows. The [migration plan](integrations.md#hype-history-migration) defines when the legacy tool and index can be retired.

## V1 release

Complete CLI packaging, client SDK documentation, installation and upgrade procedures, native/JSONL/STS exports, backup/restore, resource limits, and the tested compatibility matrix. Run the acceptance scenarios below and publish remaining limitations. No hosted billing or browser UI is required for this release.

## Acceptance scenarios

| ID | Scenario | Required result |
| --- | --- | --- |
| A01 | Sync Codex, Claude, and Pi sessions into SessionDB | All three are discoverable and readable through the CLI and API. |
| A02 | Two clients use the same instance key | Identical full access to content, counts, artifacts, exports, and planned retrieval regardless of labels. |
| A03 | Repeated snapshot and lost upload response | Identical retry returns current receipt with unchanged=true, including a preceding version token; no duplicate messages. |
| A04 | Server/network outage and client restart | Local originals remain intact; pending work catches up from persisted state. |
| A05 | Active transcript has an unfinished final line | The complete prefix is accepted; the tail appears after completion on a later sync. |
| A06 | An existing file grows, then is rewritten | Appends advance history; a rewrite becomes a new generation with earlier original data preserved. |
| A07 | Local file disappears or moves to a native archive | Remote history remains available; identical copies do not become duplicate evidence silently. |
| A08 | Unsupported record or missing inherited parent | Unsupported records reject the whole file before storage; missing inherited parents remain explicit structural links without invented content. |
| A09 | Claude subagent repeats parent session ID | Child remains distinguishable; tool/result links and usage are scoped correctly. |
| A10 | Pi branches/context edits and Codex compaction/rollback | Historical branches/events are accessible and no exact replay is falsely claimed. |
| A11 | No embedding provider or provider outage | Core sync/read and keyword search work; explicit semantic requests report their capability/dependency error. |
| A12 | Embedding model/profile changes | Old and new vector spaces are never compared; backfill progress and coverage are visible. |
| A13 | Instance key rotated | Subsequent old-key API requests fail; updated clients regain full access; no signed object URLs are issued. |
| A14 | A key holder deletes a session and client syncs again | Session stays removed through source suppression; local harness file is not deleted. |
| A15 | Backup restored into a clean deployment | Catalog, original reads/exports, and collection recovery work; derived indexes can be rebuilt. |
| A16 | API result is consumed by the CLI, SDK and SessionDB agent tools | Same key boundary, filters, error semantics, and session meanings apply. |
| A17 | Request has no key or a wrong key | Every data API and `/v1/info` rejects it; public health/readiness/static contract stay available. |
| A18 | A session starts in a home directory and has no matching rule | It syncs unassigned, remains readable, and participates in keyword and configured semantic/hybrid search. |
| A19 | Assign two projects, replace them, then clear them | Only catalog associations and filter results change; native files and readers stay the same. |
| A20 | A manually labeled session receives another native append | Content updates while project labels remain unchanged. |
| A21 | A session moves between project labels during change-feed consumption | Authorized consumers receive the reclassification, including previous/current associations, without duplicate session records. |
| A22 | Second file fails after first succeeds | First remains accepted/checkpointed; failed file remains pending, with no atomic batch guarantee. |
| A23 | Unsupported original or metadata/hash mismatch | Unsupported native records and invalid upload bytes reject the whole file before storage and leave prior accepted state unchanged. |
| A24 | The only verified workflow is a side exchange in a session on another topic | Archive-wide search surfaces the original exchange and the agent uses it to complete a fresh task. |
| A25 | A broad code-review question has examples favoring simplification and an accepted exception | The agent cites independent original decisions, identifies their scope, and applies both to a held-out review containing a redundant wrapper and a necessary retry. |
| A26 | A survey asks how often a decision occurred in a fixed corpus containing duplicate records | The reported count and denominator match the fixture, and the client reports coverage. |
| A27 | A fresh agent runs with only SessionDB available after migration | Search, anchored reads, timelines, on-demand briefings and cross-session research work. Migrated memories retain their review status and resolve to the original evidence. |

## Later work

Consider a browser application, additional harnesses, ATIF/OTel/Agent Trace integrations, richer analytics, and native resume integrations after the core service is useful. Each addition gets a requirement, contract, and acceptance scenario before implementation.
