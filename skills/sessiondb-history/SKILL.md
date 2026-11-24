---
name: sessiondb-history
description: Look up and cite prior coding-agent work in SessionDB on demand, including follow-up questions about the user's preferences, decisions and tradeoffs across sessions.
---

# Search past work with SessionDB

Use the configured SessionDB CLI for the whole lookup, including follow-up questions. Fetch history when earlier evidence would change the answer or the next action.

1. Run `sessiondb search --mode hybrid --json -- "question or distinctive terms"`. The first page contains up to three excerpts. Use `--mode keyword` for a literal or quoted phrase. For recent activity, `--mode keyword --sort newest` orders matching sources by time. Filters include `--harness`, `--role`, `--after`, `--before`, `--project PROJECT_UUID` and `--include-tools`. Use the returned cursor with the same query and filters to continue.
2. Follow a hit's `show_command` to open the original exchange. It uses `sessiondb messages show MESSAGE_ID -C 2` and supplies `--leaf` when a descendant selects the branch. Use `-B` and `-A` to inspect earlier context and later corrections. To expand a truncated original, use `sessiondb messages content MESSAGE_ID`.
3. Cite the message URL. Check the speaker, origin, date and branch. For independent evidence, compare the harness and `native_session_id` from `sessiondb sessions show SESSION_ID --json` with the current conversation. Skip self-matches, repeated prompts and quoted recaps.

## Preferences and cross-session questions

Run separate SessionDB searches for a practice and for a competing choice, accepted exception or later correction. Use concrete decisions as search terms. Start with `--role user` for personal statements. Open the original exchanges from independent sessions and inspect how each decision ended. An assistant suggestion, a harness instruction or a question about someone's preferences provides different evidence from the user's own decision.

Explain the scope of each example and distinguish stated preferences from inferences. A project filter needs a SessionDB project UUID. If the requested scope, citation or evidence cannot be verified through SessionDB, say what is missing. An exact frequency claim needs a defined corpus and coverage.
