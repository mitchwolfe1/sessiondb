# Automatic sync

The client collects native Codex, Claude Code, and Pi stores, backfills history, and uploads later changes regardless of starting cwd or project labels. One-shot sync and polling watch are implemented; filesystem notifications and OS service installation remain planned.

Status: accepted synchronous per-file protocol; see [ADR 0014](decisions/0014-sessions-and-synchronous-files.md).

## Sources and capture

Discover Codex under `CODEX_HOME` or `~/.codex`, including native archived sessions; Claude under its configured home or `~/.claude/projects`; and Pi under its configured directory or `~/.pi/agent/sessions`. Explicit roots override discovery. A source records harness, device label, and native store root, not a repository boundary or owner.

`sessiondb sync` runs one pass; `sessiondb sync --watch` reconciles periodically until interrupted. Stop the client/watch process to stop collection; there is no server pause flag. Local disappearance or an offline device does not remove accepted history.

For active JSONL, capture a bounded snapshot through the last complete newline. Defer incomplete tail bytes until the next pass. Complete malformed lines remain in the captured snapshot and cause rejection; do not silently remove them. Preserve native parent/child/subagent files and allowlisted artifacts, without sweeping adjacent credential directories.

## One-file HTTP contract

`POST /v1/sources/{source_id}/files` uses authenticated `multipart/form-data` with exactly two parts, in order:

1. `metadata`: `application/json` containing [FileUpload](examples/file-upload.json): source-relative `path`, exact `byte_length`, lowercase SHA-256, required `expected_version`, and optional initial `project_ids`.
2. `file`: the raw JSONL snapshot bytes (`application/x-ndjson`).

A new path requires explicit `"expected_version": null`, not an omitted field. The API rejects malformed/missing/extra parts, invalid paths, excessive length, and actual length/hash mismatches without changing previously accepted file/session state.

The server verifies bytes and fully parses the transcript before registering/storing originals/artifacts under fresh immutable S3 keys, and atomically publishes one file's catalog changes before returning **HTTP 200 FileUploadResult**. The receipt contains `file`, related `session_ids`, and `unchanged`; it is the checkpoint boundary. There are no signed client-to-S3 uploads, completion requests, import resources, jobs, acceptance polling, or server upload leases.

Every supplied record must be supported and valid. Unsupported/malformed records, invalid JSON function arguments, or unsupported inline artifacts reject the whole file with HTTP 400 `unsupported_native_record` before object registration, S3 writes, or catalog mutation. Direct malformed/incomplete input rejects; only CLI active-prefix capture defers incomplete tails. Prior accepted state and checkpoints remain unchanged.

## Disconnects and version conflicts

Persist pending local snapshots before transfer. Advance each path's checkpoint only after its successful receipt; a later file failure does not undo earlier accepted files. Multi-file sync is deliberately **not all-or-nothing**.

After a disconnect or lost response, retry the same metadata and snapshot using a fresh reader. If the current accepted size/digest match, the server returns that file/version and related session IDs with `unchanged: true`, even if `expected_version` is the previous token. This prevents duplicate acceptance after a lost receipt without a persisted idempotency table.

Otherwise the server compares `expected_version` again before committing. A stale value returns `409 version_conflict`; refresh source files and reconcile rather than blindly overwriting newer bytes. A request interrupted before publication leaves the previous accepted state intact; unreferenced prepared objects are cleaned later.

Client state holds pending snapshots and accepted checkpoints. `sessiondb sync status --json` reports their counts. On `400 unsupported_native_record`, sync reports the source-relative path and reason, discards that pending snapshot, and continues with the remaining saved and newly discovered files. Each rejected path is attempted once per pass and remains eligible on the next pass after a local repair or server upgrade. Only successful receipts advance checkpoints. A one-shot sync with any rejection exits nonzero. Authentication errors stop attempts until a valid key is supplied. Watch can recover from transient outages on a later reconciliation.

## Appends and rewrites

Byte-changing uploads receive a new version. The server compares the full prior prefix: verified growth keeps the generation ID and advances its immutable snapshot reference. A shorter file or changed earlier bytes creates a new generation and preserves the old one. Header similarity alone does not prove an append.

Sync preserves existing catalog titles, tags, and projects on append. Optional initial project labels do not determine collection; unavailable labels reject the upload before storage. Native archive moves and copies retain source provenance and explicit relations; divergent histories are not silently combined.

## Storage and limits

Only the API needs access to the S3 endpoint. It records object metadata before S3 upload, fully parses before object registration and prepares bytes outside the publication transaction, then rechecks versions and commits. No database transaction spans network I/O.

Periodic cleanup atomically removes old unreferenced object rows and returns their keys, then deletes S3 bytes outside the transaction. Foreign keys/row locks protect concurrent publication. Never-reused keys prevent delayed cleanup from deleting a newer request's content. Accepted snapshots and preserved rewrite generations remain referenced.

The only upload limit is advertised `limits.max_file_bytes`, default **268435456 bytes (256 MiB)**. There is no batch/manifest limit, upload TTL, generic worker setting, or hidden file drop. See [operations](operations.md), [API](api.md), and [acceptance scenarios](roadmap.md#acceptance-scenarios).
