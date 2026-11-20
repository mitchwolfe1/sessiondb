# Implementation status

SessionDB has a server, CLI, and async SDK for authenticated per-file sync, original storage, session reads, artifacts, and native export across Codex, Claude Code, and Pi.

## Available

- Eleven sessions-first domain tables originating in the immutable initial PostgreSQL migration, with forward deployed-data repairs.
- API-mediated synchronous multipart upload: verify and fully normalize one file before HTTP 200, reject unsupported/malformed input as a whole, and safely recover lost responses.
- S3 immutable originals/artifacts and age-based unreferenced-object cleanup.
- Required nonempty instance key on every `/v1` operation, including info; equal full access for all correct-key clients.
- Optional OS credential storage, contexts, CLI JSON, and shell completions.
- Source registration, one-shot and polling watch sync, persistent per-file snapshots/checkpoints, and `pending_files` recovery.
- Native branch/subagent/inherited-history handling, typed content expansion, artifact reads, ranges, and native ZIP exports.
- Readiness, request IDs, and optional aggregate request metrics.

[OpenAPI](openapi.yaml) marks **15 baseline operations** with `x-first-build` and **17 implemented operations**, including the two artifact reads, with `x-implemented`. `/v1/info` advertises actual operation capabilities.

## Planned

Keyword search, embedding profiles/providers, hybrid retrieval, project administration/matching, metadata mutation, change feeds, normalized/STS exports, service installation, and published release artifacts remain delivery milestones. Their retained target-v1 contracts are explicitly marked unimplemented. Removed import/job/index-job APIs are not future compatibility routes.

An unavailable planned route returns `501 not_implemented`; unsupported parameters return input errors. CLI help lists available commands. Source pause, project archive, transcript MIME, upload TTLs, and generic workers are not part of the current design.

## Compatibility and validation

Only supported, valid native files are accepted. Any unsupported or malformed supplied record rejects the whole file with HTTP 400 before storage/catalog mutation. Native compatibility is bounded by the [fixture families](../fixtures/README.md), not a guarantee of exact model requests or harness resume.

[Validation](validation/first-build.md) distinguishes observed checks for each revision from historical results. Native compatibility remains bounded; run the [development checks](development.md#checks) before claiming a new format or deployed upgrade works.
