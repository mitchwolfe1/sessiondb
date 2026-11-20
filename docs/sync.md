# Automatic sync

The client collects native Codex, Claude Code, and Pi stores, backfills history, and uploads later changes regardless of starting cwd or project labels. One-shot sync, foreground polling, and user-login services are implemented.

Status: accepted synchronous per-file protocol; see [ADR 0014](decisions/0014-sessions-and-synchronous-files.md).

## Sources and capture

Discover Codex under `CODEX_HOME` or `~/.codex`, including native archived sessions; Claude under its configured home or `~/.claude/projects`; and Pi under its configured directory or `~/.pi/agent/sessions`. Explicit roots override discovery. A source records harness, device label, and native store root, not a repository boundary or owner.

`sessiondb sync` runs one pass; `sessiondb sync --watch` reconciles periodically until interrupted. Stop the client/watch process to stop collection; there is no server pause flag. Local disappearance or an offline device does not remove accepted history.

For active JSONL, capture a bounded snapshot through the last complete newline. Defer incomplete tail bytes until the next pass. Complete malformed lines remain in the captured snapshot and cause rejection; do not silently remove them. Preserve native parent/child/subagent files and allowlisted artifacts, without sweeping adjacent credential directories.

## Background service

Configure a context and its source roots, then save its API key with `sessiondb auth login --key-stdin` or supply the key in the user service manager's environment. The generated service definition contains only nonsecret arguments. An API key exported in an interactive shell is scoped to that shell and its children.

```sh
sessiondb sync install
sessiondb sync status
sessiondb sync logs --follow
sessiondb sync stop
sessiondb sync start
sessiondb sync uninstall
```

Installation is opt-in. It starts a systemd user unit on Linux or a LaunchAgent in the user's GUI login domain on macOS. It runs immediately and at subsequent logins under the same user. `stop` stops it for the current login. `uninstall` also removes the login registration, preserving checkpoints, native files, credentials, and remote data. The default service is `sessiondb-sync.service` or `io.sessiondb.sync`. An explicit state directory has its own service identity and sync lock. Use the same `--state-dir` to manage it. [systemd services](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html) and [Apple’s launch agent guide](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html) describe the underlying managers.

The installer captures the current context, server origin, timeout, config path and state directory. Changes to that context's source roots and polling interval take effect at the next pass. Reinstall to change the service target or to restart with a replacement binary. Linux logs go to the user journal. macOS logs go to `sync.log` in the state directory. `sync logs` shows recent lines on either platform.

Watch reuses a local file digest while its Unix identity, size and modification/change times match. It refreshes the remote file listing when the source sync time changes, after recovered uploads, and at least every 15 minutes of polling. Files changed during capture are deferred to the next watch pass. One-shot sync performs a full reconciliation.

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

Client state holds pending snapshots and accepted checkpoints. `sessiondb sync status --json` reports their counts. On `400 unsupported_native_record`, sync reports the source-relative path and reason, discards that pending snapshot, and continues with the remaining saved and newly discovered files. The client remembers the rejected complete-prefix SHA-256, source-relative path, server origin and server version. A watch pass skips an unchanged rejection and reports it in `cached_rejections`. A changed prefix or changed server version is retried automatically. After a parser rebuild that keeps the same server version, use `sessiondb sync retry` to clear the remembered hashes for that server. One-shot sync explicitly retries all current files that still need upload. Only successful receipts advance checkpoints. A one-shot sync with any rejection exits nonzero. Authentication errors stop attempts until a valid key is supplied. Watch can recover from transient outages on a later reconciliation.

A structured `500 internal_error` on an individual upload retains the exact pending snapshot and its expected version. The client continues with other paths and reports an incomplete pass. The next pass retries the saved snapshot before capturing newer bytes for that path. The JSON report lists these paths under `retryable_errors`.

## Appends and rewrites

Byte-changing uploads receive a new version. The server compares the full prior prefix: verified growth keeps the generation ID and advances its immutable snapshot reference. A shorter file or changed earlier bytes creates a new generation and preserves the old one. Header similarity alone does not prove an append.

Sync preserves existing catalog titles, tags, and projects on append. Optional initial project labels do not determine collection; unavailable labels reject the upload before storage. Native archive moves and copies retain source provenance and explicit relations; divergent histories are not silently combined.

## Storage and limits

Only the API needs access to the S3 endpoint. It records object metadata before S3 upload, fully parses before object registration and prepares bytes outside the publication transaction, then rechecks versions and commits. No database transaction spans network I/O.

Periodic cleanup atomically removes old unreferenced object rows and returns their keys, then deletes S3 bytes outside the transaction. Foreign keys/row locks protect concurrent publication. Never-reused keys prevent delayed cleanup from deleting a newer request's content. Accepted snapshots and preserved rewrite generations remain referenced.

The only upload limit is advertised `limits.max_file_bytes`, default **268435456 bytes (256 MiB)**. There is no batch/manifest limit, upload TTL, generic worker setting, or hidden file drop. See [operations](operations.md), [API](api.md), and [acceptance scenarios](roadmap.md#acceptance-scenarios).
