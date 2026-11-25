import json
import tempfile
import unittest
from types import SimpleNamespace
from pathlib import Path
from contextlib import ExitStack
from http.client import HTTPConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Thread
from uuid import UUID

from fixture import BIDI, NATIVE, grade, perform, write_corpus
from installed import fixture_proxy, materialize_home
from run import usage_metrics

ROUTE_ID = str(UUID(int=3))


class BrowserCase(unittest.TestCase):
    def setUp(self):
        self.world = {
            "status": "Needs owner review",
            "tab_id": "237",
            "home": "/home/fixture",
        }
        self.trace = []

    def call(self, tool, **args):
        result = perform(self.world, tool, args)
        self.trace.append({"tool": tool, "args": args, "result": result})
        return result

    def returned_evidence(self):
        self.trace.extend([
            {"tool": "sessiondb", "time": 0, "evidence": [f"I read the tab via {NATIVE}."]},
            {"tool": "command_execution", "completed_at": 1, "output": f"I read the tab via {NATIVE}."},
        ])

    def test_corpus_omits_the_turn_at_the_cutoff(self):
        rows = json.loads(Path(__file__).with_name("corpus.json").read_text())
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            documents = write_corpus(rows, root)
            expected_ids = {row["id"] for row in rows} - {"c4"}
            self.assertEqual({m["id"] for ms in documents.values() for m in ms}, expected_ids)
            written_ids = set()
            for source in (root / "sources").glob("*.jsonl"):
                records = [json.loads(line) for line in source.read_text().splitlines()]
                ids = [r["payload"]["id"] for r in records[1:]]
                self.assertEqual(ids, [m["id"] for m in documents[source.stem]])
                written_ids.update(ids)
            self.assertEqual(written_ids, expected_ids)

    def test_full_original_is_written_when_display_excerpt_is_shorter(self):
        with tempfile.TemporaryDirectory() as tmp:
            full = "The appendix records an emerald compass."
            write_corpus([{"session": "deep", "id": "deep-source", "role": "user",
                           "at": "2026-06-01T00:00:00Z", "text": "The appendix.",
                           "index_text": full}], Path(tmp))
            source = (Path(tmp) / "sources" / "session-0.jsonl").read_text().splitlines()
            self.assertEqual(json.loads(source[-1])["payload"]["content"][0]["text"], full)

    def test_history_can_help_before_browser_choice(self):
        self.returned_evidence()
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["pass"])
        self.assertTrue(score["evidence_before_first_choice"])
        self.assertTrue(score["evidence_before_report_read"])
        self.assertFalse(
            grade(self.world, self.trace, "Approved for circulation")["pass"]
        )

    def test_unaided_discovery_gets_full_credit(self):
        tabs = self.call("browser", connection=NATIVE, operation="tabs")["tabs"]
        report = next(t for t in tabs if t["title"] == "Weekly report")
        self.call(
            "browser",
            operation="read",
            connection=self.world["home"] + "/plugins/firefox-native",
            tab_id=report["id"],
        )
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["pass"])
        self.assertFalse(score["evidence_before_report_read"])

    def test_real_shell_home(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            materialize_home(home, ["Documents", "example.txt", "plugins", "tools"])
            self.assertTrue((home / "example.txt").is_file())
            self.assertEqual(sorted(x.name for x in (home / "plugins").iterdir()), ["firefox-bidi", "firefox-native"])
            for bad in ("..", ".", "../fixture", "/control"):
                with self.assertRaises(ValueError):
                    materialize_home(home, [bad])

    def test_guess_and_other_profile_fail_even_with_right_answer(self):
        self.call("browser", connection=BIDI, operation="status")
        self.call("browser", connection=NATIVE, operation="status")
        self.call("browser", connection=NATIVE, operation="read", tab_id="settings")
        self.call("browser", connection="default", operation="read", tab_id="237")
        self.assertFalse(grade(self.world, self.trace, "Needs owner review")["pass"])

    def test_late_lookup_and_restart(self):
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        self.returned_evidence()
        self.call("browser", connection=NATIVE, operation="restart")
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["read_report"])
        self.assertFalse(score["evidence_before_report_read"])
        self.assertFalse(score["pass"])

    def test_a_search_suppressed_by_a_pipeline_is_not_visible_evidence(self):
        self.trace.extend([
            {"tool": "sessiondb", "time": 0, "evidence": [f"I read the tab via {NATIVE}."]},
            {"tool": "command_execution", "completed_at": 1, "output": "3 matches"},
        ])
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        score = grade(self.world, self.trace, "The status is **Needs owner review**.")
        self.assertTrue(score["pass"])
        self.assertFalse(score["evidence_before_report_read"])

    def test_contradictory_or_negated_header_fails(self):
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        for answer in ("It is not Needs owner review.", "Needs owner review or On hold."):
            self.assertFalse(grade(self.world, self.trace, answer)["pass"])

    def test_cli_output_metrics_survive_delayed_shell_events(self):
        trace = [
            {"tool": "sessiondb", "path": "/v1/search", "request": {"mode": "hybrid"},
             "time": "2026-10-01T10:00:00+00:00", "completed_at": "2026-10-01T10:00:01+00:00"},
            {"tool": "command_execution", "command": "/bin/sh -c 'sessiondb search --local --json tabs'",
             "time": "2026-10-01T10:00:02+00:00", "output": "the matching source"},
            {"tool": "command_execution", "command": "cat ~/.agents/skills/sessiondb-history/SKILL.md", "output": "skill"},
        ]
        metrics = usage_metrics(trace, SimpleNamespace(encode_ordinary=str.split))
        self.assertEqual(metrics["history_tokens"], 3)
        self.assertEqual(metrics["history_bytes"], len("the matching source"))
        self.assertEqual(metrics["lookup_ms"], [1000])

    def test_overlapping_calls_do_not_get_credit_for_prior_evidence(self):
        self.returned_evidence()
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        self.trace[1]["completed_at"] = 3
        self.trace[2].update(time=2, completed_at=4)
        self.assertFalse(grade(self.world, self.trace, "Needs owner review")["evidence_before_report_read"])


class FixtureProxyCase(unittest.TestCase):
    def test_real_query_and_response_are_preserved_and_other_destinations_are_blocked(self):
        forwarded = []
        response = {"data": [{"message_id": ROUTE_ID, "excerpt": f"Use {NATIVE}", "leaf_id": "branch"}]}
        class Backend(BaseHTTPRequestHandler):
            def do_POST(self):
                forwarded.append((self.path, self.headers["Authorization"],
                                  json.loads(self.rfile.read(int(self.headers["Content-Length"])))))
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(json.dumps(response).encode())

            def log_message(self, *args):
                pass

        with ThreadingHTTPServer(("127.0.0.1", 0), Backend) as backend, tempfile.TemporaryDirectory() as tmp, ExitStack() as cleanup:
            thread = Thread(target=backend.serve_forever)
            thread.start()
            cleanup.callback(thread.join)
            cleanup.callback(backend.shutdown)
            trace = Path(tmp) / "trace.jsonl"
            with fixture_proxy(f"http://127.0.0.1:{backend.server_port}", "fixture-key", [ROUTE_ID], trace) as (url, token):
                def call(path, request, auth):
                    con = HTTPConnection("127.0.0.1", int(url.rsplit(":", 1)[1]))
                    try:
                        con.request("POST", path, json.dumps(request), {"Authorization": "Bearer " + auth})
                        r = con.getresponse()
                        return r.status, r.read()
                    finally:
                        con.close()
                query = {"mode": "hybrid", "filters": {"member": "pilot", "device_label": "workstation"}, "query": "tabs"}
                good = "http://sessiondb.test/v1/search"
                status, body = call(good, query, token)
                self.assertEqual((status, json.loads(body)), (200, response))
                self.assertEqual(call(good, query, "wrong")[0], 401)
                self.assertEqual(call("http://localhost/v1/search", query, token)[0], 403)
                self.assertEqual(call("http://sessiondb.test/v1/sources", query, token)[0], 403)
            self.assertEqual(forwarded, [("/v1/search", "Bearer fixture-key", query)])
            self.assertEqual(json.loads(trace.read_text())["evidence"], [f"Use {NATIVE}"])
