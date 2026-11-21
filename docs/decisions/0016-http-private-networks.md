# HTTP on private networks

[ADR 0020](0020-connect-by-url.md) replaces the named CLI context examples with direct URL selection.

Status: accepted. Date: 2026-11-18. Supersedes the HTTPS-only transport requirement in [0013](0013-instance-api-key.md), not its shared-key authentication boundary.

## Decision

Accept both HTTP and HTTPS server origins in server configuration, CLI contexts, and the SDK. The operator chooses the URL scheme; no loopback-only exception, hostname/IP allowlist, automatic network detection, or extra insecure-mode flag is required.

`public_url` advertises the address clients use. It does not configure TLS: the server listener serves HTTP, while HTTPS requires an external TLS terminator. For example, a tailnet-only deployment may advertise `http://athena:6767`.

The API key remains mandatory over either scheme. Reject unsupported URL schemes, embedded credentials, and invalid server-origin paths, queries, or fragments. Do not forward the API key to redirected object requests or disable HTTPS certificate verification.

## Operating boundary

HTTP itself does not encrypt transcripts or bearer credentials. Use it only over a trusted, protected connection such as an encrypted Tailscale network or loopback; use HTTPS across untrusted networks. Bind to the private interface or restrict access with a firewall. SessionDB does not determine whether a deployment is actually private.
