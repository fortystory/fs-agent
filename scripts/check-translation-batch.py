#!/usr/bin/env python3
"""语言迁移的作业工具：一个批次怎么验收、还剩哪些没翻。

它是**迁移期**的工具，不是常驻护栏 —— 常驻的那条线在 `scripts/check-language.py`
（冻结面不许新增中文、混住文件里的模型可见 / 进流串必须仍是英文、docs 中文占比、
注释中文行棘轮）。这个脚本回答两个问题：

    python3 scripts/check-translation-batch.py --diff <文件…> [--assert-msgs]
        一个批次改完后：diff 里除注释行（以及测试的断言消息）之外，不许有代码改动。

    python3 scripts/check-translation-batch.py --remaining
        还剩什么：① 给人看的英文散文（按文件列字数，已排除冻结面与断言消息）；
        ② 中文占比低于下限的 `docs/*.md`。

用法见 `.scratch/language-migration/spec.md` 的「方法」一节。
"""

from __future__ import annotations

import importlib.util
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def load_guard():
    spec = importlib.util.spec_from_file_location(
        "check_language", os.path.join(HERE, "check-language.py")
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


guard = load_guard()
CJK = guard.CJK


def diff_lines(files: list[str]) -> list[str]:
    diff = subprocess.run(
        ["git", "diff", "-U0", "--"] + files, capture_output=True, text=True
    ).stdout
    out = []
    for line in diff.split("\n"):
        if not line or line[0] not in "+-" or line.startswith(("+++", "---")):
            continue
        out.append(line)
    return out


def check_diff(files: list[str], allow_asserts: bool) -> int:
    bad = []
    for line in diff_lines(files):
        body = line[1:].strip()
        if not body or body.startswith("//"):
            continue
        if allow_asserts and guard.ASSERTION.search(body):
            continue
        bad.append(line)
    if bad:
        print(f"这个批次动了 {len(bad)} 行非注释代码：")
        for line in bad[:20]:
            print("  " + line)
        return 1
    print("验收通过：改动只落在注释行（以及测试断言消息）上")
    return 0


def remaining() -> int:
    # 冻结面（整个文件都是模型可见 / 进流的）整份跳过；混住文件只扣掉正面清单里
    # 那几条必须保持英文的串。
    frozen_files = set(guard.FROZEN_FILES)
    frozen_literals = [(p, prefix) for p, prefix in guard.FROZEN_LITERALS]
    rows = []
    for dirpath, _, files in os.walk("src"):
        for name in sorted(files):
            if not name.endswith(".rs") or "wording" in name:
                continue
            path = os.path.join(dirpath, name)
            if path in frozen_files:
                continue
            src = guard.code_only(open(path, encoding="utf-8").read())
            chars = 0
            for _, body, line_text in guard.literals(src):
                if CJK.search(body) or guard.ASSERTION.search(line_text):
                    continue
                if len(re.findall(r"[A-Za-z]{2,}", body)) < 5:
                    continue
                if any(p == path and prefix in body for p, prefix in frozen_literals):
                    continue
                chars += len(body)
            if chars:
                rows.append((chars, path))
    rows.sort(reverse=True)
    print(
        f"① 给人看的英文散文（src/，排除冻结面与断言消息）："
        f"{sum(r[0] for r in rows):,} 字符"
    )
    for chars, path in rows:
        print(f"   {chars:>6}  {path}")

    print("\n② 中文占比低于下限的 docs：")
    low = 0
    for path, floor in sorted(guard.DOCS_MIN_RATIO.items()):
        if not os.path.exists(path):
            continue
        text = open(path, encoding="utf-8").read()
        ratio = 100 * len(CJK.findall(text)) / max(len(text), 1)
        if ratio < floor:
            low += 1
            print(f"   {ratio:5.1f}% < {floor}%  {path}")
    if not low:
        print("   无")
    return 0


def main() -> int:
    args = sys.argv[1:]
    if "--remaining" in args:
        return remaining()
    if "--diff" in args:
        files = [a for a in args[args.index("--diff") + 1:] if not a.startswith("--")]
        if not files:
            print("--diff 后面要跟文件（或 `$(git show --stat --name-only --format= HEAD)`）")
            return 2
        return check_diff(files, "--assert-msgs" in args)
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
