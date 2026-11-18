# Rust workspace and server architecture

Status: workspace accepted; historical import/job/worker execution below superseded by [0014](0014-sessions-and-synchronous-files.md), identity by [0013](0013-instance-api-key.md). Date: 2026-11-15. Historical baseline: 2026-11-16.

## Decision

Use five initial Cargo packages: types, adapters, client, CLI, and server. Use Rust 2024, Tokio/axum, serde, SQLx, reqwest, clap, and structured tracing as the implementation baseline. Keep persistence, imports, search, embeddings, and job handling as server modules.

Use PostgreSQL-backed durable job leases and one initial server deployment. Split API/worker operating modes when measured workloads require it, while retaining the same contracts.

## Alternatives considered

One package would make it easy for clients to depend on internal server models. A crate per entity or an initial microservice topology would introduce coordination without demonstrated reuse. The proposed workspace separates public interfaces and native formats with a small number of packages.

## Review needed

Dependency compatibility, object-client signing, and the OIDC reference flow are recorded in [validation](../validation/reference-stack.md). Use Rust 1.94.1 and the selected library versions, then commit the actual workspace lockfile during implementation. Release targets remain a later CI/package check. This record does not authorize scaffolding during the documentation phase.
