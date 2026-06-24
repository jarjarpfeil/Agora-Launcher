#!/usr/bin/env python3
"""Unit tests for pure functions in Agora utility scripts."""

import hashlib
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(__file__))

import fetch_loader_manifests
import fetch_registry_db
import refresh_loader_manifests


class TestIsStandardRelease(unittest.TestCase):
    """Tests for refresh_loader_manifests._is_standard_release."""

    def test_stable_two_part(self):
        """Two-part numeric version like '1.21' is standard."""
        self.assertTrue(refresh_loader_manifests._is_standard_release("1.21"))

    def test_stable_three_part(self):
        """Three-part numeric version like '1.21.1' is standard."""
        self.assertTrue(refresh_loader_manifests._is_standard_release("1.21.1"))

    def test_stable_minor_patch(self):
        """Version '1.20.6' is standard."""
        self.assertTrue(refresh_loader_manifests._is_standard_release("1.20.6"))

    def test_snapshot_weekly(self):
        """Weekly snapshot format like '24w14a' is NOT standard (regex excludes it)."""
        self.assertFalse(refresh_loader_manifests._is_standard_release("24w14a"))

    def test_snapshot_26w(self):
        """26w-prefixed snapshot like '26w07a' is NOT standard."""
        self.assertFalse(refresh_loader_manifests._is_standard_release("26w07a"))

    def test_prerelease(self):
        """Prerelease like '1.21-pre1' is NOT standard (regex excludes suffixes)."""
        self.assertFalse(refresh_loader_manifests._is_standard_release("1.21-pre1"))

    def test_release_candidate(self):
        """Release candidate like '1.21-rc1' is NOT standard."""
        self.assertFalse(refresh_loader_manifests._is_standard_release("1.21-rc1"))

    def test_invalid(self):
        """Non-numeric / malformed version is not standard."""
        self.assertFalse(refresh_loader_manifests._is_standard_release("0.0.0-invalid"))

    def test_empty(self):
        """Empty string is not standard."""
        self.assertFalse(refresh_loader_manifests._is_standard_release(""))


class TestSha256Hex(unittest.TestCase):
    """Tests for fetch_loader_manifests._sha256_hex."""

    def test_known_hello(self):
        """SHA-256 of b'hello' matches the known constant."""
        expected = (
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        )
        self.assertEqual(
            fetch_loader_manifests._sha256_hex(b"hello"),
            expected,
        )

    def test_empty(self):
        """SHA-256 of empty bytes matches the known empty-hash constant."""
        expected = (
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        )
        self.assertEqual(fetch_loader_manifests._sha256_hex(b""), expected)

    def test_deterministic(self):
        """Same input always produces the same hash."""
        data = b"test data for determinism"
        self.assertEqual(
            fetch_loader_manifests._sha256_hex(data),
            fetch_loader_manifests._sha256_hex(data),
        )


class TestStableJsonSha256(unittest.TestCase):
    """Tests for fetch_loader_manifests._stable_json_sha256."""

    def test_canonicalizes_key_order(self):
        """Different key orderings in JSON produce the same hash when keys are sorted."""
        obj1 = {"b": 1, "a": 2}
        obj2 = {"a": 2, "b": 1}
        hash1 = fetch_loader_manifests._stable_json_sha256(
            json.dumps(obj1, separators=(",", ":")).encode()
        )
        hash2 = fetch_loader_manifests._stable_json_sha256(
            json.dumps(obj2, separators=(",", ":")).encode()
        )
        self.assertEqual(hash1, hash2)

    def test_drops_default_keys(self):
        """Default drop set removes 'time' and 'releaseTime' before hashing."""
        payload_with_time = json.dumps(
            {"keep": 1, "time": "2025-01-01T00:00:00Z", "releaseTime": "2025-01-02T00:00:00Z"}
        ).encode()
        payload_without_time = json.dumps(
            {"keep": 1}
        ).encode()
        self.assertEqual(
            fetch_loader_manifests._stable_json_sha256(payload_with_time),
            fetch_loader_manifests._stable_json_sha256(payload_without_time),
        )

    def test_custom_drop(self):
        """Custom drop set removes specified keys before hashing."""
        payload_with_ignore = json.dumps(
            {"keep": 1, "ignore_me": "should not matter"}
        ).encode()
        payload_without_ignore = json.dumps(
            {"keep": 1}
        ).encode()
        self.assertEqual(
            fetch_loader_manifests._stable_json_sha256(
                payload_with_ignore, drop={"ignore_me"}
            ),
            fetch_loader_manifests._stable_json_sha256(payload_without_ignore),
        )

    def test_different_content_different_hash(self):
        """Different JSON content produces different hashes."""
        hash1 = fetch_loader_manifests._stable_json_sha256(b'{"a":1}')
        hash2 = fetch_loader_manifests._stable_json_sha256(b'{"a":2}')
        self.assertNotEqual(hash1, hash2)


class TestSha256File(unittest.TestCase):
    """Tests for fetch_registry_db.sha256_file."""

    def test_known_content(self):
        """SHA-256 of a temp file with known content matches expected hash."""
        expected = (
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        )
        with tempfile.NamedTemporaryFile(delete=False) as tmp:
            tmp.write(b"hello")
            tmp_path = tmp.name
        try:
            self.assertEqual(fetch_registry_db.sha256_file(Path(tmp_path)), expected)
        finally:
            os.unlink(tmp_path)

    def test_empty_file(self):
        """SHA-256 of an empty file matches the known empty-hash constant."""
        expected = (
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        )
        with tempfile.NamedTemporaryFile(delete=False) as tmp:
            tmp_path = tmp.name
        try:
            self.assertEqual(fetch_registry_db.sha256_file(Path(tmp_path)), expected)
        finally:
            os.unlink(tmp_path)


class TestVerifySha256AgainstDigest(unittest.TestCase):
    """Tests for fetch_registry_db.verify_sha256_against_digest."""

    def test_no_digest_skips(self):
        """When digest_field is None, no verification is performed (no exit)."""
        with tempfile.NamedTemporaryFile(delete=False) as tmp:
            tmp.write(b"hello")
            tmp_path = tmp.name
        try:
            # Should not raise or call sys.exit
            fetch_registry_db.verify_sha256_against_digest(Path(tmp_path), None)
        except SystemExit:
            self.fail("verify_sha256_against_digest called sys.exit with no digest")
        finally:
            os.unlink(tmp_path)

    def test_hex_digest_matches(self):
        """When digest is a hex string matching the file's SHA-256, no exit occurs."""
        expected = hashlib.sha256(b"hello").hexdigest()
        digest_field = f"sha256:{expected}"
        with tempfile.NamedTemporaryFile(delete=False) as tmp:
            tmp.write(b"hello")
            tmp_path = tmp.name
        try:
            fetch_registry_db.verify_sha256_against_digest(Path(tmp_path), digest_field)
        except SystemExit:
            self.fail("verify_sha256_against_digest called sys.exit on matching digest")
        finally:
            os.unlink(tmp_path)


if __name__ == "__main__":
    unittest.main()
