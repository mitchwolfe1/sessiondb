# Keyword search and optional embeddings

Status: retrieval proposal; identity scope superseded by [0013](0013-instance-api-key.md), background-job workflow below superseded by [0014](0014-sessions-and-synchronous-files.md). Date: 2026-11-15.

The current proposal uses one instance profile and optional project filters, including unassigned sessions. Historical organization/profile terminology below is not active access policy.

## Decision

Provide keyword search by default. Add semantic and hybrid modes through an explicitly configured immutable embedding profile and background jobs. Propose PostgreSQL full-text search plus pgvector exact cosine search as the first storage backend.

## Rationale

Collection and retrieval must remain useful without model infrastructure. Separating query/document transforms and profile identity supports different model families. Exact search avoids assuming that every model dimension fits a default ANN index.

## Consequences

Advertise coverage and actual retrieval mode. Fail explicitly when a requested semantic capability is unconfigured or unavailable. Keep permission filters consistent across retrieval methods. Select a provider contract and benchmark the representative workload before accepting capacity promises or ANN settings.
