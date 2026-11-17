# Implementation status

SessionDB has an initial server, CLI, and async client SDK. It implements the [first-build contract](first-build.md), plus all three native adapters, polling watch mode, and authorized inline-artifact reads.

## Available

- PostgreSQL catalog with explicit migrations and a forward instance-key cutover migration.
- S3-compatible original storage using signed staging uploads and immutable verified acceptance.
- Required nonempty `SESSIONDB_API_KEY`, bearer-key verification on all `/v1` APIs including `/v1/info`, and equal full collection access for every correct-key client.
- Optional per-server OS credential storage via `auth login --key-stdin`; no browser login, network token exchange, identity model, or scoped credentials.
- Source registration, one-shot sync, polling watch, persistent client checkpoints, and pending-import recovery.
- Codex, Claude Code, and Pi adapters with native contract-fixture tests.
- Session/message reads, historical branch selection, typed full-content expansion, inline artifacts, and native ZIP export.
- Durable PostgreSQL jobs, expired-lease recovery, readiness checks, request IDs, and optional aggregate request metrics.
- CLI JSON output, contexts, shell completions, and a reusable typed client.

The 18 operations marked `x-first-build` in [OpenAPI](openapi.yaml) are implemented. Two artifact reads are also implemented; `x-implemented` identifies the running API subset. `/v1/info` reports the server's actual capabilities.

## Planned

Keyword search, embedding profiles/providers, hybrid retrieval, project administration and matching, title/tag mutations, change feeds, normalized exports, service installation, multipart transfer, and published release artifacts remain delivery milestones.

The target-v1 specification includes these interfaces. An unavailable route returns a `501 not_implemented` problem; unsupported parameters return a documented input error. The CLI's help lists available commands.

## Compatibility and validation

Native compatibility is bounded by the [fixture families](../fixtures/README.md). Unsupported files remain archived and retrievable with visible processing errors. Historical views and related native files are available; exact model-request reconstruction and native harness resume are not implemented.

The integration suite uses real PostgreSQL/S3 storage against the binaries and SDK. [First-build validation](validation/first-build.md) records historical checks from the former OIDC build; they are not evidence that the instance-key cutover has passed. Cutover checks remain to be run by the integrating agent; no new tests were run for this documentation update. [Development](development.md) documents how to run the checks. Production sizing, additional provider matrices, and release-platform testing remain later work.
