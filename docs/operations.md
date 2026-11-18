# Operations

SessionDB runs with PostgreSQL and an S3-compatible store. This guide covers server configuration, synchronous acceptance, cleanup, health, migrations, and backup/restore. See [access control](access-control.md) for the shared instance key.

Status: initial operating model with later target-v1 features called out in [implementation status](implementation.md). The repository contains migrations, a server Dockerfile, and a local Compose profile. Published images, binary releases, and client-service installers remain later work.

## Deployment topology

Run the `sessiondb-server` service with PostgreSQL and an existing S3-compatible object store. Provide a nonempty `SESSIONDB_API_KEY` and use HTTPS across untrusted networks. HTTP is supported for protected private connections such as a Tailscale network. The development Compose profile supplies PostgreSQL and S3 dependencies; production instructions must identify persistent volumes and external secret configuration explicitly.

The API verifies/stores/parses one native file before responding. A periodic orphan-cleanup task removes unreferenced objects; there is no durable job queue, generic worker subsystem, or separate parse runtime.

Only the server needs the S3 endpoint. Sync clients upload through the API, and original/artifact reads proxy through it; no public S3 endpoint or presigned upload/download TTL is configured. SeaweedFS 4.54 is the reference; [historical storage validation](validation/reference-stack.md) does not guarantee every provider.

## Configuration and credentials

Use TOML with `--config` and documented environment overrides. The [server example](examples/server.toml) reflects the current shape. `[limits]` contains only `max_file_bytes` (default 268435456); there are no worker or upload-lease settings. `[objects]` configures the server's endpoint, bucket, region, and path style.

`public_url` is the address clients use, not a requirement for internet exposure. Both HTTP and HTTPS are accepted without an extra insecure-mode flag. The server listener serves HTTP; setting an HTTPS `public_url` does not enable TLS. Use an external TLS terminator for HTTPS. Bind to the private interface or restrict its port with a firewall when serving HTTP.

Secrets use fixed environment variables, as defined in [ADR 0017](decisions/0017-fixed-secret-environment-variables.md):

| Variable | Value |
| --- | --- |
| `SESSIONDB_DATABASE_URL` | Required PostgreSQL connection URL. |
| `SESSIONDB_API_KEY` | Required nonempty shared instance key. |
| `SESSIONDB_S3_ACCESS_KEY_ID` | Optional static S3 access key, paired with the secret below. |
| `SESSIONDB_S3_SECRET_ACCESS_KEY` | Optional static S3 secret key, paired with the access key above. |

Set both S3 variables or neither. If neither is set, the standard AWS credential chain is used, including instance/task roles. An incomplete or empty explicit pair fails instead of silently falling back.

There are no `[database]` or `[auth]` TOML sections and no configurable secret-variable names. Supply secrets through the service/container environment or a protected environment file; SessionDB does not automatically load `.env` files. Do not put secret values in TOML.

Provision one strong shared key securely. Startup rejects an absent/empty key. There is no account bootstrap or external identity service. Every correct-key client can access all instance data and implemented mutations; use independent instances for isolation. See [authentication](authentication.md).

Clients use `Authorization: Bearer <key>`, via `SESSIONDB_API_KEY` or the optional OS credential store. Rotate by replacing the server key and restarting, then updating clients. There is no per-client revocation, scope restriction, refresh, or overlapping legacy-key mode. Do not put keys in arguments, logs, ordinary config, or object requests.

## Service behavior

- `/healthz` answers whether the process is alive.
- `/v1/info` reports capabilities and limits and requires the key; static `/openapi.yaml` is public.
- `/readyz` checks migrations and core catalog/object access. A missing optional embedding endpoint does not make keyword/session service unready.
- `/metrics` exposes aggregate Prometheus-compatible operational metrics to an operator-controlled network/authentication boundary.
- Structured logs include request IDs, status, latency, and bounded error codes; exclude transcripts and credentials.
- Shutdown drains bounded requests. A disconnected client retries its saved snapshot; acceptance is recovered by digest/version, not by server job state.

Foreground `sync --watch` currently performs polling and reconnects after transient outages. Linux systemd/macOS launchd installers remain planned; stopping the client stops collection without mutating source metadata.

## Migrations and upgrades

Ordered SQL migrations live with the server. Run `sessiondb-server migrate` explicitly before serving. Startup checks migration versions/checksums and fails with an actionable message if migrations are required; it does not unexpectedly apply destructive changes.

The project has not been deployed, so `0001_catalog.sql` directly creates the current instance-wide schema. There is no second migration or upgrade path from the abandoned identity model. Disposable development databases created with earlier migration history must be recreated; startup correctly rejects their obsolete versions/checksums. Do not edit the migration ledger to bypass that mismatch.

Future deployed upgrades will need an explicit compatibility policy. There are no old clients or workers to overlap in this undeployed clean cutover.

The toolchain and dependencies are locked in Cargo. Release checks include formatting, Clippy, unit/integration tests, API compatibility, and migration tests. PostgreSQL/pgvector/object-store versions belong in the tested deployment matrix rather than being inferred from `latest` tags. Supported release targets still require packaging checks.

## Backup and restore

Back up both PostgreSQL and referenced original objects. A database-only backup cannot reconstruct original files, and an object-only copy cannot reconstruct the catalog.

Stop API writes and orphan cleanup, drain in-flight requests, take a consistent PostgreSQL backup and referenced-object inventory/copy, then restart service. Record the restore point and verify every referenced original/artifact exists. Unreferenced prepared objects are not accepted data. A future online procedure needs equivalent consistency guarantees.

A restore drill must recover catalog/session reads, originals/artifacts, native export, and per-file client resynchronization without duplication. Client snapshots remain on clients, not in a server queue. Preserve future index/profile configuration needed for rebuilds; keep credentials external.

## Retention and deletion

Accepted originals and rewrite generations have no automatic expiry by default. Local disappearance never deletes server history. Resource-limit failures return visible errors rather than silently dropping old content.

Verified append publication can orphan its preceding snapshot. Periodic cleanup atomically deletes old unreferenced `objects` rows and returns keys, then deletes S3 bytes outside the transaction. Foreign keys protect publication, and fresh never-reused keys prevent delayed deletion of newer uploads. No lease/state/retirement columns are needed.

Session deletion and source suppression remain planned. Their future implementation must define backup retention and prevent ordinary sync from recreating explicitly deleted data; the current schema does not contain a pretend suppression mechanism.

## Capacity and failure visibility

Measure upload bytes/rate, request latency, rejected native files, storage use, orphan-cleanup errors, and query latency. Future search coverage is separate from file acceptance. Never put raw session IDs or content into aggregate metric labels.

Indexing capacity and query targets must be established with a representative synthetic/masked corpus, including 4,096-dimensional embeddings if that profile is selected. The benchmark must include classification filters and active sync; an isolated vector benchmark is not the whole product.
