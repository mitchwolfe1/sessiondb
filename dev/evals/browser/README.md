# Browser retrieval pilot

A fresh agent is asked to read the approval status of a report left open in the user's browser. The benchmark measures whether on-demand access to past work helps the agent find the right browser connection and read a new value.

## Native SessionDB pilot

The `installed-skill-v1` protocol compares the teammate installation with a history-off control. Each condition runs in a fresh Codex CLI process with its ordinary coding-agent instructions and native shell tools. Both agents can explore a real synthetic home directory and inspect the simulated browser via the same browser MCP tool.

| Condition | Setup |
| --- | --- |
| Off | Synthetic home, shell, and browser. The control has no configured SessionDB connection or installed history skill. |
| Installed | The [team installer](../../../scripts/install-harnesses) installs the tracked [`sessiondb` skill](../../../skills/sessiondb/SKILL.md) and the Codex global instructions. A real `sessiondb` binary on `PATH` uses a saved connection, member and device. The agent chooses whether to load the skill, which search mode and scope to use, and whether to follow a hit’s `show_command`. |

The caller’s task names only the report. Search results, the skill body and sample commands are never added to that request. The runner checks Codex's discovered skill path and instruction marker before each trial. A seeded source belongs to member `pilot` on device `browser-workstation`, so `sessiondb search --mode hybrid --local` and branch-aware message reads work as on an installed workstation.

### Run a comparison

Use Linux with Bubblewrap, Python 3.11+, `uv`, and a Codex CLI signed in with ChatGPT using file-based credentials. The pilot uses Codex named permission profiles and the managed network proxy. Supply an empty disposable SessionDB with pgvector, its instance key, and the built CLI. From the repository root:

```sh
uv run --with tiktoken==0.14.0 python dev/evals/browser/run.py \
  --sessiondb-url http://127.0.0.1:58080 --sessiondb-bin target/debug/sessiondb \
  --embedding-profile /path/to/pilot-profile.json \
  --corpus dev/evals/browser/side-topic.json --seed 231126 \
  --output /tmp/sessiondb-installed-pilot --judge
```

Set `SESSIONDB_API_KEY` for the **disposable instance**. Setting its `public_url` to `http://sessiondb.test` gives the actor citations on its evaluation connection. Use the same embedding model, deployment, dimensions and query/document prefixes as the installation being evaluated. A profile in the [create format](../../../docs/examples/embedding-profile.json) must be enabled. If the disposable server already has its active profile, omit `--embedding-profile`. The runner waits for coverage after syncing the corpus and records the effective server-side prefixes in `summary.json`. For example, copy the nonsecret settings of an enabled, unauthenticated embedding endpoint from a configured installation **before switching connections**:

```sh
sessiondb embeddings show PROFILE_ID --json |
  jq '{name, provider, base_url, model, model_revision, dimensions, query_prefix,
       document_prefix, batch_size, timeout_seconds, include_tools, enabled}' > pilot-profile.json
```

Three paired states share the same prompt, tab ID, and current header within each pair. The starting condition alternates. Select `--model` for the actor and blind judge and `--turn-timeout` for their time limit. Use a new output directory for every run.

To compare two sets of instructions with history enabled in both conditions, pass `--baseline-install PATH` pointing to a trusted checkout or unpacked release containing `scripts/install-harnesses` and `skills/sessiondb/SKILL.md`. The baseline uses that installer's guidance and skill. The other condition uses the current checkout. Both use the same CLI binary, corpus, browser state, embedding profile and sandbox. The summary records the installer and skill hashes for each version. Use `actor_started_at` to align trace events with the task and `actor_elapsed_s` to compare time to answer independently of fixture setup and judging.

### Isolation and evidence

The controller runs the mock browser and seeds an empty instance from the [pre-cutoff corpus](side-topic.json). The actor shell has a read-only filesystem allowlist containing the synthetic home, Linux runtime, and the harness's installed instructions and sandbox executable. The code checkout, source transcripts, hidden report state, controller traces, and Codex subscription credential are outside that shell. The installed condition routes native HTTP requests through a per-trial proxy for `sessiondb.test`; it forwards fixture API reads and `POST /v1/search`. The proxy supplies the disposable instance key upstream. The control has networking disabled. A preflight checks the effective shell filesystem and direct loopback restrictions and confirms `--local` returns a seeded session on the installed connection.

The agent can list and explore the [139-entry home fixture](home-listing.json) with ordinary shell commands. Under `~/plugins` the native and BiDi connectors are both discoverable. The browser MCP simulates their behavior: the default browser is signed out, BiDi is disconnected, and the native connector can open the existing report tab. Only turns before October 27, 2026 UTC are seeded. In the side-topic corpus the useful browser procedure is exchange 12 of 13 in a campaign report conversation.

Each trial records shell commands and returned output, browser actions, actual search requests (including mode and scope), and API message reads. History tokens measure captured output of shell commands naming the SessionDB CLI. Lookup latency measures time at the fixture API proxy. The scorer checks the report read and final status separately from retrieval. Evidence gets timing credit when text from a labeled API result also appears in a completed shell output. The trial reports whether the skill was advertised and whether its body appeared in shell output. The judge sees only the observed browser actions, expected status and answer. Treat setup or inference errors separately from task failures. Keep private corpora and traces outside the repository.

Run the fixture tests with `python3 -m unittest discover -s dev/evals/browser`.

In a three-state run with `gpt-6.1-sol` and the engineering-search query prefix, installed agents read the skill, ran a local hybrid search, opened the original and cited it in all three successful tasks. The controls read the correct report in 0/3 cases. The blind judge agreed on all six outcomes. These are three paired states of the same synthetic task.

### Astra comparison of installed instructions

Six matched browser states compared the v0.2.0 guidance with [an experimental instruction to search alongside the first independent read-only check](https://github.com/mitchwolfe1/sessiondb/commit/934c64513b861602c737b6f9be56cbb4fa5e0c6a). Both arms ran `gpt-6.2-astra` with the v0.2.0 CLI, the same seeded archive, the same active Qwen query prefix and seed 231126. Each trial made one local hybrid search and opened the original exchange. The blind judge agreed with all twelve outcome grades.

| Metric | v0.2.0 | Experimental guidance |
| --- | ---: | ---: |
| Correct report | 6/6 | 6/6 |
| Supporting text visible before the first browser call | 6/6 | 0/6 |
| Cited the browser-access source in the final answer | 6/6 | 0/6 |
| Median time to first search | 9.1 s | 16.6 s |
| Median time to final answer | 17.7 s | 26.5 s |

With v0.2.0 the first browser call used the native connector in all six trials. With the experimental guidance it tried the signed-out default first in all six. The experimental wording changed both the global instruction and the skill text. This comparison measures their combined effect on one task.

## Historical tool-interface trials

Earlier runs exposed dedicated history tools and forced the chosen retrieval mode. The following measurements belong to that protocol.

In three paired trials on this synthetic fixture, the native keyword agents read the correct header 3/3 times and the history-off agents did so 0/3 times. The blind judge agreed on all six outcomes. In all three successes the first browser action preceded the verified excerpt, and the excerpt arrived before the report read. Each keyword trial received 1,047 tokens across two lookups. The six lookups took 23–27 ms each on the test host.

A frozen replay used the full text of 241 exchanges from 100 sessions. In 79 messages the display excerpt was shorter than the index input. SessionDB ranked the verified exchange first for each of three queries in both keyword and hybrid search. The fixed-query vector replay used the recorded Qwen exchange and query vectors on the evaluation host. The archive contained exchange vectors only. Live-provider trials use the synthetic fixture.

In the three-arm run with a live 4,096-dimensional Qwen provider, keyword and hybrid each completed 3/3 browser tasks, and history off completed 0/3. Both retrieval arms returned the verified instruction after the first browser action and before the successful report read in every completed task. Keyword returned 1,039 tokens per trial across two lookups and hybrid returned 1,117. The hybrid searches took 141–147 ms. Three actor timeouts and two judge timeouts were retried with the same browser states. The blind judge agreed with the deterministic grade on all nine outcomes.

A separate synthetic query, “reuse authenticated webpages without relaunching”, returned zero keyword hits. Hybrid placed the correct exchange first. The initial excerpt was the user's question, and opening the returned branch with `messages show -C 2` exposed the verified answer.
