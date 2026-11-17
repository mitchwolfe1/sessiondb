# Access control

A SessionDB instance has one shared collection and one API key. Every correct-key client has identical full access to every resource and implemented operation. There are no accounts, organizations, owners, memberships, teams, per-session visibility, grants, or scoped credentials.

Status: accepted in [ADR 0013](decisions/0013-instance-api-key.md), superseding all earlier identity-based access proposals. [Authentication](authentication.md) defines key provisioning, request verification, and rotation.

## Collection boundary

Require bearer-key authentication on all `/v1` APIs, including capability discovery at `/v1/info`. Only liveness, readiness, and the static OpenAPI document are public. The same rule applies to original files, parsed content, artifacts, native exports, changes, search, and background job results. Planned resource APIs inherit it; they do not introduce separate privileges.

A project is optional classification metadata, not a container or security boundary. Source IDs, device labels, harness names, and native paths describe provenance, not the caller's identity. All key holders can inspect unparsed originals and linked parent/dependency data. Missing dependencies remain explicit; there is no per-reader export omission policy.

## Safe collection and deployment

The operator or client explicitly selects native stores to collect. Starting cwd and repository matching do not decide whether a session may be stored. Supporting-file allowlists avoid collecting unrelated credentials/configuration beside transcripts. Ingestion treats text as data and never executes recorded commands or follows arbitrary remote content URLs.

Use separate instances, databases, and object namespaces when independent collections must remain isolated. A read-only application cannot be safely given the same key with an assumption of reduced rights. Secure remote transport with HTTPS; keep storage private and protect metrics separately at the operator's network boundary.

## Revocation and migration

Rotate the instance key to revoke API access for every old-key client, then distribute the replacement only to intended clients. There is no per-client revocation mechanism. Already issued signed downloads may remain usable for their documented maximum lifetime of 60 seconds; use API-proxied downloads when that exposure is unacceptable.

The cutover makes formerly isolated records accessible through the same key. It preserves accepted content and IDs while removing identity fields/tables. This is a breaking data-disclosure and client configuration/state change, not just a login simplification. See [upgrade guidance](operations.md#migrations-and-upgrades).

## Acceptance scenarios

- Two independent clients with the same key see the same sessions, source files, artifacts, jobs, exports, and query results.
- Project assignment, clearing labels, and native append do not alter access.
- Missing/invalid keys fail on every data path and on `/v1/info`.
- Health, readiness, and the static contract reveal no session content without a key.
- Rotating the key rejects the old value on subsequent API requests.
