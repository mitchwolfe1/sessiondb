# Integrations and formats

Applications use SessionDB to collect, read, and search coding-agent history. Its APIs support session browsers, briefing and memory tools, analytics, and evaluation pipelines.

Status: agreed integration direction with proposed export and migration contracts.

## First consumers

| Consumer | Uses SessionDB for | Owns itself |
| --- | --- | --- |
| CLI and SDK | Collection, browsing, import/export, query access | Local presentation and collection service installation |
| SessionDB agent integrations (planned) | Search, original exchanges and reviewed memory | On-demand briefings, timelines and cross-session evidence gathering |
| Session browser | Projects, session lists, messages, tools, artifacts | Navigation and UI |
| Analytics application | Recorded usage and session activity | Reports and interpretation of estimated cost |
| Evaluation pipeline | Selected trajectories and exports | Tasks, graders, environment outcomes, experiment results |

Existing products such as Hugging Face Agent Traces, ccusage, Entire, and observability tools provide useful interoperability targets and examples of these workflows.

## Original and normalized formats

Original native files remain authoritative for what was collected. The normalized model is a versioned, rebuildable application view. Parser changes can improve it without asking every engineer to re-upload unchanged originals.

Retain supported provider-specific events in their documented native event representation rather than forcing every event into one shape. Unsupported native records reject the whole file before storage; exports contain only accepted valid originals.

| Export or integration | Role |
| --- | --- |
| Native bundle | ZIP with original session files, artifacts, and a manifest of included/missing relationships. V1. |
| SessionDB JSONL | Header plus typed normalized messages/events with a schema version. V1. |
| Hugging Face STS | Viewer-oriented header/messages/tool links; selected conversation branch when requested. V1 export. |
| Harbor ATIF | Rich trajectory interchange for evaluation/datasets; design after an exporter validates branching/subagent mappings. Later. |
| OpenTelemetry/OpenInference | Execution spans where operations/timing are actually recorded; observability export. Later. |
| Agent Trace | Code ranges/revisions linked to conversation URLs. Later. |

Native bundle export includes source-relative paths and a manifest instead of rewriting references inside archived JSONL. It identifies absent linked files explicitly; all available dependencies use the same instance-wide access boundary. A bundle is an archival export, not a promise that a native harness can resume it on another machine.

STS and ATIF are projections over selected conversation history. Define which branch and which structural events were included. Incomplete timing remains unknown; the exporter cannot reconstruct unrecorded operations. Agent Trace code attribution is a different schema from an execution trace or transcript.

## On-demand agent lookup

The [SessionDB history skill](../skills/sessiondb-history/SKILL.md) gives shell-capable agents the search and source-reading workflow. Install the skill directory in a harness's discovered skill location. Codex and Pi can share `~/.agents/skills/sessiondb-history`. Claude Code can load the same directory through `~/.claude/skills/sessiondb-history`. A lookup uses the CLI connection already configured for that user. Skill discovery itself issues no history request.

Preference and cross-session follow-ups use the same CLI. Search for competing decisions, open independent original exchanges and cite the message URLs. Keep preserved migration utilities outside the active harness tool list. If a source or scope cannot be resolved in SessionDB, report the gap.

## hype-history migration

[ADR 0022](decisions/0022-replace-hype-history.md) makes SessionDB the successor to the entire history tool. The proposed cutover has three steps.

1. Deliver native SessionDB search, branch-aware reads, timelines, on-demand briefings, cross-session research and reviewed-memory access. Exercise the workflows used by Codex, Claude Code and Pi, including requests for tool evidence and later corrections.
2. Map legacy session and message references to SessionDB resources and check the original content and branch. Legacy message IDs incorporate absolute source paths. Migrate memory records with their scope, review status, relationships and source citations. Report unresolved references for reconciliation. Map repository labels to optional project associations.
3. Compare results against the legacy baseline, switch agent tool registrations and workspace instructions to SessionDB, and run the acceptance suite with the legacy tools and index offline. After verification, retire the legacy indexing and embedding schedules. SessionDB becomes the sole running history system.

## SDK and API consumers

The first native SDK is `sessiondb-client` for Rust. OpenAPI provides the contract for other languages. SessionDB’s agent integrations and external language bindings use the same HTTP contract.

Consumers use server-issued IDs and optional project filters. A headless application gets the same full-access key, not read-only/source-limited access. Use a separate instance for isolation. Raw uploads/downloads use the API; saved citations use durable SessionDB resource URLs.

## Extensibility boundaries

Harness adapters, object storage, and planned embedding providers have focused Rust interfaces. Keep SQL models and ingestion internals private. Add exporters/consumers through the public API/normalized model and retain the instance-key boundary.
