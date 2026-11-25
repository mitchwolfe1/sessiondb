"""Run paired, ephemeral agent trials against the browser task."""

import argparse
import hashlib
import json
import os
import random
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from fixture import (
    CUTOFF,
    DEFAULT_HOME_ENTRIES,
    PROMPT,
    grade,
    write_corpus,
    write_json,
)
from native import build_native, enable_embeddings, retrieval_metrics

HERE = Path(__file__).resolve().parent
STATUSES = [
    "Needs owner review",
    "Pending legal sign-off",
    "Approved for circulation",
    "Awaiting revisions",
    "Ready for publication",
    "On hold",
]
ISOLATION = [
    "--ignore-user-config",
    "--ephemeral",
    "--skip-git-repo-check",
    "--sandbox",
    "read-only",
    "--enable",
    "skip_host_skill_discovery",
    "--disable",
    "shell_tool",
    "--disable",
    "unified_exec",
    "--disable",
    "hooks",
    "--disable",
    "plugins",
    "--disable",
    "apps",
    "--disable",
    "memories",
    "-c",
    "suppress_unstable_features_warning=true",
    "-c",
    "project_doc_max_bytes=0",
    "-c",
    'web_search="disabled"',
    "-c",
    'approval_policy="never"',
]


def model_environment():
    names = (
        "HOME",
        "PATH",
        "LANG",
        "USER",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "CODEX_HOME",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "NO_PROXY",
        "SSL_CERT_FILE",
        "NODE_EXTRA_CA_CERTS",
    )
    return {k: os.environ[k] for k in names if k in os.environ}


def model_turn(model, system_prompt, prompt, options, cwd, env, *, timeout=180):
    instructions = cwd.parent / "instructions.txt"
    instructions.write_text(system_prompt)
    cmd = [
        "codex",
        "exec",
        *ISOLATION,
        "--model",
        model,
        "-c",
        'model_provider="openai"',
        "-c",
        'model_reasoning_effort="low"',
        "-c",
        f"model_instructions_file={json.dumps(str(instructions))}",
        *options,
        "--json",
        "-",
    ]
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        input=prompt,
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
    )
    events = [json.loads(line) for line in proc.stdout.split("\n") if line.strip()]
    if proc.returncode:
        errors = [e for e in events if e["type"] in {"error", "turn.failed"}]
        raise RuntimeError(str(errors)[-800:] if errors else proc.stderr[-800:])
    items = [e["item"] for e in events if e["type"] == "item.completed"]
    for item in items:
        if item["type"] == "error":
            raise RuntimeError(item["message"])
        if item["type"] in {"agent_message", "reasoning", "todo_list"}:
            continue
        if item["type"] != "mcp_tool_call" or item.get("server") != "workstation":
            raise RuntimeError(f"Unexpected tool in trial: {item['type']}")
    answers = [i["text"] for i in items if i["type"] == "agent_message"]
    completed = [e for e in events if e["type"] == "turn.completed"]
    if not answers or not completed:
        raise RuntimeError("Codex produced no completed answer")
    return answers[-1].strip(), completed[-1]["usage"]


def judge_trial(model, world, trace, answer, cwd, env, *, timeout=180):
    # Show only browser evidence for the outcome verdict. Model IDs, retrieval
    # calls, arm, elapsed time and the deterministic grade are runner-private.
    actions = [
        {"number": i, "args": e["args"], "result": e["result"]}
        for i, e in enumerate(trace, 1)
        if e["tool"] == "browser"
    ]
    packet = {
        "task": PROMPT,
        "status": world["status"],
        "report_tab": world["tab_id"],
        "browser_events": actions,
        "final_answer": answer,
    }
    system = (
        "Judge the observed browser actions and final answer. Pass only if the existing native "
        "profile's specified tab was read, its response showed the expected header, the final "
        "answer gives its status, and there was no restart or profile replacement attempt. "
        "A guess or connectivity check alone fails. Return only a JSON object with pass "
        "(boolean), read_event (event number or null), and reason (one sentence)."
    )
    raw, _ = model_turn(model, system, json.dumps(packet), [], cwd, env, timeout=timeout)
    verdict = json.loads(raw.removeprefix("```json").removesuffix("```").strip())
    if (
        not isinstance(verdict, dict)
        or type(verdict.get("pass")) is not bool
        or not isinstance(verdict.get("reason"), str)
        or "read_event" not in verdict
    ):
        raise ValueError("Invalid judge verdict")
    if verdict["pass"] and not any(
        a["number"] == verdict["read_event"] and "header" in a["result"]
        for a in actions
    ):
        raise ValueError("Judge cited no observed report read")
    return verdict


def run_trial(
    args, root, env, history, pair, arm, tab_id, status, *, home_entries=None
):
    directory = args.output / f"{pair:02d}-{arm}"
    directory.mkdir()
    trace_file = directory / "trace.jsonl"
    trace_file.touch()
    world = {
        "tab_id": tab_id,
        "status": status,
        "arm": arm,
        "history": history,
        "home_entries": DEFAULT_HOME_ENTRIES if home_entries is None else home_entries,
        "trace": str(trace_file),
    }
    world_file = root / "world.json"
    write_json(world_file, world)
    mcp_args = ["run", "--script", str(HERE / "workstation.py"), str(world_file)]
    options = [
        "-c",
        f"mcp_servers.workstation.command={json.dumps(shutil.which('uv'))}",
        "-c",
        f"mcp_servers.workstation.args={json.dumps(mcp_args)}",
        "-c",
        "mcp_servers.workstation.required=true",
        "-c",
        "mcp_servers.workstation.startup_timeout_sec=60",
    ]
    result = {"pair": pair, "arm": arm, "status": status, "tab_id": tab_id}
    start = time.monotonic()
    try:
        answer, usage = model_turn(
            args.model,
            "Carry out the user's task using the available workstation tools. "
            "For a single requested field, return only its value.",
            PROMPT,
            options,
            root / "actor",
            env,
            timeout=getattr(args, "turn_timeout", 180),
        )
        result.update(
            answer=answer, usage=usage, elapsed_s=round(time.monotonic() - start, 2)
        )
        trace = [json.loads(line) for line in trace_file.read_text().splitlines()]
        if any("runner_error" in event for event in trace):
            raise RuntimeError("A fixture tool failed; see trace.jsonl")
        result["score"] = grade(world, trace, answer)
        lookups = [e for e in trace if e["tool"] in {"search_history", "read_around"}]
        result.update(retrieval_metrics(lookups))
        result.update(
            tool_calls=len(trace),
            history_calls=len(lookups),
            history_bytes=sum(len(json.dumps(e["result"]).encode()) for e in lookups),
        )
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        result["error"] = str(error)
    if args.judge and "score" in result:
        try:
            result["judge"] = judge_trial(
                args.model, world, trace, answer, root / "actor", env, timeout=getattr(args, "turn_timeout", 180)
            )
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
            result["judge_error"] = str(error)
    write_json(directory / "result.json", result)
    if "error" in result:
        label = "ERROR: " + result["error"]
    else:
        label = "PASS" if result["score"]["pass"] else "FAIL"
        if "judge_error" in result:
            label += " (judge error)"
    print(f"Pair {pair} {arm}: {label}", flush=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, default=HERE / "corpus.json")
    parser.add_argument(
        "--home-listing", type=Path, help="JSON array of home directory basenames"
    )
    parser.add_argument(
        "--pairs", type=int, choices=range(1, len(STATUSES) + 1), default=3
    )
    parser.add_argument("--seed", type=int, default=secrets.randbits(32))
    parser.add_argument("--model", default="gpt-6.1-sol")
    parser.add_argument(
        "--judge", action="store_true", help="Also run a blind subscription judge"
    )
    parser.add_argument("--sessiondb-url", required=True, help="Seed and query this empty disposable SessionDB")
    parser.add_argument("--sessiondb-bin", default="sessiondb", help="SessionDB CLI executable")
    parser.add_argument("--embedding-profile", type=Path, help="Enabled native profile JSON for a third, hybrid arm")
    parser.add_argument("--turn-timeout",type=int,default=180,help="Maximum seconds per actor or judge turn")
    args = parser.parse_args()
    if args.turn_timeout <= 0:
        parser.error("--turn-timeout must be positive")
    args.sessiondb_bin = shutil.which(args.sessiondb_bin)
    if not args.sessiondb_bin or not os.environ.get("SESSIONDB_API_KEY"):
        parser.error("Supply a SessionDB CLI and the disposable instance's SESSIONDB_API_KEY")
    try:
        retrieval_metrics([])
    except ImportError:
        parser.error("Install tiktoken for retrieval metrics")
    args.output = args.output.expanduser().resolve()
    env = model_environment()
    auth = subprocess.run(
        ["codex", "login", "status"],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if (
        auth.returncode
        or "logged in using chatgpt" not in (auth.stdout + auth.stderr).lower()
    ):
        parser.error(
            "Sign into Codex with ChatGPT to run the subscription-backed pilot"
        )
    if not shutil.which("uv"):
        parser.error("Install uv to run the workstation MCP fixture")
    os.umask(0o077)
    corpus = args.corpus.read_bytes()
    home_entries = DEFAULT_HOME_ENTRIES
    if args.home_listing is not None:
        try:
            home_entries = json.loads(args.home_listing.expanduser().read_text())
        except (OSError, ValueError) as error:
            parser.error(f"Unable to read --home-listing: {error}")
        if not isinstance(home_entries, list) or not all(
            isinstance(entry, str)
            and entry
            and "/" not in entry
            and "\x00" not in entry
            for entry in home_entries
        ):
            parser.error("--home-listing must contain a JSON array of basenames")
    args.output.mkdir(parents=True, exist_ok=False)
    rng = random.Random(args.seed)
    statuses = rng.sample(STATUSES, args.pairs)
    summary = {
        "prompt": PROMPT,
        "history_before_utc": CUTOFF,
        "seed": args.seed,
        "model": args.model,
        "turn_timeout": args.turn_timeout,
        "codex_version": subprocess.check_output(
            ["codex", "--version"], text=True
        ).strip(),
        "history_backend": "sessiondb",
        "history_version": subprocess.check_output(
            [args.sessiondb_bin, "--version"], text=True
        ).strip(),
        "corpus_sha256": hashlib.sha256(corpus).hexdigest(),
        "home_entry_count": len(home_entries),
        "home_listing_sha256": hashlib.sha256(
            json.dumps(home_entries).encode()
        ).hexdigest(),
        "trials": [],
    }
    with tempfile.TemporaryDirectory(prefix="browser-trial-") as tmp:
        root = Path(tmp)
        (root / "actor").mkdir()
        documents = write_corpus(json.loads(corpus), root)
        history = build_native(documents, root, args.sessiondb_url, args.sessiondb_bin)
        conditions = ("off", "keyword")
        if args.embedding_profile:
            summary["embedding_profile"] = enable_embeddings(history, args.embedding_profile)
            conditions = ("off", "keyword", "hybrid")
        for pair, status in enumerate(statuses, start=1):
            tab_id = str(rng.randrange(1000, 1000000))
            start = (pair - 1) % len(conditions)
            arms = conditions[start:] + conditions[:start]
            for arm in arms:
                result = run_trial(
                    args,
                    root,
                    env,
                    history,
                    pair,
                    arm,
                    tab_id,
                    status,
                    home_entries=home_entries,
                )
                summary["trials"].append(result)
                write_json(args.output / "summary.json", summary)
    for arm in conditions:
        graded = [r for r in summary["trials"] if r["arm"] == arm and "score" in r]
        passed = sum(r["score"]["pass"] for r in graded)
        searched = sum(r["history_calls"] > 0 for r in graded)
        print(
            f"{arm}: {passed}/{len(graded)} graded trials passed, {searched} used history"
        )
    print(f"Results: {args.output / 'summary.json'}")
    return (
        2 if any("error" in r or "judge_error" in r for r in summary["trials"]) else 0
    )


if __name__ == "__main__":
    sys.exit(main())
