---
name: sessiondb
description: Search and cite earlier coding-agent sessions when knowing what has already been tried, learned, or decided could guide your next step or save repeated investigation.
---

# Search past work with SessionDB

Would someone who has done related work here have a head start? If so, start one focused lookup with the configured SessionDB CLI alongside your first independent read-only check. Proceed directly when the current context settles the task. Revisit history if a new uncertainty comes up.

## Choose the scope for this question

- **Shared operations.** When a past procedure could be useful for an unfamiliar operation, look it up before acting, even when the request only names the action. Start across the team. Include the actual service, queue, project or environment in the query. Check the original exchange for what worked and verify the current target. A backlog, a dead-letter queue and a local test can call for different actions.
- **A particular person.** Add `--member HANDLE` for a named teammate or `--mine` for the current user's preferences. Search across that person's devices. A source label identifies the collecting member. Inspect message role, origin, quoted material and outcomes before attributing a decision.
- **This workstation.** For a local browser profile, connector or machine setup, start with `--local` (current member and device). A shared report workflow can use team history in a separate lookup. If local evidence is missing, inspect the current setup. Another person's settings require independent verification on this machine.
- `sessiondb config show --json` shows the member and device for this connection. `sessiondb sources list` lists registered handles and devices. If the member or device cannot be established, say which scope is unknown.

## Search and open originals

1. Run `sessiondb search --mode hybrid --json -- "question or distinctive terms"`. Add the scope flags above before `--`. The default search includes every member and unattributed sources. The first page contains up to three excerpts. Use `--mode keyword` for a literal or quoted phrase. For recent activity, `--mode keyword --sort newest` orders matching sources by time. Filters include `--harness`, `--role`, `--after`, `--before`, `--project PROJECT_UUID` and `--include-tools`. Use the returned cursor with the same query and filters to continue.
2. Follow a hit's `show_command` to open the original exchange. It uses `sessiondb messages show MESSAGE_ID -C 2` and supplies `--leaf` when a descendant selects the branch. Use `-B` and `-A` to inspect earlier context and later corrections. To expand a truncated original, use `sessiondb messages content MESSAGE_ID`.
3. Cite the message URL. Check the member, device, speaker, origin, date and branch. Use the project IDs and recorded cwd to assess applicability. For independent evidence, compare the source, harness and `native_session_id` from `sessiondb sessions show SESSION_ID --json` with the current conversation. Skip self-matches, repeated prompts and quoted recaps.

## Preferences and cross-session questions

Run separate SessionDB searches for a practice and for a competing choice, accepted exception or later correction. Use concrete decisions as search terms. Start with `--role user` for personal statements. Open the original exchanges from independent sessions and inspect how each decision ended. An assistant suggestion, a harness instruction or a question about someone's preferences provides different evidence from the user's own decision.

Explain the scope of each example and distinguish stated preferences from inferences. A project filter needs a SessionDB project UUID. If the requested scope, citation or evidence cannot be verified through SessionDB, say what is missing. An exact frequency claim needs a defined corpus and coverage.
