# Data model

Sessions are the primary records. Messages, tool calls, events, and artifacts describe their contents. Sources describe where native files are collected. Projects provide optional grouping independently of collection and access.

Status: proposed logical schema, incorporating [the session-first design](decisions/0008-collection-and-project-associations.md). This is not a database migration.

## Session model

```mermaid
erDiagram
    SOURCE ||--o{ SOURCE_FILE : collects
    SOURCE_FILE ||--o{ FILE_GENERATION : preserves
    SOURCE_FILE }o--o{ SESSION : provides
    SESSION ||--o{ MESSAGE : contains
    SESSION ||--o{ SESSION_EVENT : records
    SESSION ||--o{ ARTIFACT : references
    SESSION ||--o{ SESSION_PROJECT : has
    PROJECT ||--o{ SESSION_PROJECT : groups
```

A source file can contribute records to a session, and a session can depend on several files or related sessions. A session can have no project associations or several. Native copies and inherited history remain distinguishable in the catalog.

## Resources

| Resource | Main fields and behavior |
| --- | --- |
| Session | Organization, owner, contributor attribution, harness/native ID, title, tags, `project_ids`, recorded cwd, start/activity times, visibility, source links, and processing state. |
| Message | Session, canonical/native role, origin, typed content, revision, recorded time, sequence, native references, and optional conversation parent. |
| Tool call | Call ID/name, explicitly JSON or text input, source message, and linked result when available; links are scoped to the originating session/source. |
| Session event | Compaction, model change, context edit, branch marker, or another recorded event, retaining native type and source reference. |
| Session relation | Parent/child, fork, delegation, or inherited-context reference, with an explicit resolution state. |
| Artifact | File/image/large content reference, media type, size, and access inherited from the session. |
| Source | Registering account, device label, harness, native store root, default visibility for new sessions, and last successful sync. No project binding. |
| Project | ID, slug, name, optional repository URLs, creator, and status `active` or `archived`. A logical label within the current organization. |
| Session project | A unique session/project association. Changing it does not change native files, ownership, or sharing. |
| Session grant | A session, user/team principal, and viewer/editor role. Separate from project associations. |
| Job | Organization, requester, kind, optional source/session/project filters, state, progress, attempts, timestamps, and bounded errors. |

## Collection and directory metadata

Sources identify configured harness session stores, such as `~/.codex/sessions`. The recorded starting cwd is an attribute of a session, not its collection boundary or project identity. A session started in `/home/ada` can work on Payments in `/home/ada/work/payments` and later be assigned that project.

Native IDs help identify sessions within a harness, but do not replace source bindings. Claude subagents can repeat their parent's native session ID. Copies from different sources may differ, and divergent files are not silently combined.

An imported file may identify no session yet, one session, or inherited records. Archive the file and report parsing state independently. Unknown native formats remain retrievable by the source owner even before a session can be normalized.

The source owner initializes the session owner and contributor attribution in v1. A message's wire role remains distinct from that account: native user-role data can be tool output or injected context.

## Project associations

`project_ids` is always an array; `[]` means unassigned. `PATCH` with a replacement array sets associations, omission preserves them, and an empty array clears them. Multiple project matches are valid.

Explicit configured rules may supply initial associations at first import. A nonmatch never blocks collection. Later sync updates recorded content but does not reset project labels, title/tags, or visibility chosen in the catalog. Repository metadata and referenced paths can provide suggestions without becoming authoritative classification.

Projects are searchable grouping metadata, not containers that determine ownership or visibility. Project names can be listed within the current organization, while session results and counts are filtered by actual session access. See [project associations](project-associations.md).

## Messages and events

Content is a typed list of text, available reasoning, tool call, tool result, structured JSON, artifact reference, and unknown/native blocks. [Normalized content](normalized-content.md) defines the schemas and provider rules. Missing reasoning or timestamps remain unknown. Unknown blocks retain native type and opaque data, with message references locating the original records.

Conversation ancestry and operation nesting are separate fields. `conversation_parent_id` describes branching. A delegated operation or tool invocation has its own relationship; adjacent timestamps alone do not establish it.

The default message endpoint returns historical records in deterministic recorded order, including alternate branches. `leaf_id` selects a known message's ancestry. Compaction and context edits remain structural events; v1 does not promise exact provider-request reconstruction or deterministic replay.

The API returns bounded previews with `truncated` and a content URL where needed. Full normalized content and native export provide separate expansion paths.

## Original files

A source-relative file has accepted generations, each with a current immutable object reference, complete byte length, digest, and collection time. Verified prefix growth advances that generation; a rewrite preserves the preceding generation and starts another.

Source references address original uncompressed line/byte ranges. A newer verified append includes earlier referenced ranges. Old append snapshots can be collected once no explicit pin/export needs them; rewrite generations follow retention policy. See [sync](sync.md) and [operations](operations.md).

Source roots and unparsed files are visible to their owner and administrators. Session readers can access associated native content only when the complete file is authorized; a file containing other inaccessible sessions is omitted from their native bundle with a bounded explanation. Separately referenced sessions do not inherit access merely by being linked.

## Shared access

Organization membership scopes accounts and metadata. A session is visible to its owner, administrators, explicitly granted users/teams, or all members when its visibility is `organization`. Default visibility is `private`, meaning owner/admin plus explicit grants.

Users and teams receive viewer/editor session grants. Labels do not grant access. Project reassignment, removal, and matching rules therefore do not publish a private session or move a shared session into another permission boundary. See [access control](access-control.md).

## Storage and deletion

Use PostgreSQL foreign keys, uniqueness constraints, UTC timestamps, organization ownership, and transactional changes. Public IDs are opaque UUIDs; native IDs remain strings. Provider-specific fields remain native metadata rather than becoming top-level fields for every harness.

Archiving a project stops new associations to that label. It does not stop source collection, delete sessions, or remove existing associations. An owner/admin can delete a session: it disappears from queries immediately and cleanup removes derived content and exclusively referenced objects asynchronously.

Deletion suppresses its existing source bindings so ordinary sync does not recreate it. The client receives a `suppressed` receipt and leaves the native local file intact. Restore/reimport requires a separate explicit contract.
