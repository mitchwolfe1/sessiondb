# First implementation validation

## Strict-records cutover

[ADR 0015](../decisions/0015-strict-native-records-and-simple-reads.md) supersedes the archival-failure, partial-normalization, parser-warning, and read-revision behavior in the historical sections below.

Verified locally using synthetic transcripts and disposable PostgreSQL/S3 services; no personal session store or model API was used:

- Workspace Clippy passes with warnings denied. All 21 unit/fixture tests pass, with the separate Docker integration test also passing.
- New-file and overwrite requests containing unsupported or malformed records return HTTP 400 `unsupported_native_record`. Previously accepted versions, generations, original bytes, and messages remain unchanged; rejected paths are not published.
- Valid large integers, all three harnesses, branch ancestry, tools, artifacts, related native exports, full-content expansion, append/rewrite identity, and manual metadata remain covered.
- Two pending CLI files recover after an object-service outage. A confirmed unsupported-file rejection does not advance its checkpoint or trap subsequent sync: repairing the local file succeeds without resetting state.
- Upload write-CAS, concurrent identical uploads, complete-prefix capture, watch mode, SIGINT, restart, backup/restore, changed public URLs, and instance-key rotation pass.
- Contract validation passes for 31 target operations, 17 implemented operations, 12 API examples, 11 native cases, 13 native files, 35 expected messages, and 255 documentation links.
- Markdown lint passes for 44 files. The actual message-content and session-message CLI help were inspected; no revision flag remains.

Production capacity, release-container builds, additional platforms, and broader native-version compatibility were not checked for this cutover. Everything below is historical evidence, not proof of current behavior.

Historical record: the pre-cutover OIDC implementation was exercised on 2026-11-16 on Linux x86-64 with the pinned Rust 1.94.1 toolchain. Tests used synthetic identities/transcripts and disposable local services. No personal session store or model API was used.

## Sessions-first synchronous revision

[ADR 0014](../decisions/0014-sessions-and-synchronous-files.md) replaces the import/job implementation exercised in the historical sections below. The revised eleven-table catalog and synchronous per-file API were verified with synthetic transcripts and disposable PostgreSQL/S3 services; no personal session store or model API was used.

- Server Clippy passes with warnings denied, and all ten workspace tests pass.
- The real server/CLI/SDK integration passes fresh catalog initialization, concurrent identical uploads, stale-version rejection, corrupt-length/checksum rejection, and rejection of extra multipart parts before publication.
- Unsupported originals remain retrievable with an explicit parse failure; invalid native tool arguments produce partial status without losing structured content or large integers.
- Append and rewrite checks preserve original generations, stable conversation identity, and manually assigned titles, tags, and projects. Branch ancestry, related native exports, artifact MIME types, full-content expansion, and conditional revisions pass.
- Two pending CLI files recover after an object-service outage, including an unsupported transcript. Successful receipts are checkpointed individually; an unchanged subsequent sync uploads neither file. Watch mode, complete-prefix capture, single-sync exclusion, SIGINT, backup/restore, changed public URLs, and key rotation pass.
- API/documentation validation passes for 31 target operations, 17 implemented operations, 12 examples, and 247 links. Markdown lint passes; actual CLI and sync help were inspected and contain no job commands.

Release-container, production-capacity, and additional platform/provider checks were not rerun for this revision.

## Instance-key cutover status

The authentication/authorization observations below belong to the superseded build described by ADR 0010. [ADR 0013](../decisions/0013-instance-api-key.md) removes that protocol and identity model.

The instance-key build passed Clippy with warnings denied and the affected Rust package tests (seven tests passed; Docker integration is separate). The API/documentation checks passed for 37 target operations, 20 implemented operations, 13 API examples, and all documentation links. Markdown lint reported no issues. The simplified development launcher passed shell syntax and Compose configuration checks.

The historical Docker integration scenario passed against the then-built server/CLI with PostgreSQL and S3, without an identity provider:

- Missing, empty, and incorrect keys are rejected; all data surfaces require authentication, and correct-key clients have equal access to sessions, related history, originals, and artifacts.
- Replacing the instance key and restarting rejects previously configured clients while preserving stored data.
- All three native harnesses, immutable acceptance, append/rewrite handling, native exports, large content, CLI watch/outage recovery, expired worker leases, and backup/restore still pass.

The undeployed schema is consolidated in `0001_catalog.sql`; the obsolete catalog-upgrade test has been removed. The rebuilt server passes the integration scenario using fresh catalog initialization followed by storage, sync, and recovery checks. API/documentation contracts and Markdown lint also pass after consolidation.

The actual CLI help and credential-store login help were also inspected. Container release builds and additional platform/provider compatibility were not rerun for this cutover.

## Native formats

[Adapter conformance](../../crates/adapters/tests/fixtures.rs) checks all ten cases in the native fixture pack, including JSON/text tool inputs, large integers, Claude fragment coalescing, subagents, Pi branches/context edits, compaction, artifacts, inherited history, and malformed/incomplete capture.

## Complete storage and client path

Before the cutover, the [Docker integration test](../../crates/cli/tests/end_to_end.rs) ran the real server and CLI binaries with PostgreSQL 18.4, S3-compatible storage, and a synthetic signed OIDC issuer. Its former identity coverage is listed below only as history; the current instance-key results are recorded above.

Historical passing scenarios included:

- Required JWT claims, audience/issuer/signature/expiry rejection, verified-email invitations, and membership revocation.
- SDK Authorization Code/PKCE callback, nonce checks, refresh, and rejection of a changed refreshed identity.
- Signed uploads, idempotent create/completion, atomic rejection of a corrupt batch, and immutable acceptance despite later staging changes.
- Unassigned home-directory sessions, preserved metadata during append, new rewrite generations, and retrieval of preceding originals.
- Reads for all three harnesses, branch ancestry, large typed-content expansion, and conditional revision checks.
- Cross-source parent relationships with separately enforced access, including omission of private dependencies from another member's native export.
- Inline-artifact storage and authorized download, range reads, and exact native ZIP round-trips.
- CLI complete-prefix capture, persistent pending state after an object-service outage, restart recovery, watch updates, single-sync exclusion, and SIGINT exit behavior.
- Recovery of queued verification and an expired worker lease after process replacement.
- API/object-service restart, catalog dump/restore into a clean database, and client resynchronization without duplicate uploads.
- Updated content URLs and representation ETags after changing the public server address.

## Historical reference provider and container

The SDK login example also completed a real Chromium login against Keycloak 26.8.2 using the local development profile. Login registered the synthetic initial owner, and refresh succeeded with the implemented ID-token checks. This check used an isolated browser session; credentials were not written into the client configuration.

The server container built in release mode and ran as its non-root user. Its readiness and capability endpoints succeeded against the local PostgreSQL/S3/OIDC services, and the same browser login worked against that container.

## Repeat the checks

[Development](../development.md#checks) lists formatting, Clippy, unit/fixture, integration, documentation, and contract checks. [CI](../../.github/workflows/ci.yml) runs them on changes. Production capacity, additional providers, and release-platform packaging require their own validation.

## GitHub Actions startup status

At the time of this historical record, local checks passed but GitHub Actions had not executed the hosted jobs. Both push and manual dispatch returned `startup_failure` with zero jobs and no step logs on 2026-11-16. The workflow also passes `actionlint`, and its action references resolve.

The active CI workflow is `420557309`. [A push run](https://github.com/mitchwolfe1/sessiondb/actions/runs/43948795214) instead references workflow `420557352`, whose API record has an empty name, path `BuildFailed`, and state `deleted`. GitHub rejects disabling that record because it is inactive. [Manual execution of the active workflow](https://github.com/mitchwolfe1/sessiondb/actions/runs/43948918264) also creates no jobs.

This resembles [reported GitHub Actions registry/dispatch failures](https://github.com/orgs/community/discussions/206103); GitHub has not supplied a diagnosis for this repository. The local verification commands remain available while hosted execution is unresolved. These run/workflow IDs provide a diagnostic starting point for a private GitHub support request.
