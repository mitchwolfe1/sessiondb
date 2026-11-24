"""Run the browser fixture against an explicitly selected disposable SessionDB."""

import json
import subprocess
from pathlib import Path
from uuid import UUID


def command(history, *args):
    return subprocess.run(
        [
            history["cli"],
            "--server",
            history["url"],
            "--config",
            history["config"],
            "--state-dir",
            history["state"],
            "--json",
            *args,
        ],
        check=True,
        capture_output=True,
        text=True,
        timeout=90,
    )


def load_page(history, *args):
    return json.loads(command(history, *args).stdout)


def build_native(documents, root, url, binary):
    history = {
        "cli": str(Path(binary).resolve()),
        "url": url,
        "config": str(root / "client.toml"),
        "state": str(root / "client-state"),
    }
    command(history, "connect", url)
    if load_page(history, "sessions", "list", "--limit", "1")["data"]:
        raise ValueError("The browser corpus requires an empty disposable SessionDB")
    command(
        history, "sources", "add", "--harness", "codex", "--root", str(root / "sources")
    )
    command(history, "sync")

    # Map judge labels through the public native anchors once. The tools given
    # to the actor always retrieve content from SessionDB using its actual IDs.
    labeled = {
        m["id"] for ms in documents.values() for m in ms if m.get("supports_route")
    }
    seen, native_labels = set(), []
    cursor = []
    while True:
        page = load_page(history, "sessions", "list", "--limit", "200", *cursor)
        for session in page["data"]:
            doc = session["native_session_id"]
            if doc not in documents or session["message_count"] != len(documents[doc]):
                raise ValueError("Native catalog differs from the frozen corpus")
            seen.add(doc)
            if not any(m["id"] in labeled for m in documents[doc]):
                continue
            position = []
            while True:
                messages = load_page(
                    history,
                    "sessions",
                    "messages",
                    session["id"],
                    "--limit",
                    "200",
                    *position,
                )
                native_labels.extend(
                    m["id"]
                    for m in messages["data"]
                    if any(r["record_id"] in labeled for r in m["source_refs"])
                )
                if not messages["next_cursor"]:
                    break
                position = ["--cursor", messages["next_cursor"]]
        if not page["next_cursor"]:
            break
        cursor = ["--cursor", page["next_cursor"]]
    if len(native_labels) != len(labeled) or seen != set(documents):
        raise ValueError("Native corpus or verified source labels are incomplete")
    history["native_labels"] = native_labels
    return history


def lookup(history, name, args, *, mode="keyword"):
    if name == "search_history":
        flags = ["--cursor", args["cursor"]] if args.get("cursor") else []
        try:
            result = load_page(history, "search", "--mode", mode, *flags, "--", args["query"])
        except subprocess.CalledProcessError as error:
            if "invalid_query" in error.stderr or "invalid_cursor" in error.stderr:
                return {
                    "error": "Use positive keywords and the cursor from the same query"
                }
            raise
        return {
            "messages": [
                {
                    "session": m["session_id"],
                    "id": m["message_id"],
                    "at": m["timestamp"],
                    "role": m["role"],
                    "text": m["excerpt"],
                    "url": m["url"],
                    "leaf_id": m["leaf_id"],
                }
                for m in result["data"]
            ],
            "next_cursor": result["next_cursor"],
        }

    try:
        anchor = str(UUID(args["message_id"]))
        leaf = ["--leaf", str(UUID(args["leaf_id"]))] if args.get("leaf_id") else []
    except (ValueError, TypeError):
        return {"error": "Use a message ID from search"}
    try:
        result = load_page(history, "messages", "show", anchor, "-C", "2", *leaf)
    except subprocess.CalledProcessError as error:
        if error.returncode == 4 or "invalid_branch" in error.stderr:
            return {"error": "Unknown anchor or branch"}
        raise
    return {
        "messages": [
            {
                "session": m["session_id"],
                "id": m["id"],
                "at": m["timestamp"],
                "role": m["role"],
                "text": "\n".join(
                    b["text"] for b in m["content"] if b["type"] == "text"
                ),
                "content_url": m["content_url"],
            }
            for m in result["data"]
        ],
        "branch_choices": result["branch_choices"],
    }


def retrieval_metrics(lookups):
    from datetime import datetime
    import tiktoken

    encoding = tiktoken.get_encoding("o200k_base")
    return {
        "history_tokens": sum(
            len(encoding.encode_ordinary(json.dumps(e["result"]))) for e in lookups
        ),
        "lookup_ms": [
            round(
                (
                    datetime.fromisoformat(e["completed_at"])
                    - datetime.fromisoformat(e["time"])
                ).total_seconds()
                * 1000
            )
            for e in lookups
        ],
    }

def enable_embeddings(history, profile_file):
    """Wait for the same complete frozen corpus before starting any actor."""
    import time

    if not json.loads(profile_file.read_text()).get("enabled"):
        raise ValueError("Supply an enabled immutable profile for the hybrid trial")
    profile = load_page(history, "embeddings", "create", str(profile_file))
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        detail = load_page(history, "embeddings", "show", profile["id"])
        progress = detail["progress"]
        if progress["ready_inputs"] and not progress["pending_inputs"]:
            return {"model": profile["model"], "revision": profile["model_revision"],
                    "dimensions": profile["dimensions"], "ready_inputs": progress["ready_inputs"]}
        time.sleep(1)
    raise RuntimeError("The frozen corpus did not finish embedding within ten minutes")
