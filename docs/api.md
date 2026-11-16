# HTTP API contract

The SessionDB API provides session, message, import, and search resources for the CLI, SDK, and other applications.

Status: proposed v1 resource and behavior contract. The [OpenAPI draft](openapi.yaml) defines wire schemas and operations. The [contract examples](examples/README.md) exercise those schemas without claiming that an HTTP server exists. The session-first revision is `0.2.0-draft`, following [0008](decisions/0008-collection-and-project-associations.md). It remains a design contract before implementation.

## Conventions

- Base namespace `/v1`; JSON requests and responses use `snake_case` fields.
- Resource IDs are opaque UUIDs. Native harness IDs and source paths remain separate string attributes. CLI slugs resolve through list/detail APIs.
- Bearer authentication; organization IDs are explicit in paths. Authentication configuration and health are public without exposing session content.
- Timestamps are RFC 3339 with UTC output. `after` is inclusive and `before` is exclusive; message search uses the matching message's timestamp, with an explicit session-time fallback when absent.
- List responses use `data` and nullable `next_cursor`. Default `limit=50`, maximum `200`. Cursors are opaque and bound to filters, organization scope, and ordering.
- Default session order is `updated_at` descending with ID as a deterministic tie-breaker. Messages use recorded sequence with ID tie-breaking, not arbitrary timestamp sorting.
- `POST` creates a resource/job, `GET` reads, `PATCH` changes documented mutable fields, and `DELETE` revokes or schedules deletion as documented. Unknown request fields are rejected; clients ignore unknown response fields and preserve unknown content block types.
- Request IDs appear in responses and logs. Use HTTP statuses consistently and errors in `application/problem+json` following RFC 9457.

## Resource inventory

Relative paths below use the prefix `/v1/organizations/{org_id}`. Public, authentication, and organization root paths are shown in full.

### Sessions and search

| Method | Path | Meaning |
| --- | --- | --- |
| GET | `/sessions` | Filter and paginate accessible sessions. |
| GET, PATCH, DELETE | `/sessions/{session_id}` | Read, change title/tags/project associations or explicit visibility, or schedule authorized deletion. |
| GET, POST | `/sessions/{session_id}/grants` | List/add explicit user/team viewer or editor grants; owner/admin management. |
| DELETE | `/sessions/{session_id}/grants/{grant_id}` | Revoke an explicit session grant. |
| GET | `/sessions/{session_id}/messages` | Read historical messages, optionally filtered by conversation leaf. |
| GET | `/sessions/{session_id}/events` | Read recorded structural events and native event metadata. |
| GET | `/sessions/{session_id}/export` | Stream a native bundle or normalized JSONL/STS projection. |
| GET | `/messages/{message_id}` | Read a bounded normalized message. |
| GET | `/messages/{message_id}/content` | Read full normalized content. |
| POST | `/search` | Keyword, semantic, or hybrid message search. |

### Collection and imports

| Method | Path | Meaning |
| --- | --- | --- |
| GET, POST | `/sources` | List/register collection sources. |
| GET, PATCH | `/sources/{source_id}` | Inspect/pause a native store source or change its default visibility for future sessions. |
| GET | `/sources/{source_id}/files` | List accepted original files, including files that failed parsing. |
| GET | `/sources/{source_id}/files/{file_id}/generations` | List preserved original generations. |
| GET | `/sources/{source_id}/files/{file_id}/content` | Download current or selected original generation, with optional byte Range. |
| GET, POST | `/imports` | Inspect import batches or create an upload manifest. |
| GET | `/imports/{import_id}` | Get upload/processing state and file-level results. |
| POST | `/imports/{import_id}/complete` | Verify and durably accept uploaded files; enqueue processing. |

### Indexing and changes

| Method | Path | Meaning |
| --- | --- | --- |
| GET, POST | `/embedding-profiles` | Inspect profiles or configure a new immutable embedding space. |
| GET, PATCH | `/embedding-profiles/{profile_id}` | Inspect or enable/disable a profile; model/dimensions cannot be edited in place. |
| POST | `/index-jobs` | Request indexing with optional source/session/project-label filters; administrator operation. |
| GET | `/jobs/{job_id}` | Read parsing, indexing, or deletion job state. |
| GET | `/changes` | Read an authorized, cursor-based change feed. |

### Projects

| Method | Path | Meaning |
| --- | --- | --- |
| GET, POST | `/projects` | List/create logical project labels within the organization. |
| GET, PATCH | `/projects/{project_id}` | Inspect, rename, or archive a project. |

### Connections and administration

| Method | Path | Meaning |
| --- | --- | --- |
| GET | `/healthz` | Process liveness; no resource content. |
| GET | `/readyz` | Core dependencies and migration readiness. |
| GET | `/v1/auth/config` | Public issuer, audience, CLI client, and login capabilities. |
| GET | `/v1/info` | Public API version, capabilities, and upload limits. |
| POST | `/v1/auth/session` | Register an authenticated login and claim matching pending invitations. |
| GET | `/v1/me` | Current user and accessible organization memberships. |
| GET | `/v1/organizations` | Accessible organizations; this path has no organization prefix. |
| GET, PATCH | `/v1/organizations/{org_id}` | Read the selected organization or change its display name; PATCH is owner-only. Absolute path. |
| GET, POST | `/invitations` | List/create pending organization invitations; administrators only. |
| DELETE | `/invitations/{invitation_id}` | Revoke a pending invitation. |
| GET | `/members` | List organization members. |
| PATCH, DELETE | `/members/{user_id}` | Change organization role or remove membership. |
| GET, POST | `/teams` | List/create teams. |
| GET, PATCH | `/teams/{team_id}` | Inspect/update a team. |
| GET | `/teams/{team_id}/members` | List team members. |
| PUT, DELETE | `/teams/{team_id}/members/{user_id}` | Add/remove an existing organization member. |
| GET, POST | `/tokens` | List/create member-scoped application or source-bound sync tokens; secret returned only on creation. |
| DELETE | `/tokens/{token_id}` | Revoke a token. |

## Session and message queries

Session lists support `project_id`, `unassigned`, `author_id`, `team_id`, `harness`, `tag`, `after`, `before`, `sort`, `cursor`, and `limit`. `project_id` matches optional associations; `unassigned=true` selects an empty association set and cannot be combined with a project filter. `team_id` selects sessions explicitly granted to that team. All filters narrow sessions already readable by the caller.

Session detail contains title, tags, `project_ids`, owner/contributor, visibility, recorded starting cwd, harness/native IDs, activity times, relationships, counts, and processing state. An empty project array is valid. It does not include all message bodies. The starting cwd need not be inside any associated project directory.

Message/event lists default to all historical records. `leaf_id` chooses a conversation ancestry where supported; incomplete ancestry is returned with a warning rather than silently represented as exact model context. Pagination cursors include a projection revision. If a rewrite/reparse invalidates that revision, return `409 cursor_stale` and instruct the client to restart the listing.

Bound individual previews at 64 KiB and set `truncated=true` and a full-content URL when needed. This limit is UTF-8 bytes, not characters. A truncation boundary must be valid UTF-8. Large artifact blocks are references rather than embedded base64 in list responses.

## Assignment and sharing

`PATCH /sessions/{id}` can replace `project_ids`; omission preserves the set and `[]` clears it. An editor can change labels, while only the owner/admin can change visibility. Session grants live under `/sessions/{id}/grants` and are independent of project metadata. Sync never resets these catalog edits. A label change updates filters without moving native files or changing readers.

Sources register one owned native store root and harness. They have no project/workspace binding. Per-file import descriptors may optionally provide initial `project_ids` for newly recognized sessions. Absent/nonmatching associations never prevent ingestion; unavailable/archived labels are omitted with a classification warning. Invalid manual PATCH associations are rejected without changing the session.

## Imports and jobs

Import creation returns `201` with a `Location` header and upload targets. Completion returns `202` once objects and the catalog are durable; indexing proceeds asynchronously. Finalization is atomic for a manifest: an invalid/missing object or version conflict prevents new catalog acceptance for the whole batch. Repeated completion with the same idempotency key returns the same receipt. A failed parse does not revoke durable archival acceptance.

Jobs expose `queued`, `running`, `succeeded`, `partial`, `failed`, or `cancelled`, with counters and bounded errors. Retry semantics are determined by job kind. Embedding outage never changes an accepted source upload into a failed sync. Changes to labels emit previous/current associations so filtered change consumers can remove as well as add a session; they do not emit a sharing change unless visibility/grants actually change.

## Search

Search requests contain `query`, `mode`, `filters`, `limit`, and optional `cursor`. Filters include optional project associations, `unassigned`, author, explicit team grants, harness, role, tool name, tags, and date. Default mode is `keyword`. Semantic/hybrid use one enabled organization profile and work with no project filter, several projects, or unassigned sessions. Project labels never authorize a result.

Search defaults to 20 matching messages, with a maximum of 50. Return at most three matching messages from one session; the wire result is a flat list of message matches that clients can group for display. `sort=newest` is supported with keyword mode in v1; reject semantic/hybrid plus newest ordering as an incompatible request.

Return matching message/session summaries, bounded excerpts, retrieval methods, profile ID if used, and index coverage. Apply session access before retrieval and result counts. No mode silently calls an unconfigured model provider or substitutes another embedding space. See [search behavior](search-and-embeddings.md).

## Exports and objects

`format=native` returns an `application/zip` bundle containing the session's authorized native files and a manifest of related/missing files. It does not promise native harness resume. `format=jsonl` returns a SessionDB projection; `format=sts` returns a Hugging Face-compatible projection. Normalized exports accept `leaf_id`; native export always preserves original recorded files and ignores no branches silently. See [integration formats](integrations.md).

Native object downloads either proxy bytes or redirect to a short-lived signed URL after authorization. Originals that have no parsed session remain accessible through source-file endpoints. Range reads address original uncompressed bytes. A compression layer must translate ranges correctly or serve the uncompressed representation; a gzip byte offset is not a JSONL source offset. Reject `leaf_id` with native bundle export instead of implying that original files were filtered to a branch.

## Errors and compatibility

Use `400` for invalid input, `401` for missing/invalid authentication, `403` for a denied known management action, `404` for absent/inaccessible data, `409` for state conflicts or unavailable configured capabilities, `413` for upload limits, `429` for throttling, and `503` for a temporarily unavailable required dependency. Embedding failures may leave keyword operations healthy.

Problem details include `type`, `title`, `status`, `detail`, `instance`, stable `code`, and `request_id`; file/field errors are bounded extensions. Do not put transcripts, credentials, or signed URLs in errors. Use `Retry-After` when appropriate.

Within `/v1`, additive optional response fields and new enum/content variants are allowed. Removing fields, changing meanings/default ordering, or altering access semantics requires an explicit compatibility decision. Clients must tolerate unknown response variants rather than making every new harness event a client-breaking release.

## Machine readable contract review

The checked-in [OpenAPI 3.1 contract](openapi.yaml) covers this inventory with resource/request/response schemas, operation IDs, examples, authentication, errors, and pagination. Validate it, compare it against CLI commands, and review its diff before API implementation. [Open questions](open-questions.md) records acceptance of this draft as a blocking phase-zero decision; syntactic validation alone is not product approval.
