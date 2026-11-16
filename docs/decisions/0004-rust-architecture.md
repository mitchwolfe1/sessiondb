# Rust workspace and server architecture

Status: proposed. Date: 2026-11-15.

## Decision

Use five initial Cargo packages: types, adapters, client, CLI, and server. Use Rust 2024, Tokio/axum, serde, SQLx, reqwest, clap, and structured tracing as starting candidates. Keep persistence, imports, search, embeddings, and job handling as server modules.

Use PostgreSQL-backed durable job leases and one initial server deployment. Split API/worker operating modes when measured workloads require it, while retaining the same contracts.

## Alternatives considered

One package would make it easy for clients to depend on internal server models. A crate per entity or an initial microservice topology would introduce coordination without demonstrated reuse. The proposed workspace separates public interfaces and native formats with a small number of packages.

## Review needed

Verify dependency compatibility, object-client signing support, OIDC/CLI login, packaging targets, and an MSRV. Commit exact versions and the workspace lockfile during implementation. This record does not authorize scaffolding during the documentation phase.
