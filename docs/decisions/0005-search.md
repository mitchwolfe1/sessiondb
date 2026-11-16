# Keyword search and optional embeddings

Status: proposed. Date: 2026-11-15.

## Decision

Provide keyword search by default. Add semantic and hybrid modes through an explicitly configured immutable embedding profile and background jobs. Propose PostgreSQL full-text search plus pgvector exact cosine search as the first storage backend.

## Rationale

Collection and retrieval must remain useful without model infrastructure. Separating query/document transforms and profile identity supports different model families. Exact search avoids assuming that every model dimension fits a default ANN index.

## Consequences

Advertise coverage and actual retrieval mode. Fail explicitly when a requested semantic capability is unconfigured or unavailable. Keep permission filters consistent across retrieval methods. Select a provider contract and benchmark the representative workload before accepting capacity promises or ANN settings.
