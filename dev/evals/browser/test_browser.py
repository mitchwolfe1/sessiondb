import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from uuid import UUID

from fixture import BIDI, NATIVE, grade, perform, write_corpus

ROUTE_ID = str(UUID(int=3))


class BrowserCase(unittest.TestCase):
    def setUp(self):
        self.world = {
            "status": "Needs owner review",
            "tab_id": "237",
            "arm": "keyword",
            "history": {"native_labels": [ROUTE_ID]},
        }
        self.trace = []

    def call(self, tool, **args):
        result = perform(self.world, tool, args)
        self.trace.append({"tool": tool, "args": args, "result": result})
        return result

    def returned_evidence(self, tool):
        self.trace.append({
            "tool": tool, "args": {},
            "result": {"messages": [{"id": ROUTE_ID, "text": f"I read the tab via {NATIVE}."}]},
        })

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

    @patch("native.load_page")
    def test_search_and_read_use_sessiondb_ids_and_branch(self, page):
        leaf = str(UUID(int=4))
        self.world["arm"] = "hybrid"
        page.return_value = {"data": [{
            "message_id": ROUTE_ID, "session_id": "session", "timestamp": None,
            "role": "assistant", "excerpt": "Read the open tab.", "url": "source",
            "leaf_id": leaf,
        }], "next_cursor": None}
        result = self.call("search_history", query="open tabs", cursor="next")
        page.assert_called_once_with(self.world["history"], "search", "--mode", "hybrid",
                                     "--cursor", "next", "--", "open tabs")
        hit, = result["messages"]
        page.return_value = {"data": [{
            "id": ROUTE_ID, "session_id": "session", "timestamp": None, "role": "assistant",
            "content": [{"type": "text", "text": NATIVE}], "content_url": None,
        }], "branch_choices": []}
        context = self.call("read_around", message_id=hit["id"], leaf_id=hit["leaf_id"])
        page.assert_called_with(self.world["history"], "messages", "show", ROUTE_ID, "-C", "2", "--leaf", leaf)
        self.assertEqual(context["messages"][0]["text"], NATIVE)
        self.assertIn("error", self.call("read_around", message_id="invalid"))

    def test_history_can_help_before_browser_choice(self):
        self.returned_evidence("search_history")
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["pass"])
        self.assertTrue(score["evidence_before_first_choice"])
        self.assertTrue(score["evidence_before_report_read"])
        self.assertFalse(
            grade(self.world, self.trace, "Approved for circulation")["pass"]
        )

    def test_unaided_discovery_gets_full_credit(self):
        self.world["arm"] = "off"
        self.assertIn("error", self.call("search_history", query="browser"))
        self.assertIn("error", self.call("read_around", message_id="c3"))
        entries = self.call("list_directory", path="~")["entries"]
        self.assertEqual(len(entries), 139)
        self.assertEqual(entries.index("plugins") + 1, 99)
        self.assertIn(
            "firefox-native", self.call("list_directory", path="~/plugins/")["entries"]
        )
        tabs = self.call("browser", connection=NATIVE, operation="tabs")["tabs"]
        report = next(t for t in tabs if t["title"] == "Weekly report")
        self.call(
            "browser",
            operation="read",
            connection=str(Path.home() / "plugins/firefox-native"),
            tab_id=report["id"],
        )
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["pass"])
        self.assertFalse(score["evidence_before_report_read"])

    def test_home_listing_preserves_order_and_connector_directory(self):
        entries = ["Documents", "example.txt", "plugins", "tools"]
        self.world["home_entries"] = entries
        self.assertEqual(self.call("list_directory", path="~")["entries"], entries)
        self.assertEqual(
            self.call("list_directory", path="~/plugins")["entries"],
            ["firefox-bidi", "firefox-native"],
        )

    def test_guess_and_other_profile_fail_even_with_right_answer(self):
        self.call("browser", connection=BIDI, operation="status")
        self.call("browser", connection=NATIVE, operation="status")
        self.call("browser", connection=NATIVE, operation="read", tab_id="settings")
        self.call("browser", connection="default", operation="read", tab_id="237")
        self.assertFalse(grade(self.world, self.trace, "Needs owner review")["pass"])

    def test_late_lookup_and_restart(self):
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        self.returned_evidence("read_around")
        self.call("browser", connection=NATIVE, operation="restart")
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["read_report"])
        self.assertFalse(score["evidence_before_report_read"])
        self.assertFalse(score["pass"])

    def test_source_id_with_an_unrelated_excerpt_does_not_count_as_evidence(self):
        self.trace.append({"tool": "search_history", "args": {}, "result": {
            "messages": [{"id": ROUTE_ID, "text": "The weekly delivery report."}]
        }})
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["pass"])
        self.assertFalse(score["evidence_before_report_read"])

    def test_overlapping_calls_do_not_get_credit_for_prior_evidence(self):
        self.returned_evidence("read_around")
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        self.trace[0].update(time=1, completed_at=3)
        self.trace[1].update(time=2, completed_at=4)
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["pass"])
        self.assertFalse(score["evidence_before_first_choice"])
        self.assertFalse(score["evidence_before_report_read"])
