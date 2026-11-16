# Authentication

Human CLI login uses OpenID Connect Authorization Code with PKCE S256. The first reference provider is Keycloak 26.8.2. Session access after login follows [access control](access-control.md), independently of project labels.

Status: accepted first-build protocol. [Reference validation](validation/reference-stack.md) records the actual browser/provider checks; SessionDB's own implementation tests follow during the first build.

## Provider setup

Configure a public `sessiondb-cli` client with Authorization Code enabled, direct password grants disabled, and PKCE S256 required. Use the `basic`, `profile`, and `email` client scopes. The reference test confirmed that omitting Keycloak's `basic` scope can omit the access token's subject claim.

Register `http://127.0.0.1/callback`. Keycloak's tested loopback handling accepts the requested dynamic port, so the CLI can bind an available port rather than depending on one fixed port being free. Do not register an unrestricted wildcard redirect.

Map `sessiondb-api` into the access-token audience only. The client-audience ID token is used to validate the CLI authentication response, not to authorize SessionDB API calls. The [realm example](examples/keycloak-realm.json) contains the reference client/scopes/mapper and no user passwords or client secret.

The reference validation used a disposable local issuer. Production issuer/discovery/token endpoints use HTTPS. Existing providers can replace Keycloak when they satisfy this JWT access-token profile and the same interoperability checks; opaque access-token introspection is not v1 behavior.

## Login sequence

1. Read the server's public `/v1/auth/config`, then fetch and validate discovery for its exact configured issuer.
2. Bind the callback listener to `127.0.0.1` on an available port. Generate fresh state, nonce, and a cryptographically random PKCE verifier; send its S256 challenge.
3. Open the system browser at the authorization endpoint, requesting `openid profile email` for the public CLI client.
4. Accept the callback only on the expected path and validate state. Handle `error`/`error_description` callbacks as failures; an OAuth failure can arrive through a redirect rather than a non-2xx browser page.
5. Exchange the code with the verifier and exact redirect URI. Validate the ID token's signature, issuer, client audience, and nonce through the selected OIDC library.
6. Present the API-audience access token to `POST /v1/auth/session`. The server validates it, registers the user, and claims matching active invitations using verified identity information.
7. Store access/refresh credentials in the OS credential store. The context configuration holds the server and organization selection, not the tokens.

A cancelled/failed browser flow expires after five minutes and releases the loopback listener. Refresh follows the provider's token endpoint and lifetime policy. A failed refresh asks for sign-in rather than falling back to another identity or an API-key billing path.

For a headless environment, `sessiondb auth login --token-stdin` or `SESSIONDB_TOKEN` accepts a member-issued scoped API token. Device authorization is a later optional flow, not required to implement this login.

## API token verification

The first provider profile uses RS256 JWT access tokens. Require a valid signature and `iss`, `sub`, `aud`, and `exp`; the exact issuer and `sessiondb-api` audience must match the server configuration. Allow 30 seconds of clock skew. Reject ID tokens with the CLI audience, altered signatures, an unexpected algorithm, missing required claims, and expired tokens.

Use discovered JWKS with a five-minute cache, refreshing once when an unfamiliar key ID appears. Do not accept keys embedded in an untrusted token or expand the allowed algorithms from a token header. `openidconnect` handles the CLI protocol/ID-token checks; `jsonwebtoken` handles the API JWT profile. Use the OIDC crate's own HTTP-client interface where its dependency version differs from the main SDK client.

The chosen JWT profile is narrower than claiming support for every possible OIDC deployment. Another signing algorithm or token-introspection mode requires an explicit compatibility addition.

## Bootstrap and automation

On an empty installation, configuration names the initial organization and one exact issuer/subject that becomes its owner. Bootstrap is a one-time transaction. Later edits to that setting do not grant another owner. Obtain the subject from the identity-provider directory or a verified login, not from an unverified token body.

Member-issued API tokens have a random 256-bit secret, a recognizable `sdb_` prefix, an explicit expiry, and a stored SHA-256 verifier. They are not passwords. The CLI's proposed default is 90 days, with a one-year maximum configurable downward by the server. Token creation returns the secret once; revocation and the issuer's current access are checked by the server.

Read/write scopes and explicit session restrictions do not exceed that member's current rights. A sync credential also binds one owned source and cannot change sharing, source defaults, or organization membership. Source-token provisioning and rotation are later than the first human-login slice; the first one-shot sync can use the authenticated CLI credential.

## Conformance

The implementation must reproduce the reference checks for successful login, missing PKCE, wrong verifier, state/nonce validation, refresh, wrong audience/issuer, ID-token misuse, altered signatures, and expiry. Add missing-claim, key-rotation, cancellation, and membership-revocation tests against the actual server. Protocol validation is a starting point for those tests, not a replacement for them.
