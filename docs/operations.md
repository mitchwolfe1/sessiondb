# Operations

SessionDB runs as a service backed by PostgreSQL and an S3-compatible object store. This guide covers server configuration, background processing, health, upgrades, and backup/restore. Account and session-sharing setup are described in [access control](access-control.md).

Status: proposed operating model. There are no Docker images, Compose files, migrations, or installers in this documentation-only repository.

## Deployment topology

Run the `sessiondb-server` service with PostgreSQL and an existing S3-compatible object store. Put TLS termination and the configured identity provider in the documented deployment environment. A future development Compose profile may supply local dependencies; production instructions must identify persistent volumes and external secret configuration explicitly.

The server starts API and bounded worker tasks in one deployment. Parsing and keyword jobs use different concurrency from embedding jobs. A PostgreSQL job table provides durable queue state and leases. An external broker or separate vector service is not a v1 deployment requirement.

S3 upload URLs must resolve to an endpoint reachable by authorized sync clients and be signed for that endpoint. An internal-only object endpoint cannot be handed to a remote laptop. SeaweedFS 4.54 and its tested client configuration are the first reference; see [reference validation](validation/reference-stack.md). Validate internal/public endpoint configuration during deployment; do not claim that every S3-compatible provider has identical presign/checksum behavior.

## Configuration and bootstrap

Use TOML configuration, a `--config` flag, and environment overrides. Secrets are supplied by environment/secret references rather than embedded in checked-in examples. The [server example](examples/server.toml) illustrates the proposed shape.

The operator configures the initial organization and exact OIDC issuer/subject for its first owner. Bootstrap runs once against an empty catalog and records completion. The operator then signs in through that subject, verifies organization membership, and removes the bootstrap subject from routine configuration. Configuration changes never grant owners after initialization. The selected [authentication profile](authentication.md) and sanitized Keycloak realm example specify the first provider setup.

Provision a public CLI OAuth client with Authorization Code/PKCE and a loopback redirect, plus an API audience accepted by the identity provider. Test the chosen provider against the [authentication contract](authentication.md); do not treat an ID token as an API access token. The first one-shot sync uses the signed-in CLI identity. Background collection later uses revocable source-bound tokens; applications can use read-scoped session tokens issued by permitted members.

## Service behavior

- `/healthz` answers whether the process is alive.
- `/readyz` checks migrations and core catalog/object access. A missing optional embedding endpoint does not make keyword/session service unready.
- `/metrics` exposes aggregate Prometheus-compatible operational metrics to an operator-controlled network/authentication boundary.
- Structured logs include request/job IDs, status, latency, and bounded error codes. Transcript content, tokens, credentials, and signed object URLs are excluded by default.
- Graceful shutdown stops taking new work, drains bounded requests, persists sync/job progress, and releases or lets worker leases expire safely.

Client background collection runs as a user service using systemd on Linux or launchd on macOS. It starts after login, reconnects when the server is available, and uses private platform state/credential directories. A foreground `sync --watch` remains available for debugging and unsupported service environments.

## Migrations and upgrades

Store ordered SQL migrations with the server once code begins. Provide `sessiondb-server migrate` as an explicit operator command. Production startup checks the schema version and fails readiness with an actionable message if migrations are required; it does not unexpectedly apply destructive changes.

Use expand/contract changes when an upgrade must allow older clients or worker tasks to overlap. Publish supported server/client/API versions. Native harness version compatibility is independent of API compatibility and belongs in adapter release notes.

Choose Rust dependency versions, TLS backend, MSRV, and supported binary targets after the first integration spike, then lock them in Cargo. Release checks include formatting, Clippy, unit/integration tests, API compatibility, and migration tests. PostgreSQL/pgvector/object-store versions belong in the tested deployment matrix rather than being inferred from `latest` tags.

## Backup and restore

Back up both PostgreSQL and referenced original objects. A database-only backup cannot reconstruct original files, and an object-only copy cannot reconstruct organization access or the catalog.

For the first documented procedure, pause import finalization and destructive cleanup, finish in-flight catalog writes, take a consistent catalog backup and object inventory/copy, then resume. Objects already uploaded but not accepted may be ignored as staging. Record the restore point and validate that every referenced original exists. A later online backup procedure requires equivalent consistency guarantees.

A restore drill must recover organization memberships, session grants, session reads, raw exports, client resynchronization, and pending jobs. Treat embedding vectors and keyword projections as rebuildable, while preserving the configuration needed to reproduce them. Keep credentials external and rotate/revalidate them during restore.

## Retention and deletion

Proposed v1 behavior is no automatic expiry of accepted session generations until an organization administrator configures an explicit policy. Local disappearance does not imply server deletion. Storage limits pause affected imports with a visible error rather than silently deleting old sessions.

Verified append snapshots can supersede an earlier object covering the same generation prefix. Cleanup removes only objects no longer referenced by a current generation, preserved rewrite, explicit pin, or export. Use the initial 24-hour grace after import close or append-object retirement, and never clean active leases or referenced generations. Rewrite generations and explicit session deletion follow separate policies.

Session deletion hides data from all query paths immediately and queues cleanup of messages, passages, vectors, and exclusively referenced objects. Keep its source suppression state so ordinary sync does not recreate it. Publish the effect on backups and the retention of already taken backups in deployment policy; deletion from a live server does not rewrite an offline backup.

## Capacity and failure visibility

Measure import bytes/rate, accepted-to-searchable delay, failed parses, pending jobs, provider latency, storage use, and query latency. Do not put raw high-cardinality session IDs or message text into aggregate metric labels.

Indexing capacity and query targets must be established with a representative synthetic/masked corpus, including 4,096-dimensional embeddings if that profile is selected. The benchmark must include session access filters and active sync; an isolated vector benchmark is not the whole product.
