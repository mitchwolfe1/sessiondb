# First implementation validation

The initial implementation was exercised on 2026-11-16 on Linux x86-64 with the pinned Rust 1.94.1 toolchain. Tests used synthetic identities/transcripts and disposable local services. No personal session store or model API was used.

## Native formats

[Adapter conformance](../../crates/adapters/tests/fixtures.rs) checks all ten cases in the native fixture pack, including JSON/text tool inputs, large integers, Claude fragment coalescing, subagents, Pi branches/context edits, compaction, artifacts, inherited history, and malformed/incomplete capture.

## Complete storage and client path

The [Docker integration test](../../crates/cli/tests/end_to_end.rs) runs the real server and CLI binaries with PostgreSQL 18.4, S3-compatible storage, and a synthetic signed OIDC issuer. The SDK uses the same endpoints as the CLI.

Passing scenarios include:

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

## Reference provider and container

The SDK login example also completed a real Chromium login against Keycloak 26.8.2 using the local development profile. Login registered the synthetic initial owner, and refresh succeeded with the implemented ID-token checks. This check used an isolated browser session; credentials were not written into the client configuration.

The server container built in release mode and ran as its non-root user. Its readiness and capability endpoints succeeded against the local PostgreSQL/S3/OIDC services, and the same browser login worked against that container.

## Repeat the checks

[Development](../development.md#checks) lists formatting, Clippy, unit/fixture, integration, documentation, and contract checks. [CI](../../.github/workflows/ci.yml) runs them on changes. Production capacity, additional providers, and release-platform packaging require their own validation.
