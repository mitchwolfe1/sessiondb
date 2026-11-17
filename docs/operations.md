# Operations

SessionDB runs as a service backed by PostgreSQL and an S3-compatible object store. This guide covers server configuration, background processing, health, upgrades, and backup/restore. Instance-key management and the shared collection boundary are described in [access control](access-control.md).

Status: initial operating model with later target-v1 features called out in [implementation status](implementation.md). The repository contains migrations, a server Dockerfile, and a local Compose profile. Published images, binary releases, and client-service installers remain later work.

## Deployment topology

Run the `sessiondb-server` service with PostgreSQL and an existing S3-compatible object store. Provide TLS termination for remote access and a nonempty `SESSIONDB_API_KEY`. The development Compose profile supplies PostgreSQL and S3 dependencies; production instructions must identify persistent volumes and external secret configuration explicitly.

The server starts API and bounded worker tasks in one deployment. Parsing and keyword jobs use different concurrency from embedding jobs. A PostgreSQL job table provides durable queue state and leases. An external broker or separate vector service is not a v1 deployment requirement.

S3 upload URLs must resolve to an endpoint reachable by authorized sync clients and be signed for that endpoint. An internal-only object endpoint cannot be handed to a remote laptop. SeaweedFS 4.54 and its tested client configuration are the first reference; see [reference validation](validation/reference-stack.md). Validate internal/public endpoint configuration during deployment; do not claim that every S3-compatible provider has identical presign/checksum behavior.

## Configuration and credentials

Use TOML configuration, a `--config` flag, and environment overrides. Secrets are supplied by environment/secret references rather than embedded in checked-in examples. The [server example](examples/server.toml) illustrates the proposed shape.

The operator provisions one strong shared key in `SESSIONDB_API_KEY`; `[auth] api_key_env = "SESSIONDB_API_KEY"` is the default environment reference, never a literal secret. Startup rejects an absent/empty key. There is no account bootstrap or external identity service. Every correct-key client can access all instance data and implemented mutations; use independent instances for isolation. See [authentication](authentication.md).

Clients use `Authorization: Bearer <key>`, via `SESSIONDB_API_KEY` or the optional OS credential store. Rotate by replacing the server key and restarting, then updating clients. There is no per-client revocation, scope restriction, refresh, or overlapping legacy-key mode. Do not put keys in arguments, logs, ordinary config, or object requests.

## Service behavior

- `/healthz` answers whether the process is alive.
- `/v1/info` reports capabilities and limits and requires the key; static `/openapi.yaml` is public.
- `/readyz` checks migrations and core catalog/object access. A missing optional embedding endpoint does not make keyword/session service unready.
- `/metrics` exposes aggregate Prometheus-compatible operational metrics to an operator-controlled network/authentication boundary.
- Structured logs include request/job IDs, status, latency, and bounded error codes. Transcript content, tokens, credentials, and signed object URLs are excluded by default.
- Graceful shutdown stops taking new work, drains bounded requests, persists sync/job progress, and releases or lets worker leases expire safely.

Client background collection runs as a user service using systemd on Linux or launchd on macOS. It starts after login, reconnects when the server is available, and uses private platform state/credential directories. A foreground `sync --watch` remains available for debugging and unsupported service environments.

## Migrations and upgrades

Ordered SQL migrations live with the server. Run `sessiondb-server migrate` explicitly before serving. Startup checks migration versions/checksums and fails with an actionable message if migrations are required; it does not unexpectedly apply destructive changes.

The instance-key cutover is a deliberate breaking upgrade, not an expand/contract rollout. Back up the catalog and original objects, stop old clients/workers, and review the disclosure: formerly isolated content becomes one collection fully accessible with the new key. Apply the forward migration; do not rewrite migration 0001. It retains accepted sessions, files/generations, original bytes, projects/associations, imports, jobs, and UUIDs while dropping obsolete identity tables/columns and identity fields in stored session JSON. Existing object keys remain opaque historical locations. Preserve distinct source UUIDs even when old isolated sources used matching device/root tuples; never silently merge/delete them.

Update all SDK callers to global `/v1` paths and the static-key constructor. Remove old identity settings from server/client configuration, replace old login credentials with the instance key, and upgrade local sync state. CLI configuration rejects removed organization/default-visibility fields; remove them manually. The CLI SQLite forward migration retains source IDs, checkpoints, and pending snapshots while removing registration scope. Keep those recovery records: clearing state can lose pending work. No old client, route, DTO, or token compatibility shim remains.

Use expand/contract changes when an upgrade must allow older clients or worker tasks to overlap. Publish supported server/client/API versions. Native harness version compatibility is independent of API compatibility and belongs in adapter release notes.

The toolchain and dependencies are locked in Cargo. Release checks include formatting, Clippy, unit/integration tests, API compatibility, and migration tests. PostgreSQL/pgvector/object-store versions belong in the tested deployment matrix rather than being inferred from `latest` tags. Supported release targets still require packaging checks.

## Backup and restore

Back up both PostgreSQL and referenced original objects. A database-only backup cannot reconstruct original files, and an object-only copy cannot reconstruct the catalog.

For the first documented procedure, pause import finalization and destructive cleanup, finish in-flight catalog writes, take a consistent catalog backup and object inventory/copy, then resume. Objects already uploaded but not accepted may be ignored as staging. Record the restore point and validate that every referenced original exists. A later online backup procedure requires equivalent consistency guarantees.

A restore drill must recover session reads, raw exports, client resynchronization, and pending jobs. Treat embedding vectors and keyword projections as rebuildable, while preserving the configuration needed to reproduce them. Keep credentials external and rotate/revalidate them during restore.

## Retention and deletion

Proposed v1 behavior is no automatic expiry of accepted session generations until the operator configures an explicit policy. Local disappearance does not imply server deletion. Storage limits pause affected imports with a visible error rather than silently deleting old sessions.

Verified append snapshots can supersede an earlier object covering the same generation prefix. Cleanup removes only objects no longer referenced by a current generation, preserved rewrite, explicit pin, or export. Use the initial 24-hour grace after import close or append-object retirement, and never clean active leases or referenced generations. Rewrite generations and explicit session deletion follow separate policies.

Session deletion hides data from all query paths immediately and queues cleanup of messages, passages, vectors, and exclusively referenced objects. Keep its source suppression state so ordinary sync does not recreate it. Publish the effect on backups and the retention of already taken backups in deployment policy; deletion from a live server does not rewrite an offline backup.

## Capacity and failure visibility

Measure import bytes/rate, accepted-to-searchable delay, failed parses, pending jobs, provider latency, storage use, and query latency. Do not put raw high-cardinality session IDs or message text into aggregate metric labels.

Indexing capacity and query targets must be established with a representative synthetic/masked corpus, including 4,096-dimensional embeddings if that profile is selected. The benchmark must include classification filters and active sync; an isolated vector benchmark is not the whole product.
