# Harness adapters

Adapters give native Codex, Claude Code, and Pi sessions a consistent SessionDB representation while preserving original files. They are implemented in `sessiondb-adapters` and do not require a running harness, model credentials, or database access.

Status: implemented bounded adapter contract. Native format support must be verified against versioned synthetic fixtures; new producer kinds are not silently accepted as unknown data.

## Adapter responsibilities

Each adapter can discover default/configured roots, inspect recorded cwd and repository metadata, select supporting artifacts, normalize a bounded native record stream, and report native session relationships. Discovery is usable by the CLI; normalization runs on the server before acceptance or storage.

Outputs include session metadata, messages/content blocks, tool calls/results, structural events, and relationships. Keep source-relative references for artifacts. Reject the entire file on unsupported or malformed native records before any storage or publication.

The adapter reports observed metadata without making project assignment a collection requirement. It must not perform HTTP requests to native content URLs, execute recorded commands, or apply hype-history's application-specific memory filters.

## Native support matrix

| Harness | Primary transcript locations | Important behavior |
| --- | --- | --- |
| Codex | `CODEX_HOME/sessions`, `CODEX_HOME/archived_sessions` | Typed rollout records; model items and UI events can overlap; newer forks inherit another rollout prefix. |
| Claude Code | Configured home or `~/.claude/projects` | Parent-linked records, per-session subagent files, tool results in user-role blocks, external tool outputs. |
| Pi | Configured session directory or `~/.pi/agent/sessions` | Versioned header, entry tree, system prompt updates, compaction, context edits, and extension entries. |

Paths are defaults, not a requirement that data lives under a particular username or OS home. Honor provider configuration and explicit overrides. Windows collection is a later platform target; do not mistake path encoding for a portable source ID.

## Codex

Read metadata from `session_meta`, per-turn settings from `turn_context`, model-facing messages/tools from `response_item`, and relevant lifecycle/structural events from `event_msg` and other typed records. Avoid returning a second visible message merely because the same action is also projected as a completed UI item.

Resolve call/results by native call IDs where available. Preserve opaque encrypted reasoning/replay fields as original data, without presenting them as readable hidden reasoning. Keep model/phase information when recorded.

Known web-search items normalize recorded actions as tool calls without fabricating IDs or results. Tool-definition items, world-state snapshots, and token-usage records remain complete structural events. Complete injected AGENTS.md and environment-context messages retain native content and roles but use harness origin, so they do not supply default human-request titles.

Web-search records support searches, page opens, and in-page finds. A completion record with no action remains an event. Reasoning configuration updates and agent-to-agent messages and their metadata remain structural events, preserving the recorded author, recipient, and content. Base64 data images in messages and tool results produce ordered artifact references with the recorded media type.

Newer metadata can distinguish thread ID, root session ID, fork ancestry, and `history_base` references to a bounded parent rollout. Resolve references through retained source bindings; preserve unresolved references when a parent is missing. Rollback and compaction are recorded events, not permission to erase original source bytes.

Codex's local SQLite history projections and prompt-recall file are not substitutes for the native rollout. V1 collection uses rollout files and explicitly selected artifacts rather than synchronizing the provider's live SQLite databases.

## Claude Code

Do not require a fixed first-line header. Session metadata can appear on repeated records. Distinguish transcript record UUIDs from provider response IDs; several assistant rows can share one model response and repeated usage data. Usage aggregation must count the recorded response correctly rather than summing every repeated block row.

Tool calls are assistant `tool_use` blocks; results can be `tool_result` blocks in user messages. Preserve meta/summary markers and distinguish them from a direct human input. Include parent-linked non-message nodes needed to connect conversation branches.

Mode/permission changes, native attachments, and AI/custom titles are structural events with their complete native fields. Attachment nodes remain part of ancestor traversal; recognizing them does not authorize following arbitrary paths or fetching external content.

Session-owner, cost, bridge, tool-usage, ATIS, file-history-delta, and API request/shape/blob metadata are retained as structural events. A tool result can contain a `tool_reference` block naming a tool. Its JSON retains the name, order, and recorded extension fields.

Subagent files may share their parent's session ID; agent identity and source file scope keep them separate. Nested workflow directories and `fork-context-ref` records must not merge a child into its parent. Resolve external tool outputs only through the adapter's artifact allowlist.

## Pi

Read the session header and retain its format version and optional parent session reference. Entries link through `id` and `parentId`; the header is not an entry node. Preserve typed system/user/assistant/tool-result messages, model/thinking changes, and extension metadata.

`custom` extension state is not a message. `custom_message` contributes recorded context. `compaction`, `branch_summary`, and `context_edit` alter model-context interpretation without removing the older recorded history. Keep them as structural events and distinguish conversation ancestry from operation nesting.

Entry timestamps are ISO strings; nested message timestamps use epoch milliseconds. Preserve the distinction when normalizing times. Native image blocks can be extracted into instance-key-protected artifacts while originals remain intact.

## Compatibility and fixtures

Record harness name, native client version when available, format version, and adapter version. Publish a tested compatibility matrix at release time instead of claiming all future versions work. Missing fields have explicit unknown states; unknown native types reject the file.

Create synthetic fixtures for ordinary conversations, tool calls, repeated usage, branches, compaction, inherited context, subagents, artifacts, malformed records, and incomplete tails. Keep expected normalized results small enough for review. Tests must verify source references and the complete user-visible session behavior, including explicit unresolved links for missing relationships and rejection of unsupported records.

The existing hype-history prototype is a design reference for incremental reading and branch-aware evidence. Its Python implementation is not copied into a Rust server, and its narrower text projection is not treated as a complete native replay model.
