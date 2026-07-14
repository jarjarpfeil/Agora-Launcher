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


class TestMavenNameToPath(unittest.TestCase):
    """Tests for Maven coordinate → path conversion covering the full grammar:
    group:artifact:version[:classifier][@extension]"""

    def test_maven_name_to_path_standard(self):
        """Standard three-part Maven coordinate → .jar."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("net.fabricmc:fabric-loader:0.19.0"),
            "net/fabricmc/fabric-loader/0.19.0/fabric-loader-0.19.0.jar",
        )

    def test_maven_name_to_path_with_classifier(self):
        """Four-part Maven coordinate with classifier → .jar."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("org.lwjgl:lwjgl:3.3.1:natives-windows"),
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar",
        )

    def test_maven_name_to_path_at_jar(self):
        """@jar suffix (default packaging) strips to .jar extension."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("org.apache.commons:commons-lang3:3.13.0@jar"),
            "org/apache/commons/commons-lang3/3.13.0/commons-lang3-3.13.0.jar",
        )

    def test_maven_name_to_path_at_jar_with_classifier(self):
        """@jar with classifier strips to -classifier.jar."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("net.neoforged:mergetool:2.0.0:api@jar"),
            "net/neoforged/mergetool/2.0.0/mergetool-2.0.0-api.jar",
        )

    def test_maven_name_to_path_at_zip(self):
        """@zip extension produces .zip path."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412@zip"),
            "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip",
        )

    def test_maven_name_to_path_at_txt(self):
        """@txt extension produces .txt path."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("net.minecraft:client:1.20.1-20230612.114412:mappings@txt"),
            "net/minecraft/client/1.20.1-20230612.114412/client-1.20.1-20230612.114412-mappings.txt",
        )

    def test_maven_name_to_path_classifier_at_extension(self):
        """Classifier with non-jar extension."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412:mappings-merged@txt"),
            "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412-mappings-merged.txt",
        )

    def test_maven_name_to_path_all_classifier_jar(self):
        """Classifier with :all suffix."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("net.minecraftforge:ForgeAutoRenamingTool:0.1.22:all"),
            "net/minecraftforge/ForgeAutoRenamingTool/0.1.22/ForgeAutoRenamingTool-0.1.22-all.jar",
        )

    def test_maven_name_to_path_short(self):
        """Two-part coordinate (unusual) just appends .jar."""
        result = fetch_loader_manifests._maven_name_to_path("a:b")
        self.assertTrue(result.endswith(".jar"))

    def test_maven_name_to_path_neoforge_processor(self):
        """NeoForge processor jar reference (common pattern)."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("net.neoforged.installertools:installertools:2.1.2"),
            "net/neoforged/installertools/installertools/2.1.2/installertools-2.1.2.jar",
        )

    def test_maven_name_to_path_neoforge_at_jar_classpath(self):
        """NeoForge classpath entry with @jar."""
        self.assertEqual(
            fetch_loader_manifests._maven_name_to_path("net.sf.jopt-simple:jopt-simple:5.0.4@jar"),
            "net/sf/jopt-simple/jopt-simple/5.0.4/jopt-simple-5.0.4.jar",
        )


class TestLibraryPinHelpers(unittest.TestCase):
    """Tests for fetch_loader_manifests library-pin helpers."""


    def test_is_safe_maven_path_accepts_normal_jar(self):
        """Normal relative .jar path."""
        self.assertTrue(
            fetch_loader_manifests._is_safe_maven_path(
                "net/fabricmc/fabric-loader/0.19.0/fabric-loader-0.19.0.jar"
            )
        )

    def test_is_safe_maven_path_accepts_zip(self):
        """Relative .zip path is accepted."""
        self.assertTrue(
            fetch_loader_manifests._is_safe_maven_path(
                "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip"
            )
        )

    def test_is_safe_maven_path_accepts_txt(self):
        """Relative .txt path is accepted."""
        self.assertTrue(
            fetch_loader_manifests._is_safe_maven_path(
                "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412-mappings.txt"
            )
        )

    def test_is_safe_maven_path_accepts_plus_in_version(self):
        """Path with '+' in the version component is accepted (e.g. sponge-mixin)."""
        self.assertTrue(
            fetch_loader_manifests._is_safe_maven_path(
                "net/fabricmc/sponge-mixin/0.14.0+mixin.0.8.6/sponge-mixin-0.14.0+mixin.0.8.6.jar"
            )
        )

    def test_is_safe_maven_path_rejects_absolute(self):
        """Leading / is rejected."""
        self.assertFalse(
            fetch_loader_manifests._is_safe_maven_path("/absolute/path/lib.jar")
        )

    def test_is_safe_maven_path_rejects_dotdot(self):
        """Traversal via .. is rejected."""
        self.assertFalse(
            fetch_loader_manifests._is_safe_maven_path("../../lib.jar")
        )

    def test_is_safe_maven_path_rejects_drive_letter(self):
        """Windows drive prefix is rejected."""
        self.assertFalse(
            fetch_loader_manifests._is_safe_maven_path("C:/Windows/lib.jar")
        )

    def test_is_safe_maven_path_rejects_unknown_extension(self):
        """Path with unknown extension (.dll) is rejected."""
        self.assertFalse(
            fetch_loader_manifests._is_safe_maven_path("path/to/lib.dll")
        )

    def test_is_safe_maven_path_rejects_extra_dot_pattern(self):
        """Non-standard extension without .jar/.zip/.txt is rejected."""
        self.assertFalse(
            fetch_loader_manifests._is_safe_maven_path("path/to/lib.bin")
        )

    def test_is_valid_sha256_accepts_lowercase_hex(self):
        """64-char lowercase hex is valid."""
        self.assertTrue(
            fetch_loader_manifests._is_valid_sha256("a" * 64)
        )

    def test_is_valid_sha256_rejects_uppercase(self):
        """Uppercase hex chars are rejected."""
        self.assertFalse(
            fetch_loader_manifests._is_valid_sha256("A" * 64)
        )

    def test_is_valid_sha256_rejects_short(self):
        """Less than 64 chars is invalid."""
        self.assertFalse(
            fetch_loader_manifests._is_valid_sha256("a" * 63)
        )

    def test_is_valid_sha256_rejects_non_hex(self):
        """Non-hex characters are rejected."""
        self.assertFalse(
            fetch_loader_manifests._is_valid_sha256("z" + "a" * 63)
        )

    def test_extract_pins_from_profile_grabs_sha256_from_downloads_artifact(self):
        """Library with downloads.artifact.sha1 → no SHA-256 pin emitted."""
        profile = {
            "libraries": [
                {
                    "name": "org.ow2.asm:asm:9.7",
                    "downloads": {
                        "artifact": {
                            "path": "org/ow2/asm/asm/9.7/asm-9.7.jar",
                            "url": "https://maven.fabricmc.net/org/ow2/asm/asm/9.7/asm-9.7.jar",
                            "sha1": "abc123...",
                            "size": 12345,
                        }
                    },
                }
            ]
        }
        pins = fetch_loader_manifests._extract_pins_from_profile(profile)
        # No SHA-256 field → no pin
        self.assertEqual(pins, {})

    def test_extract_pins_from_profile_uses_top_level_sha256(self):
        """Library with top-level sha256 field produces a pin."""
        profile = {
            "libraries": [
                {
                    "name": "net.fabricmc:fabric-loader:0.19.0",
                    "url": "https://maven.fabricmc.net/",
                    "sha256": "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
                }
            ]
        }
        pins = fetch_loader_manifests._extract_pins_from_profile(profile)
        expected_path = "net/fabricmc/fabric-loader/0.19.0/fabric-loader-0.19.0.jar"
        self.assertIn(expected_path, pins)
        self.assertEqual(
            pins[expected_path],
            "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
        )

    def test_extract_pins_from_profile_ignores_invalid_sha256(self):
        """Library with invalid SHA-256 (non-hex) does not produce a pin."""
        profile = {
            "libraries": [
                {
                    "name": "some.group:artifact:1.0",
                    "url": "https://maven.example.com/",
                    "sha256": "z" + "a" * 63,  # starts with non-hex 'z'
                }
            ]
        }
        pins = fetch_loader_manifests._extract_pins_from_profile(profile)
        self.assertEqual(pins, {})

    def test_merge_pins_into_accumulates(self):
        """Multiple libraries with distinct paths are accumulated."""
        acc: dict[str, str] = {}
        pins1 = {"path/a.jar": "a" * 64}
        pins2 = {"path/b.jar": "b" * 64}
        fetch_loader_manifests._merge_pins_into(acc, pins1)
        fetch_loader_manifests._merge_pins_into(acc, pins2)
        self.assertEqual(len(acc), 2)

    def test_merge_pins_into_detects_conflict(self):
        """Same path with different SHA-256 raises ValueError."""
        acc: dict[str, str] = {"path/a.jar": "a" * 64}
        with self.assertRaises(ValueError):
            fetch_loader_manifests._merge_pins_into(
                acc, {"path/a.jar": "b" * 64}, source_label="conflict-test"
            )

    def test_merge_pins_into_allows_identical_hash(self):
        """Same path with identical SHA-256 is silently accepted."""
        acc: dict[str, str] = {"path/a.jar": "a" * 64}
        fetch_loader_manifests._merge_pins_into(acc, {"path/a.jar": "a" * 64})
        self.assertEqual(acc["path/a.jar"], "a" * 64)

    def test_extract_library_paths_from_profile_downloads_artifact(self):
        """Library with downloads.artifact.path yields a path."""
        profile = {
            "libraries": [
                {
                    "downloads": {
                        "artifact": {
                            "path": "org/ow2/asm/asm/9.7/asm-9.7.jar",
                            "url": "https://maven.fabricmc.net/org/ow2/asm/asm/9.7/asm-9.7.jar",
                        }
                    },
                }
            ]
        }
        paths = fetch_loader_manifests._extract_library_paths_from_profile(profile)
        self.assertIn("org/ow2/asm/asm/9.7/asm-9.7.jar", paths)

    def test_extract_library_paths_from_profile_maven_name(self):
        """Library with Maven name + url yields a path."""
        profile = {
            "libraries": [
                {
                    "name": "net.fabricmc:fabric-loader:0.19.0",
                    "url": "https://maven.fabricmc.net/",
                }
            ]
        }
        paths = fetch_loader_manifests._extract_library_paths_from_profile(profile)
        self.assertIn(
            "net/fabricmc/fabric-loader/0.19.0/fabric-loader-0.19.0.jar",
            paths,
        )

    def test_extract_library_paths_from_profile_deduplicates(self):
        """Identical paths across libraries are de-duplicated."""
        profile = {
            "libraries": [
                {
                    "downloads": {
                        "artifact": {
                            "path": "libs/dup.jar",
                            "url": "https://example.com/libs/dup.jar",
                        }
                    },
                },
                {
                    "downloads": {
                        "artifact": {
                            "path": "libs/dup.jar",
                            "url": "https://example.com/libs/dup.jar",
                        }
                    },
                },
            ]
        }
        paths = fetch_loader_manifests._extract_library_paths_from_profile(profile)
        self.assertEqual(paths, ["libs/dup.jar"])

    def test_extract_library_paths_from_profile_skips_unsafe_path(self):
        """Unsafe paths (absolute, traversal) are filtered out."""
        profile = {
            "libraries": [
                {
                    "downloads": {
                        "artifact": {
                            "path": "/etc/passwd",
                            "url": "file:///etc/passwd",
                        }
                    },
                }
            ]
        }
        paths = fetch_loader_manifests._extract_library_paths_from_profile(profile)
        self.assertEqual(paths, [])

    def test_profile_library_paths_accumulator_persistence(self):
        """Simulate accumulation of profile_library_paths during fetching."""
        acc: dict[str, list[str]] = {}
        profile_a = {
            "libraries": [
                {
                    "name": "net.fabricmc:fabric-loader:0.19.0",
                    "url": "https://maven.fabricmc.net/",
                }
            ]
        }
        profile_b = {
            "libraries": [
                {
                    "name": "org.ow2.asm:asm:9.7",
                    "url": "https://maven.fabricmc.net/",
                }
            ]
        }
        file_a = "fabric-loader-0.19.0-1.21.json"
        file_b = "fabric-loader-0.18.6-1.21.json"

        acc[file_a] = fetch_loader_manifests._extract_library_paths_from_profile(profile_a)
        acc[file_b] = fetch_loader_manifests._extract_library_paths_from_profile(profile_b)

        self.assertEqual(len(acc), 2)
        self.assertIn(
            "net/fabricmc/fabric-loader/0.19.0/fabric-loader-0.19.0.jar",
            acc[file_a],
        )
        self.assertIn(
            "org/ow2/asm/asm/9.7/asm-9.7.jar",
            acc[file_b],
        )

    def test_pin_accumulator_merged_into_manifest(self):
        """Simulate the full accumulation flow: fetch profiles → merge → manifest."""
        # Two Fabric profile excerpts
        profile_a = {
            "libraries": [
                {
                    "name": "net.fabricmc:fabric-loader:0.19.0",
                    "url": "https://maven.fabricmc.net/",
                    "sha256": "a" * 64,
                }
            ]
        }
        profile_b = {
            "libraries": [
                {
                    "name": "org.ow2.asm:asm:9.7",
                    "url": "https://maven.fabricmc.net/",
                    "sha256": "b" * 64,
                }
            ]
        }

        acc: dict[str, str] = {}
        pins_a = fetch_loader_manifests._extract_pins_from_profile(profile_a)
        pins_b = fetch_loader_manifests._extract_pins_from_profile(profile_b)
        fetch_loader_manifests._merge_pins_into(acc, pins_a, source_label="profile_a")
        fetch_loader_manifests._merge_pins_into(acc, pins_b, source_label="profile_b")

        self.assertEqual(len(acc), 2)
        self.assertEqual(
            acc["net/fabricmc/fabric-loader/0.19.0/fabric-loader-0.19.0.jar"],
            "a" * 64,
        )
        self.assertEqual(
            acc["org/ow2/asm/asm/9.7/asm-9.7.jar"],
            "b" * 64,
        )


class TestInstallerLibraryVersionJson(unittest.TestCase):
    """Tests for version.json library extraction (replaces processor-scanned paths).

    The managed installer processor subsystem has been replaced by
    installed-profile adoption. _extract_installer_library_paths now
    only returns paths from the version.json inside the installer JAR.
    """

    def _make_version_json(self, libraries: list[dict] | None = None) -> dict:
        """Build a minimal version.json with the given libraries (or empty)."""
        return {
            "id": "forge-1.21-47.1.0",
            "inheritsFrom": "1.21",
            "mainClass": "net.minecraftforge.Main",
            "libraries": libraries or [],
        }

    def test_empty_version_json_returns_empty(self):
        """Empty version.json libraries produce no paths."""
        vj = self._make_version_json([])
        paths = fetch_loader_manifests._extract_installer_library_paths(vj)
        self.assertEqual(paths, [])

    def test_none_version_json_returns_empty(self):
        """None version.json produces no paths."""
        paths = fetch_loader_manifests._extract_installer_library_paths(None)
        self.assertEqual(paths, [])

    def test_version_json_library_path_included(self):
        """Library with downloads.artifact.path is included."""
        vj = self._make_version_json([
            {
                "name": "net.minecraft:minecraft:1.21",
                "downloads": {
                    "artifact": {
                        "path": "net/minecraft/minecraft/1.21/minecraft-1.21.jar",
                        "url": "https://libraries.minecraft.net/net/minecraft/minecraft/1.21/minecraft-1.21.jar",
                        "sha1": "abc123",
                    }
                },
            }
        ])
        paths = fetch_loader_manifests._extract_installer_library_paths(vj)
        self.assertIn(
            "net/minecraft/minecraft/1.21/minecraft-1.21.jar", paths
        )

    def test_version_json_maven_name_fallback(self):
        """Library with only Maven name produces a path via _maven_name_to_path."""
        vj = self._make_version_json([
            {
                "name": "net.minecraftforge:forge:1.21-47.1.0",
                "url": "https://maven.minecraftforge.net/",
            }
        ])
        paths = fetch_loader_manifests._extract_installer_library_paths(vj)
        expected = "net/minecraftforge/forge/1.21-47.1.0/forge-1.21-47.1.0.jar"
        self.assertIn(expected, paths)

    def test_version_json_sorted_unique(self):
        """Paths are de-duplicated and sorted."""
        lib = {
            "name": "org.ow2.asm:asm:9.7",
            "downloads": {
                "artifact": {
                    "path": "org/ow2/asm/asm/9.7/asm-9.7.jar",
                    "url": "https://maven.fabricmc.net/org/ow2/asm/asm/9.7/asm-9.7.jar",
                }
            },
        }
        vj = self._make_version_json([lib, lib])
        paths = fetch_loader_manifests._extract_installer_library_paths(vj)
        self.assertEqual(len(paths), 1)

    def test_extract_pins_from_install_profile_includes_version_json_lib(self):
        """_extract_pins_from_install_profile processes version.json libraries."""
        vj = self._make_version_json([
            {
                "name": "net.fabricmc:fabric-loader:0.19.0",
                "url": "https://maven.fabricmc.net/",
            }
        ])
        pins: dict[str, str] = {}
        fetch_loader_manifests._extract_pins_from_install_profile(vj, pins)
        # The library has no embedded SHA-256, so no pin is emitted.
        # This test verifies the function runs without error.
        self.assertIsInstance(pins, dict)

    def test_extract_pins_from_install_profile_none_works(self):
        """None version_json results in no pins."""
        pins: dict[str, str] = {}
        fetch_loader_manifests._extract_pins_from_install_profile(None, pins)
        self.assertEqual(pins, {})

    # -------------------------------------------------------------------
    # verify_pin_coverage tests (unchanged)
    # -------------------------------------------------------------------

    def test_verify_pin_coverage_all_covered(self):
        """_verify_pin_coverage returns True when all paths have pins."""
        paths = ["path/a.jar", "path/b.jar"]
        pins = {"path/a.jar": "a" * 64, "path/b.jar": "b" * 64}
        self.assertTrue(
            fetch_loader_manifests._verify_pin_coverage(paths, pins, "test")
        )

    def test_verify_pin_coverage_missing(self):
        """_verify_pin_coverage returns False when a path is missing a pin."""
        paths = ["path/a.jar", "path/b.jar"]
        pins = {"path/a.jar": "a" * 64}
        self.assertFalse(
            fetch_loader_manifests._verify_pin_coverage(paths, pins, "test")
        )

    def test_verify_pin_coverage_empty_paths(self):
        """_verify_pin_coverage returns True when paths list is empty."""
        self.assertTrue(
            fetch_loader_manifests._verify_pin_coverage([], {}, "test")
        )


if __name__ == "__main__":
    unittest.main()
