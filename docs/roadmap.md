# Delivery plan

Delivery proceeds through reviewable vertical slices. The current repository is phase zero: specifications and documentation, without application code.

## Phase zero

Review session workflows, the Rust workspace, sync semantics, query/search interfaces, and server configuration. Resolve the relevant [open questions](open-questions.md) and review the [OpenAPI draft](openapi.yaml), with request/response examples checked against its schemas.

The result is an implementation plan for the first sync-to-query path.

## First vertical slice

Create the Rust workspace and a local server with PostgreSQL and S3-compatible storage. Import one native session, list it, read its messages, and export its original data through the API and CLI. Include the connection/authentication setup and project access needed to exercise that path.

Prove the API client boundary and import acceptance semantics before adding more retrieval capabilities. Readiness, migrations, durable jobs, and a basic restore drill are part of this slice.

## Automatic collection and all harnesses

Add Codex, Claude Code, and Pi adapters, initial backfill, continuous sync, local state recovery, supporting artifacts, and Linux/macOS service installation. Verify branch/subagent/context-event handling and visible partial-parse states. Add team grants and full administration for the documented v1 role model.

## Session search

Add PostgreSQL keyword indexing, authorized filtering, session-diverse results, pagination, source expansion, and changes consumption. Connect hype-history through the public API. Validate that a browser-style consumer can use the same contract without direct database access.

## Optional semantic search

Add the provider/profile interface and one configured endpoint implementation, background backfill, pgvector exact retrieval, hybrid ranking, and coverage/status reporting. Prove disabled/unavailable-provider behavior, compatible dimensions, and access filtering before optimizing ANN indexes.

## V1 release

Complete CLI packaging, Rust SDK documentation, installation and upgrade procedures, native/JSONL/STS exports, backup/restore, resource limits, and the tested compatibility matrix. Run the acceptance scenarios below and publish remaining limitations. No hosted billing or browser UI is required for this release.

## Acceptance scenarios

| ID | Scenario | Required result |
| --- | --- | --- |
| A01 | Sync Codex, Claude, and Pi sessions into SessionDB | All three are discoverable and readable through the CLI and API. |
| A02 | Another team lacks that project's grant | No session, snippet, count, artifact, job result, export, or vector result reveals its content. |
| A03 | Repeated upload and lost completion response | Retrying returns the accepted result without duplicate visible sessions/messages. |
| A04 | Server/network outage and client restart | Local originals remain intact; pending work catches up from persisted state. |
| A05 | Active transcript has an unfinished final line | The complete prefix is accepted; the tail appears after completion on a later sync. |
| A06 | An existing file grows, then is rewritten | Appends advance history; a rewrite becomes a new generation with earlier original data preserved. |
| A07 | Local file disappears or moves to a native archive | Remote history remains available; identical copies do not become duplicate evidence silently. |
| A08 | Unsupported record or missing inherited parent | Original is retrievable and partial support is visible; parser does not invent content. |
| A09 | Claude subagent repeats parent session ID | Child remains distinguishable; tool/result links and usage are scoped correctly. |
| A10 | Pi branches/context edits and Codex compaction/rollback | Historical branches/events are accessible and no exact replay is falsely claimed. |
| A11 | No embedding provider or provider outage | Core sync/read and keyword search work; explicit semantic requests report their capability/dependency error. |
| A12 | Embedding model/profile changes | Old and new vector spaces are never compared; backfill progress and coverage are visible. |
| A13 | Grant revoked or membership removed | Subsequent reads/search stop; preissued download URLs follow their documented maximum lifetime. |
| A14 | Maintainer deletes a session and client syncs again | Session stays removed through source suppression; local harness file is not deleted. |
| A15 | Backup restored into a clean deployment | Memberships, grants, original reads/exports, and collection recovery work; derived indexes can be rebuilt. |
| A16 | API result is consumed by CLI, Rust SDK, and hype-history | Same permissions, filters, error semantics, and session meanings apply. |
| A17 | Several accounts contribute to a shared project | Project members can find and read one another's sessions according to their grants. |

## Later work

Consider a browser application, hosted organization provisioning, SSO/SCIM administration conveniences, additional harnesses, ATIF/OTel/Agent Trace integrations, richer analytics, and native resume integrations after the core service is useful. Each addition gets a requirement, contract, and acceptance scenario before implementation.
