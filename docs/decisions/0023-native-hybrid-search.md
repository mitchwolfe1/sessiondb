# Native hybrid search

Status: implemented for the optional semantic search milestone in [the delivery plan](../roadmap.md#optional-semantic-search). This implements the provider and storage proposal in [ADR 0005](0005-search.md) under the instance-key and synchronous upload contracts.

## Decision

Use PostgreSQL full-text search and optional exact pgvector cosine ranking over the same branch-specific exchange segments. Hybrid search fuses up to 200 entries from each ranking with reciprocal rank fusion and a constant of 60. Each result cites an eligible original message and identifies the retrievers that found its exchange.

An explicitly enabled instance profile pins one provider, served model revision, dimensions, input transforms and tool policy. Profile-scoped hashes identify the full text to embed. Keeping vectors for both exchange segments and individual members lets role and date filters exclude text before scoring and lets the server choose a source excerpt.

## Consequences

Acceptance commits keyword content and pending hashes together. Indexing contacts the provider after commit. Restarts, identical uploads and unchanged text reuse completed vectors. A provider outage leaves keyword reads and sync available. A request that needs an unavailable provider receives a retryable error.

Stock PostgreSQL supports keyword-only deployments. An operator can install the vector extension and rerun the migration command to provision vector storage. Exact ranking supports the reference 4,096-dimensional profile with no approximate index to build or tune. Query cost grows with the eligible embedded collection.

The [retrieval contract](../search-and-embeddings.md) documents setup, coverage counts, model identity and error semantics. The [browser evaluation](../../dev/evals/browser/README.md#native-sessiondb-pilot) records the frozen replay and the separate live-provider trials.
