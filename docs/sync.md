# Automatic sync

The sync client automatically collects sessions from configured workspaces and keeps them updated in SessionDB. It supports initial backfill, one-shot synchronization, continuous watch mode, and collection from multiple machines.

Status: proposed v1 protocol. The client is part of the Rust CLI and uses the public import API and ordinary signed object uploads.

## Discovery and configuration

Discover Codex under `CODEX_HOME` or `~/.codex`, including `sessions` and `archived_sessions`; Claude Code under its configured home or `~/.claude/projects`; and Pi under its configured session directory or `~/.pi/agent/sessions`. Explicit roots override automatic discovery.

Discovery identifies available sources; it does not enable uploading all of them. A mapping binds a local workspace and harness to a server project. Use the recorded cwd and configured workspace containment to select sessions. If a session matches several mappings, stop that session with an actionable ambiguity rather than choosing a destination by order. Sessions outside all mappings remain local.

Register one server source per project/harness/device mapping. The CLI creates sources through the API after checking contributor access. The source stores a device label, not a requirement to expose arbitrary host identifiers.

Supporting artifacts are allowlisted through the adapter. Preserve their relative layout. A reference outside the configured source is reported as missing, unless the user separately allows that location. No credential/configuration directory is included merely because it sits beside a transcript.

## Client behavior

`sessiondb sync` performs one pass and exits. `sessiondb sync --watch` performs initial backfill and continues until stopped. The installed background service runs the same watch mode.

Use filesystem notifications as a prompt to scan, with periodic reconciliation to catch missed events. Proposed defaults are a 60-second reconciliation interval, a 2-second write debounce, four concurrent uploads, and bounded batches of 32 files. These defaults are reviewable settings, not performance claims.

Persist local source registration, accepted file position, and pending import state in a client SQLite state database. It is operational client state, not a second authoritative session server. A restart resumes a pending import or safely repeats its request. Never advance the accepted checkpoint on an object upload alone.

For active JSONL, read a bounded snapshot through the last complete newline. Defer unfinished tail bytes to the next pass. Complete malformed lines are uploaded as recorded and reported by the parser; they are not silently removed from the archived representation. Supporting binary artifacts use a stable bounded file read and retry if they change during capture.

## Import lifecycle

1. `POST /v1/organizations/{org_id}/imports` creates a batch manifest with a registered source and file descriptors. Each descriptor supplies a source-relative path, complete byte length, media type, digest, and previously accepted version when available.
2. The server checks project/source permission, expected versions, path validity, and configured limits. It returns an import ID and upload targets for content it does not already have.
3. The client uploads through the provided S3-compatible targets, preserving original bytes. Already available objects require no transfer.
4. `POST /v1/organizations/{org_id}/imports/{import_id}/complete` verifies that every required object is durable and matches the descriptor. It transactionally accepts updates and enqueues processing.
5. Finalization is atomic for the manifest. The receipt lists accepted, unchanged, and suppressed files. A missing object, digest mismatch, version conflict, or newly denied source prevents all new catalog acceptance in that batch. Correct the upload and retry, or create a corrected manifest with a new idempotency key. The client advances checkpoints only after the completion receipt.
6. Background processing identifies sessions, normalizes records, and updates the keyword index. Optional embedding follows independently.

Create and complete requests use an `Idempotency-Key` scoped to the organization, source, and operation. The client persists the key with its pending request. Reusing a key with the same body returns the original result; a different body returns `409`. This is an application retry convention, not a claim that POST is inherently idempotent.

Import states are `awaiting_upload`, `queued`, `processing`, `ready`, `partial`, `failed`, and `expired`. A successful completion receipt means durable acceptance even while the import is `queued`. Parsing can succeed for some files and fail for others, producing `partial` or `failed` processing while every accepted original remains retrievable. Partial parsing is distinct from atomic archival acceptance.

## Growing and rewritten files

V1 uploads changed file snapshots using existing object operations. The server compares an accepted prior prefix with the new bytes. Proven prefix growth extends the current file generation; different earlier content or a shorter file starts a new generation. A first-kilobyte header match alone does not prove an append.

Publish the new object/catalog reference atomically in PostgreSQL after object verification. The preceding append snapshot can become eligible for cleanup when no explicit pin or export requires it, because the new snapshot includes its complete prefix. A rewrite preserves the previous generation. Native references that require an earlier prefix remain resolvable through the file-generation catalog.

An expected-version conflict returns `409`. The client refreshes source state and reconciles before creating a new request; it does not overwrite the server's newer state blindly. Identical bytes collected from another source may share physical blob storage while retaining separate source bindings.

Native archive moves and copied files may create another source path. Keep their collected session relation explicit and avoid creating a second visible session when the adapter can prove an identical copy. Divergent copies remain visible as separate source variants; v1 does not merge competing conversation edits.

## Missing files and deletion

Local deletion, provider cleanup, or an offline source does not delete server history. Mark source availability through sync status. Server deletion is an explicit authorized operation. A suppression receipt prevents the same source binding from being recreated by ordinary collection; the local original is untouched.

## Retry and limits

Retry connection errors, timeouts, `429`, and appropriate `5xx` responses with bounded exponential backoff, jitter, and `Retry-After`. Authentication failures pause the affected source and request sign-in. Permanent validation errors are shown with file-level detail and do not block unrelated sources.

The proposed v1 defaults are at most 128 files per import, a 2 MiB manifest, and 256 MiB per file. Operators can configure lower organizational limits. Limits must be advertised by server metadata and checked before transfer. Large-file/multipart support is a later compatibility decision, not a reason to drop a file silently.

`sessiondb sync status` reports last successful acceptance, pending files/bytes, paused sources, and bounded errors. Server parsing/indexing state is reported separately. It must be possible to distinguish an offline laptop, a failed upload, an unsupported native format, and embedding backlog.

See [the API contract](api.md), [CLI behavior](cli.md), and [acceptance scenarios](roadmap.md#acceptance-scenarios).
