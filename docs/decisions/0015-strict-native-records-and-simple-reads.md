# 0015: Strict native records and simple reads

Status: accepted by the user; supersedes the permissive archival and conditional-read portions of ADRs 0011 and 0014.

## Decision

Accept a native file only when every supplied record is supported and valid. Parse the complete supplied snapshot before object registration, S3 writes, or catalog publication. Malformed JSON, invalid record structures, unsupported record/content kinds, invalid JSON function arguments, and unsupported inline artifacts reject the entire request with HTTP 400 `unsupported_native_record`. Return bounded safe reasons/record positions, never raw transcript contents. Preserve previous accepted file, generation, session, message, artifact, and checkpoint state on rejection. Store originals only for accepted valid files; there is no archive-invalid fallback or partial projection.

Keep supported native metadata/event envelopes, branches, subagents, artifacts, and structural references. Missing parent/call history remains an explicit unresolved link, not invented context or automatic rejection. Unknown tool outcome is legitimate metadata, not an unsupported native type. Known free-form custom-tool inputs remain text. CLI capture of active files still defers incomplete tails before upload; direct malformed/incomplete file input rejects.

Remove `revision`, `parse_status`, `parse_error`, and `warnings` from their former session/message/source-file/content wire and catalog locations, and remove diagnostic warnings from native manifests and future responses. Reads are simple current-state reads: no message/content ETags, If-Match, read revision parameters, stale-page revision guarantees, or CLI revision flag. Pagination still validates route/filter scope but provides no snapshot guarantee while data changes. SDK tolerance of future wire variants does not permit adapters to accept unsupported native records as opaque blocks.

Upload `expected_version` and SourceFile `version` remain the separate write-CAS/retry protocol. Stable identities, bounded previews/full expansion, immutable accepted originals/rewrite generations, synchronous per-file publication, shared key, and restart-safe independent file checkpoints remain unchanged. Fifteen baseline operations plus two artifact reads remain implemented. This package is undeployed: edit its one initial migration without compatibility aliases or upgrade shims.

## Consequences and validation boundary

New native formats require explicit support before acceptance; rejection replaces warning-based degradation. Preserve malformed/unknown raw fixtures as negative evidence and add a valid large-integer precision fixture. Negative expectations use `{case,error:"unsupported_native_record"}`; successful expectations contain sessions.

Earlier validation results remain historical evidence only. Integrated checks for strict rejection before mutation, upload recovery/CAS, native semantics, fixture/schema conformance, and simple reads are pending; this decision records no executed checks.
