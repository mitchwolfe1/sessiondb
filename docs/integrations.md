# Integrations and formats

Applications use SessionDB to collect, read, and search coding-agent history. Its APIs support session browsers, briefing and memory tools, analytics, and evaluation pipelines.

Status: agreed integration direction with proposed export and migration contracts.

## First consumers

| Consumer | Uses SessionDB for | Owns itself |
| --- | --- | --- |
| CLI and Rust SDK | Collection, browsing, import/export, query access | Local presentation and collection service installation |
| hype-history | Accessible sessions/messages and search | Research packets, briefings, preference analysis, reviewed memory |
| Session browser | Projects, session lists, messages, tools, artifacts | Navigation and UI |
| Analytics application | Recorded usage and session activity | Reports and interpretation of estimated cost |
| Evaluation pipeline | Selected trajectories and exports | Tasks, graders, environment outcomes, experiment results |

Existing products such as Hugging Face Agent Traces, ccusage, Entire, and observability tools provide useful interoperability targets and examples of these workflows.

## Original and normalized formats

Original native files remain authoritative for what was collected. The normalized model is a versioned, rebuildable application view. Parser changes can improve it without asking every engineer to re-upload unchanged originals.

Do not turn an external viewer/export format into a requirement that every provider-specific event must be discarded or forced into the same shape. Retain unsupported native records and expose completeness warnings where a projection is partial.

| Export or integration | Role |
| --- | --- |
| Native bundle | ZIP with original session files, authorized artifacts, and a manifest of included/missing relationships. V1. |
| SessionDB JSONL | Header plus typed normalized messages/events with a schema version. V1. |
| Hugging Face STS | Viewer-oriented header/messages/tool links; selected conversation branch when requested. V1 export. |
| Harbor ATIF | Rich trajectory interchange for evaluation/datasets; design after an exporter validates branching/subagent mappings. Later. |
| OpenTelemetry/OpenInference | Execution spans where operations/timing are actually recorded; observability export. Later. |
| Agent Trace | Code ranges/revisions linked to conversation URLs. Later. |

Native bundle export includes source-relative paths and a manifest instead of rewriting references inside archived JSONL. It identifies absent or inaccessible linked files without leaking another project's paths or content. A bundle is an archival export, not a promise that a native harness can resume it on another machine.

STS and ATIF are projections over selected conversation history. Define which branch and which structural events were included. Incomplete timing remains unknown; the exporter cannot reconstruct unrecorded operations. Agent Trace code attribution is a different schema from an execution trace or transcript.

## hype-history migration

Reuse the prototype's requirements and test cases for incremental capture, tool links, branch endings, search diversity, and source-backed reads. Port appropriate behavior into Rust adapters rather than introducing a Python runtime dependency into SessionDB.

Keep hype-history-specific memory review and authorship judgments outside the server. A SessionDB role or uploader identifies a recorded source/contributor, not automatic proof that every user-role entry is a personal statement.

The existing prototype's message IDs incorporate absolute source paths. Its client integration needs a migration map from existing session/message references to server resources before local data is retired. Verify content and relationships and preserve existing reviewed-memory state; moving to remote storage is not a reason to reapprove evidence automatically.

Use public HTTP/SDK methods, not direct PostgreSQL access. Start by migrating read/search access in parallel with the local implementation, compare selected results and branch endings, and then switch collection. Keep a documented fallback while the pilot is being evaluated.

## SDK and API consumers

The first native SDK is `sessiondb-client` for Rust. OpenAPI provides the contract for other languages; add a generated Python binding or a small HTTP integration when hype-history needs it. A Python application SDK does not change the Rust implementation requirement.

Consumers use organization/project filters and server-issued resource IDs. A headless application can use a project-scoped `sessions:read` token; a sync-only source token does not implicitly authorize search. Raw downloads use authorized API endpoints or short-lived URLs. Saved citations use durable SessionDB resource URLs rather than expiring object URLs.

## Extensibility boundaries

Harness adapters, object storage access, and embedding providers are explicit Rust interfaces. Keep internal SQL models and job internals private. Add exporters and consumers through the API/normalized model when possible. Every new integration must retain the same organization/project access boundaries.
