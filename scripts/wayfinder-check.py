#!/usr/bin/env python3
"""Check a wayfinder map against its child tickets (local-markdown tracker).

The tracker in this repo is local markdown (docs/agents/issue-tracker.md), which
has **no native sub-issue or dependency edges**. The wayfinder rule therefore
falls back to a task list in the map body plus `Part of:` in every child, and
this script is the "expected vs actual" check that the native-edge script would
otherwise perform:

  expected = entries in the map's `## 任务清单` section
  actual   = files under `<map-dir>/issues/*.md`

It also verifies every child points back at the map (`Part of:`), that every
`Blocked by:` number resolves to a sibling ticket, and that `Type:`/`Status:`
are present and sane. It prints `closed/total` so a backed map can never read
`0/0` while it still has children. Exits non-zero on any mismatch, so a caller
must not read a rejection as success.

Usage:

    python3 scripts/wayfinder-check.py .scratch/tui-ux/map.md
"""

from __future__ import annotations

import glob
import os
import re
import sys

TYPES = {"research", "prototype", "grilling", "task"}
CLOSED_STATUSES = {"resolved", "done", "closed"}

TASK_ROW = re.compile(r"^\s*-\s*\[([ xX])\]\s*\[([^\]]+)\]\(([^)]+)\)\s*$")
SECTION = re.compile(r"^##\s+(.+?)\s*$")


def read(path: str) -> str:
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def section_lines(text: str, heading: str) -> list[str]:
    want = heading.strip()
    out: list[str] = []
    inside = False
    for line in text.splitlines():
        match = SECTION.match(line)
        if match:
            inside = match.group(1).strip() == want
            continue
        if inside:
            out.append(line)
    return out


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {argv[0]} <map.md>", file=sys.stderr)
        return 2
    map_path = os.path.abspath(argv[1])
    map_dir = os.path.dirname(map_path)
    if not os.path.isfile(map_path):
        print(f"FAIL map not found: {map_path}")
        return 1

    text = read(map_path)

    rows = [TASK_ROW.match(line) for line in section_lines(text, "任务清单")]
    rows = [row for row in rows if row]
    expected = len(rows)
    actual_paths = sorted(glob.glob(os.path.join(map_dir, "issues", "*.md")))
    actual = len(actual_paths)
    by_number = {
        os.path.basename(path).split("-", 1)[0]: path for path in actual_paths
    }

    failures: list[str] = []
    if expected != actual:
        failures.append(
            f"task-list count {expected} != child-file count {actual}"
        )

    listed: set[str] = set()
    for row in rows:
        target = os.path.normpath(os.path.join(map_dir, row.group(3)))
        listed.add(target)
        if not os.path.isfile(target):
            failures.append(f"task-list entry points at missing file: {row.group(3)}")
    for path in actual_paths:
        if path not in listed:
            failures.append(f"child file not in task list: {os.path.relpath(path, map_dir)}")

    closed = 0
    for path in actual_paths:
        rel = os.path.relpath(path, map_dir)
        body = read(path)
        lines = body.splitlines()

        status = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Status:")), None)
        if status is None:
            failures.append(f"{rel}: missing Status:")
        elif status.lower() in CLOSED_STATUSES:
            closed += 1

        kind = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Type:")), None)
        if kind is None:
            failures.append(f"{rel}: missing Type:")
        elif kind not in TYPES:
            failures.append(f"{rel}: unknown Type: {kind!r}")

        part = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Part of:")), None)
        if part is None:
            failures.append(f"{rel}: missing Part of:")
        else:
            resolved = os.path.normpath(os.path.join(os.path.dirname(path), part))
            if resolved != map_path:
                failures.append(f"{rel}: Part of: {part!r} does not resolve to the map")

        blocked = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Blocked by:")), None)
        if blocked is None:
            failures.append(f"{rel}: missing Blocked by:")
        else:
            cleaned = blocked.strip()
            if cleaned not in {"—", "-", "", "None", "none"}:
                for token in cleaned.split(","):
                    number = token.strip()
                    if not number:
                        continue
                    if number not in by_number:
                        failures.append(f"{rel}: Blocked by: {number!r} resolves to no ticket")

    if actual == 0:
        failures.append("map has no child tickets (closed/total would be 0/0)")

    print(f"map:      {os.path.relpath(map_path)}")
    print(f"expected: {expected} task-list entries")
    print(f"actual:   {actual} child files under issues/")
    print(f"closed/total: {closed}/{actual} by Status (checkbox marks: "
          f"{sum(1 for r in rows if r.group(1).lower() == 'x')}/{expected})")
    if failures:
        print("FAIL")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
