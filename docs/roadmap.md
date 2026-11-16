# Delivery plan

Delivery proceeds through reviewable vertical slices. The current repository is phase zero: specifications and documentation, without application code.

## Phase zero

Review product scope, team permissions, Rust workspace, sync semantics, CLI behavior, and retrieval behavior. Resolve the blocking questions in [the decision queue](open-questions.md). Validate and accept the [OpenAPI draft](openapi.yaml) before implementing routes, with request/response examples checked against its schemas.

The phase-zero exit is an explicitly reviewed implementation plan. Creating documentation does not itself authorize an agent to start application development.

## First vertical slice

Create the Rust workspace and one local deployment with PostgreSQL, S3-compatible storage, and the selected identity integration. Support two engineers, one organization/project, contributor/viewer permissions, one harness import, session listing/reading, and raw export through both API and CLI.

Prove the API client boundary and import acceptance semantics before adding more retrieval capabilities. Readiness, migrations, durable jobs, and a basic restore drill are part of this slice.

## Automatic collection and all harnesses

Add Codex, Claude Code, and Pi adapters, initial backfill, continuous sync, local state recovery, supporting artifacts, and Linux/macOS service installation. Verify branch/subagent/context-event handling and visible partial-parse states. Add team grants and full administration for the documented v1 role model.

## Shared search

Add PostgreSQL keyword indexing, authorized filtering, session-diverse results, pagination, source expansion, and changes consumption. Connect hype-history through the public API. Validate that a browser-style consumer can use the same contract without direct database access.

## Optional semantic search

Add the provider/profile interface and one configured endpoint implementation, background backfill, pgvector exact retrieval, hybrid ranking, and coverage/status reporting. Prove disabled/unavailable-provider behavior, compatible dimensions, and access filtering before optimizing ANN indexes.

## V1 release

Complete CLI packaging, Rust SDK documentation, installation and upgrade procedures, native/JSONL/STS exports, backup/restore, resource limits, and the tested compatibility matrix. Run the acceptance scenarios below and publish remaining limitations. No hosted billing or browser UI is required for this release.

## Acceptance scenarios

| ID | Scenario | Required result |
| --- | --- | --- |
| A01 | Two engineers contribute Codex, Claude, and Pi work into one project | An authorized colleague finds and reads all three through CLI/API. |
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

## Later work

Consider a browser application, hosted organization provisioning, SSO/SCIM administration conveniences, additional harnesses, ATIF/OTel/Agent Trace integrations, richer analytics, and native resume integrations after the core service is useful. Each addition gets a requirement, contract, and acceptance scenario before implementation.
