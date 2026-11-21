# One instance-wide API key

[ADR 0021](0021-connect-stores-client-keys.md) supersedes the OS credential-store workflow described below.

[ADR 0020](0020-connect-by-url.md) replaces the named CLI context examples with direct URL selection.

Status: key boundary accepted; catalog/count/object-URL assumptions below superseded by [0014](0014-sessions-and-synchronous-files.md), HTTPS-only transport by [0016](0016-http-private-networks.md). Date: 2026-11-16.

## Decision

Use a Redis-simple shared-secret boundary: the server requires a nonempty `SESSIONDB_API_KEY`, and clients send it as `Authorization: Bearer <key>`. Every correct-key client has identical full access to one instance collection. Verify the key with a constant-time comparison and never log or display it. Use HTTPS remotely; loopback HTTP remains a development option.

Protect every `/v1` API, including `/v1/info`. Only health, readiness, and static `/openapi.yaml` are public. Remove identity/login/admin routes and schemas entirely: no users, organizations, owners, memberships, teams, invitations, visibility, grants, scoped credentials, browser login, refresh, or optional legacy mode. Resources use global `/v1/...` paths. Source/device/harness IDs remain provenance; project labels remain optional classification.

The SDK constructor is `Client::new(server: &str, api_key: String, timeout: Duration)`, with a required nonempty static key and no organization arguments or token-provider interface. CLI contexts store only server URLs. Environment credentials suffice for all client operations; `auth login --key-stdin`, logout, and status are OS credential-store conveniences, not network credential issuance. Never forward the key to signed object URLs.

## Superseded decisions

This replaces the identity/access assumptions in 0001, 0004, 0005, 0006, 0007, 0008, and 0010, and the identity-operation count in 0012. Their historical rationale and test observations remain historical evidence, not active product requirements. Source-based collection, optional project labels, original-byte preservation, typed content, storage, and non-auth planned features remain in force.

Historical cutover count: 18 baseline plus two artifact reads, after removing five identity operations from 23. ADR 0014 now selects 15 baseline plus two artifact reads. Planned search, embeddings, projects, and changes retain the same key boundary.

## Initial schema and consequences

SessionDB is undeployed, so define the catalog directly in `0001_catalog.sql` with no identity conversion. The import/job table assumption from this cutover is superseded: ADR 0014's sessions-first migration is the active schema.

Disposable development databases initialized with the earlier migration history must be recreated. SQLx rejects their obsolete migration versions/checksums; do not bypass those checks or carry the abandoned identity schema forward.

One key cannot provide partial/read-only access, per-client revocation, or human attribution. Use separate instances for independent collections. Rotation replaces the server key and intended clients' copies. Signed object URLs from the previous protocol are no longer issued.

## Validation boundary

Earlier OIDC/Keycloak checks in the validation records prove only the superseded implementation/protocol. Integrated validation covers fresh catalog initialization, missing/empty server keys, incorrect/missing request keys on all data routes and `/v1/info`, equal access across correct-key clients, global URLs, client state recovery, and withholding bearer secrets from object requests. See [first-build validation](../validation/first-build.md) for recorded results.
