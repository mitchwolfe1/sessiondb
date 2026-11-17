# One instance-wide API key

Status: accepted. Date: 2026-11-16.

## Decision

Use a Redis-simple shared-secret boundary: the server requires a nonempty `SESSIONDB_API_KEY`, and clients send it as `Authorization: Bearer <key>`. Every correct-key client has identical full access to one instance collection. Verify the key with a constant-time comparison and never log or display it. Use HTTPS remotely; loopback HTTP remains a development option.

Protect every `/v1` API, including `/v1/info`. Only health, readiness, and static `/openapi.yaml` are public. Remove identity/login/admin routes and schemas entirely: no users, organizations, owners, memberships, teams, invitations, visibility, grants, scoped credentials, browser login, refresh, or optional legacy mode. Resources use global `/v1/...` paths. Source/device/harness IDs remain provenance; project labels remain optional classification.

The SDK constructor is `Client::new(server: &str, api_key: String, timeout: Duration)`, with a required nonempty static key and no organization arguments or token-provider interface. CLI contexts store only server URLs. Environment credentials suffice for all client operations; `auth login --key-stdin`, logout, and status are OS credential-store conveniences, not network credential issuance. Never forward the key to signed object URLs.

## Superseded decisions

This replaces the identity/access assumptions in 0001, 0004, 0005, 0006, 0007, 0008, and 0010, and the identity-operation count in 0012. Their historical rationale and test observations remain historical evidence, not active product requirements. Source-based collection, optional project labels, original-byte preservation, typed content, storage, and non-auth planned features remain in force.

The first-build set is 18 baseline operations, plus two implemented artifact operations: the previous 23 baseline operations minus authentication configuration, login registration, current-account lookup, organization listing, and invitation creation. Search, embeddings, projects, changes, and other planned non-auth resource APIs retain their implementation status and inherit the same key boundary.

## Migration and consequences

Apply a forward server migration without changing migration 0001. Retain accepted native content, UUIDs, metadata labels, original bytes, source/file generations, project associations, imports, jobs, and sync recovery. Drop obsolete identity tables/columns and remove identity fields from stored session JSON. Retain distinct source UUIDs even when former isolated sources had matching device/root tuples; do not merge or delete them. Existing opaque object keys can remain unchanged.

This is a breaking disclosure change: formerly isolated content becomes one key-accessible collection. Operators must review that effect before migration and retain backups. Clients must upgrade paths/DTOs, remove obsolete config fields, replace old credentials, and migrate persistent local state without losing source IDs, checkpoints, or pending snapshots. No compatibility shim is provided.

One key cannot provide partial/read-only access, per-client revocation, or human attribution. Use separate instances for independent collections. Rotation replaces the server key and updates every intended client; already issued signed object URLs remain subject to their expiry.

## Validation boundary

Earlier OIDC/Keycloak checks in the validation records prove only the superseded implementation/protocol. This decision does not claim cutover tests have run. Integrated validation must cover missing/empty server key, incorrect/missing request keys on all data routes and `/v1/info`, equal access across correct-key clients, migration preservation, global URLs, client state recovery, and withholding bearer secrets from object requests.
