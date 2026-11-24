# Implementation status

SessionDB has a server, CLI, and async SDK for authenticated per-file sync, original storage, session reads, keyword search, artifacts, and native export across Codex, Claude Code, and Pi.

## Available

- Eleven sessions-first domain tables originating in the immutable initial PostgreSQL migration, with forward deployed-data repairs.
- API-mediated synchronous multipart upload: verify and fully normalize one file before HTTP 200, reject unsupported/malformed input as a whole, and safely recover lost responses.
- S3 immutable originals/artifacts and age-based unreferenced-object cleanup.
- Required nonempty instance key on every `/v1` operation, including info; equal full access for all correct-key clients.
- Verified URL-based connections with per-server keys in a private client config, environment overrides, and CLI JSON.
- Source registration, one-shot and polling watch sync, Linux systemd and macOS launchd user-service management, persistent rejection hashes, per-file snapshots/checkpoints, and `pending_files` recovery. A sync pass reports individual native-file rejections and continues collecting other files.
- Native branch/subagent/inherited-history handling, typed content expansion, artifact reads, ranges, and native ZIP exports.
- Codex inline message and tool-result images, reasoning configuration updates, and web page actions. Claude session and request metadata and tool-reference results.
- Archive-wide PostgreSQL keyword search through the API, SDK and CLI, with compact source excerpts, filters, pagination and branch-aware reads around the matching message. Forward migrations 0004 and 0005 backfill accepted history and pack oversized blocks into whole-word passages.
- Readiness, request IDs, and optional aggregate request metrics.

[OpenAPI](openapi.yaml) marks **15 baseline operations** with `x-first-build` and **19 implemented operations**, including artifact reads, keyword search and anchored context, with `x-implemented`. `/v1/info` advertises actual operation capabilities.

## Planned

Full [replacement of hype-history](decisions/0022-replace-hype-history.md) is a product goal. Keyword search and anchored reads are available. Timelines, on-demand briefings, cross-session research and migration of saved evidence remain planned.

Embedding profiles/providers, hybrid retrieval, project administration/matching, metadata mutation, change feeds, normalized/STS exports and published release artifacts remain delivery milestones. Their retained target-v1 contracts are explicitly marked unimplemented. Removed import/job/index-job APIs are not future compatibility routes.

An unavailable planned route returns `501 not_implemented`; unsupported parameters return input errors. CLI help lists available commands. Source pause, project archive, transcript MIME, upload TTLs, and generic workers are not part of the current design.

## Compatibility and validation

Migration 0004 was verified on fresh catalogs and on an isolated upgrade containing previously accepted Codex, Claude and Pi sessions. The CLI opened the original source anchors after backfill. The [browser pilot](../dev/evals/browser/README.md#native-sessiondb-pilot) records the task outcomes and the separate frozen-query replay.

Only supported, valid native files are accepted. Any unsupported or malformed supplied record rejects the whole file with HTTP 400 before storage/catalog mutation. Native compatibility is bounded by the [fixture families](../fixtures/README.md), not a guarantee of exact model requests or harness resume.

[Validation](validation/first-build.md) distinguishes observed checks for each revision from historical results. Native compatibility remains bounded; run the [development checks](development.md#checks) before claiming a new format or deployed upgrade works.
