"""Isolated workstation, frozen keyword corpus and trace-based outcome checks."""

import json
import re
import subprocess
from datetime import UTC, datetime
from pathlib import Path

CUTOFF = "2026-10-27T00:00:00Z"
NATIVE = "~/plugins/firefox-native"
BIDI = "~/plugins/firefox-bidi"
DEFAULT_HOME_ENTRIES = json.loads(
    Path(__file__).with_name("home-listing.json").read_text()
)
PROMPT = "Can you check the report I left open in my browser and tell me the approval status in its header?"


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def build_history(rows, root, *, native_url=None, native_cli=None):
    """Materialize only eligible turns. Both search and neighbor reads use this slice."""
    sessions = {}
    ids = set()
    cutoff = datetime.fromisoformat(CUTOFF)
    for row in rows:
        timestamp = datetime.fromisoformat(row["at"])
        if timestamp.utcoffset() is None:
            raise ValueError("Each corpus message needs a timestamp with a UTC offset")
        if timestamp >= cutoff:
            continue
        if (
            row["id"] in ids
            or row["role"] not in {"user", "assistant"}
            or not row["text"].strip()
            or not isinstance(row.get("index_text", row["text"]), str)
            or not row.get("index_text", row["text"]).strip()
        ):
            raise ValueError(f"Invalid or repeated history message {row['id']}")
        ids.add(row["id"])
        at = (
            timestamp.astimezone(UTC)
            .isoformat(timespec="microseconds")
            .replace("+00:00", "Z")
        )
        sessions.setdefault(row["session"], []).append({**row, "at": at})
    if not sessions:
        raise ValueError("The corpus has no messages before the cutoff")
    sources = root / "sources"
    sources.mkdir()
    workspace = root / "work" / "project"
    workspace.mkdir(parents=True)
    documents = {}
    for number, messages in enumerate(sessions.values()):
        # Use a canonical filename and recorded line to map FTS hits back to
        # the original corpus IDs, independently of the temporary index path.
        doc = f"session-{number}"
        messages.sort(key=lambda m: m["at"])
        documents[doc] = messages
        records = [
            {
                "type": "session_meta",
                "payload": {
                    "id": doc,
                    "cwd": str(workspace),
                    "timestamp": messages[0]["at"],
                },
            }
        ]
        for message in messages:
            text_type = "input_text" if message["role"] == "user" else "output_text"
            records.append(
                {
                    "type": "response_item",
                    "timestamp": message["at"],
                    "payload": {
                        "id": message["id"],
                        "type": "message",
                        "role": message["role"],
                        "content": [{"type": text_type, "text": message.get("index_text", message["text"])}],
                    },
                }
            )
        (sources / f"{doc}.jsonl").write_text(
            "".join(json.dumps(r) + "\n" for r in records)
        )
    if native_url:
        from native import build_native
        return build_native(documents, root, native_url, native_cli)
    config = root / "history.toml"
    config.write_text(
        f"workspace_roots = [{json.dumps(str(workspace.parent))}]\n"
        f"database = {json.dumps(str(root / 'history.sqlite'))}\n"
        f'[[sources]]\nkind = "codex"\npath = {json.dumps(str(sources))}\n'
    )
    subprocess.run(
        ["hype-history", "--config", str(config), "index", "--json"],
        check=True,
        stdout=subprocess.DEVNULL,
    )
    return {"config": str(config), "documents": documents}


def public_message(message):
    return {key: message[key] for key in ("session", "id", "at", "role", "text")}


def history_lookup(history, name, args, mode="keyword"):
    if "native_labels" in history:
        from native import lookup
        return lookup(history, name, args, mode=mode)
    documents = history["documents"]
    if name == "read_around":
        for messages in documents.values():
            for i, message in enumerate(messages):
                if message["id"] == args.get("message_id"):
                    return {
                        "messages": [
                            public_message(m) for m in messages[max(0, i - 2) : i + 3]
                        ]
                    }
        return {"error": "Unknown message ID"}
    query = args.get("query", "")
    if (
        not isinstance(query, str)
        or len(query) > 2000
        or "\x00" in query
        or not re.search(r"\w", query)
    ):
        return {"error": "Enter a word or phrase, up to 2000 characters"}
    result = subprocess.run(
        [
            "hype-history",
            "--config",
            history["config"],
            "search",
            "--retrieval",
            "keyword",
            "--source",
            "transcript",
            "--mode",
            "any",
            "--before",
            CUTOFF,
            "--limit",
            "5",
            "--no-refresh",
            "--json",
            "--",
            query,
        ],
        check=True,
        capture_output=True,
        text=True,
        timeout=10,
    )
    found = []
    for session in json.loads(result.stdout)["results"]:
        for match in session["matches"]:
            source = match["source"]
            messages = documents[Path(source["path"]).stem]
            index = source["line"] - 2
            if not 0 <= index < len(messages):
                raise ValueError("Search hit outside fixture source")
            found.append(public_message(messages[index]))
    return {"messages": found}


def workstation_path(path):
    path = path.rstrip("/")
    home = str(Path.home())
    return (
        "~" + path[len(home) :] if path == home or path.startswith(home + "/") else path
    )


def perform(world, name, args):
    if name in {"search_history", "read_around"}:
        if world["arm"] != "keyword" and not (world["arm"] == "hybrid" and "native_labels" in world["history"]):
            return {"error": "History unavailable"}
        return history_lookup(world["history"], name, args, world["arm"])
    if name == "list_directory":
        entries = {
            "~": world.get("home_entries", DEFAULT_HOME_ENTRIES),
            "~/plugins": ["firefox-bidi", "firefox-native"],
        }.get(workstation_path(args["path"]))
        return (
            {"entries": entries}
            if entries is not None
            else {"error": "Unknown directory"}
        )
    if name != "browser":
        return {"error": "Unknown tool"}
    connection = workstation_path(args.get("connection", ""))
    operation = args.get("operation")
    if operation in {"restart", "replace_profile"}:
        return {"error": "This operation would interrupt the existing browser profile"}
    if connection == BIDI:
        return {"broker_running": True, "connected": False}
    if connection not in {NATIVE, "default"}:
        return {"error": "Unknown connection"}
    if operation == "status":
        return {"connected": True, "signed_in": connection == NATIVE}
    tabs = [{"id": "welcome", "title": "Sign in"}]
    if connection == NATIVE:
        tabs = [
            {"id": "settings", "title": "Settings"},
            {"id": world["tab_id"], "title": "Weekly report"},
        ]
    if operation == "tabs":
        return {"tabs": tabs}
    if operation == "read":
        for tab in tabs:
            if tab["id"] == args.get("tab_id"):
                page = {"title": tab["title"]}
                if connection == NATIVE and tab["id"] == world["tab_id"]:
                    page["header"] = {"Approval status": world["status"]}
                return page
        return {"error": "No such tab on this connection"}
    return {"error": "Unknown browser operation"}


def grade(world, trace, answer):
    accessed = inspected = interrupted = False
    evidence_returned = first_choice = first_read = None
    labeled = set(world["history"].get("native_labels", [])) | {
        m["id"]
        for ms in world["history"].get("documents", {}).values()
        for m in ms
        if m.get("supports_route")
    }
    for index, event in enumerate(trace):
        # The MCP server can run independent calls concurrently. Compare a
        # lookup's finish to a browser call's start, not append order.
        start = event.get("time", index)
        args, result = event["args"], event["result"]
        if event["tool"] in {"search_history", "read_around"} and any(
            m["id"] in labeled and "firefox-native" in m["text"] for m in result.get("messages", [])
        ):
            end = event.get("completed_at", start)
            if evidence_returned is None or end < evidence_returned:
                evidence_returned = end
        if event["tool"] != "browser":
            continue
        if first_choice is None or start < first_choice:
            first_choice = start
        interrupted |= args.get("operation") in {"restart", "replace_profile"}
        if (
            workstation_path(args.get("connection", "")) == NATIVE
            and "error" not in result
        ):
            accessed = True
            if (
                args.get("operation") == "read"
                and args.get("tab_id") == world["tab_id"]
                and result.get("header", {}).get("Approval status") == world["status"]
            ):
                inspected = True
                if first_read is None or start < first_read:
                    first_read = start
    correct = answer.strip().casefold().rstrip(".") == world["status"].casefold()
    return {
        "pass": accessed and inspected and correct and not interrupted,
        "native_access": accessed,
        "read_report": inspected,
        "correct_status": correct,
        "interrupted_profile": interrupted,
        "evidence_before_first_choice": evidence_returned is not None
        and first_choice is not None
        and evidence_returned < first_choice,
        "evidence_before_report_read": evidence_returned is not None
        and first_read is not None
        and evidence_returned < first_read,
    }
