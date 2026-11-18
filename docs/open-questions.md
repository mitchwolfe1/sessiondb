# Implementation decisions and follow-ups

The decisions needed for the [first build](first-build.md) are settled. The reference stack has passed compatibility checks, the content model has typed schemas/fixtures, and the selected API subset is identified in OpenAPI.

## Resolved for the first build

| ID | Decision | Resolution |
| --- | --- | --- |
| Q01 | Dependency set and compiler baseline | [Tested library set and Rust 1.94.1](validation/reference-stack.md); the application toolchain and lockfile are committed. |
| Q02 | Authentication | [Required instance key](authentication.md), shared full access, no identity provider. |
| Q03 | Storage and ingestion | [ADR 0014](decisions/0014-sessions-and-synchronous-files.md): server-only S3 and synchronous per-file API upload. |
| Q04 | HTTP surface | [15 baseline plus two artifact reads](first-build.md), explicit capabilities; no import/job/index-job APIs. |
| Q05 | Limits and retention | Only max_file_bytes, default 256 MiB; periodic orphan cleanup and no automatic accepted-original expiry. No upload TTL/lease. |
| Q08 | Native normalization fixtures | [Ten synthetic cases](../fixtures/README.md), with virtual native paths and schema-checked expectations. Actual adapter conformance is implementation work. |
| Q09 | History, leaf, and exports | [Historical message view](normalized-content.md), explicit ancestry and inherited relations, native bundle first. |
| Q11 | Collection versus classification | Native stores collect independently of cwd; labels are optional and sync preserves manual edits. Optional matching is a later collection feature. |
| Q12 | Sessions-first schema | Eleven domain tables in one initial migration; scalar metadata, one content value, optional projects, client-owned collection start/stop. |

## Later milestone work

| ID | Follow-up | When |
| --- | --- | --- |
| Q06 | Implement and exercise the configured embedding endpoint contract | Semantic-search milestone; provider/model is runtime configuration rather than a prerequisite for collection. |
| Q07 | Verify binary targets, SDK publication, and license/distribution policy | Release preparation. Private implementation does not require an open-source licensing decision. |
| Q10 | Benchmark and tune ranking, capacity, and optional ANN indexes | After a working query/index path; initial keyword/exact-vector/RRF defaults are already specified. |

These follow-ups do not block the implemented storage-to-CLI path. Update the relevant contract and acceptance case when implementation feedback changes a decision.
