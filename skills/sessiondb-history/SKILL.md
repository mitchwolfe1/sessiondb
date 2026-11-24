---
name: sessiondb-history
description: Look up and cite past coding-agent work in SessionDB when a request depends on an earlier decision, a previous investigation, or a workflow that succeeded before.
---

# Search past work with SessionDB

Fetch history when a past decision would change the answer or the next action. Use the configured `sessiondb` CLI connection.

1. Run `sessiondb search --mode hybrid --json -- "question or distinctive terms"`. The first page contains up to three excerpts. For a literal or a quoted phrase, use `--mode keyword`. Filters include `--harness`, `--role`, `--after`, `--before`, `--project PROJECT_UUID` and `--include-tools`. Reuse the query and filters with `--cursor` to see another page.
2. Follow the hit's `show_command` to open the original with context. Its usual form is `sessiondb messages show MESSAGE_ID -C 2`, with `--leaf` supplied for the selected branch. Use `-B` and `-A` for a wider read or a later correction. When a fork offers choices, select a child with `--leaf`. Expand a truncated message with `sessiondb messages content MESSAGE_ID`.
3. Cite the original message URL. Check speaker, origin, date and branch before attributing a decision. For independent evidence, use `sessiondb sessions show SESSION_ID --json` to compare the hit's harness and `native_session_id` with the active conversation. Skip self-matches and quoted recaps.

A SessionDB project filter takes a project UUID. If a request requires an exact repository scope or uses a legacy citation, follow that workspace's migration instructions. For a broad preference question, look for both supporting decisions and exceptions in comparable contexts.
