# Browser retrieval pilot

A fresh agent is asked to read the approval status of a report left open in the user's browser. The benchmark measures whether on-demand access to past work helps the agent find the right browser connection and read a new value.

Run the legacy baseline from the repository root with Python 3.11+, `uv`, the Codex CLI signed in with ChatGPT, and `hype-history` on PATH. The first run of `uv` resolves the pinned Python MCP SDK.

```sh
python3 dev/evals/browser/run.py --output /tmp/browser-pilot --judge
```

This runs three paired browser states, with a fresh ephemeral Codex process for each arm. Every pair shares its report tab ID and status. The starting arm alternates. Use `--seed` to repeat the fixtures and `--model` to pin the actor and judge model. The output directory must be new so runs keep their own traces.

The workstation exposes directory discovery and browser operations through a local MCP fixture. The native bridge sees the existing tabs. A BiDi broker is listening but disconnected, and the default browser opens signed out. The agent can discover all three routes using the workstation tools. The keyword arm adds search and a read around a matching message. Codex runs from an empty directory with user configuration, project instructions, host skill discovery, shell, plugins and web search disabled.

The [default home listing](home-listing.json) contains 139 synthetic entries in alphabetical order, with `plugins` at position 99. Use `--home-listing PATH` to replay a frozen workstation snapshot. The file is a JSON array of basenames in display order. The runner uses the same listing for both arms and records its size and digest in the results. Keep private snapshots outside the repository.

The runner builds a temporary `hype-history` keyword index from [synthetic passages](corpus.json). Only turns strictly before October 27, 2026 UTC enter the index or context reads. Each session in the baseline corpus is a linear conversation. A reviewed private corpus with the same fields can be supplied via `--corpus PATH`; keep it and its run traces outside the repository. Message IDs identify the original sources, and the optional `supports_route` field is a judge annotation. Search and context tools return the eligible message text and source IDs. This backend provides the migration baseline for comparison with native retrieval.

Every tool call and result is recorded by the fixture. A pass requires a successful native connection, inspection of the correct tab, and a final answer with the current header value. Attempting to restart or replace the profile fails the task. The runner records whether a labeled history result returned before the first browser call and before the successful report read. This establishes the order in which evidence was available. The optional independent judge sees the browser trace, expected result, and final answer with the retrieval condition hidden. The summary records outcomes, tool usage and elapsed time. An inference or infrastructure error is recorded separately from a wrong answer.

To check the mock, cutoff and grader locally:

```sh
python3 -m unittest discover -s dev/evals/browser
```

## Native SessionDB pilot

Supply an empty disposable SessionDB and its instance key in the environment. The runner loads the pre-cutoff corpus through the native CLI using temporary client configuration. Search and read-around use the SDK and HTTP API. The grader maps the fixture labels to actual message IDs through native source references. An excerpt earns evidence credit when the agent receives the labeled source with the connector name visible.

```sh
uv run --with tiktoken==0.14.0 python dev/evals/browser/run.py \
  --sessiondb-url http://127.0.0.1:58080 --sessiondb-bin target/debug/sessiondb \
  --corpus dev/evals/browser/side-topic.json --seed 231126 \
  --output /tmp/sessiondb-native-pilot --judge
```

The [side-topic fixture](side-topic.json) places the verified instructions in exchange 12 of 13 in a campaign report conversation. The native run records the total history payload in o200k_base tokens, elapsed milliseconds per lookup, task completion and evidence arrival relative to both browser actions. Use a fresh instance for each frozen corpus. The legacy baseline remains available with the original invocation.

In three paired trials on this synthetic fixture, the native keyword agents read the correct header 3/3 times and the history-off agents did so 0/3 times. The blind judge agreed on all six outcomes. In all three successes the first browser action preceded the verified excerpt, and the excerpt arrived before the report read. Each keyword trial received 1,047 tokens across two lookups. The six lookups took 23–27 ms each on the test host.

A separate local replay used 241 frozen exchanges from 100 sessions. For the three original successful hybrid queries, native keyword search placed the verified source excerpt at ranks 25, 11 and 13. This measures retrieval for those fixed queries. The paired browser outcomes above were measured on the synthetic fixture.
