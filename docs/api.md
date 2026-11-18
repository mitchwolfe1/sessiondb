# HTTP API contract

The SessionDB API provides sessions, messages, original files, artifacts, and planned search resources for the CLI, SDK, and applications.

Status: target-v1 resource and behavior contract. The [OpenAPI draft](openapi.yaml) defines wire schemas and operations; `x-implemented` identifies the current server subset. The instance-key revision is `0.3.0-draft`, with [typed content](normalized-content.md) and the [first-build subset](first-build.md). See [implementation status](implementation.md) before using later target-v1 routes.

## Conventions

- Base namespace `/v1`; JSON requests and responses use `snake_case` fields.
- Resource IDs are opaque UUIDs. Native harness IDs and source paths remain separate string attributes. CLI slugs resolve through list/detail APIs.
- One required instance-wide API key in `Authorization: Bearer <key>`. Every `/v1` API, including `/v1/info`, is protected. Health, readiness, and static `/openapi.yaml` are public.
- Timestamps are RFC 3339 with UTC output. `after` is inclusive and `before` is exclusive; message search uses the matching message's timestamp, with an explicit session-time fallback when absent.
- List responses use `data` and nullable `next_cursor`. Default `limit=50`, maximum `200`. Cursors are opaque and bound to filters and ordering.
- Default session order is `updated_at` descending with ID as a deterministic tie-breaker. Messages use recorded sequence with ID tie-breaking, not arbitrary timestamp sorting.
- `POST` creates a resource or uploads a file, `GET` reads, and planned `PATCH`/`DELETE` operations change documented metadata or delete data. Unknown request fields are rejected; clients ignore unknown response fields and preserve unknown content block types.
- Request IDs appear in responses and logs. Use HTTP statuses consistently and errors in `application/problem+json` following RFC 9457.

## Resource inventory

Relative resource paths below use the prefix `/v1`. Service paths are shown in full. All correct-key clients have identical full access.

### Sessions and search

| Method | Path | Meaning |
| --- | --- | --- |
| GET | `/sessions` | Filter and paginate accessible sessions. |
| GET, PATCH, DELETE | `/sessions/{session_id}` | Read, change title/tags/project associations, or schedule deletion. |
| GET | `/sessions/{session_id}/messages` | Read historical messages, optionally filtered by conversation leaf. |
| GET | `/sessions/{session_id}/events` | Read recorded structural events and native event metadata. |
| GET | `/sessions/{session_id}/export` | Stream a native bundle or normalized JSONL/STS projection. |
| GET | `/messages/{message_id}` | Read a bounded normalized message. |
| GET | `/messages/{message_id}/content` | Read full typed content, without conditional-read metadata. |
| GET | `/artifacts/{artifact_id}` | Read authorized artifact metadata. |
| GET | `/artifacts/{artifact_id}/content` | Download artifact bytes with the instance API key. |
| POST | `/search` | Keyword, semantic, or hybrid message search. |

### Sources and file uploads

| Method | Path | Meaning |
| --- | --- | --- |
| GET, POST | `/sources` | List/register collection sources. |
| GET | `/sources/{source_id}` | Inspect source provenance; clients own stopping/starting collection. |
| GET, POST | `/sources/{source_id}/files` | List originals or synchronously accept one multipart file upload. |
| GET | `/sources/{source_id}/files/{file_id}/generations` | List preserved original generations. |
| GET | `/sources/{source_id}/files/{file_id}/content` | Download current or selected original generation, with optional byte Range. |

### Planned embeddings and changes

| Method | Path | Meaning |
| --- | --- | --- |
| GET, POST | `/embedding-profiles` | Inspect profiles or configure a new immutable embedding space. |
| GET, PATCH | `/embedding-profiles/{profile_id}` | Inspect or enable/disable a profile; model/dimensions cannot be edited in place. |
| GET | `/changes` | Read an authorized, cursor-based change feed. |

### Projects

| Method | Path | Meaning |
| --- | --- | --- |
| GET, POST | `/projects` | List/create logical project labels in the shared collection. |
| GET, PATCH | `/projects/{project_id}` | Planned name/repository metadata, without an archive lifecycle. |

### Service endpoints

| Method | Path | Meaning |
| --- | --- | --- |
| GET | `/healthz` | Process liveness; no resource content. |
| GET | `/readyz` | Core dependencies and migration readiness. |
| GET | `/v1/info` | Protected API version, capabilities, and upload limits. |

## Session and message queries

Session lists support `project_id`, `unassigned`, `harness`, `tag`, `after`, `before`, `sort`, `cursor`, and `limit`. `project_id` matches optional associations; `unassigned=true` selects an empty association set and cannot be combined with a project filter. All filters narrow the shared collection without changing access.

Session detail contains title, tags, `project_ids`, recorded cwd, harness/native IDs, activity times, relationships, and counts. An empty project array is valid. Bodies remain separate. Starting cwd need not lie inside a project directory.

Message/event lists default to all historical records. `leaf_id` chooses conversation ancestry where supported; unresolved structural links remain explicit rather than inventing exact model context. Cursors bind filters and ordering but do not guarantee a snapshot while data changes.

Content follows the [typed block union](normalized-content.md). Bound previews at 64 KiB of UTF-8; oversized structured blocks become `content_reference` rather than broken JSON. `truncated` makes omissions explicit. Full-content responses exclude preview references. Expansion reads current content without a snapshot guarantee. Artifact bytes remain separate. Content URLs are generated for the current public server address.

## Project assignment

Planned `PATCH /sessions/{id}` replaces `project_ids`; omission preserves the set and `[]` clears it. All key holders share mutations once implemented. Sync preserves catalog edits.

Sources identify native stores, not projects. `FileUpload.project_ids` optionally supplies initial labels. Unavailable labels reject the request before storage. Project administration/matching remains planned; projects have no archive lifecycle.

## Synchronous file upload

`POST /sources/{source_id}/files` accepts exactly two ordered multipart parts: `metadata` is JSON [FileUpload](examples/file-upload.json), and `file` is raw native JSONL. Metadata contains path, exact byte length, SHA-256, required `expected_version` (explicit null for a new path), and optional project IDs. No transcript MIME or import identifier is supplied.

HTTP **200** returns [FileUploadResult](examples/file-upload-result.json) only after verification, immutable original/artifact storage, normalization, and one-file catalog commit. It includes the accepted `file`, related `session_ids`, and `unchanged`. Every supplied record must be supported and valid. Unsupported or malformed records, invalid JSON function arguments, and invalid inline artifacts reject the entire file with HTTP 400 `unsupported_native_record` before any storage or catalog mutation.

Identical current size/digest returns `unchanged: true`, including retry with the previous version token after a lost response. Nonidentical stale versions return `409 version_conflict`. Missing/extra/malformed parts or actual length/hash mismatches leave previously accepted state unchanged. Clients persist snapshots and checkpoint each successful receipt; there is no atomic multi-file guarantee.

There are no import/job/index-job endpoints, signed upload targets, completion polling, leases, or persisted server upload states. See [sync](sync.md) for append generations, recovery, and orphan cleanup.

## Search

Search requests contain `query`, `mode`, `filters`, `limit`, and optional `cursor`. Filters include optional project associations, `unassigned`, harness, role, tool name, tags, and date. Default mode is `keyword`. Semantic/hybrid use one enabled instance profile and work with no project filter, several projects, or unassigned sessions. Project labels never authorize a result.

Search defaults to 20 matching messages, with a maximum of 50. Return at most three matching messages from one session; the wire result is a flat list of message matches that clients can group for display. `sort=newest` is supported with keyword mode in v1; reject semantic/hybrid plus newest ordering as an incompatible request.

Return matching message/session summaries, bounded excerpts, retrieval methods, profile ID if used, and index coverage. Require the instance API key before retrieval and result counts. No mode silently calls an unconfigured model provider or substitutes another embedding space. See [search behavior](search-and-embeddings.md).

## Exports and objects

`format=native` returns an `application/zip` bundle containing the session's native files and a manifest of related/missing files. It does not promise native harness resume. `format=jsonl` returns a SessionDB projection; `format=sts` returns a Hugging Face-compatible projection. Normalized exports accept `leaf_id`; native export always preserves original recorded files and ignores no branches silently. See [integration formats](integrations.md).

Native downloads proxy authorized bytes through the API; clients need no object-store endpoint. Only accepted valid originals are stored and accessible through source files. Range offsets address uncompressed original bytes. Reject `leaf_id` with native export rather than implying branch-filtered original files.

## Errors and compatibility

Use `400` for invalid input, `401` for missing/invalid authentication, `404` for absent data, `409` for state conflicts or unavailable configured capabilities, `413` for upload limits, `429` for throttling, and `503` for a temporarily unavailable required dependency. Embedding failures may leave keyword operations healthy.

Problem details include `type`, `title`, `status`, `detail`, optional `instance`, stable `code`, and `request_id`. Native rejection uses HTTP 400 `unsupported_native_record` with bounded safe reasons and record positions, never raw source excerpts. Never include transcripts or credentials in errors.

Within `/v1`, additive optional response fields and new enum/content variants are allowed. Removing fields, changing meanings/default ordering, or altering access semantics requires an explicit compatibility decision. Clients must tolerate unknown response variants rather than making every new harness event a client-breaking release.

## Machine readable contract review

The [OpenAPI 3.1 contract](openapi.yaml) covers this inventory, schemas, authentication, errors, and pagination. Fifteen baseline operations plus two artifact reads are implemented; other retained operations are explicitly planned. Validate it and compare it against CLI/SDK behavior as part of integrated verification. Syntax validation alone is not product approval.
