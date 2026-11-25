---
name: sessiondb
description: Look up and cite earlier coding-agent sessions when prior work could be useful, including team procedures, workstation setup, decisions and preferences.
---

# Search past work with SessionDB

When prior work could be useful, look it up with the configured SessionDB CLI. Use the same CLI for follow-up questions.

## Choose the scope for this question

- **Shared operations.** When a past procedure could be useful for an unfamiliar operation, look it up before acting, even when the request only names the action. Start across the team. Include the actual service, queue, project or environment in the query. Check the original exchange for what worked and verify the current target. A backlog, a dead-letter queue and a local test can call for different actions.
- **A particular person.** Add `--member HANDLE` for a named teammate or `--mine` for the current user's preferences. Search across that person's devices. A source label identifies the collecting member. Inspect message role, origin, quoted material and outcomes before attributing a decision.
- **This workstation.** For a local browser profile, connector or machine setup, start with `--local` (current member and device). A shared report workflow can use team history in a separate lookup. If local evidence is missing, inspect the current setup. Another person's settings require independent verification on this machine.
- `sessiondb config show --json` shows the member and device for this connection. `sessiondb sources list` lists registered handles and devices. If the member or device cannot be established, say which scope is unknown.

## Search and open originals

1. Run `sessiondb search --mode hybrid --open-first -- "question or distinctive terms"` with the scope flags before `--`. The page shows up to three hits and includes the first hit’s original exchange on its branch. Add `--json` for structured fields such as speaker origin or timestamps. Use `--mode keyword` for a literal or quoted phrase and `--mode keyword --sort newest` for recent activity. Filters include `--harness`, `--role`, `--after`, `--before`, `--project PROJECT_UUID` and `--include-tools`. Continue with the returned cursor and the same query and filters.
2. Check the original against the task. For another hit, follow its `show_command`, which supplies `--leaf` when needed. Use `-B` and `-A` to inspect earlier context or later corrections. Expand truncated originals with `sessiondb messages content MESSAGE_ID`. On older clients, omit `--open-first` and open the relevant hit with its `show_command`.
3. Cite the message URL. Check the member, device, speaker, origin, date and branch. Use the project IDs and recorded cwd to assess applicability. For independent evidence, compare the source, harness and `native_session_id` from `sessiondb sessions show SESSION_ID --json` with the current conversation. Skip self-matches, repeated prompts and quoted recaps.

## Preferences and cross-session questions

Run separate SessionDB searches for a practice and for a competing choice, accepted exception or later correction. Use concrete decisions as search terms. Start with `--role user` for personal statements. Open the original exchanges from independent sessions and inspect how each decision ended. An assistant suggestion, a harness instruction or a question about someone's preferences provides different evidence from the user's own decision.

Explain the scope of each example and distinguish stated preferences from inferences. A project filter needs a SessionDB project UUID. If the requested scope, citation or evidence cannot be verified through SessionDB, say what is missing. An exact frequency claim needs a defined corpus and coverage.
