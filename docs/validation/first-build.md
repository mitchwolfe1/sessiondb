# First implementation validation

## Client connections

Terminal verification exercised first-use hidden input, saved-key reuse and replacement of a rejected key. The CLI integration checks piped credentials, environment precedence, origin isolation, and redaction in both human and JSON output. Private config writes and parse-error redaction have regression coverage. The login-service integration authenticates from its saved config across restarts and a change of CLI default.

The CLI integration verifies that `connect` saves a verified server URL, a supplied URL takes precedence over an environment override for that command, and a rejected key preserves the previous config bytes. The Linux login-service test loads an older named config, switches the default between two loopback servers, and verifies that the running and restarted watcher retains its original destination and source roots. Config and local-state tests cover old registration receipts and the saved watch heartbeat.

## Background sync

The Linux user-service integration exercises installation, login enablement, status, stop/start, configuration reload, reinstallation and removal against disposable PostgreSQL and S3 services. It verifies that unchanged rejected files stay cached across service restarts, an explicit retry resubmits them, and repaired and newly discovered files are accepted. Scanner regressions cover server-version changes, matching remote data, local file replacement, and remote catalog restoration. Portable tests exercise launchd argument encoding, lifecycle commands, and rollback with a simulated manager. Live macOS login behavior still requires a macOS host.

## Per-file rejection recovery and native compatibility

A Docker integration test recovers a mixed queue after an object-storage outage. The CLI reports each rejected path once, exits nonzero, accepts both saved and newly discovered valid files, and accepts a repaired file on a later pass.

A separate PostgreSQL/S3 test uploads inline Codex images in a message and a tool result, reads the normalized content and both distinct artifacts through the API, and verifies the original bytes. Adapter tests cover the supported configuration, communication, page action, Claude metadata, and tool-reference records, including malformed variants. These changes use the existing catalog migrations.

A synthetic Claude append with repeated message UUIDs reproduced a duplicate message primary key on the earlier server. The regression verifies stable IDs for the original prefix, distinct IDs and source anchors for replays, both branches and image references, and byte-identical reads from the original-content API. The CLI recovery test also forces an isolated per-file catalog error, verifies that other files are accepted, and retries the saved snapshot after the catalog error clears.

## Native compatibility and deployed title repair

Synthetic regressions cover observed Codex web searches/tool definitions/world-state/usage records and Claude mode, permission, attachment, and title records. Unknown or malformed native kinds still reject the whole file. No personal transcript content was added to repository fixtures.

- All 33 workspace tests pass. Workspace Clippy passes with warnings denied, and the workspace binary build passes.
- Both real PostgreSQL/S3 integration tests pass. Fresh catalog creation and upgrade from migration 0001 verify automatic instruction-title repair, preserved manual titles, unchanged message IDs/text/native roles/tags, genuine marker-like human text, mixed blocks, blank text, instruction-only sessions, and safe derived titles without altering original control characters.
- Existing storage, authentication, branch/artifact/native-export, atomic rejection, per-file recovery, backup/restore, and HTTP public-URL checks pass. CLI upload failures identify the source-relative filename.
- Contract/link validation and Markdown lint pass.
- Inspecting the concurrent Pi pending-upload window identified a NUL-bearing tool result. A minimal synthetic native upload reproduced HTTP 500 and PostgreSQL's `unsupported Unicode escape sequence` error before the fix. Migration `0003_native_json_payloads.sql` and the native JSON parameter codec now pass exact tool-text preview/full-content, provider-usage, event-data, and original-byte round trips. An unsupported overwrite still returns HTTP 400 and leaves the accepted NUL-bearing data intact. No personal transcript was used as a committed fixture, and the live Athena deployment has not been upgraded by these checks.

## Fixed credential environment variables

Server secrets now use fixed environment names. The `[database]`/`[auth]` sections and all secret-variable-name options have been removed from server configuration, examples, development tooling, and integration setup.

- Clippy passes for the affected server/CLI packages with warnings denied, and all six affected-package tests pass.
- Isolated-process cases cover missing/empty API keys, fixed S3 credentials, incomplete/empty S3 pairs, and leaving both S3 variables unset for the standard credential chain.
- The real PostgreSQL/S3 integration passes with canonical database, API-key, and S3 environment variables and no secret-reference TOML fields, including migrations, sync, authentication, originals, recovery, and backup/restore.
- Contract/example validation and Markdown lint pass. The development launcher passes shell syntax validation and the Compose profile renders with the canonical S3 names. The development services were not started by this check, and no real cloud-role credentials were exercised.

## Private-network HTTP

HTTP and HTTPS origins are now accepted by the server, SDK, and CLI without a loopback-only restriction. The API key remains required.

- Clippy passes for the three affected packages with warnings denied, and all seven affected-package tests pass.
- The SDK regression makes an authenticated HTTP request to an `athena` hostname resolved to an isolated local listener.
- The PostgreSQL/S3 integration passes with both HTTPS and `http://athena:6767` advertised URLs, verifying startup, regenerated content URLs, full-content reads, and the existing sync/authentication/recovery paths.
- The actual CLI accepts the Athena HTTP URL and attempts a connection. That endpoint returned connection refused; the live Athena deployment itself was not verified.
- OpenAPI/examples/fixture/link validation passes, and Markdown lint passes for 45 files.

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
