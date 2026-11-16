# Reference authentication

Status: accepted. Date: 2026-11-16.

## Decision

Use OIDC Authorization Code with PKCE S256 for human CLI login. The reference provider is Keycloak 26.8.2; configure its `basic`, `profile`, and `email` client scopes, a public `sessiondb-cli` client, and an access-token-only `sessiondb-api` audience mapper.

Register `http://127.0.0.1/callback`; the tested provider accepts the CLI's dynamically allocated loopback port. Use a browser for authentication, validate state/nonce, and handle callback error parameters. The server accepts signed API-audience access tokens, not client-audience ID tokens.

## Consequences

Use `openidconnect` for the CLI flow and `jsonwebtoken` with an explicit supported algorithm allowlist for API JWT verification. The first reference profile uses RS256 JWT access tokens; opaque access-token introspection is not implicit support. Exact issuer/subject bootstrap and member-issued API tokens remain as specified in [access control](../access-control.md).

The [browser/provider checks](../validation/reference-stack.md) passed with the selected audience/scopes and refresh. Production uses HTTPS. Loopback HTTP in the disposable reference environment is a development exception, not a TLS setting for public deployments.
