# Search and embeddings

Agents can search past work when a decision calls for it and open the original messages on their recorded branch. PostgreSQL keyword retrieval is available through the CLI, HTTP API and Rust SDK. Optional semantic and hybrid retrieval are the [next milestone](roadmap.md#optional-semantic-search).

## Keyword search

`sessiondb search "browser connector"` ranks exchanges across the selected archive and returns three excerpts by default. Each hit includes its session and message IDs, role, origin, available timestamp, content block index, branch endpoint and original message URL. Excerpts contain at most 480 characters. A cursor opens more results, including additional exchanges in the same session.

Plain keywords match English full-text lexemes. Exchanges covering more distinct query terms rank first, followed by PostgreSQL cover-density rank with document-length normalization. Use `AND`, `OR`, quoted phrases or `-word` to request web-search matching. Each alternative must require a positive searchable term. The winning passage supplies the excerpt. Shared branch prefixes and multiple matching segments of an original message yield one source citation.

An exchange follows a recorded user request through its same-branch assistant responses, including responses separated by tool activity. Standalone messages have their own entries. The index uses full text blocks, JSON values and artifact descriptions. Reasoning and binary data are outside the keyword corpus. Tool calls and results form separate entries, selectable with `--include-tools`, `--role tool` or `--tool-name NAME`. Tool hits include the recorded name when available. A result inherits its name from its unambiguous recorded call ID.

Large exchanges split at block boundaries. Large individual blocks split into word groups, with a hard bound for unbroken strings. Each segment retains its original message and block index. Role and date filters apply to contributing passages before scoring. Projects, tags and harness filters use current session metadata.

Migrations 0004 and 0005 index accepted messages one session at a time. Migration 0005 packs oversized blocks into whole-word groups. File acceptance publishes updated passages and exchange vectors in the same transaction as the messages. Identical retries reuse the accepted projection. Search coverage counts distinct messages with an eligible passage in the selected scope, independently of the query and page.

Run the hit’s `show_command` to open the source with `sessiondb messages show MESSAGE_ID -C 2`. It follows up to two parents and two children and supplies `--leaf` when the continuation is known. At an unresolved fork, choose a returned child ID with `--leaf`. Use `-B` and `-A` for different amounts of earlier and later context.

## Embedding extension

The server supports a small provider interface with separate document and query embedding operations. Its first implementation is proposed to use an OpenAI-compatible embedding endpoint through reqwest; the exact compatible request/response contract must be documented and tested before implementation. No generation/judging API is needed for ordinary indexing.

Provider configuration includes endpoint, model, expected dimensions, authentication secret reference, batch limit, timeout, and explicit query/document transforms when the model requires them. A configurable provider is not permission to silently use a fallback vendor or billing method.

See the [embedding profile example](examples/embedding-profile.json). Runtime provider calls occur only after a key holder explicitly configures and enables the profile.

## Profiles and planned indexing

An embedding profile belongs to the instance and defines one vector space: provider/model revision, dimensions, normalization, chunking version, and query/document transforms. These values are immutable after vectors are produced. Changing them creates another profile and a backfill; vectors from different spaces never share a similarity comparison. At most one profile is enabled per instance in v1. It covers eligible sessions independently of project assignment.

Indexing execution/backfill is future work, not a retained `/index-jobs` or `/jobs` API. The milestone must define progress, bounded batching, transient-provider recovery, deletion handling, and coverage independently of synchronous file acceptance.

Enabling a profile will cover eligible future accepted content, including unassigned sessions. Historical backfill may filter sources, sessions, or optional project labels; labels select content rather than define a vector space or permission boundary. Embedding profile operations remain planned as marked in OpenAPI.

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

The proposed fusion policy is reciprocal rank fusion with rank constant 60 and up to 200 candidates from each retriever before deduplicating the original message citations. Treat its scores as ranking values, not confidence or factual correctness. Benchmark these proposed settings during the search milestone. Newest sorting uses keyword mode only in v1; semantic/hybrid requests with that sort are rejected rather than silently switching modes.

If semantic/hybrid is requested without an enabled instance profile, return `409 embedding_not_configured`. If the required provider is temporarily unavailable, return a specific retryable `503`; clients can explicitly retry in keyword mode. Never relabel a keyword fallback as semantic success.

## Access and privacy

Require the instance API key before candidates, vector distances, counts, snippets, and artifact links. All key holders search the same collection. Project labels only narrow classification. Retrieval caches identify instance/profile and query filters, never account/grant scope.

Only normalized configured content is sent to the embedding endpoint. Available model reasoning, large tools, and attachments need explicit eligibility rules; the proposed v1 default indexes user/assistant text and keeps tool payload embedding opt-in. Wire role does not establish authorship or quality.

## Required checks

The [browser retrieval pilot](../dev/evals/browser/README.md) compares agent task completion with history off and on-demand keyword lookup in a frozen corpus.

Verify unassigned and multi-project sessions in every mode; verify keyword search while embedding is disabled/down; profile changes without mixed spaces; deletion and missing/invalid-key rejection across all modes; interrupted backfill recovery; partial coverage reporting; and retrieval of recent unembedded messages in hybrid mode. Use deterministic local embedding fixtures for tests, then an explicitly authorized runtime integration test for the configured provider.
