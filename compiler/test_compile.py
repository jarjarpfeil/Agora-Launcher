#!/usr/bin/env python3
"""Standalone unit tests for compiler/compile.py pure functions.

Run with:  python compiler/test_compile.py
No pytest dependency — uses only stdlib (unittest, sys, tempfile, json, os, re).

Covers: validate_sha256, _get_registry_repo, _load_poll_blacklist,
        _extract_mod_id, _extract_review_text, _scrub_review_text,
        and regex DoS protections via insert_crash_signature.
"""

from __future__ import annotations

import json
import os
import re
import sys
import tempfile
import unittest

# Ensure we can import the compiler module from the repo root.
sys.path.insert(0, os.path.dirname(__file__))
import compile as _compile  # noqa: E402


# ---------------------------------------------------------------------------
# validate_sha256
# ---------------------------------------------------------------------------

class TestValidateSha256(unittest.TestCase):
    """Tests for validate_sha256."""

    def test_valid_64_hex(self):
        """A valid 64-char hex string passes and is returned unchanged."""
        result = _compile.validate_sha256("a" * 64)
        self.assertEqual(result, "a" * 64)

    def test_valid_uppercase_hex(self):
        """Uppercase hex is accepted."""
        result = _compile.validate_sha256("A" * 64)
        self.assertEqual(result, "A" * 64)

    def test_valid_mixed_hex(self):
        """Mixed-case hex is accepted."""
        raw = "aB3dEf0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        self.assertEqual(len(raw), 64)
        result = _compile.validate_sha256(raw)
        self.assertEqual(result, raw)

    def test_none_rejected(self):
        """None raises SystemExit."""
        with self.assertRaises(SystemExit):
            _compile.validate_sha256(None)

    def test_empty_rejected(self):
        """Empty string raises SystemExit."""
        with self.assertRaises(SystemExit):
            _compile.validate_sha256("")

    def test_short_rejected(self):
        """32-char hex (too short) raises SystemExit."""
        with self.assertRaises(SystemExit):
            _compile.validate_sha256("a" * 32)

    def test_long_rejected(self):
        """65-char hex (too long) raises SystemExit."""
        with self.assertRaises(SystemExit):
            _compile.validate_sha256("a" * 65)

    def test_non_hex_rejected(self):
        """64-char string with non-hex chars raises SystemExit."""
        with self.assertRaises(SystemExit):
            _compile.validate_sha256("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz")

    def test_non_string_rejected(self):
        """Non-string type raises SystemExit."""
        with self.assertRaises(SystemExit):
            _compile.validate_sha256(12345)


# ---------------------------------------------------------------------------
# _get_registry_repo
# ---------------------------------------------------------------------------

class TestGetRegistryRepo(unittest.TestCase):
    """Tests for _get_registry_repo."""

    def setUp(self):
        self._saved: dict[str, str | None] = {}
        for key in ("AGORA_REGISTRY_REPO", "GITHUB_REPOSITORY"):
            self._saved[key] = os.environ.pop(key, None)

    def tearDown(self):
        for key, val in self._saved.items():
            if val is not None:
                os.environ[key] = val
            else:
                os.environ.pop(key, None)

    def test_env_var_agora_registry_repo(self):
        """AGORA_REGISTRY_REPO takes precedence and is returned."""
        os.environ["AGORA_REGISTRY_REPO"] = "test/repo"
        self.assertEqual(_compile._get_registry_repo(), "test/repo")

    def test_github_fallback(self):
        """When AGORA_REGISTRY_REPO is unset, GITHUB_REPOSITORY is used."""
        os.environ.pop("AGORA_REGISTRY_REPO", None)
        os.environ["GITHUB_REPOSITORY"] = "gh/test"
        self.assertEqual(_compile._get_registry_repo(), "gh/test")

    def test_default(self):
        """When both env vars are unset, the default is returned."""
        os.environ.pop("AGORA_REGISTRY_REPO", None)
        os.environ.pop("GITHUB_REPOSITORY", None)
        result = _compile._get_registry_repo()
        self.assertIn("Agora-Minecraft-Mod-Loader", result)

    def test_priority_agora_over_github(self):
        """AGORA_REGISTRY_REPO wins over GITHUB_REPOSITORY when both are set."""
        os.environ["AGORA_REGISTRY_REPO"] = "owner/first"
        os.environ["GITHUB_REPOSITORY"] = "owner/second"
        self.assertEqual(_compile._get_registry_repo(), "owner/first")


# ---------------------------------------------------------------------------
# _load_poll_blacklist
# ---------------------------------------------------------------------------

class TestLoadPollBlacklist(unittest.TestCase):
    """Tests for _load_poll_blacklist."""

    def test_valid_json_returns_lowercase_set(self):
        """Valid JSON with usernames is returned as a lowercase set."""
        blacklist_dir = _compile.REGISTRY_DIR / "governance"
        target = blacklist_dir / "poll_blacklist.json"
        # Back up if exists.
        backup = None
        if target.exists():
            backup = target.read_bytes()
        try:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(json.dumps({"usernames": ["Alice", "BOB"]}), encoding="utf-8")
            result = _compile._load_poll_blacklist()
            self.assertEqual(result, {"alice", "bob"})
        finally:
            if backup is not None:
                target.write_bytes(backup)
            else:
                target.unlink(missing_ok=True)

    def test_missing_file_returns_empty_set(self):
        """When the file does not exist, returns empty set (no crash)."""
        blacklist_dir = _compile.REGISTRY_DIR / "governance"
        target = blacklist_dir / "poll_blacklist.json"
        backup = None
        if target.exists():
            backup = target.read_bytes()
        try:
            target.unlink(missing_ok=True)
            result = _compile._load_poll_blacklist()
            self.assertEqual(result, set())
        finally:
            if backup is not None:
                target.write_bytes(backup)

    def test_malformed_json_returns_empty_set(self):
        """Invalid JSON returns empty set (no crash)."""
        blacklist_dir = _compile.REGISTRY_DIR / "governance"
        target = blacklist_dir / "poll_blacklist.json"
        backup = None
        if target.exists():
            backup = target.read_bytes()
        try:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("{not valid json!!!", encoding="utf-8")
            result = _compile._load_poll_blacklist()
            self.assertEqual(result, set())
        finally:
            if backup is not None:
                target.write_bytes(backup)
            else:
                target.unlink(missing_ok=True)


# ---------------------------------------------------------------------------
# _extract_mod_id
# ---------------------------------------------------------------------------

class TestExtractModId(unittest.TestCase):
    """Tests for _extract_mod_id."""

    def test_from_realistic_body(self):
        """A review-form body with Mod Registry ID returns the ID."""
        body = (
            "### Mod Registry ID\n"
            "sodium\n"
            "\n"
            "### Your Technical Review\n"
            "Great mod.\n"
        )
        self.assertEqual(_compile._extract_mod_id(body), "sodium")

    def test_none_body(self):
        """None body returns None."""
        self.assertIsNone(_compile._extract_mod_id(None))

    def test_empty_body(self):
        """Empty string body returns None."""
        self.assertIsNone(_compile._extract_mod_id(""))

    def test_no_field_returns_none(self):
        """Body without the Mod Registry ID field returns None."""
        body = "### Feature Request\nAdd mod X.\n"
        self.assertIsNone(_compile._extract_mod_id(body))

    def test_case_insensitive_heading(self):
        """Heading casing is ignored; ID is lowercased."""
        body = "### mod registry ID\n" "CaveClient\n"
        self.assertEqual(_compile._extract_mod_id(body), "caveclient")

    def test_with_crlf(self):
        """Windows CRLF line endings parse correctly."""
        body = "### Mod Registry ID\r\n" "sodium\r\n"
        self.assertEqual(_compile._extract_mod_id(body), "sodium")

    def test_trailing_whitespace_trimmed(self):
        """Trailing whitespace after the ID is trimmed."""
        body = "### Mod Registry ID\n" "sodium   \n"
        self.assertEqual(_compile._extract_mod_id(body), "sodium")


# ---------------------------------------------------------------------------
# _extract_review_text
# ---------------------------------------------------------------------------

class TestExtractReviewText(unittest.TestCase):
    """Tests for _extract_review_text."""

    def test_from_realistic_body(self):
        """Body with review heading → extracted text."""
        body = (
            "### Mod Registry ID\n"
            "sodium\n"
            "\n"
            "### Your Technical Review (50 character minimum)\n"
            "Excellent performance improvement over vanilla rendering.\n"
            "\n"
            "### Additional Comments\n"
            "None.\n"
        )
        result = _compile._extract_review_text(body)
        self.assertIsNotNone(result)
        self.assertIn("Excellent performance improvement", result)

    def test_none_body(self):
        """None body returns None."""
        self.assertIsNone(_compile._extract_review_text(None))

    def test_empty_body(self):
        """Empty string body returns None."""
        self.assertIsNone(_compile._extract_review_text(""))

    def test_no_review_field_returns_none(self):
        """Body without the review field returns None."""
        body = "### Mod Registry ID\n" "sodium\n"
        self.assertIsNone(_compile._extract_review_text(body))

    def test_strips_whitespace(self):
        """Leading/trailing whitespace in the extracted text is stripped."""
        body = "### Your Technical Review\n" "  lots of text  \n"
        result = _compile._extract_review_text(body)
        self.assertEqual(result, "lots of text")


# ---------------------------------------------------------------------------
# _scrub_review_text
# ---------------------------------------------------------------------------

class TestScrubReviewText(unittest.TestCase):
    """Tests for _scrub_review_text."""

    def test_version_begging_filtered(self):
        """Version-begging text is filtered out."""
        passed, cleaned, reason = _compile._scrub_review_text("Please update to 1.21")
        self.assertFalse(passed)
        self.assertEqual(reason, "version-begging")

    def test_legitimate_review_preserved(self):
        """A substantive review passes the scrub pipeline."""
        passed, cleaned, reason = _compile._scrub_review_text(
            "This mod adds great features and runs smoothly."
        )
        self.assertTrue(passed)
        self.assertEqual(reason, "")
        self.assertEqual(cleaned, "This mod adds great features and runs smoothly.")

    def test_empty_praise_filtered(self):
        """Short empty praise is filtered."""
        passed, cleaned, reason = _compile._scrub_review_text("Good mod.")
        self.assertFalse(passed)
        self.assertEqual(reason, "empty-praise")

    def test_empty_text_filtered(self):
        """Empty text is filtered."""
        passed, cleaned, reason = _compile._scrub_review_text("")
        self.assertFalse(passed)
        self.assertEqual(reason, "empty")

    def test_whitespace_only_filtered(self):
        """Whitespace-only text is filtered."""
        passed, cleaned, reason = _compile._scrub_review_text("   \t\n  ")
        self.assertFalse(passed)
        self.assertEqual(reason, "empty")

    def test_strips_clean_text(self):
        """Passed text is stripped of leading/trailing whitespace."""
        passed, cleaned, reason = _compile._scrub_review_text("  hello world  ")
        self.assertTrue(passed)
        self.assertEqual(cleaned, "hello world")


# ---------------------------------------------------------------------------
# Regex DoS protections (via insert_crash_signature)
# ---------------------------------------------------------------------------

class TestRegexDosProtection(unittest.TestCase):
    """Tests for regex DoS protections in insert_crash_signature."""

    def _create_schema(self, conn):
        """Create the crash_signatures table in *conn*."""
        conn.execute("""
            CREATE TABLE IF NOT EXISTS crash_signatures (
                id TEXT PRIMARY KEY,
                name TEXT,
                regex_pattern TEXT,
                solution_markdown TEXT,
                action_button_json TEXT
            )
        """)

    def setUp(self):
        self._conn = _compile.sqlite3.connect(":memory:")
        self._create_schema(self._conn)
        self._rejected: list[int] = [0]

    def tearDown(self):
        self._conn.close()

    def test_long_pattern_rejected(self):
        """A pattern >256 characters is rejected."""
        long_pattern = "a" * 257
        sig = {
            "id": "long_test",
            "name": "Long Pattern",
            "regex_pattern": long_pattern,
            "solution_markdown": "",
            "action_button_json": "[]",
        }
        _compile.insert_crash_signature(self._conn, sig, self._rejected)
        self.assertEqual(self._rejected[0], 1)
        # Should not have been inserted.
        row = self._conn.execute(
            "SELECT id FROM crash_signatures WHERE id = ?", ("long_test",)
        ).fetchone()
        self.assertIsNone(row)

    def test_valid_pattern_accepted(self):
        """A normal short pattern is accepted and inserted."""
        sig = {
            "id": "nullptr_test",
            "name": "Null Pointer",
            "regex_pattern": r"java\.lang\.NullPointerException",
            "solution_markdown": "Check for nulls.",
            "action_button_json": "[]",
        }
        _compile.insert_crash_signature(self._conn, sig, self._rejected)
        self.assertEqual(self._rejected[0], 0)
        row = self._conn.execute(
            "SELECT id, regex_pattern FROM crash_signatures WHERE id = ?",
            ("nullptr_test",),
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(row[0], "nullptr_test")
        self.assertEqual(row[1], r"java\.lang\.NullPointerException")

    def test_invalid_regex_rejected(self):
        """An invalid regex pattern is rejected."""
        sig = {
            "id": "bad_regex",
            "name": "Bad Regex",
            "regex_pattern": "[invalid(regex",
            "solution_markdown": "",
            "action_button_json": "[]",
        }
        _compile.insert_crash_signature(self._conn, sig, self._rejected)
        self.assertEqual(self._rejected[0], 1)

    def test_exact_256_pattern_accepted(self):
        """A pattern exactly 256 characters is accepted (boundary)."""
        pattern_256 = "a" * 256
        sig = {
            "id": "boundary_test",
            "name": "Boundary",
            "regex_pattern": pattern_256,
            "solution_markdown": "",
            "action_button_json": "[]",
        }
        _compile.insert_crash_signature(self._conn, sig, self._rejected)
        self.assertEqual(self._rejected[0], 0)
        row = self._conn.execute(
            "SELECT id FROM crash_signatures WHERE id = ?", ("boundary_test",)
        ).fetchone()
        self.assertIsNotNone(row)


if __name__ == "__main__":
    unittest.main()
