"""Fetch registry.db and registry.db.sig from the latest GitHub Release.

Usage:
    python scripts/fetch_registry_db.py [output_dir] [--force]

The script reads GITHUB_REPOSITORY (owner/repo) and optional GITHUB_TOKEN
from environment, queries the GitHub API for releases, finds the most recent
release whose tag_name starts with "registry-", and downloads the DB and
signature assets to the target directory.

Integrity verification (per AGENTS.md: "Verify every download with SHA-256
and package signatures"):
  - SHA-256: always checked against the GitHub asset `digest` field when
    the API returns one. Aborts on mismatch.
  - Ed25519 signature: when registry.db.sig is present AND the `cryptography`
    Python package is importable, the signature is verified against the
    Ed25519 public key. The key is read from the AGORA_REGISTRY_PUBKEY env
    var (hex); if unset, the same hardcoded default the desktop build uses
    applies. Signature mismatch aborts the fetch.

Overwrite safety: refuses to overwrite an existing registry.db unless
`--force` is passed, so a developer's locally-compiled DB is not silently
replaced by a release artifact.

Environment variables:
    GITHUB_REPOSITORY       Required. Format: owner/repo (e.g. MyOrg/my-repo).
    GITHUB_TOKEN            Optional. Bearer token for authenticated requests.
    OUTPUT_DIR              Optional. CLI arg overrides this.
    AGORA_REGISTRY_PUBKEY   Optional. Ed25519 public key (hex) for signature
                            verification. Defaults to the in-repo pinned key.
"""

import base64
import hashlib
import json
import os
import sys
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


# Default Ed25519 public key (hex). Must match registry_sync.rs::REGISTRY_PUBKEY_HEX.
# Overridable via the AGORA_REGISTRY_PUBKEY environment variable so CI can pin
# a different key without rebuilding the script.
DEFAULT_REGISTRY_PUBKEY_HEX = (
    "47adee76cf587ee618f79eb2fa5bde003824d3bfc2dbb5080d33073c5a8f8c18"
)


def log(msg: str) -> None:
    print(f"[fetch_registry_db] {msg}")


def api_get(url: str, token: str | None) -> dict:
    """Perform a GET request and return parsed JSON."""
    headers = {"Accept": "application/vnd.github+json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = Request(url, headers=headers)
    log(f"GET {url}")
    with urlopen(req) as resp:
        return json.loads(resp.read().decode())


def download_file(url: str, dest: Path) -> None:
    """Stream a file from url to dest."""
    req = Request(url)
    log(f"Downloading {dest.name} ...")
    with urlopen(req) as resp:
        dest.write_bytes(resp.read())
    log(f"Downloaded {dest.name} ({dest.stat().st_size} bytes)")


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def verify_sha256_against_digest(path: Path, digest_field: str | None) -> None:
    """Verify the downloaded file's SHA-256 against the GitHub asset digest.

    GitHub's assets[].digest is formatted as "<algo>:<value>". The value is
    base64-encoded for most assets, but some paths return hex; we normalize
    both forms and compare against the local hex digest.
    """
    if not digest_field:
        log("WARNING: GitHub asset has no `digest` field; skipping SHA-256 check.")
        return

    algo, _, value = digest_field.partition(":")
    if algo.lower() != "sha256":
        print(
            f"ERROR: Unexpected digest algorithm '{algo}' (expected sha256).",
            file=sys.stderr,
        )
        sys.exit(1)

    actual = sha256_file(path)

    # Normalize the expected value to lowercase hex.
    expected_hex = value.lower()
    # If it doesn't look like hex (odd length or non-hex chars), try base64.
    is_hex = len(expected_hex) % 2 == 0 and all(
        c in "0123456789abcdef" for c in expected_hex
    )
    if not is_hex:
        try:
            expected_hex = base64.b64decode(value).hex().lower()
        except Exception:
            print(
                f"ERROR: Could not decode digest value '{value}' as hex or base64.",
                file=sys.stderr,
            )
            sys.exit(1)

    if actual != expected_hex:
        print(
            f"ERROR: SHA-256 mismatch for {path.name}.\n"
            f"  expected: {expected_hex}\n  actual:   {actual}",
            file=sys.stderr,
        )
        sys.exit(1)
    log(f"SHA-256 verified: {actual}")


def verify_ed25519_signature(db_path: Path, sig_path: Path) -> None:
    """Verify the registry.db Ed25519 signature using the `cryptography` package.

    Aborts on mismatch. If `cryptography` is not importable, falls back to a
    warning (SHA-256 integrity is still enforced by verify_sha256_against_digest)
    so the script remains runnable in minimal environments.
    """
    pubkey_hex = os.environ.get("AGORA_REGISTRY_PUBKEY", DEFAULT_REGISTRY_PUBKEY_HEX).strip()
    if not pubkey_hex:
        log("WARNING: AGORA_REGISTRY_PUBKEY empty; skipping Ed25519 signature verification.")
        return

    try:
        from cryptography.hazmat.primitives.asymmetric.ed25519 import (
            Ed25519PublicKey,
        )
    except ImportError:
        log(
            "WARNING: `cryptography` package not importable; "
            "skipping Ed25519 signature verification (SHA-256 still enforced)."
        )
        return

    try:
        pubkey_bytes = bytes.fromhex(pubkey_hex)
    except ValueError:
        print(
            f"ERROR: AGORA_REGISTRY_PUBKEY is not valid hex: '{pubkey_hex}'.",
            file=sys.stderr,
        )
        sys.exit(1)

    try:
        pub = Ed25519PublicKey.from_public_bytes(pubkey_bytes)
    except Exception as exc:
        print(f"ERROR: Invalid registry public key: {exc}", file=sys.stderr)
        sys.exit(1)

    db_bytes = db_path.read_bytes()
    sig_bytes = sig_path.read_bytes()
    try:
        pub.verify(sig_bytes, db_bytes)
    except Exception:
        print(
            f"ERROR: Ed25519 signature verification FAILED for {db_path.name}. "
            "Refusing to use an untrusted registry.db.",
            file=sys.stderr,
        )
        # Remove the untrusted artifacts so they cannot be accidentally used.
        try:
            db_path.unlink()
        except OSError:
            pass
        try:
            sig_path.unlink()
        except OSError:
            pass
        sys.exit(1)
    log("Ed25519 signature verified.")


def parse_args(argv: list[str]) -> tuple[Path, bool]:
    """Parse [output_dir] [--force]."""
    force = False
    positional: list[str] = []
    for arg in argv[1:]:
        if arg == "--force":
            force = True
        elif arg.startswith("-"):
            print(f"ERROR: Unknown flag '{arg}'.", file=sys.stderr)
            print(__doc__, file=sys.stderr)
            sys.exit(2)
        else:
            positional.append(arg)
    output_dir = Path(positional[0]) if positional else Path(".")
    return output_dir, force


def main() -> None:
    output_dir, force = parse_args(sys.argv)
    output_dir.mkdir(parents=True, exist_ok=True)

    db_dest = output_dir / "registry.db"
    sig_dest = output_dir / "registry.db.sig"

    if db_dest.exists() and not force:
        print(
            f"ERROR: {db_dest} already exists. Pass --force to overwrite.\n"
            "This guard prevents silently replacing a locally-compiled registry.db.",
            file=sys.stderr,
        )
        sys.exit(1)

    repo = os.environ.get("GITHUB_REPOSITORY")
    if not repo:
        print("ERROR: GITHUB_REPOSITORY environment variable is not set.", file=sys.stderr)
        sys.exit(1)

    token = os.environ.get("GITHUB_TOKEN")

    log(f"Repository: {repo}")
    log(f"Output directory: {output_dir}")

    # --- Fetch releases ---------------------------------------------------
    releases_url = (
        f"https://api.github.com/repos/{repo}/releases?per_page=100"
    )
    try:
        releases = api_get(releases_url, token)
    except (HTTPError, URLError) as exc:
        print(f"ERROR: Failed to fetch releases: {exc}", file=sys.stderr)
        sys.exit(1)

    if not releases:
        print("ERROR: No releases found.", file=sys.stderr)
        sys.exit(1)

    # --- Find the latest registry-* release -------------------------------
    registry_release = None
    for rel in releases:
        if rel["tag_name"].startswith("registry-"):
            registry_release = rel
            break

    if registry_release is None:
        print(
            "ERROR: No release with tag_name starting with 'registry-' found.",
            file=sys.stderr,
        )
        sys.exit(1)

    tag = registry_release["tag_name"]
    log(f"Using release: {tag}")

    # --- Resolve asset URLs + digests ------------------------------------
    db_url = None
    db_digest = None
    sig_url = None
    for asset in registry_release.get("assets", []):
        name = asset["name"]
        if name == "registry.db":
            db_url = asset["browser_download_url"]
            db_digest = asset.get("digest")
        elif name == "registry.db.sig":
            sig_url = asset["browser_download_url"]

    if db_url is None:
        print("ERROR: 'registry.db' asset not found in release.", file=sys.stderr)
        sys.exit(1)

    log(f"registry.db URL: {db_url}")
    if sig_url:
        log(f"registry.db.sig URL: {sig_url}")
    else:
        log("registry.db.sig not present in release; skipping signature.")

    # --- Download ---------------------------------------------------------
    try:
        download_file(db_url, db_dest)
    except (HTTPError, URLError) as exc:
        print(f"ERROR: Failed to download registry.db: {exc}", file=sys.stderr)
        sys.exit(1)

    if sig_url:
        try:
            download_file(sig_url, sig_dest)
        except (HTTPError, URLError) as exc:
            print(
                f"ERROR: Failed to download registry.db.sig: {exc}",
                file=sys.stderr,
            )
            sys.exit(1)

    # --- Integrity verification -------------------------------------------
    verify_sha256_against_digest(db_dest, db_digest)

    if sig_dest.exists():
        verify_ed25519_signature(db_dest, sig_dest)
    else:
        log("WARNING: No signature file present; only SHA-256 (if available) was verified.")

    log("Done.")


if __name__ == "__main__":
    main()
