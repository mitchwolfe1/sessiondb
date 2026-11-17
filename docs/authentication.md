# Authentication

SessionDB uses one instance-wide API key. Configure a nonempty `SESSIONDB_API_KEY` in the server environment; startup fails closed when it is missing or empty. Clients send `Authorization: Bearer <key>`. The key is a shared secret, not an identity, scoped credential, JWT, or expiring login session.

Status: accepted clean cutover in [ADR 0013](decisions/0013-instance-api-key.md). Historical OIDC checks in [reference validation](validation/reference-stack.md) and [first-build validation](validation/first-build.md) do not verify this cutover. No cutover checks have been run as part of this documentation update.

## Request boundary

Every `/v1` endpoint, including `/v1/info`, requires the key before returning data or processing a request. `/healthz`, `/readyz`, and static `/openapi.yaml` are public. Missing, malformed, or incorrect bearer credentials return `401`; valid-key clients all have the same full access to the shared collection. There is no network login, discovery, refresh, account lookup, or credential-issuance API.

Use HTTPS for remote deployments. Explicit loopback HTTP remains available for local development. Verify the secret with a constant-time comparison. Never write it into logs, diagnostics, ordinary configuration, examples, URLs, or command-line arguments.

## CLI and SDK

`SESSIONDB_API_KEY` is sufficient for every CLI operation. A context selects only a server URL. `sessiondb auth login --key-stdin` requires that flag, reads a supplied key, validates protected `/v1/info`, and saves it per normalized server URL in the OS credential store. It does not authenticate with a provider or exchange tokens. `SESSIONDB_API_KEY` overrides the saved key; an explicitly empty value fails without fallback. Logout removes only the local stored copy, not the environment value; status validates `/v1/info` and reports only the server/authenticated state, never the secret. See [CLI](cli.md) for the available interface.

The Rust SDK requires `Client::new(server: &str, api_key: String, timeout: Duration)`. A missing/empty key is an error. It has no token provider, refresh task, or organization selection. Never forward the key to signed upload/download URLs: object requests receive only their server-provided headers.

## Provisioning and rotation

Generate a strong random key in the operator's secret manager and distribute it through an appropriate secret channel. No initial owner or bootstrap account is needed. Keep the key external to PostgreSQL backups and client TOML files. To rotate, replace the server environment key and restart the service, then update all client environments or credential-store entries. There is one accepted key, not a dual-key compatibility window; holders of the old key lose API access together. Previously issued signed object URLs can remain usable until their short expiry.

Anyone given the key can read every session and original, sync content, and invoke all implemented mutations. Do not distribute it to a consumer that should have read-only or partial access. Use separate instances when collection isolation is required; project labels and provenance are not access controls.

## Upgrade warning

The forward migration removes the former identity model without deleting accepted session/file/project/job data. Previously isolated content becomes one collection accessible to every correct-key client. Preserve backups and review this disclosure before migration. Existing object locations may retain opaque historical components; those names do not enforce isolation.

Existing clients must update their API paths, DTOs, credential configuration, and local state format. Old organization contexts, login credentials, and identity-scoped sync state are not a compatibility mode. Follow [operations](operations.md#migrations-and-upgrades) and the current CLI migration behavior rather than silently reusing old state.
