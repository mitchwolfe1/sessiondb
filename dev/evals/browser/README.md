# Browser retrieval pilot

A fresh agent is asked to read the approval status of a report left open in the user's browser. The benchmark measures whether on-demand access to past work helps the agent find the right browser connection and read a new value.

## Native SessionDB pilot

Run from the repository root with Python 3.11+, `uv`, the SessionDB CLI and the Codex CLI signed in with ChatGPT. Set `SESSIONDB_API_KEY` to the key for an empty, disposable SessionDB instance. The runner syncs the fixture into that instance using a temporary client configuration.

```sh
uv run --with tiktoken==0.14.0 python dev/evals/browser/run.py \
  --sessiondb-url http://127.0.0.1:58080 --sessiondb-bin target/debug/sessiondb \
  --corpus dev/evals/browser/side-topic.json --seed 231126 \
  --output /tmp/sessiondb-native-pilot --judge
```

The default comparison runs history off and SessionDB keyword search on three paired browser states. Each pair shares its report tab ID and status. Each arm runs in a fresh Codex process, and the starting arm rotates. Use `--seed` to repeat the fixtures and `--model` to pin the actor and judge model. Supply a new output directory for each run.

The workstation exposes directory discovery and browser operations through a local MCP fixture. The native bridge sees the existing tabs. A BiDi broker is listening but disconnected, and the default browser opens signed out. The agent can discover all three routes using the workstation tools. The keyword arm adds search and a read around a matching message. Codex runs from an empty directory with user configuration, project instructions, host skill discovery, shell, plugins and web search disabled.

The [default home listing](home-listing.json) contains 139 synthetic entries in alphabetical order, with `plugins` at position 99. Use `--home-listing PATH` to replay a frozen workstation snapshot. The file is a JSON array of basenames in display order. The runner uses the same listing across arms and records its size and digest in the results. Keep private snapshots outside the repository.

Only turns strictly before October 27, 2026 UTC enter the [fixture archive](corpus.json). Each fixture session is a linear conversation. An optional `index_text` field supplies the full original when the display excerpt is shorter. Use `--corpus PATH` for another frozen corpus in the same format. Keep private corpora and traces outside the repository.

Search and branch reads use the SessionDB CLI, SDK and HTTP API. The grader maps `supports_route` annotations to SessionDB message IDs through the original record references. An excerpt earns evidence credit when it includes both the labeled message ID and the connector name.

Every tool call and result is recorded by the fixture. A pass requires a successful native connection, inspection of the correct tab, and a final answer with the current header value. Attempting to restart or replace the profile fails the task. The runner records whether a labeled history result returned before the first browser call and before the successful report read. This establishes the order in which evidence was available. The optional independent judge sees the browser trace, expected result, and final answer with the retrieval condition hidden. The summary records outcomes, tool usage and elapsed time. An inference or infrastructure error is recorded separately from a wrong answer.

To check the workstation fixture, corpus cutoff and grader locally:

```sh
python3 -m unittest discover -s dev/evals/browser
```

The [side-topic fixture](side-topic.json) places the verified instructions in exchange 12 of 13 in a campaign report conversation. The native run records the total history payload in o200k_base tokens, elapsed milliseconds per lookup, task completion and evidence arrival relative to both browser actions. Use a fresh instance for each frozen corpus.

In three paired trials on this synthetic fixture, the native keyword agents read the correct header 3/3 times and the history-off agents did so 0/3 times. The blind judge agreed on all six outcomes. In all three successes the first browser action preceded the verified excerpt, and the excerpt arrived before the report read. Each keyword trial received 1,047 tokens across two lookups. The six lookups took 23–27 ms each on the test host.

A frozen replay used the full text of 241 exchanges from 100 sessions. In 79 messages the display excerpt was shorter than the index input. SessionDB ranked the verified exchange first for each of three queries in both keyword and hybrid search. The fixed-query vector replay used the recorded Qwen exchange and query vectors on the evaluation host. The archive contained exchange vectors only. Live-provider trials use the synthetic fixture.

Add `--embedding-profile PATH` to the native invocation to compare history off, keyword and hybrid. Supply an enabled profile and a disposable server with the PostgreSQL vector extension initialized. The runner waits for all exchange and member inputs, rotates arm order across paired states and records model identity, returned tokens, source timing and task outcomes. Use `--turn-timeout SECONDS` to set the actor and judge time limit.

In the three-arm run with a live 4,096-dimensional Qwen provider, keyword and hybrid each completed 3/3 browser tasks, and history off completed 0/3. Both retrieval arms returned the verified instruction after the first browser action and before the successful report read in every completed task. Keyword returned 1,039 tokens per trial across two lookups and hybrid returned 1,117. The hybrid searches took 141–147 ms. Three actor timeouts and two judge timeouts were retried with the same browser states. The blind judge agreed with the deterministic grade on all nine outcomes.

A separate synthetic query, “reuse authenticated webpages without relaunching”, returned zero keyword hits. Hybrid placed the correct exchange first. The initial excerpt was the user's question, and opening the returned branch with `messages show -C 2` exposed the verified answer.
