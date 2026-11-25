# Search and embeddings

When prior work could be useful, agents can search SessionDB and open the original messages on their recorded branch. PostgreSQL keyword retrieval is available through the CLI, HTTP API and Rust SDK. An enabled embedding profile adds exact semantic and hybrid retrieval to the same endpoint and CLI.

## Keyword search

`sessiondb search "browser connector"` ranks exchanges across the selected archive and returns three excerpts by default. Each hit includes its collecting source ID, member (null if unknown), device, project IDs, recorded cwd, session and message IDs, role, origin, available timestamp, content block index, branch endpoint and original message URL. Excerpts contain at most 480 characters. A cursor opens more results, including additional exchanges in the same session.

Plain keywords match English full-text lexemes. Exchanges covering more distinct query terms rank first, followed by PostgreSQL cover-density rank with document-length normalization. Use `AND`, `OR`, quoted phrases or `-word` to request web-search matching. Each alternative must require a positive searchable term. The winning passage supplies the excerpt. Shared branch prefixes and multiple matching segments of an original message yield one source citation.

An exchange follows a recorded user request through its same-branch assistant responses, including responses separated by tool activity. Standalone messages have their own entries. The index uses full text blocks, JSON values and artifact descriptions. Reasoning and binary data are outside the keyword corpus. Tool calls and results form separate entries, selectable with `--include-tools`, `--role tool` or `--tool-name NAME`. Tool hits include the recorded name when available. A result inherits its name from its unambiguous recorded call ID.

Large exchanges split at block boundaries. Large individual blocks split into word groups, with a hard bound for unbroken strings. Each segment retains its original message and block index. Role and date filters apply to contributing passages before scoring. Projects, tags and harness filters use current session metadata. The default ranking includes all teammates. Member and exact device filters restrict the candidate sessions and coverage before keyword, vector or fused ranking.

Migrations 0004 and 0005 index accepted messages one session at a time. Migration 0005 packs oversized blocks into whole-word groups. File acceptance publishes updated passages and exchange vectors in the same transaction as the messages. Identical retries reuse the accepted projection. Search coverage counts distinct messages with an eligible passage in the selected scope, independently of the query and page.

Add `--open-first` to a CLI search to see the first hit’s original, or run any hit’s `show_command` to open it with `sessiondb messages show MESSAGE_ID -C 2`. Both follow up to two parents and two children and use `--leaf` when the continuation is known. At an unresolved fork, choose a returned child ID with `--leaf`. Use `-B` and `-A` for different amounts of earlier and later context.

## Configure semantic and hybrid search

Install pgvector for the PostgreSQL server version, run `CREATE EXTENSION vector` in the SessionDB database, then run `sessiondb-server migrate`. Migration 0006 adds immutable profile metadata and source text hashes. The migrate command provisions vector storage when the extension is present, including on an already migrated catalog. Ordinary PostgreSQL supports keyword retrieval and sync.

Use an OpenAI-compatible embeddings service with an immutable served model deployment. The [profile example](examples/embedding-profile.json) specifies the provider base URL, model name, revision, expected dimensions, optional credential reference, document and query prefixes, batch size, timeout and enabled state. For an unauthenticated local endpoint omit `credential_ref`. For a bearer credential set `SESSIONDB_EMBEDDING_API_KEY` and its matching `SESSIONDB_EMBEDDING_BASE_URL` in the server environment, then select `env:SESSIONDB_EMBEDDING_API_KEY` in the profile. The key is restricted to that exact base URL. Provider requests disable redirects.

```sh
sessiondb embeddings create profile.json
sessiondb embeddings enable PROFILE_ID
sessiondb embeddings show PROFILE_ID --json
sessiondb search "reuse an authenticated browser session" --mode hybrid
```

Profiles record chunking version 1 and L2 normalization. Profiles are immutable apart from `enabled`. Disable the current profile before enabling another. Changing the model, its revision, dimensions, transforms or tool policy creates a new profile and a separate cache. Use a distinct versioned deployment when replacing model weights under the same served name. The provider response identifies the served model and dimensions. The separate revision label records the operator's deployed version.

The server posts `model`, an array of `input` strings and `encoding_format: "float"` to `BASE_URL/embeddings`. It checks the response model, batch size, indexes, dimensions and finite, nonzero norms. Responses may arrive in any index order. The server applies the selected prefix to each input. It splits large texts at whitespace or UTF-8 boundaries into inputs of at most 7,900 bytes including the prefix, combines normalized chunk vectors weighted by text bytes, then normalizes the result. This bounds inputs for the reference endpoint's 8,192-token window and includes the entire eligible text.

## Indexing and coverage

The background indexer embeds each exchange segment and its member passages. The default corpus includes recorded user and assistant text. A profile with `include_tools: true` also embeds tool entries. Tool results still require the caller's tool filters. Profile activation seeds previously accepted content. Sync adds distinct text hashes in the same transaction as the accepted original and keyword index. Provider calls run outside that transaction. Identical text reuses a vector within its profile across appends, retries and sessions.

PostgreSQL persists pending inputs, expiring claims and retry times. An orderly shutdown releases outstanding claims. Failed batches back off and retry individual inputs. Search follows the current accepted message and branch references on every request. A background pass reclaims cached vectors with no eligible reference in that profile. Profile detail reports counts of distinct ready, pending and retrying inputs.

## Retrieval modes

| Mode | Behavior |
| --- | --- |
| `keyword` | Default. Rank matching exchange terms in PostgreSQL. |
| `semantic` | Embed the query and rank eligible exchange segments by exact pgvector cosine distance. |
| `hybrid` | Fuse up to 200 keyword and 200 semantic exchange ranks with reciprocal rank fusion, using `1 / (60 + rank)` per contributing list. Newly accepted text participates through keywords while vectors catch up. |

Apply member, device, project, harness, tag, tool, role and time filters before scoring. For an intact exchange, use its whole-text vector. When role, date or tool-name filters select a subset, score only the eligible members using the closest available member vector. Select a cited member by query similarity, with lexical evidence available while individual members are pending. Each result identifies its contributing retrieval methods. The same original message appears once.

Vector-mode responses identify the profile, the number of eligible exchange segments and the number with a usable vector for the selected filters. Keyword-only responses identify keyword-ready messages. Cursors bind the profile, mode, query, filters and ordering. Newest ordering applies to keyword search.

A missing enabled profile returns `409 embedding_not_configured`. An unavailable or incompatible required provider returns `503 embedding_provider_unavailable`. Keyword retrieval, sync and original reads operate independently. [pgvector](https://github.com/pgvector/pgvector) provides the vector type and exact cosine operator used here.

The [browser pilot](../dev/evals/browser/README.md#native-sessiondb-pilot) records source retrieval and task completion separately.
