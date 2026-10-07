"""`scripts/check-doc-size.py` 的测试。

测的是**脚本的 CLI**：断言只打在「退出码」与「stdout 里有没有那一项」上，不打内部函数 ——
内部实现可以随意重构而不动这里。fixture 全部写在临时目录里（跑脚本时把工作目录切过去），
不依赖真实仓库文件；真实 40 份文档的状态由 `python3 scripts/check-doc-size.py` 本身与人工走查核。

形态照 `scripts/tests/test_lifecycle_check.py`：一票一条用例，跑法是 `python3 -m unittest`。
覆盖清单见 `.scratch/docs-slim/issues/08-doc-size-guardrail.md` 的「验证」一节。
"""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from contextlib import contextmanager

SCRIPT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "check-doc-size.py"
)


def load_module():
    """只为拿 `DOC_FILES` 这份清单来搭 fixture；被测行为一律走 CLI。"""
    spec = importlib.util.spec_from_file_location("check_doc_size_under_test", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_doc_size_under_test"] = module
    spec.loader.exec_module(module)
    return module


_GUARDRAIL = load_module()
DOC_FILES = _GUARDRAIL.DOC_FILES
# 行数上限从脚本里现取：写死数字的话，预算一放宽（`AGENTS.md` 2026-10-06、`README.md`
# 2026-10-07 各放宽过一次）这条用例就变成假红。
README_LINE_LIMIT = _GUARDRAIL.ENTRY_BUDGET["README.md"]["lines"]

SHORT = "# 标题\n\n这是一句短话，用中文写。\n"


@contextmanager
def fixture(overrides=None, drop=None, extra=None):
    """在临时目录里搭一个「仓库根」：40 份清单文档齐全，再按需替换 / 删除 / 追加。"""
    overrides, drop, extra = overrides or {}, drop or [], extra or {}
    with tempfile.TemporaryDirectory() as root:
        for rel in DOC_FILES:
            path = os.path.join(root, rel)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(overrides.get(rel, SHORT))
        for rel in drop:
            os.remove(os.path.join(root, rel))
        for rel, text in extra.items():
            path = os.path.join(root, rel)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(text)
        yield root


def run(*args, cwd=None):
    """跑脚本；`cwd` 指向 fixture 搭出来的「仓库根」（脚本的相对路径都相对它）。"""
    return subprocess.run(
        [sys.executable, SCRIPT, *args], capture_output=True, text=True, cwd=cwd
    )


def long_prose():
    """一个 >500 的散文单元：四个短句，避免命中 R3 / R4 那两条复核规则。"""
    return "甲" * 130 + "。" + "乙" * 130 + "。" + "丙" * 130 + "。" + "丁" * 130 + "。"


def table_with(cell):
    return "# 标题\n\n| 头 |\n| --- |\n| " + cell + " |\n"


class CleanFixtureTest(unittest.TestCase):
    """首装即绿：全部 fixture 在基线内时退出码为 0。"""

    def test_clean_fixture_passes(self):
        with fixture() as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("check-doc-size: OK", result.stdout)


class UnitThresholdTest(unittest.TestCase):
    """硬线：>500 违规且指出位置，=500 合格。"""

    def test_unit_over_limit_fails(self):
        unit = long_prose()  # 524 字符的四个短句：长度越线，但不命中 R3 / R4
        with fixture(overrides={"docs/bash.md": f"# 标题\n\n{unit}\n"}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("docs/bash.md", result.stdout)
            self.assertIn("524", result.stdout)

    def test_unit_at_limit_passes(self):
        unit = "甲" * 499 + "乙"  # 500 字符
        with fixture(overrides={"docs/bash.md": f"# 标题\n\n{unit}。\n"}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


class UnitSplitTest(unittest.TestCase):
    """单元定义第 ② 步：块首是散文、块里又出现清单项时，散文另算一个单元。"""

    def test_prose_head_before_list_is_its_own_unit(self):
        # 整块 604 字符，但拆成「散文 401 + 清单项 202」之后两边都合格。
        head = "甲" * 400 + "。"
        item = "- " + "乙" * 200
        with fixture(overrides={"docs/bash.md": f"# 标题\n\n{head}\n{item}\n"}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


class ExemptionTest(unittest.TestCase):
    """R1 / R2 自动放行；R3–R5 只打 `review:`，退出码不受影响。"""

    def test_quote_is_exempt(self):
        quoted = "> " + "引" * 300 + "\n> " + "文" * 300
        with fixture(overrides={"docs/bash.md": f"# 标题\n\n{quoted}\n"}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_pointer_cell_is_exempt(self):
        cell = "、".join(f"[文档{i}](docs/d{i}.md)" for i in range(25))
        with fixture(overrides={"docs/bash.md": table_with(cell)}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_review_rule_hints_without_failing(self):
        # 一个 600 字符的单句：R3 命中（按 `；、` 也切不出 ≤500 的片）。
        with fixture(overrides={"docs/bash.md": "# 标题\n\n" + "长" * 600 + "\n"}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("review:", result.stdout)

    def test_causal_chain_rule_hints(self):
        # R4：四句都以「这」开头（≥3 句且 ≥60%），总长 >500；最长句占比远低于 0.8，所以不触发 R3。
        unit = "。".join("这" + "甲" * 130 for _ in range(4)) + "。"
        with fixture(overrides={"docs/bash.md": f"# 标题\n\n{unit}\n"}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("R4", result.stdout)

    def test_ordered_sequence_rule_hints(self):
        # R5：单条编号项、四个次序词、不含 `；`。
        unit = "1. 先做甲，再做乙，然后做丙，最后做丁。" + "填充" * 250
        with fixture(overrides={"docs/bash.md": f"# 标题\n\n{unit}\n"}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("R5", result.stdout)

    def test_pointer_cell_short_of_separators_is_not_exempt(self):
        # 链接跨度够，但只有 3 个分隔符：R2 不命中，按违规报。
        cell = "、".join(f"[文档{i}](docs/d{i}.md)" for i in range(3)) + "尾" * 500
        with fixture(overrides={"docs/bash.md": table_with(cell)}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)


class ListTest(unittest.TestCase):
    """`--list` 打印全部单元与每条规则的当日命中数。"""

    def test_list_prints_units_and_rule_hits(self):
        with fixture(overrides={"docs/bash.md": "# 标题\n\n" + "长" * 600 + "\n"}) as root:
            result = run("--list", cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("全部单元", result.stdout)
            self.assertIn("docs/bash.md", result.stdout)
            self.assertIn("R3: 1 项", result.stdout)


class ManifestTest(unittest.TestCase):
    """清单自检：缺文件非零退出，清单外的 md 只提示。"""

    def test_missing_listed_file_fails(self):
        with fixture(drop=["docs/web.md"]) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("docs/web.md", result.stdout)
            self.assertIn("不存在", result.stdout)

    def test_unlisted_md_hint_does_not_fail(self):
        with fixture(extra={"docs/extra.md": SHORT}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("warn:", result.stdout)
            self.assertIn("docs/extra.md", result.stdout)

    def test_list_still_reports_missing_listed_file(self):
        # `--list` 不吞掉自检：缺清单里的文件时退出码仍非零。
        with fixture(drop=["docs/web.md"]) as root:
            result = run("--list", cwd=root)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("不存在", result.stdout)


class EntryBudgetTest(unittest.TestCase):
    """入口三份的两条并列上限：非空白字符数与行数。"""

    def test_over_char_budget_fails(self):
        with fixture(overrides={"README.md": "甲" * 21000}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("README.md", result.stdout)
            self.assertIn("非空白字符数", result.stdout)

    def test_over_line_budget_fails(self):
        # 段与段之间空一行：只撞「行数」那条上限，不顺手撞「单元 > 500」。
        with fixture(overrides={"README.md": "短句。\n\n" * README_LINE_LIMIT}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("行数", result.stdout)


class BaselineTest(unittest.TestCase):
    """违规计数：本轮收口后字典已清零 —— 任何一份文档新增一个超长单元都立刻报红。"""

    def test_a_single_new_violation_fails_loudly(self):
        with fixture(overrides={"CONTEXT.md": "# 标题\n\n" + long_prose() + "\n"}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("CONTEXT.md", result.stdout)
            self.assertIn("基线 0", result.stdout)

    def test_files_are_judged_one_by_one(self):
        # 判据按文件记：一个文件越线，不牵连另一份干净的。
        with fixture(
            overrides={
                "CONTEXT.md": "# 标题\n\n" + long_prose() + "\n",
                "docs/bash.md": "# 标题\n\n这一份没有超长单元。\n",
            }
        ) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("CONTEXT.md", result.stdout)
            self.assertNotIn("docs/bash.md", result.stdout)


class RatioWarnTest(unittest.TestCase):
    """中文占比余量 < 0.5 点 → 一行 `warn:`，退出码不变。"""

    def test_low_margin_warns_without_failing(self):
        # `docs/bash.md` 的下限是 32：100 / 309 = 32.36%，余量 0.36 点。
        text = "# 标题\n\n" + "中" * 100 + "a" * 209 + "\n"
        with fixture(overrides={"docs/bash.md": text}) as root:
            result = run(cwd=root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("warn:", result.stdout)
            self.assertIn("docs/bash.md", result.stdout)


if __name__ == "__main__":
    unittest.main()
