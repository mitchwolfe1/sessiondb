# SessionDB contributor instructions

## Product direction

SessionDB is a database for coding-agent sessions. Lead with storing, automatically syncing, accessing, querying, and searching Codex, Claude Code, and Pi sessions through conventional CLI/API/SDK interfaces, with embedding extensibility.

Lead with capabilities and usage, as database and developer-tool documentation does. Every client with the instance API key has identical full access to one shared collection. There are no users, organizations, scoped credentials, or per-session permissions. Put key management and deployment details in the relevant reference pages. Explain sessions and useful workflows before credential configuration or internal storage bookkeeping.

[ADR 0015](docs/decisions/0015-strict-native-records-and-simple-reads.md) requires strict whole-file native validation before storage/publication. Only accepted valid originals are preserved; unsupported/malformed records reject with HTTP 400. Reads carry no revision, parser-status, or warning metadata and no snapshot guarantees. Upload write-CAS remains separate; per-file recovery is unchanged.

Use established HTTP, CLI, PostgreSQL, and S3 conventions. SessionDB exposes session resources through its API and uses an existing object storage implementation for raw files. Do not implement a new S3 server or invent a SQL language for the CLI.

[ADR 0016](docs/decisions/0016-http-private-networks.md) permits HTTP and HTTPS server origins in the server, CLI, and SDK without loopback-only checks or extra insecure-mode flags. Operators provide network protection for HTTP; the instance key remains required and HTTPS certificate validation stays enabled.

[ADR 0017](docs/decisions/0017-fixed-secret-environment-variables.md) fixes credential names to `SESSIONDB_DATABASE_URL`, `SESSIONDB_API_KEY`, `SESSIONDB_S3_ACCESS_KEY_ID`, and `SESSIONDB_S3_SECRET_ACCESS_KEY`. Do not add configurable secret-variable names or `[database]`/`[auth]` wrapper sections. Set both static S3 credentials or leave both unset for the standard AWS credential chain.

Collection follows configured native session stores, not repository/workspace containment. Starting cwd is metadata. Sessions can have zero or more logical project associations, and changing these labels does not change instance access. Keep collection and project association separate in the docs and contracts.

The accepted schema/ingestion direction is [ADR 0014](docs/decisions/0014-sessions-and-synchronous-files.md): sessions first, explicit relational metadata, no duplicate document JSON, optional projects, provenance-only sources, client-owned start/stop, and synchronous API-mediated per-file upload. Preserve native originals, generations, branches, subagents, artifacts, export, and restart-safe per-file recovery. Do not restore import/job/index-job/pause/archive machinery, signed client uploads, worker settings, or transcript MIME. Each successful file is individually accepted; no atomic multi-file guarantee. Athena is deployed: keep `0001_catalog.sql` immutable and use forward migrations for all subsequent schema/data repairs, preserving accepted history and client checkpoints. See [ADR 0018](docs/decisions/0018-deployed-native-compatibility.md).

[ADR 0019](docs/decisions/0019-native-json-payloads.md) requires PostgreSQL `json` and the `NativeJson` binder for opaque message/provider/event payloads. SQLx's default `Json` binding uses JSONB, which rejects valid native `\u0000` strings. Preserve full content; do not scrub it or cast payloads back to JSONB. Derived display titles may normalize control characters without changing original messages.

## Implementation language

Use Rust for the server, CLI, sync agent, and first client SDK. Document language, crate, and toolchain details in architecture, development, and SDK references. Product descriptions lead with capabilities rather than implementation language.

## Current phase

This project follows documentation-first development. The user has authorized implementation. Keep the working server, CLI, SDK, migrations, and tests consistent with the accepted first-build contract. Distinguish implemented behavior from the remaining target-v1 specification; update `docs/implementation.md` as capabilities ship.

Read `docs/README.md`, the relevant feature documents, and applicable decision records before editing. Product decisions and implementation proposals have different statuses. Keep the decision register, API descriptions, CLI examples, and acceptance criteria consistent.

Do not treat a list of open questions as permission to stop documentation work. Make concrete proposals where useful and record unresolved choices with their consequences and the milestone they block.

## Changes and validation

- Use Conventional Commits: `docs:`, `feat:`, `fix:`, `refactor:`, `test:`, `build:`, `ci:`, `perf:`, `chore:`, or `revert:` as appropriate.
- Keep commits focused and describe the final change. Do not include real session data, credentials, private source contents, or machine-specific state.
- Use synthetic examples. Preserve native source bytes in the design and distinguish recorded user roles from human authorship.
- Check relative links, document consistency, example syntax, and whitespace. Validate machine-readable contracts when introduced.
- Use the workspace conventions in `docs/architecture.md` and `CONTRIBUTING.md`. Keep the toolchain and dependency lockfile checked in and run the relevant checks for implementation changes.

## Boundaries

SessionDB is the planned replacement for hype-history under [ADR 0022](docs/decisions/0022-replace-hype-history.md). SessionDB owns collection, retrieval, on-demand briefings, cross-session research and reviewed-memory workflows. Use SessionDB for agent history lookups, including preference research. The preserved implementation is available for explicitly requested migration and evaluation work. Keep evidence provenance and review decisions explicit.

Require the nonempty `SESSIONDB_API_KEY` at server startup and bearer-key authentication for all `/v1` operations, including `/v1/info`. Only health, readiness, and the static contract are public. The optional collecting member and source/device/harness identifiers describe provenance under [ADR 0024](docs/decisions/0024-team-history.md); project filters classify content, never restrict credential access. Follow ADR 0013 for the clean cutover; do not restore identity routes or compatibility modes.
