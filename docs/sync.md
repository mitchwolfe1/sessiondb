# Automatic sync

The sync client automatically collects sessions from configured native harness stores and keeps them updated in SessionDB. It supports initial backfill, one-shot synchronization, continuous watch mode, and collection from multiple machines. A project association or repository cwd is not required.

Status: proposed v1 protocol. The client is part of the CLI and uses the public import API and ordinary signed object uploads.

## Discovery and configuration

Discover Codex under `CODEX_HOME` or `~/.codex`, including `sessions` and `archived_sessions`; Claude Code under its configured home or `~/.claude/projects`; and Pi under its configured session directory or `~/.pi/agent/sessions`. Explicit roots override automatic discovery.

Discovery identifies native stores. Enabling selected roots determines which sessions are uploaded; optional include/exclude filters operate on native file paths or session identifiers. With no such filter, eligible sessions in those stores are collected regardless of their recorded starting cwd, including sessions launched from a home directory.

Register one source per account/device/harness/store root. The source has no project binding. A single source can contain unassigned sessions and sessions associated with different or multiple projects. Its default visibility initializes new sessions; later source updates never reset per-session sharing or catalog labels.

Optional project rules supply initial `project_ids` as grouping metadata. No match produces an unassigned session, and several matches can produce several associations. A descriptor can carry those initial associations. Unavailable/archived labels are omitted with a classification warning instead of blocking valid original bytes; a malformed manifest is still a validation error. Labels are not permission grants, and later sync preserves existing associations.

Supporting artifacts are allowlisted through the adapter. Preserve their relative layout. A reference outside the configured source is reported as missing, unless the user separately allows that location. No credential/configuration directory is included merely because it sits beside a transcript.

## Client behavior

`sessiondb sync` performs one pass and exits. `sessiondb sync --watch` performs initial backfill and continues until stopped. The installed background service runs the same watch mode.

Use filesystem notifications as a prompt to scan, with periodic reconciliation to catch missed events. Proposed defaults are a 60-second reconciliation interval, a 2-second write debounce, four concurrent uploads, and bounded batches of 32 files. These defaults are reviewable settings, not performance claims.

Persist local source registration, accepted file position, and pending import state in a client SQLite state database. It is operational client state, not a second authoritative session server. A restart resumes a pending import or safely repeats its request. Never advance the accepted checkpoint on an object upload alone.

For active JSONL, read a bounded snapshot through the last complete newline. Defer unfinished tail bytes to the next pass. Complete malformed lines are uploaded as recorded and reported by the parser; they are not silently removed from the archived representation. Supporting binary artifacts use a stable bounded file read and retry if they change during capture.

## Import lifecycle

1. `POST /v1/organizations/{org_id}/imports` reserves a manifest for an owned source and returns signed staging-upload targets. Descriptors contain source-relative path, complete length, media type, SHA-256, expected version, and optional initial project labels.
2. The client uploads the bounded native snapshots using each target's method and headers. New targets expire after 15 minutes or at the end of the import lease, whichever comes first. Before the lease ends, `GET /imports/{id}` can issue fresh targets for an awaiting upload.
3. `POST /imports/{id}/complete` schedules durable verification/promotion work and returns `202` with status `verifying`. This acknowledges the request, not archival acceptance of the files. Repeating completion returns the same import/job identity. `job_id` remains the verification job; the import's status tracks later parsing separately.
4. The worker copies each staging object to a new server-only immutable key, using the observed source ETag as a copy precondition. It streams the accepted copy to check its actual length and SHA-256. Later writes to a staging URL cannot change that accepted copy.
5. The worker rechecks source access and expected versions and atomically commits the manifest's file updates, receipt, and parse job. It sets `accepted_at`; only then may the client advance its source checkpoint. A failed object/version check leaves every new file in that batch unaccepted and reports bounded file errors.
6. The client polls `GET /imports/{id}` until `accepted_at` is set or verification fails. Parsing and optional embedding proceed after acceptance; the client does not re-upload originals just because parsing is slow or fails.

Import states are `awaiting_upload`, `verifying`, `queued`, `processing`, `ready`, `partial`, `failed`, and `expired`. `accepted_at: null` means originals have not been accepted. `failed` plus a non-null acceptance time can mean a later parser failure; the accepted originals remain retrievable. Atomic archival acceptance and partial parsing are separate results.

Create/complete requests use an `Idempotency-Key`, retained for 30 days and scoped to organization/source/operation. The same request returns the same resource and effects, with its current status; transient upload URLs can be renewed. Reusing a key with different content returns `409`. A resumed client reconciles source versions when its saved request is older than receipt retention.

The upload lease is 24 hours from creation. Once expired, create a new manifest/key without advancing unaccepted checkpoints. Staging and unreferenced promotion objects are eligible for cleanup 24 hours after the import closes, with no active lease/reference. Accepted originals have no automatic expiry by default.

Classification is optional: unavailable/archived initial project labels produce warnings while valid original bytes can still be accepted. Invalid JSON/paths/limits remain request errors. An asynchronous expected-version conflict is reported in the import/job result; the client refreshes source state and creates a reconciled manifest.

## Growing and rewritten files

V1 uploads changed file snapshots using existing object operations. The server compares an accepted prior prefix with the new bytes. Proven prefix growth extends the current file generation; different earlier content or a shorter file starts a new generation. A first-kilobyte header match alone does not prove an append.

Publish the new object/catalog reference atomically in PostgreSQL after object verification. The preceding append snapshot can become eligible for cleanup when no explicit pin or export requires it, because the new snapshot includes its complete prefix. A rewrite preserves the previous generation. Native references that require an earlier prefix remain resolvable through the file-generation catalog.

An expected-version conflict detected while creating a manifest returns `409`; one detected during asynchronous verification appears in the failed import/job result. The client refreshes source state and reconciles before creating a new request; it does not overwrite the server's newer state blindly. Identical bytes collected from another source may share physical blob storage while retaining separate source bindings.

Native archive moves and copied files may create another source path. Keep their collected session relation explicit and avoid creating a second visible session when the adapter can prove an identical copy. Divergent copies remain visible as separate source variants; v1 does not merge competing conversation edits.

Sync changes recorded native content only. It preserves manual project assignments, tags/title changes, and per-session visibility or grants. Classification therefore remains stable as a home-launched session continues to work in another repository.

## Missing files and deletion

Local deletion, provider cleanup, or an offline source does not delete server history. Mark source availability through sync status. Server deletion is an explicit authorized operation. A suppression receipt prevents the same source binding from being recreated by ordinary collection; the local original is untouched.

## Retry and limits

Retry connection errors, timeouts, `429`, and appropriate `5xx` responses with bounded exponential backoff, jitter, and `Retry-After`. Authentication failures pause the affected source and request sign-in. Permanent validation errors are shown with file-level detail and do not block unrelated sources.

The proposed v1 defaults are at most 128 files per import, a 2 MiB manifest, and 256 MiB per file. Operators can configure lower organizational limits. Limits must be advertised by server metadata and checked before transfer. Large-file/multipart support is a later compatibility decision, not a reason to drop a file silently.

`sessiondb sync status` reports last successful acceptance, pending files/bytes, paused sources, and bounded errors. Server parsing/indexing state is reported separately. It must be possible to distinguish an offline laptop, a failed upload, an unsupported native format, and embedding backlog.

See [the API contract](api.md), [CLI behavior](cli.md), and [acceptance scenarios](roadmap.md#acceptance-scenarios).
