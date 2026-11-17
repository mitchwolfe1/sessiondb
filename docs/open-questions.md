# Implementation decisions and follow-ups

The decisions needed for the [first build](first-build.md) are settled. The reference stack has passed compatibility checks, the content model has typed schemas/fixtures, and the selected API subset is identified in OpenAPI.

## Resolved for the first build

| ID | Decision | Resolution |
| --- | --- | --- |
| Q01 | Dependency set and compiler baseline | [Tested library set and Rust 1.94.1](validation/reference-stack.md); the application toolchain and lockfile are committed. |
| Q02 | Authentication and client credentials | [One required instance API key](authentication.md), bearer authentication on all `/v1` APIs, no identity provider or account bootstrap; cutover validation remains to be run. |
| Q03 | S3 implementation/client | [SeaweedFS 4.54 and aws-sdk-s3 1.159.0](decisions/0009-reference-storage-and-dependencies.md), with signed staging, immutable promotion, and verified bytes. |
| Q04 | Initial HTTP surface | [18 first-build operations](first-build.md), typed API draft 0.3.0, and explicit capability reporting. |
| Q05 | Initial lease/retention/limits | [Sync defaults](sync.md): 24-hour upload lease, 15-minute renewed URLs, 30-day idempotency, 24-hour cleanup grace, 256 MiB files, and no automatic original expiry. |
| Q08 | Native normalization fixtures | [Ten synthetic cases](../fixtures/README.md), with virtual native paths and schema-checked expectations. Actual adapter conformance is implementation work. |
| Q09 | History, leaf, and exports | [Historical message view](normalized-content.md), explicit ancestry and inherited relations, native bundle first. |
| Q11 | Collection versus classification | Native stores collect independently of cwd; labels are optional and sync preserves manual edits. Optional matching is a later collection feature. |

## Later milestone work

| ID | Follow-up | When |
| --- | --- | --- |
| Q06 | Implement and exercise the configured embedding endpoint contract | Semantic-search milestone; provider/model is runtime configuration rather than a prerequisite for collection. |
| Q07 | Verify binary targets, SDK publication, and license/distribution policy | Release preparation. Private implementation does not require an open-source licensing decision. |
| Q10 | Benchmark and tune ranking, capacity, and optional ANN indexes | After a working query/index path; initial keyword/exact-vector/RRF defaults are already specified. |

These follow-ups do not block the implemented storage-to-CLI path. Update the relevant contract and acceptance case when implementation feedback changes a decision.
