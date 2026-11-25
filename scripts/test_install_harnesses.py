"""Exercise the checkout installer with isolated homes and PATHs."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("install-harnesses").resolve()
SKILL = Path("skills/sessiondb-history/SKILL.md")
BUNDLED = (SCRIPT.parents[1] / SKILL).read_bytes()
START = b"<!-- sessiondb-history:start -->"
END = b"<!-- sessiondb-history:end -->"


class HarnessInstallTest(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.home = Path(temp.name) / "user home"
        self.home.mkdir()
        self.bin = Path(temp.name) / "bin"
        self.bin.mkdir()
        self.env = {"HOME": str(self.home), "PATH": str(self.bin)}

    def install(self, *args, check=True):
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args], env=self.env, cwd=self.home,
            text=True, capture_output=True, check=check,
        )

    def test_no_harness_writes_nothing(self):
        self.assertIn("No Codex", self.install().stdout)
        self.assertEqual(list(self.home.iterdir()), [])

    def test_binary_only_custom_claude_home_and_dry_run(self):
        command = self.bin / "claude"
        command.write_text("#!/bin/sh\nexit 99\n")
        command.chmod(0o755)
        config = self.home / "custom claude"
        self.env["CLAUDE_CONFIG_DIR"] = str(config)
        preview = self.install("--dry-run").stdout
        self.assertIn("Detected: Claude Code\n", preview)
        self.assertIn(str(config / "CLAUDE.md"), preview)
        self.assertEqual(list(self.home.iterdir()), [])
        self.install()
        self.assertEqual((config / SKILL).read_bytes(), BUNDLED)
        self.assertIn(START, (config / "CLAUDE.md").read_bytes())
        self.assertEqual(list(self.home.iterdir()), [config])

    def test_existing_codex_skill_and_empty_override(self):
        codex = self.home / ".codex"
        local = codex / SKILL
        local.parent.mkdir(parents=True)
        local.write_bytes(b"Old installed skill")
        (codex / "AGENTS.override.md").write_bytes(b"")
        self.assertIn("Detected: Codex\n", self.install().stdout)
        self.assertEqual(local.read_bytes(), BUNDLED)
        self.assertIn(START, (codex / "AGENTS.md").read_bytes())
        self.assertEqual((codex / "AGENTS.override.md").read_bytes(), b"")
        self.assertFalse((self.home / ".agents").exists())

    def test_pi_only_from_configuration(self):
        pi = self.home / ".pi/agent"
        pi.mkdir(parents=True)
        self.assertIn("Detected: Pi\n", self.install().stdout)
        self.assertIn(START, (pi / "AGENTS.md").read_bytes())
        self.assertEqual((self.home / ".agents" / SKILL).read_bytes(), BUNDLED)
        self.assertFalse((self.home / ".codex").exists())
        self.assertFalse((self.home / ".claude").exists())

    def test_shared_skill_overrides_symlinks_and_repeat_install(self):
        codex = self.home / "custom codex"
        pi = self.home / "custom pi"
        self.env.update(CODEX_HOME=str(codex), PI_CODING_AGENT_DIR=str(pi))
        codex.mkdir()
        pi.mkdir()
        override = codex / "AGENTS.override.md"
        override.write_bytes(b"Original instructions\r\n")
        (codex / "AGENTS.md").write_bytes(b"Fallback")
        guide = pi / "CLAUDE.md"
        guide.write_bytes(b"Other guidance\n")
        claude = self.home / ".claude"
        (claude / "skills").mkdir(parents=True)
        (claude / "CLAUDE.md").symlink_to(guide)
        shared = self.home / ".agents" / SKILL
        shared.parent.mkdir(parents=True)
        shared.write_bytes(b"Previous skill with local edits")
        (claude / SKILL.parent).symlink_to(shared.parent, target_is_directory=True)
        self.assertIn("Detected: Codex, Claude Code, Pi", self.install().stdout)
        self.assertEqual((codex / "AGENTS.md").read_bytes(), b"Fallback")
        self.assertTrue(override.read_bytes().startswith(b"Original instructions\r\n\r\n"))
        self.assertNotIn(b"\n", override.read_bytes().replace(b"\r\n", b""))
        self.assertFalse((pi / "AGENTS.md").exists())
        self.assertTrue((claude / "CLAUDE.md").is_symlink())
        self.assertTrue((claude / SKILL.parent).is_symlink())
        self.assertEqual((claude / SKILL).read_bytes(), BUNDLED)
        backups = list(shared.parent.glob("*.bak"))
        self.assertEqual([p.read_bytes() for p in backups], [b"Previous skill with local edits"])
        guide.write_bytes(b"Other guidance\n\n" + START + b"\nold rule\n" + END + b"\nUser footer")
        self.install()
        self.assertEqual(guide.read_bytes().count(START), 1)
        self.assertTrue(guide.read_bytes().startswith(b"Other guidance\n\n" + START))
        self.assertTrue(guide.read_bytes().endswith(END + b"\nUser footer"))
        self.assertNotIn(b"old rule", guide.read_bytes())
        files = [guide, shared, override, *backups]
        mtimes = [p.stat().st_mtime_ns for p in files]
        self.assertIn("already up to date", self.install().stdout)
        self.assertEqual([p.stat().st_mtime_ns for p in files], mtimes)
        self.assertEqual(list(shared.parent.glob("*.bak")), backups)

    def test_invalid_markers_do_not_partially_install(self):
        (self.home / ".codex").mkdir()
        (self.home / ".pi/agent").mkdir(parents=True)
        damaged = self.home / ".pi/agent/AGENTS.md"
        damaged.write_bytes(START + b"\nMy notes")
        result = self.install(check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(str(damaged), result.stderr)
        self.assertEqual(damaged.read_bytes(), START + b"\nMy notes")
        self.assertFalse((self.home / ".agents").exists())
        self.assertFalse((self.home / ".codex/AGENTS.md").exists())


if __name__ == "__main__":
    unittest.main()
