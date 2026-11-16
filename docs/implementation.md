# Implementation status

SessionDB has an initial server, CLI, and async client SDK. It implements the [first-build contract](first-build.md), plus all three native adapters, polling watch mode, and authorized inline-artifact reads.

## Available

- PostgreSQL catalog with explicit migrations and one-time initial-owner bootstrap.
- S3-compatible original storage using signed staging uploads and immutable verified acceptance.
- OIDC browser login with PKCE, OS credential storage, refresh, and an API-audience JWT profile.
- Private and organization-visible sessions, membership invitations, and consistent access checks.
- Source registration, one-shot sync, polling watch, persistent client checkpoints, and pending-import recovery.
- Codex, Claude Code, and Pi adapters with native contract-fixture tests.
- Session/message reads, historical branch selection, typed full-content expansion, inline artifacts, and native ZIP export.
- Durable PostgreSQL jobs, expired-lease recovery, readiness checks, request IDs, and optional aggregate request metrics.
- CLI JSON output, contexts, shell completions, and a reusable typed client.

The 23 operations marked `x-first-build` in [OpenAPI](openapi.yaml) are implemented. Two artifact reads are also implemented; `x-implemented` identifies the running API subset. `/v1/info` reports the server's actual capabilities.

## Planned

Keyword search, embedding profiles/providers, hybrid retrieval, project administration and matching, title/tag/sharing mutations, team grants and administration, member-issued API tokens, change feeds, normalized exports, service installation, multipart transfer, and published release artifacts remain delivery milestones.

The target-v1 specification includes these interfaces. An unavailable route returns a `501 not_implemented` problem; unsupported parameters return a documented input error. The CLI's help lists available commands.

## Compatibility and validation

Native compatibility is bounded by the [fixture families](../fixtures/README.md). Unsupported files remain archived and retrievable with visible processing errors. Historical views and related native files are available; exact model-request reconstruction and native harness resume are not implemented.

The integration suite runs real PostgreSQL/S3 storage and a synthetic OIDC issuer against the binaries and SDK. [First-build validation](validation/first-build.md) records the checked paths; [development](development.md) documents how to repeat them. Production sizing, additional provider matrices, and release-platform testing remain later work.
