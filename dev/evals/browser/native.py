"""Run the browser fixture against an explicitly selected disposable SessionDB."""

import json
import subprocess
from pathlib import Path
from installed import DEVICE, MEMBER


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


def build_native(documents, root, url, binary, profile_file=None):
    history = {
        "cli": str(Path(binary).resolve()),
        "url": url,
        "config": str(root / "client.toml"),
        "state": str(root / "client-state"),
    }
    # Pin the selected CLI and keep corpus seeding local to the disposable instance.
    Path(history["config"]).write_text("[sync]\nauto_update = false\n")
    command(history, "connect", url, "--member", MEMBER)
    if load_page(history, "sessions", "list", "--limit", "1")["data"]:
        raise ValueError("The browser corpus requires an empty disposable SessionDB")
    enabled = [p for p in load_page(history, "embeddings", "list")["data"] if p["enabled"]]
    if bool(enabled) == bool(profile_file):
        raise ValueError("Use an empty disposable SessionDB with one enabled profile, or supply --embedding-profile to configure it")
    if profile_file and not json.loads(profile_file.read_text()).get("enabled"):
        raise ValueError("The supplied embedding profile must be enabled")
    profile = (load_page(history, "embeddings", "create", str(profile_file)) if profile_file else enabled[0])
    command(history, "sources", "add", "--harness", "codex", "--root", str(root / "sources"),
            "--device-label", DEVICE)
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
    history["profile"] = wait_for_embeddings(history, profile["id"])
    return history


def wait_for_embeddings(history, profile_id):
    """Freeze coverage and record the actual query transform used by the server."""
    import time

    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        detail = load_page(history, "embeddings", "show", profile_id)
        progress = detail["progress"]
        if progress["ready_inputs"] and not progress["pending_inputs"] and not progress["retrying_inputs"]:
            return {**{k: detail[k] for k in ("model", "model_revision", "dimensions", "query_prefix", "document_prefix")},
                    "ready_inputs": progress["ready_inputs"]}
        time.sleep(1)
    raise RuntimeError("The frozen corpus did not finish embedding within ten minutes")
