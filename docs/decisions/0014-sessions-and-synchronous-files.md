# Sessions-first catalog and synchronous files

Historical boundary: [ADR 0015](0015-strict-native-records-and-simple-reads.md) supersedes the permissive archival, parser-diagnostic, and conditional-read assumptions below. Other native semantics and per-file write-CAS/recovery decisions remain in force.

Status: accepted by the user. Date: 2026-11-16.

## Decision

Explain the domain through sessions, messages, structural events, and artifacts first. Sources describe client native-store provenance; projects are optional labels. File generations preserve original rewrites, and object rows describe immutable S3 bytes. Use explicit relational metadata and one full typed content value, not duplicate resource-document JSON.

Remove source pause state, project archive state, per-transcript MIME, object lifecycle state, import batches, and durable jobs. Collection start/stop belongs to the client. Transcript bytes are JSONL; binary artifacts retain MIME types. Objects store key, digest, length, and creation time once, referenced by originals/artifacts.

The user chose API-mediated synchronous per-file ingestion: `POST /v1/sources/{source_id}/files` receives ordered multipart `metadata` (FileUpload JSON) and `file` (raw JSONL). Required `expected_version` is explicitly null for a new path. HTTP 200 FileUploadResult means verified originals/artifacts, parsing, and one-file catalog publication are complete. Unsupported originals are still archived with failed/partial parse status and bounded diagnostics; normalized sessions can be absent.

Identical accepted size/digest returns the current receipt with `unchanged: true`, even when a lost-response retry has the preceding version token. Otherwise stale versions return 409 without replacing accepted state. Verified appends retain generation identity and manual metadata; rewrites retain previous generations.

Clients persist pending snapshots and checkpoint every successful file separately. The user explicitly accepted loss of all-or-nothing multi-file behavior. No signed upload targets, batch completion, wait-job methods, queue leases, upload TTLs, or legacy aliases remain. The only upload limit is `max_file_bytes`, default 256 MiB. Clients need only the API endpoint and instance key; S3 is server-only.

Prepare and upload outside the publication transaction. Record fresh never-reused object keys before S3 writes, then recheck optimistic versions and atomically commit catalog state. Age-based orphan cleanup removes unreferenced metadata transactionally and S3 bytes afterward; foreign keys protect publication. No network I/O spans a database transaction.

## Superseded assumptions

This supersedes ingestion/job/worker assumptions in ADR 0004, signed staging/finalization assumptions in ADR 0009, the operation counts/protocol in ADR 0012 and 0013, and index-job workflow assumptions in ADR 0005. Their rationale and observed checks remain historical evidence. The active baseline has 15 operations plus two artifact reads (17 implemented).

Retain one shared key, native originals, branches, subagents, inherited history, unknown records, artifacts, ranges, native ZIP export, automatic recovery, and backup/restore. Search, embeddings, project administration, and changes remain honestly planned, without retaining an index-job API as a shim.

## Consequences and alternatives

Synchronous requests are simpler to explain and recover: a receipt is acceptance, not merely queued work. Long uploads/parses occupy an API request; a disconnect requires retrying a saved snapshot with a fresh reader. Per-file acceptance is not a distributed transaction across files or PostgreSQL/S3. Direct signed S3 uploads and asynchronous import queues were considered and deliberately rejected for this design.

The project is undeployed. Edit the single initial migration and use clean current client state; no deployment upgrades or migration compatibility layer are required. Historical validation does not validate this revision. Run integrated checks before adding new observed results.
