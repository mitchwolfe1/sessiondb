# Authentication

SessionDB uses one instance-wide API key. Configure a nonempty `SESSIONDB_API_KEY` in the server environment; startup fails closed when it is missing or empty. Clients send `Authorization: Bearer <key>`. The key is a shared secret, not an identity, scoped credential, JWT, or expiring login session.

Status: accepted in [ADR 0013](decisions/0013-instance-api-key.md). [First-build validation](validation/first-build.md) records instance-key checks separately from historical OIDC observations.

## Request boundary

Every `/v1` endpoint, including `/v1/info`, requires the key before returning data or processing a request. `/healthz`, `/readyz`, and static `/openapi.yaml` are public. Missing, malformed, or incorrect bearer credentials return `401`; valid-key clients all have the same full access to the shared collection. There is no network login, discovery, refresh, account lookup, or credential-issuance API.

Both HTTP and HTTPS server URLs are supported. Use HTTPS across untrusted networks; HTTP is suitable over a protected connection such as an encrypted Tailscale network or loopback. HTTP itself does not encrypt the key or transcript data. See [ADR 0016](decisions/0016-http-private-networks.md).

Verify the secret with a constant-time comparison. Never write it into logs, diagnostics, ordinary configuration, examples, URLs, or command-line arguments.

## CLI and SDK

`SESSIONDB_API_KEY` is sufficient for every CLI operation. A context selects only a server URL. `sessiondb auth login --key-stdin` requires that flag, reads a supplied key, validates protected `/v1/info`, and saves it per normalized server URL in the OS credential store. It does not authenticate with a provider or exchange tokens. `SESSIONDB_API_KEY` overrides the saved key; an explicitly empty value fails without fallback. Logout removes only the local stored copy, not the environment value; status validates `/v1/info` and reports only the server/authenticated state, never the secret. See [CLI](cli.md) for the available interface.

The SDK requires `Client::new(server: &str, api_key: String, timeout: Duration)` with a nonempty key. It has no token provider, refresh task, or organization selection. Uploads and downloads use the API only; never send its bearer key to S3.

## Provisioning and rotation

Provision a strong key through the operator's secret manager. Keep it outside backups and ordinary TOML. Replace the server environment key and restart, then update client environments/credential stores. There is no initial owner or overlapping legacy-key window. Object URLs are not issued to clients.

Anyone given the key can read every session and original, sync content, and invoke all implemented mutations. Do not distribute it to a consumer that should have read-only or partial access. Use separate instances when collection isolation is required; project labels and provenance are not access controls.

## Catalog initialization

The instance-wide model is defined directly in the initial catalog migration. There are no deployed identity-based catalogs to convert. Disposable development databases created with earlier schema history must be recreated; see [operations](operations.md#migrations-and-upgrades).
