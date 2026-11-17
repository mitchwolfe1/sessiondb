# Search and embeddings

Search session content with keywords or configure an embedding provider for semantic and hybrid retrieval. The CLI and API support filters for project, harness, date, and message content.

Status: proposed v1 retrieval contract. Benchmarking and library integration precede performance commitments.

## Keyword search

Use PostgreSQL full-text search over normalized message passages and selected metadata. Titles, tags, project, harness, and activity times support filtering and navigation. Tool calls/results remain distinguishable from conversation text and can be filtered by tool name or included explicitly.

Indexing follows durable import acceptance. A session reports parse and keyword readiness. A search response identifies its actual retrieval mode, index freshness, matching messages, and bounded excerpts. Default ordering is relevance; `sort=newest` orders by matching message time with documented fallback to session time.

Search returns a flat list of matching messages, with at most three per session so one long conversation does not fill the result set. Clients can group them by session for display. The caller can retrieve more through message endpoints. Do not treat repeated copies or generated summaries as independent corroboration; label native origin and relationship information when known.

## Embedding extension

The server supports a small provider interface with separate document and query embedding operations. Its first implementation is proposed to use an OpenAI-compatible embedding endpoint through reqwest; the exact compatible request/response contract must be documented and tested before implementation. No generation/judging API is needed for ordinary indexing.

Provider configuration includes endpoint, model, expected dimensions, authentication secret reference, batch limit, timeout, and explicit query/document transforms when the model requires them. A configurable provider is not permission to silently use a fallback vendor or billing method.

See the [embedding profile example](examples/embedding-profile.json). Runtime provider calls occur only after a key holder explicitly configures and enables the profile.

## Profiles and indexing jobs

An embedding profile belongs to the instance and defines one vector space: provider/model revision, dimensions, normalization, chunking version, and query/document transforms. These values are immutable after vectors are produced. Changing them creates another profile and a backfill; vectors from different spaces never share a similarity comparison. At most one profile is enabled per instance in v1. It covers eligible sessions independently of project assignment.

Background workers batch eligible passages, persist progress, and retry transient provider failures independently of sync. Exclude deleted content and apply the configured indexing eligibility policy before making provider requests. Deletion removes derived vectors along with message passages.

Enabling a profile permits embedding eligible future accepted content, including unassigned sessions. Historical backfill is an explicit job with optional source, session, or project-label filters and progress counters. The selected labels filter work rather than define a vector space or permission boundary.

## Vector storage

PostgreSQL with pgvector is the proposed first vector backend. Begin with exact cosine search over filtered session passages. Approximate nearest-neighbor indexing is an optimization to benchmark, not a prerequisite for the provider interface.

The selected model's dimensions must be supported by the chosen storage and index type. Current pgvector documentation distinguishes storage limits from ANN index limits: `vector` storage supports up to 16,000 dimensions, while common HNSW/IVFFlat vector indexes support 2,000 and half-vector indexes 4,000. A 4,096-dimensional model therefore cannot simply use those default ANN layouts. Keep exact search available; any reduction/quantization must be explicit in a separate profile and evaluated. See [pgvector](https://github.com/pgvector/pgvector).

## Retrieval modes

| Mode | Behavior |
| --- | --- |
| `keyword` | Search all keyword-ready eligible content without a provider call. Default. |
| `semantic` | Embed the query and search compatible vectors from one enabled profile. Report eligible and embedded counts. |
| `hybrid` | Combine keyword and semantic candidates, deduplicate messages, and use a documented rank-fusion policy. Fresh unembedded messages remain eligible through keyword retrieval. |

Keyword, semantic, and hybrid requests can include no project filter, one project, several projects, or only unassigned sessions. Semantic modes use the single enabled instance profile. A session matching several selected labels appears once. Classification changes update query filters and preserve vectors unless the configured embedding text itself changes.

The proposed fusion policy is reciprocal rank fusion with rank constant 60 and up to 200 candidates from each retriever before deduplication and the per-session result cap. Treat its scores as ranking values, not confidence or factual correctness. Benchmark these proposed settings during the search milestone. Newest sorting uses keyword mode only in v1; semantic/hybrid requests with that sort are rejected rather than silently switching modes.

If semantic/hybrid is requested without an enabled instance profile, return `409 embedding_not_configured`. If the required provider is temporarily unavailable, return a specific retryable `503`; clients can explicitly retry in keyword mode. Never relabel a keyword fallback as semantic success.

## Access and privacy

Require the instance API key before candidate selection, vector distance calculation, counts, snippets, and artifact links. All key holders search the same collection. Project associations only narrow candidates by classification. Retrieval jobs and caches identify the instance/profile and query filters, never an account or grant scope.

Only normalized configured content is sent to the embedding endpoint. Available model reasoning, large tools, and attachments need explicit eligibility rules; the proposed v1 default indexes user/assistant text and keeps tool payload embedding opt-in. Wire role does not establish authorship or quality.

## Required checks

Verify unassigned and multi-project sessions in every mode; verify keyword search while embedding is disabled/down; profile changes without mixed spaces; deletion and missing/invalid-key rejection across all modes; interrupted backfill recovery; partial coverage reporting; and retrieval of recent unembedded messages in hybrid mode. Use deterministic local embedding fixtures for tests, then an explicitly authorized runtime integration test for the configured provider.
