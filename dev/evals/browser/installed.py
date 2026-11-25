"""A real Codex shell, the shipped installer, and a read-only route to the fixture API."""
import contextlib
from datetime import UTC, datetime
from http.client import HTTPConnection, HTTPSConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
from threading import Thread
from urllib.parse import parse_qs, urlsplit

from fixture import NATIVE

ROOT = Path(__file__).resolve().parents[3]
ORIGIN = "http://sessiondb.test"
MEMBER = "pilot"
DEVICE = "browser-workstation"


def now():
    return datetime.now(UTC).isoformat()


def log_event(path, event):
    with path.open("a") as file:
        file.write(json.dumps(event) + "\n")


def evidence_texts(payload, labels):
    """Keep only actual excerpts or text blocks returned for a labeled message."""
    if not isinstance(payload, dict):
        return []
    texts = []
    for row in payload.get("data", [payload]):
        if not isinstance(row, dict) or (row.get("message_id") or row.get("id")) not in labels:
            continue
        texts.append(row.get("excerpt", ""))
        texts.extend(b.get("text", "") for b in row.get("content", []) if b.get("type") == "text")
    return [text for text in texts if NATIVE in text]


@contextlib.contextmanager
def fixture_proxy(url, key, labels, trace):
    """Codex's managed shell proxy may forward only fixture reads to this upstream."""
    target = urlsplit(url)
    token = secrets.token_hex(24)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def forward(self):
            start = now()
            uri = urlsplit(self.path)
            if (uri.scheme != "http" or uri.netloc != "sessiondb.test" or not uri.path.startswith("/v1/")
                    or (self.command != "GET" and (self.command, uri.path) != ("POST", "/v1/search"))):
                self.send_error(403)
                return
            if self.headers.get("Authorization") != "Bearer " + token:
                self.send_error(401)
                return
            try:
                length = int(self.headers.get("Content-Length", 0))
            except ValueError:
                self.send_error(400)
                return
            if length < 0 or length > 65536:
                self.send_error(413)
                return
            body = self.rfile.read(length)
            connection = (HTTPSConnection if target.scheme == "https" else HTTPConnection)(
                target.hostname, target.port, timeout=60
            )
            try:
                connection.request(self.command, uri.path + ("?" + uri.query if uri.query else ""),
                                   body=body or None, headers={"Authorization": "Bearer " + key,
                                                              "Content-Type": "application/json"})
                response = connection.getresponse()
                data = response.read()
                self.send_response(response.status)
                self.send_header("Content-Type", response.getheader("Content-Type", "application/octet-stream"))
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                media_type = response.getheader("Content-Type", "").partition(";")[0].lower()
                payload = json.loads(data) if media_type == "application/json" or media_type.endswith("+json") else {}
                original_ids = []
                if self.command == "GET" and response.status == 200 and isinstance(payload, dict):
                    if uri.path.startswith("/v1/messages/") or uri.path.endswith("/messages"):
                        original_ids = [m["id"] for m in payload.get("data", [payload])
                                        if isinstance(m, dict) and "id" in m and "role" in m]
                log_event(trace, {"tool": "sessiondb", "time": start, "completed_at": now(),
                                  "method": self.command, "path": uri.path, "params": parse_qs(uri.query),
                                  "status": response.status, "request": json.loads(body) if body else None,
                                  "evidence": evidence_texts(payload, labels), "original_ids": original_ids, "bytes": len(data)})
            except (OSError, ValueError) as error:
                log_event(trace, {"tool": "sessiondb", "time": start, "runner_error": str(error)})
            finally:
                connection.close()

        do_GET = do_POST = forward

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_port}", token
        finally:
            server.shutdown()
            thread.join()


def materialize_home(home, entries):
    for entry in entries:
        if entry in {"", ".", ".."} or "/" in entry or "\0" in entry:
            raise ValueError("Home listing entries must be basenames")
        path = home / entry
        if path.suffix:
            path.touch()
        else:
            path.mkdir(exist_ok=True)
    for name in ("firefox-bidi", "firefox-native"):
        (home / "plugins" / name).mkdir(parents=True, exist_ok=True)


def actor_setup(root, upstream, history, home_entries, install_root=ROOT):
    home = root / "home"
    home.mkdir()
    materialize_home(home, home_entries)
    cwd = home / "code" / "project"
    cwd.mkdir(parents=True, exist_ok=True)
    control = root / "codex"
    control.mkdir()
    auth = Path(os.environ.get("CODEX_HOME", Path.home() / ".codex")) / "auth.json"
    (control / "auth.json").symlink_to(auth.resolve(strict=True))
    bindir = home / ".local" / "bin"
    bindir.mkdir(parents=True)
    cli_path = str(bindir) + ":/usr/bin:/bin"
    env = {k: os.environ[k] for k in ("PATH", "LANG", "USER", "HTTPS_PROXY", "SSL_CERT_FILE", "NODE_EXTRA_CA_CERTS")
           if k in os.environ}
    env.update(HOME=str(home), CODEX_HOME=str(control), SHELL="/bin/bash", XDG_CONFIG_HOME=str(home / ".config"))
    if upstream:
        proxy, token = upstream
        # Inference stays on the subscription transport. The model's shell is
        # independently confined to the managed proxy and its single allowed domain.
        bypass = "chatgpt.com,.chatgpt.com,openai.com,.openai.com"
        env.update(http_proxy=proxy, HTTP_PROXY=proxy, no_proxy=bypass, NO_PROXY=bypass)
        shutil.copy2(history["cli"], bindir / "sessiondb")
        subprocess.run([sys.executable, str(install_root / "scripts/install-harnesses")],
                       env={**env, "PATH": str(bindir)}, check=True, stdout=subprocess.DEVNULL)
        # Connect through the same alias and saved per-origin config the actor uses.
        config = home / ".config/sessiondb/config.toml"
        config.parent.mkdir(parents=True)
        config.write_text(f'device_label = "{DEVICE}"\n')
        subprocess.run([bindir / "sessiondb", "connect", ORIGIN, "--member", MEMBER],
                       env={**env, "SESSIONDB_API_KEY": token}, check=True, stdout=subprocess.DEVNULL)
    configure_sandbox(home, control, cli_path, cwd, env, bool(upstream))
    verify_sandbox(root, control, auth, cwd, env, history, bindir / "sessiondb", bool(upstream))
    return cwd, env


def configure_sandbox(home, control, cli_path, cwd, env, installed):
    # Codex launches its native sandbox helper through a per-login symlink. Grant
    # these two runtime paths, keeping auth.json and controller files outside the shell.
    helper = subprocess.check_output(
        ["codex", "sandbox", "-P", ":read-only", "--", "/bin/sh", "-c",
         'readlink -f "$(command -v codex-linux-sandbox)"'], env=env, cwd=cwd, text=True
    ).strip()
    if not helper or not Path(helper).is_file():
        raise RuntimeError("Could not locate Codex's Linux sandbox helper")
    allowed = [":minimal", str(home), str(control / "tmp/arg0-linux"), helper]
    if installed:
        allowed.append(str(control / "AGENTS.md"))
    config_text = [
        'default_permissions = "pilot"', 'approval_policy = "never"', 'allow_login_shell = false',
        'web_search = "disabled"', '[features]', 'network_proxy = true', '[shell_environment_policy]', 'inherit = "none"',
        '[shell_environment_policy.set]', f'HOME = {json.dumps(str(home))}', f'PATH = {json.dumps(cli_path)}',
        'SHELL = "/bin/bash"', '[permissions.pilot.filesystem]',
        *(f'{json.dumps(path)} = "read"' for path in allowed),
        '[permissions.pilot.network]', 'allow_local_binding = ' + ('true' if installed else 'false'),
        'enabled = ' + ('true' if installed else 'false'),
    ]
    if installed:
        config_text.extend(['allow_upstream_proxy = true', 'mode = "full"',
                            '[permissions.pilot.network.domains]', '"sessiondb.test" = "allow"'])
    (control / "config.toml").write_text("\n".join(config_text) + "\n")


def verify_sandbox(root, control, auth, cwd, env, history, binary, installed):
    sandbox = ["codex", "sandbox", "-P", "pilot", "--"]
    # Verify effective OS restrictions on this Codex build. Even a raw socket
    # must not bypass the managed proxy and connect to another host service.
    hidden = [str(control / "auth.json"), str(auth.resolve()), str(root / "world.json"),
              str(ROOT / "dev/evals/browser/side-topic.json"), str(Path(history["config"]))]
    sessions = control / "sessions"
    sessions.mkdir(exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=sessions) as private_session, socket.socket() as sentinel:
        hidden.append(private_session.name)
        sentinel.bind(("127.0.0.1", 0))
        sentinel.listen()
        program = """
import os, socket, sys
if any(os.access(path, os.R_OK) for path in sys.argv[2:]):
    sys.exit("Controller files visible to the shell")
try:
    probe = socket.socket()
except PermissionError:
    sys.exit(0)
with probe:
    probe.settimeout(2)
    if probe.connect_ex(("127.0.0.1", int(sys.argv[1]))) == 0:
        sys.exit("Direct host loopback reachable from the shell")
"""
        subprocess.run(sandbox + ["/usr/bin/python3", "-c", program, str(sentinel.getsockname()[1]), *hidden],
                       env=env, cwd=cwd, check=True)
    if installed:
        local = subprocess.check_output(sandbox + [str(binary), "sessions", "list", "--local",
                                                  "--limit", "1", "--json"], cwd=cwd, env=env, text=True)
        rows = json.loads(local)["data"]
        if not rows or (rows[0]["member"], rows[0]["device_label"]) != (MEMBER, DEVICE):
            raise RuntimeError("The actor's --local scope must resolve to the seeded workstation")
