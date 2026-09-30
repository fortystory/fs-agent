#!/usr/bin/env python3
"""拿一张 wayfinder 决策图与它的子票对账（本地 markdown tracker）。

本仓库的 tracker 是本地 markdown（docs/agents/issue-tracker.md），**没有子 issue、
也没有依赖边**。于是 wayfinder 的规矩退回到「图正文里一份任务清单 + 每张子票里的
`Part of:`」，而这个脚本就是原生 tracker 本来会做的那次「期望 vs 实际」检查：

  expected = 图里 `## 任务清单` 一节里的条目
  actual   = `<map-dir>/issues/*.md` 下的文件

它还核对每张子票都指回图（`Part of:`）、每个 `Blocked by:` 编号都能落到一张兄弟票、
`Type:`/`Status:` 都在且说得通。它打印 `closed/total`，于是一张有子票的图永远不会读出
`0/0`。任何不一致都以非零码退出，所以调用方不许把「被拒」读成「通过」。

用法：

    python3 scripts/wayfinder-check.py .scratch/tui-ux/map.md
"""

from __future__ import annotations

import glob
import os
import re
import sys

TYPES = {"research", "prototype", "grilling", "task", "implement"}
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

    has_task_list = any(
        (match := SECTION.match(line)) and match.group(1).strip() == "任务清单"
        for line in text.splitlines()
    )
    rows = [TASK_ROW.match(line) for line in section_lines(text, "任务清单")]
    rows = [row for row in rows if row]
    expected = len(rows)
    actual_paths = sorted(glob.glob(os.path.join(map_dir, "issues", "*.md")))
    actual = len(actual_paths)
    by_number = {
        os.path.basename(path).split("-", 1)[0]: path for path in actual_paths
    }

    failures: list[str] = []
    # 任务清单这条退路之前画的图（multi-agent-architecture、tui-layout）既没有
    # `## 任务清单`、也没有 `Part of:`。对它们，工具只报 closed/total 计数，不让退路
    # 检查去报红。
    if has_task_list:
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
            # 早先那些实现票（tui-layout 10–16）比 wayfinder 的 `Type:` 约定还早；
            # 只有带任务清单的图才要求这一行。
            if has_task_list:
                failures.append(f"{rel}: missing Type:")
        elif kind not in TYPES:
            failures.append(f"{rel}: unknown Type: {kind!r}")

        part = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Part of:")), None)
        if part is None:
            if has_task_list:
                failures.append(f"{rel}: missing Part of:")
        else:
            resolved = os.path.normpath(os.path.join(os.path.dirname(path), part))
            if resolved != map_path:
                failures.append(f"{rel}: Part of: {part!r} does not resolve to the map")

        blocked = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Blocked by:")), None)
        if blocked is None:
            if has_task_list:
                failures.append(f"{rel}: missing Blocked by:")
        else:
            cleaned = blocked.strip()
            if cleaned not in {"—", "-", "", "None", "none"}:
                for token in cleaned.split(","):
                    number = token.strip()
                    # 老图会在这里写散文（"29（/discuss …）"）；只有一个光编号才是
                    # 这个工具解得出的依赖。
                    if not number.isdigit():
                        continue
                    if number not in by_number:
                        failures.append(f"{rel}: Blocked by: {number!r} resolves to no ticket")

    if actual == 0:
        failures.append("map has no child tickets (closed/total would be 0/0)")

    print(f"map:      {os.path.relpath(map_path)}")
    if has_task_list:
        print(f"expected: {expected} task-list entries")
    else:
        print("expected: (no 任务清单 — legacy map, count check skipped)")
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
