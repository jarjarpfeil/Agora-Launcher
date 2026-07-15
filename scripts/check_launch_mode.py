"""Assert that the desktop settings default launch_mode remains "delegation".

This is a deploy-safety check: direct launch must stay opt-in. The default
must never silently switch to "direct", which would bypass the official Mojang
launcher without user awareness.

Usage:
    python scripts/check_launch_mode.py
"""

from pathlib import Path
import re
import sys

SETTINGS_FILE = Path("desktop/src/lib/useTypedSettings.ts")
EXPECTED_DEFAULT = "delegation"
REQUIRED_VALUES = {"direct", "delegation"}


def main() -> int:
    text = SETTINGS_FILE.read_text(encoding="utf-8")

    # Find the launchMode enumDef.
    # The third positional argument is the default.
    # TypeScript uses `as const` after the allowed-values array.
    m = re.search(
        r"launchMode\s*:\s*enumDef\(\s*['\"]launch_mode['\"]\s*,\s*"
        r"(\[[^\]]+\])\s*as\s+const\s*,\s*([^)\s]+)",
        text,
    )
    if not m:
        # Fallback: try without `as const` (older format).
        m = re.search(
            r"launchMode\s*:\s*enumDef\(\s*['\"]launch_mode['\"]\s*,\s*"
            r"(\[[^\]]+\])\s*,\s*([^)\s]+)",
            text,
        )
    if not m:
        print(
            "ERROR: Could not find launchMode enumDef in useTypedSettings.ts",
            file=sys.stderr,
        )
        return 1

    default_raw = m.group(2).strip().strip("'\"")
    if default_raw != EXPECTED_DEFAULT:
        print(
            f"ERROR: launchMode default is '{default_raw}', "
            f"expected '{EXPECTED_DEFAULT}'",
            file=sys.stderr,
        )
        return 1

    # Verify that both 'direct' and 'delegation' are in the allowed values.
    allowed_raw = m.group(1)
    for val in REQUIRED_VALUES:
        if val not in allowed_raw:
            print(
                f"ERROR: Allowed launch_mode values must include '{val}', "
                f"got {allowed_raw}",
                file=sys.stderr,
            )
            return 1

    print("OK: launchMode defaults to 'delegation' and 'direct' is available.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
