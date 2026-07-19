#!/usr/bin/env python3
"""Quick sanity check for the compiled registry.db."""
import argparse
import sqlite3
import sys
from pathlib import Path


TABLE_NAMES_QUERY = """
    SELECT name FROM sqlite_master
    WHERE type = 'table'
    ORDER BY name
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "db_path",
        nargs="?",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "registry.db",
    )
    db_path = parser.parse_args().db_path
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        return 1

    conn = sqlite3.connect(str(db_path))
    cur = conn.cursor()

    tables = [row[0] for row in cur.execute(TABLE_NAMES_QUERY)]
    for table in tables:
        count = cur.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0]
        print(f"{table}: {count}")

    print("\nregistry_items (first 20):")
    for row in cur.execute(
        "SELECT id, content_type, download_strategy, is_immune FROM registry_items LIMIT 20"
    ):
        print(" ", row)

    conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
