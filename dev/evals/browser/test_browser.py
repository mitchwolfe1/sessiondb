import json
import tempfile
import unittest
from pathlib import Path

from fixture import BIDI, NATIVE, build_history, grade, perform


class BrowserCase(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        temp = tempfile.TemporaryDirectory()
        cls.addClassCleanup(temp.cleanup)
        corpus = Path(__file__).with_name("corpus.json")
        cls.history = build_history(json.loads(corpus.read_text()), Path(temp.name))

    def setUp(self):
        self.world = {
            "status": "Needs owner review",
            "tab_id": "237",
            "arm": "keyword",
            "history": self.history,
        }
        self.trace = []

    def call(self, tool, **args):
        result = perform(self.world, tool, args)
        self.trace.append({"tool": tool, "args": args, "result": result})
        return result

    def test_search_and_read_obey_cutoff(self):
        hits = self.call("search_history", query="existing browser tabs")["messages"]
        self.assertIn("c3", [hit["id"] for hit in hits])
        self.assertTrue(all("supports_route" not in hit for hit in hits))
        context = self.call("read_around", message_id="c3")["messages"]
        self.assertEqual([m["id"] for m in context], ["c1", "c2", "c3"])
        self.assertIn("error", self.call("read_around", message_id="c4"))
        self.assertEqual(self.call("search_history", query="future")["messages"], [])
        self.assertIn("error", self.call("search_history", query="?!"))

    def test_history_can_help_before_browser_choice(self):
        self.call("search_history", query="existing browser tabs")
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
        self.call("read_around", message_id="c3")
        self.call("browser", connection=NATIVE, operation="restart")
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["read_report"])
        self.assertFalse(score["evidence_before_report_read"])
        self.assertFalse(score["pass"])

    def test_source_id_with_an_unrelated_excerpt_does_not_count_as_evidence(self):
        self.world["history"] = {"native_labels": ["c3"]}
        self.trace.append({"tool": "search_history", "args": {}, "result": {
            "messages": [{"id": "c3", "text": "The weekly delivery report."}]
        }})
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["pass"])
        self.assertFalse(score["evidence_before_report_read"])

    def test_overlapping_calls_do_not_get_credit_for_prior_evidence(self):
        self.call("read_around", message_id="c3")
        self.call("browser", connection=NATIVE, operation="read", tab_id="237")
        self.trace[0].update(time=1, completed_at=3)
        self.trace[1].update(time=2, completed_at=4)
        score = grade(self.world, self.trace, "Needs owner review")
        self.assertTrue(score["pass"])
        self.assertFalse(score["evidence_before_first_choice"])
        self.assertFalse(score["evidence_before_report_read"])
