# Browser retrieval pilot

A fresh agent is asked to read the approval status of a report left open in the user's browser. The benchmark measures whether on-demand access to past work helps the agent find the right browser connection and read a new value.

Run from the repository root with Python 3.11+, `uv`, the Codex CLI signed in with ChatGPT, and `hype-history` on PATH. The first run of `uv` resolves the pinned Python MCP SDK.

```sh
python3 dev/evals/browser/run.py --output /tmp/browser-pilot --judge
```

This runs three paired browser states, with a fresh ephemeral Codex process for each arm. Every pair shares its report tab ID and status. The starting arm alternates. Use `--seed` to repeat the fixtures and `--model` to pin the actor and judge model. The output directory must be new so runs keep their own traces.

The workstation exposes directory discovery and browser operations through a local MCP fixture. The native bridge sees the existing tabs. A BiDi broker is listening but disconnected, and the default browser opens signed out. The agent can discover all three routes using the workstation tools. The keyword arm adds search and a read around a matching message. Codex runs from an empty directory with user configuration, project instructions, host skill discovery, shell, plugins and web search disabled.

The [default home listing](home-listing.json) contains 139 synthetic entries in alphabetical order, with `plugins` at position 99. Use `--home-listing PATH` to replay a frozen workstation snapshot. The file is a JSON array of basenames in display order. The runner uses the same listing for both arms and records its size and digest in the results. Keep private snapshots outside the repository.

The runner builds a temporary `hype-history` keyword index from [synthetic passages](corpus.json). Only turns strictly before October 27, 2026 UTC enter the index or context reads. Each session in the input is one linear exchange. A reviewed private corpus with the same fields can be supplied via `--corpus PATH`; keep it and its run traces outside the repository. Message IDs identify the original sources, and the optional `supports_route` field is a judge annotation. Search and context tools return the eligible message text and source IDs. This is a reference backend for testing agent behavior while the SessionDB search API is in development.

Every tool call and result is recorded by the fixture. A pass requires a successful native connection, inspection of the correct tab, and a final answer with the current header value. Attempting to restart or replace the profile fails the task. The runner records whether a labeled history result returned before the first browser call and before the successful report read. This establishes the order in which evidence was available. The optional independent judge sees the browser trace, expected result, and final answer with the retrieval condition hidden. The summary records outcomes, tool usage and elapsed time. An inference or infrastructure error is recorded separately from a wrong answer.

To check the mock, cutoff and grader locally:

```sh
python3 -m unittest discover -s dev/evals/browser
```
