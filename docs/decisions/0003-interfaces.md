# Public interfaces

Status: accepted direction. Date: 2026-11-15.

## Decision

Provide a `sessiondb` CLI, `sessiondb-server` service, versioned REST API, OpenAPI contract, and reusable HTTP client SDK. The CLI uses the public client boundary. Raw files use established object/HTTP access; session operations use domain resources.

## Rationale

The service should feel conventional to engineers familiar with database clients, connection contexts, standard HTTP errors, and scripted output. One shared network contract prevents CLI-only or SDK-only behavior from becoming a second product.

## Consequences

Proposed names, paths, flags, and schemas must be reviewed together. OpenAPI coverage and validation precede HTTP implementation. API compatibility and Rust crate compatibility are tracked separately. No SQL-like interactive language is implied by having a database CLI.
