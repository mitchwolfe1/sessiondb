"""Run paired, ephemeral agent trials against the browser task."""

import argparse
from datetime import datetime
from contextlib import nullcontext
import hashlib
import json
import os
import random
import re
import secrets
import signal
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from threading import Thread

from fixture import (
    CUTOFF,
    DEFAULT_HOME_ENTRIES,
    PROMPT,
    STATUSES,
    grade,
    write_corpus,
    write_json,
)
from installed import ROOT, actor_setup, fixture_proxy, log_event, now
from native import build_native

HERE = Path(__file__).resolve().parent
JUDGE_ISOLATION = [
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


ACTOR_FEATURES = [
    "--disable", "hooks", "--disable", "plugins", "--disable", "apps", "--disable", "memories",
    "--disable", "shell_snapshot", "--enable", "shell_tool", "--enable", "unified_exec",
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


def codex_events(cmd, cwd, env, prompt, timeout, trace_path):
    """Timestamp shell results when Codex emits them so browser and CLI traces can be compared."""
    events = []
    errors = []
    with tempfile.TemporaryFile(mode="w+") as input_file, tempfile.TemporaryFile(mode="w+") as stderr:
        input_file.write(prompt)
        input_file.seek(0)
        with subprocess.Popen(cmd, cwd=cwd, env=env, stdin=input_file, stdout=subprocess.PIPE,
                              stderr=stderr, text=True, start_new_session=True) as proc:
            def collect():
                started = {}
                try:
                    for line in proc.stdout:
                        event = json.loads(line)
                        events.append(event)
                        item = event.get("item", {})
                        if trace_path is None or item.get("type") != "command_execution":
                            continue
                        at = now()
                        if event["type"] == "item.started":
                            started[item["id"]] = at
                        elif event["type"] == "item.completed":
                            log_event(trace_path, {"tool": "command_execution", "time": started.get(item["id"], at),
                                "completed_at": at, "command": item["command"], "exit_code": item.get("exit_code"),
                                "output": item.get("aggregated_output", "")})
                except (OSError, ValueError, KeyError) as error:
                    errors.append(error)
            reader = Thread(target=collect)
            reader.start()
            try:
                proc.wait(timeout=timeout)
            finally:
                if proc.poll() is None:
                    os.killpg(proc.pid, signal.SIGKILL)
                    proc.wait()
                reader.join()
        stderr.seek(0)
        diagnostic = stderr.read()[-800:]
    if errors:
        raise RuntimeError(f"Could not audit Codex events: {errors[0]}")
    if proc.returncode or any(e["type"] == "turn.failed" for e in events):
        errors = [e for e in events if e["type"] in {"error", "turn.failed"}]
        raise RuntimeError(str(errors)[-800:] if errors else diagnostic)
    return events


def model_turn(model, system_prompt, prompt, options, cwd, env, *, timeout=180, trace_path=None, audit_work=False):
    if trace_path is None:  # Blind judge: an independent, tool-free turn.
        instructions = cwd.parent / "judge-instructions.txt"
        instructions.write_text(system_prompt)
        policy = [*JUDGE_ISOLATION, "-c", f"model_instructions_file={json.dumps(str(instructions))}"]
    else:
        # Keep the normal Codex instructions, skill discovery and shell tools.
        policy = ["--skip-git-repo-check", "--ignore-rules", *ACTOR_FEATURES]
        if not audit_work:
            policy.insert(0, "--ephemeral")
    cmd = ["codex", "exec", *policy, "--model", model, "-c", 'model_provider="openai"',
           "-c", 'model_reasoning_effort="low"', *options, "--json", "-"]
    events = codex_events(cmd, cwd, env, prompt, timeout, trace_path)
    items = [e["item"] for e in events if e["type"] == "item.completed"]
    for item in items:
        if item["type"] == "error":
            message = item["message"]
            if message.startswith("Falling back from WebSockets to HTTPS transport.") and any(
                e["type"] == "turn.completed" for e in events
            ):
                if trace_path:
                    log_event(trace_path, {"tool": "transport", "time": now(), "message": message})
                continue
            raise RuntimeError(message)
        if item["type"] in {"agent_message", "reasoning", "todo_list"}:
            continue
        if trace_path is not None and item["type"] == "command_execution":
            continue
        if (trace_path is None or item["type"] != "mcp_tool_call" or
                (item.get("server"), item.get("tool")) != ("workstation", "browser")):
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


def usage_metrics(trace, encoding):
    calls = [e for e in trace if e["tool"] == "sessiondb" and e["path"] != "/v1/info"
             and not e["path"].startswith("/v1/embedding-profiles")]
    searches = [e["request"] for e in calls if e["path"] == "/v1/search"]
    # Codex can emit item.started after a fast HTTP request already completed.
    # Measure shell output by the invoked CLI name, and HTTP latency independently.
    cli_name = re.compile(r"""(?<![\w.-])sessiondb(?:["'])?\s""")
    outputs = [e["output"] for e in trace if e["tool"] == "command_execution" and cli_name.search(e["command"])]
    tool_outputs = [e["output"] for e in trace if e["tool"] == "command_execution"]
    tool_outputs.extend(json.dumps(e["result"]) for e in trace if e["tool"] == "browser")
    return dict(tool_calls=sum(e["tool"] in {"command_execution", "browser"} for e in trace),
                  tool_output_tokens=sum(len(encoding.encode_ordinary(text)) for text in tool_outputs),
                  skill_body_observed=any(e["tool"] == "command_execution" and "# Search past work with SessionDB" in e["output"]
                                          for e in trace),
                  history_calls=len(calls), searches=searches,
                  history_bytes=sum(len(o.encode()) for o in outputs),
                  history_tokens=sum(len(encoding.encode_ordinary(o)) for o in outputs),
                  lookup_ms=[round((datetime.fromisoformat(c["completed_at"]) -
                                    datetime.fromisoformat(c["time"])).total_seconds() * 1000) for c in calls],
                  transport_fallbacks=sum(e["tool"] == "transport" for e in trace),
                  source_reads=[e["path"] for e in calls if e["path"].startswith("/v1/messages/")
                                or e["path"].endswith("/messages")])


def model_work(control, usage):
    """Count provider usage increments and outer tool dispatches in one disposable Codex session."""
    logs = list((control / "sessions").rglob("*.jsonl"))
    if len(logs) != 1:
        raise RuntimeError(f"Expected one actor transcript for work accounting, found {len(logs)}")
    samples = dispatches = 0
    previous = {"input_tokens": 0, "output_tokens": 0}
    with logs[0].open() as lines:
        for line in lines:
            event = json.loads(line)
            payload = event.get("payload") or {}
            if event["type"] == "response_item" and payload.get("type") in {
                "function_call", "custom_tool_call", "tool_search_call", "local_shell_call", "web_search_call"
            }:
                dispatches += 1
            if event["type"] != "event_msg" or payload.get("type") != "token_count":
                continue
            info = payload.get("info") or {}
            total = info.get("total_token_usage")
            if not total:
                continue
            counts = {k: total[k] for k in previous}
            if counts == previous:
                continue
            last = info.get("last_token_usage") or {}
            if any(counts[k] - previous[k] != last.get(k) for k in counts):
                raise RuntimeError("Actor transcript has a missing or inconsistent model usage increment")
            samples += 1
            previous = counts
    if not samples or any(previous[k] != usage[k] for k in previous):
        raise RuntimeError("Actor transcript and final model usage disagree")
    return {"model_responses": samples, "model_tool_dispatches": dispatches}


def run_trial(
    args, root, env, history, pair, arm, tab_id, status, *, home_entries=DEFAULT_HOME_ENTRIES, install_root=ROOT
):
    directory = args.output / f"{pair:02d}-{arm}"
    directory.mkdir()
    trace_file = directory / "trace.jsonl"
    trace_file.touch()
    world = {
        "tab_id": tab_id,
        "status": status,
        "home": str(root / "home"),
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
        installed = arm != "off"
        upstream = (fixture_proxy(history["url"], os.environ["SESSIONDB_API_KEY"], history["native_labels"], trace_file)
                    if installed else nullcontext(None))
        with upstream as connection:
            cwd, actor_env = actor_setup(root, connection, history, home_entries, install_root)
            # Check the harness's actual discovery output without handing the actor a skill prompt.
            discovered = subprocess.check_output(["codex", "debug", "prompt-input", *ACTOR_FEATURES], cwd=cwd, env=actor_env,
                                                 text=True, stderr=subprocess.PIPE, timeout=30)
            skill_path = root / "home/.agents/skills/sessiondb/SKILL.md"
            expected_skill = install_root / "skills/sessiondb/SKILL.md"
            description = expected_skill.read_text().split("description: ", 1)[1].splitlines()[0]
            advertised = (str(skill_path.parent.parent) in discovered and
                          f"sessiondb: {description}" in discovered)
            if advertised and skill_path.read_bytes() != expected_skill.read_bytes():
                raise RuntimeError("Installed skill differs from the selected package")
            guidance = "<!-- sessiondb:start -->" in discovered
            if (advertised, guidance) != (installed, installed):
                raise RuntimeError("Harness discovery disagrees with the trial condition")
            result.update(skill_advertised=advertised, installed_instructions=guidance)
            # Keep setup probes separate from actions chosen by the model.
            trace_file.replace(directory / "setup-trace.jsonl")
            trace_file.touch()
            result["actor_started_at"] = now()
            actor_start = time.monotonic()
            answer, usage = model_turn(args.model, None, PROMPT, options, cwd, actor_env,
                                      timeout=args.turn_timeout, trace_path=trace_file, audit_work=args.audit_model_work)
            result["actor_elapsed_s"] = round(time.monotonic() - actor_start, 2)
            if args.audit_model_work:
                result["work"] = model_work(root / "codex", usage)
        result.update(answer=answer, usage=usage, elapsed_s=round(time.monotonic() - start, 2))
        trace = [json.loads(line) for line in trace_file.read_text().splitlines()]
        if any("runner_error" in event for event in trace):
            raise RuntimeError("A fixture tool failed; see trace.jsonl")
        result["score"] = grade(world, trace, answer)
        import tiktoken
        result.update(usage_metrics(trace, tiktoken.get_encoding("o200k_base")))
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        result["error"] = str(error)
    if args.judge and "score" in result:
        try:
            judge_cwd = root / "judge"
            judge_cwd.mkdir()
            result["judge"] = judge_trial(
                args.model, world, trace, answer, judge_cwd, env, timeout=getattr(args, "turn_timeout", 180)
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


def install_hashes(directory):
    return {
        "installer_sha256": hashlib.sha256((directory / "scripts/install-harnesses").read_bytes()).hexdigest(),
        "skill_sha256": hashlib.sha256((directory / "skills/sessiondb/SKILL.md").read_bytes()).hexdigest(),
    }


def trial_order(conditions, pair):
    cycle, start = divmod(pair - 1, len(conditions))
    order = conditions[start:] + conditions[:start]
    # Reverse the second set of rotations to cover every three-condition order.
    return order[::-1] if len(conditions) == 3 and cycle % 2 else order


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
    parser.add_argument(
        "--baseline-install", type=Path,
        help="Compare a previous installer and skill with the current ones, with history enabled for both",
    )
    parser.add_argument("--baseline-bin", help="Use the released CLI for the baseline installation")
    parser.add_argument("--include-history-off", action="store_true",
                        help="Also run the unaided control when comparing two installations")
    parser.add_argument("--audit-model-work", action="store_true",
                        help="Count actual model responses and outer tool calls from the disposable Codex home")
    parser.add_argument("--embedding-profile", type=Path, help="Enabled profile to create on a new disposable instance; otherwise use its active profile")
    parser.add_argument("--turn-timeout",type=int,default=180,help="Maximum seconds per actor or judge turn")
    args = parser.parse_args()
    if sys.platform != "linux" or not shutil.which("bwrap"):
        parser.error("The shell-enabled pilot needs Linux with Bubblewrap and Codex named permission profiles")
    if args.turn_timeout <= 0:
        parser.error("--turn-timeout must be positive")
    args.sessiondb_bin = shutil.which(args.sessiondb_bin)
    if args.baseline_bin:
        if not args.baseline_install:
            parser.error("--baseline-bin requires --baseline-install")
        args.baseline_bin = shutil.which(args.baseline_bin)
        if not args.baseline_bin:
            parser.error("Cannot find the baseline CLI")
        args.baseline_bin = str(Path(args.baseline_bin).resolve())
    if not args.sessiondb_bin or not os.environ.get("SESSIONDB_API_KEY"):
        parser.error("Supply a SessionDB CLI and the disposable instance's SESSIONDB_API_KEY")
    try:
        import tiktoken
        tiktoken.get_encoding("o200k_base")
    except ImportError:
        parser.error("Install tiktoken for history output metrics")
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
            and entry not in {".", ".."}
            and "/" not in entry
            and "\x00" not in entry
            for entry in home_entries
        ):
            parser.error("--home-listing must contain a JSON array of basenames")
    installations = {}
    if not args.baseline_install or args.include_history_off:
        installations["off"] = ROOT
    if args.baseline_install:
        baseline = args.baseline_install.expanduser().resolve()
        try:
            baseline_hashes = install_hashes(baseline)
        except OSError as error:
            parser.error(f"Cannot read the baseline installation: {error}")
        installations["baseline"] = baseline
    installations["installed"] = ROOT
    args.output.mkdir(parents=True, exist_ok=False)
    rng = random.Random(args.seed)
    statuses = rng.sample(STATUSES, args.pairs)
    summary = {
        "protocol": "installed-guidance-v1" if args.baseline_install else "installed-skill-v1",
        "conditions": list(installations),
        **install_hashes(ROOT),
        "prompt": PROMPT,
        "history_before_utc": CUTOFF,
        "seed": args.seed,
        "model": args.model,
        "turn_timeout": args.turn_timeout,
        "codex_version": subprocess.check_output(
            ["codex", "--version"], text=True
        ).strip(),
        "history_backend": "sessiondb",
        "audit_model_work": args.audit_model_work,
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
    if args.baseline_install:
        summary["baseline_install"] = baseline_hashes
        baseline_bin = args.baseline_bin or args.sessiondb_bin
        summary["baseline_install"]["cli_version"] = subprocess.check_output([baseline_bin, "--version"], text=True).strip()
        summary["baseline_install"]["cli_sha256"] = hashlib.sha256(Path(baseline_bin).read_bytes()).hexdigest()
    summary["cli_sha256"] = hashlib.sha256(Path(args.sessiondb_bin).read_bytes()).hexdigest()
    # Codex creates executable aliases in CODEX_HOME and requires a non-/tmp location.
    scratch = Path.home() / ".local/state/sessiondb/evals/work"
    scratch.mkdir(mode=0o700, parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="browser-", dir=scratch) as tmp:
        root = Path(tmp)
        documents = write_corpus(json.loads(corpus), root)
        history = build_native(documents, root, args.sessiondb_url, args.sessiondb_bin, args.embedding_profile)
        summary["embedding_profile"] = history["profile"]
        conditions = tuple(installations)
        for pair, status in enumerate(statuses, start=1):
            tab_id = str(rng.randrange(1000, 1000000))
            for arm in trial_order(conditions, pair):
                trial_root = Path(tempfile.mkdtemp(prefix="trial-", dir=root))
                result = run_trial(
                    args,
                    trial_root,
                    env,
                    {**history, "cli": args.baseline_bin} if arm == "baseline" and args.baseline_bin else history,
                    pair,
                    arm,
                    tab_id,
                    status,
                    home_entries=home_entries,
                    install_root=installations[arm],
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
