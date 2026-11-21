# Authentication

SessionDB uses one instance-wide API key. Configure a nonempty `SESSIONDB_API_KEY` in the server environment; startup fails closed when it is missing or empty. Clients send `Authorization: Bearer <key>`. The key is a shared secret, not an identity, scoped credential, JWT, or expiring login session.

Status: accepted in [ADR 0013](decisions/0013-instance-api-key.md), with fixed secret environment names in [ADR 0017](decisions/0017-fixed-secret-environment-variables.md). [First-build validation](validation/first-build.md) records instance-key checks separately from historical OIDC observations.

## Request boundary

Every `/v1` endpoint, including `/v1/info`, requires the key before returning data or processing a request. `/healthz`, `/readyz`, and static `/openapi.yaml` are public. Missing, malformed, or incorrect bearer credentials return `401`; valid-key clients all have the same full access to the shared collection. There is no network login, discovery, refresh, account lookup, or credential-issuance API.

Both HTTP and HTTPS server URLs are supported. Use HTTPS across untrusted networks; HTTP is suitable over a protected connection such as an encrypted Tailscale network or loopback. HTTP itself does not encrypt the key or transcript data. See [ADR 0016](decisions/0016-http-private-networks.md).

Verify the secret with a constant-time comparison. Never write it into logs, diagnostics, ordinary configuration, examples, URLs, or command-line arguments.

## CLI and SDK

`sessiondb connect SERVER_URL` prompts for the shared key and verifies it against protected `/v1/info`. The CLI saves the verified key for that exact server origin in its private TOML configuration. Both the CLI and background watcher use that file. `SESSIONDB_API_KEY` overrides the saved key for scripts. `sessiondb status` checks access, and `config show` redacts credentials. See the [CLI guide](cli.md#configuration).

The SDK requires `Client::new(server: &str, api_key: String, timeout: Duration)` with a nonempty key. It has no token provider, refresh task, or organization selection. Uploads and downloads use the API only; never send its bearer key to S3.

## Provisioning and rotation

Provision the server key through its environment. To rotate it, restart the server with the new key and run `sessiondb connect SERVER_URL` on clients to save the replacement. Clients using an environment override should update their environment.

Anyone given the key can read every session and original, sync content, and invoke all implemented mutations. Do not distribute it to a consumer that should have read-only or partial access. Use separate instances when collection isolation is required; project labels and provenance are not access controls.

## Catalog initialization

The instance-wide model is defined directly in the initial catalog migration. There are no deployed identity-based catalogs to convert. Disposable development databases created with earlier schema history must be recreated; see [operations](operations.md#migrations-and-upgrades).
