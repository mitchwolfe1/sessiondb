# Data model

SessionDB stores conversations and their original native files. A **session** contains **messages**, structural **events**, and **artifacts**. Optional **projects** label sessions. A **source** records which client harness store supplied a file; **file generations** preserve native rewrites. **Objects** describe immutable S3 bytes used by originals and artifacts.

Status: accepted and implemented, originating in the immutable [initial migration](../crates/server/migrations/0001_catalog.sql). Athena is deployed; subsequent changes use forward migrations. [ADR 0014](decisions/0014-sessions-and-synchronous-files.md) records the sessions-first design and synchronous file contract.

## Catalog tables

| Table | Purpose |
| --- | --- |
| `sessions` | Conversation identity, source/native key, title, tags, recorded cwd, times, count, and native relations. |
| `messages` | Ordered conversation records: canonical/native role, origin, ancestry, full typed content, provider metadata, and source references. |
| `session_events` | Compaction, context edits, model changes, and other structural native events. |
| `artifacts` | Session-linked image/file/audio/video metadata and an object reference; binary media types belong here. |
| `projects` | Optional labels with unique slug and name, not an access or archive lifecycle. Repository URLs remain a planned API extension. |
| `session_projects` | Unique session/project pairs; the only persisted project association set. |
| `sources` | Harness, device label, native store root, and last sync time; unique by harness/device/root. |
| `source_files` | Unique source-relative paths, current generation and upload version. |
| `file_generations` | Immutable original-object references; a verified append advances the reference in the same generation, while a rewrite starts a new generation. |
| `session_files` | Links conversations to the contributing file generations, including inherited native dependencies. |
| `objects` | Unique S3 key, SHA-256, byte length, and creation time. |

Ordinary metadata uses explicit columns, foreign keys, and uniqueness constraints. JSON is reserved for variable typed content, native relations, usage, source references, and event payloads. There are no duplicate session/message/event `document` blobs. Message previews and content URLs are generated at read time from the one full `messages.content` value. Object hashes and lengths are stored once in `objects`, not repeated in original/artifact rows.

Opaque message content, provider usage, and native event data use PostgreSQL `json`, not `jsonb`: valid native strings can contain escaped NUL characters that JSONB cannot represent. Migration `0003_native_json_payloads.sql` converts existing payloads without scrubbing data. Typed relation/source-reference metadata remains JSONB; native payloads are decoded in the application rather than cast back to JSONB.

Forward migration 0004 adds two derived tables. `search_passages` holds the searchable text and original block location. `search_exchanges` groups passage IDs on one branch and stores the GIN-indexed full-text vector. File acceptance updates both alongside the canonical messages. See [keyword search](search-and-embeddings.md#keyword-search).

Migration 0005 rebuilds oversized keyword passages with integer word-group boundaries. Migration 0006 adds embedding profiles and content digests for exchange segments and their eligible member passages. In a pgvector-enabled catalog, the optional embedding cache stores one vector per profile and digest. A pending input has a retry time and an expiring claim. A ready input has a vector.

## Conversations and native history

A source file can contribute to several sessions; one session can require several native files. Claude subagents can repeat their parent's native session ID, so source/native bindings matter. Proven copies remain explicit and divergent copies are not silently merged. Native roles describe recorded data, not proof of human authorship.

Messages contain typed text, available reasoning, tool calls/results, JSON and artifact references. [Normalized content](normalized-content.md) defines their wire shapes. `conversation_parent_id` describes branching independently of tool/delegation relationships. The default historical view includes alternate branches; `leaf_id` selects ancestry. Compaction/context edits remain events, not invented provider requests. Exact replay and native harness resume are not promised.

Complete Codex AGENTS.md/environment setup envelopes retain their content/native role but have harness origin, not direct-human origin. Default titles prefer recorded native custom/AI titles and then meaningful human text. Display titles normalize control characters to spaces while the original message content remains unchanged. Forward migration `0002_codex_instruction_origins.sql` repairs legacy Codex defaults without replacing manually different titles or altering original content.

## Collection and projects

Sources identify client session stores, not projects or callers. Starting cwd is session metadata: a session launched in `/home/ada` can be unassigned or associated with several repositories. Clients decide when to start or stop collection; the server has no `sources.paused` switch.

`project_ids: []` means unassigned. The join table stores associations, not a duplicate JSON array. Optional initial labels must exist; unavailable labels reject the request before storage. Append sync preserves existing title, tags, and projects. Project administration, matching rules, and metadata PATCH APIs remain planned; see [project associations](project-associations.md).

## Original files and acceptance

One authenticated [file upload](sync.md) verifies actual length/hash and fully parses before registering/storing immutable bytes, then commits file/catalog state before HTTP 200. Native transcripts are JSONL, so there is no per-transcript MIME column. Artifacts retain their actual MIME types.

Every supplied native record must be supported and valid. A malformed or unsupported record rejects the whole file with HTTP 400 before any object registration, storage, or catalog publication. Only accepted valid originals are retained; previous accepted state remains unchanged on rejection.

Byte-changing uploads receive a new version. Verified prefix growth keeps the generation ID; rewrites preserve prior generations. Source references address original uncompressed line/byte ranges. Old append objects become unreferenced only after publication of the replacement; preserved rewrites stay protected.

## Storage and access

Every data surface uses the one instance API key; sources and projects are provenance/classification, not access partitions. All correct-key clients share the collection.

PostgreSQL transactions publish one file's accepted state. S3/network I/O happens outside them. Objects use fresh never-reused keys and are recorded before upload so crashes leave tracked orphans rather than published missing bytes. Only old unreferenced objects are eligible for periodic cleanup; object lifecycle/status columns, import tables, and job queues are unnecessary.

There is no atomic multi-file acceptance. Clients save pending snapshots and checkpoint each successful receipt separately. Local file disappearance never deletes server history. Session deletion and suppression remain planned and need their own complete contract; the current schema does not pretend to implement them.
