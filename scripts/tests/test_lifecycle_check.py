"""`scripts/lifecycle-check.py` 的测试。

测的是**脚本的 CLI**：断言只打在「退出码」与「stdout 里有没有那一项」上，不打内部函数 ——
内部实现可以随意重构而不动这里。fixture 全部写在临时目录里，不依赖真实仓库文件
（`docs/lifecycle.md` 自己的那份由票 14 的人工清单核）。

一票一条用例对应一条校验项，逐条加进来的顺序见 `.scratch/lifecycle-diagram/issues/08`。
"""

import os
import subprocess
import sys
import tempfile
import textwrap
import unittest
from contextlib import contextmanager

SCRIPT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "lifecycle-check.py"
)

# 一份最小但完整的 fixture 文档：两个节点、两条边，表与图一致，引用的符号真实存在。
CLEAN_DOC = textwrap.dedent(
    """\
    # 测试里的生命周期文档

    ## §2 鸟瞰

    ```mermaid
    flowchart TD
        boot[启动]
        load{加载配置}
        run[进入循环]

        boot --> load
        load -->|成功| run
    ```

    ## 附录：逐图证据表

    ### 图 1 的节点

    | 节点 id | 一句话 | 证据 | 符号 |
    | --- | --- | --- | --- |
    | `boot` | 启动 | `src/x.rs:1` | `fn boot` |
    | `load` | 加载配置 | `src/x.rs:3` | `fn load` |
    | `run` | 进循环 | `src/x.rs:5` | `fn run` |

    ### 图 1 的边

    | 边 | 证据 | 说明 |
    | --- | --- | --- |
    | `boot --> load` | `src/x.rs:1` | 启动后加载 |
    | `load -->\\|成功\\| run` | `src/x.rs:3` | 成功才进循环 |
    """
)

# fixture 的 `src/x.rs`：三个符号分别在第 1、3、5 行。
CLEAN_RS = "fn boot() {}\n\nfn load() {}\n\nfn run() {}\n"

CLEAN_README = "# 索引\n\n- [`docs/lifecycle.md`](docs/lifecycle.md) 生命周期\n"


@contextmanager
def fixture(doc=CLEAN_DOC, rs=CLEAN_RS, readme=CLEAN_README):
    """在临时目录里搭一个「仓库根」：`docs/lifecycle.md` + `README.md` + `src/x.rs`。"""
    with tempfile.TemporaryDirectory() as root:
        os.makedirs(os.path.join(root, "docs"))
        os.makedirs(os.path.join(root, "src"))
        if doc is not None:
            with open(os.path.join(root, "docs", "lifecycle.md"), "w", encoding="utf-8") as fh:
                fh.write(doc)
        if rs is not None:
            with open(os.path.join(root, "src", "x.rs"), "w", encoding="utf-8") as fh:
                fh.write(rs)
        if readme is not None:
            with open(os.path.join(root, "README.md"), "w", encoding="utf-8") as fh:
                fh.write(readme)
        yield root


def run(*args, cwd=None):
    return subprocess.run(
        [sys.executable, SCRIPT, *args], capture_output=True, text=True, cwd=cwd
    )


def doc_path(root):
    return os.path.join(root, "docs", "lifecycle.md")


def replace_once(text, old, new):
    assert old in text, f"fixture 里找不到要替换的片段：{old!r}"
    return text.replace(old, new, 1)


def doc_with_mermaid_line(snippet):
    """在 fixture 的图里插一行（用来放一个不该出现的构造）。"""
    return replace_once(CLEAN_DOC, "    boot[启动]\n", f"    boot[启动]\n    {snippet}\n")


class CleanDocumentTest(unittest.TestCase):
    def test_clean_document_passes(self):
        with fixture() as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("PASS", result.stdout)


class DialectTest(unittest.TestCase):
    """C8：不认识的构造一律报红，**不静默跳过**。

    这一组是整个套件里最重要的：解析器漏掉一个节点会伪装成「图里有、表里没有」的假差异，
    护栏自己烂掉而没人知道，比没有护栏更糟。
    """

    def test_unknown_constructs_fail_loudly(self):
        cases = {
            "subgraph": "subgraph turn[一轮]",
            "ampersand": "boot & load --> run",
            "chained edge": "boot --> load --> run",
            "classDef": "classDef foo fill:#f00",
        }
        for name, snippet in cases.items():
            with self.subTest(name=name):
                with fixture(doc=doc_with_mermaid_line(snippet)) as root:
                    result = run(doc_path(root))
                    self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                    self.assertIn("不认识的", result.stdout)
                    self.assertNotIn("PASS", result.stdout)


class EvidenceTest(unittest.TestCase):
    """C1 / C2：证据仍指得到文件与符号；C9 的行号漂移只算提示。"""

    def test_missing_file_fails(self):
        doc = replace_once(CLEAN_DOC, "`src/x.rs:1` | `fn boot`", "`src/nope.rs:1` | `fn boot`")
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("文件不存在", result.stdout)

    def test_reference_in_prose_is_checked_too(self):
        # C1 扫的是**全文**的 `路径:行号`：§7 与各节正文里引的也算。
        doc = replace_once(
            CLEAN_DOC,
            "## §2 鸟瞰",
            "## §2 鸟瞰\n\n正文里引一句：见 `src/missing.rs:3`。\n",
        )
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("src/missing.rs", result.stdout)

    def test_line_beyond_file_end_fails(self):
        doc = replace_once(CLEAN_DOC, "`src/x.rs:5` | `fn run`", "`src/x.rs:99` | `fn run`")
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("超出文件行数", result.stdout)

    def test_renamed_symbol_fails(self):
        doc = replace_once(CLEAN_DOC, "`src/x.rs:1` | `fn boot`", "`src/x.rs:1` | `fn booted`")
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("不在证据点名的文件里", result.stdout)

    def test_line_drift_is_a_hint_not_a_failure(self):
        # 符号 `fn boot` 在第 1 行，证据却写第 3 行（那是 `fn load` 的位置）。
        doc = replace_once(CLEAN_DOC, "`src/x.rs:1` | `fn boot`", "`src/x.rs:3` | `fn boot`")
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("提示", result.stdout)
            self.assertIn("现在在 1 行", result.stdout)

    def test_node_without_stable_symbol_is_allowed(self):
        # 取证表里有些节点本来就没有稳定符号（`refuse`、`env`、`rt`、`kind`），符号列写 `—`。
        doc = replace_once(
            CLEAN_DOC, "| `run` | 进循环 | `src/x.rs:5` | `fn run` |", "| `run` | 进循环 | `src/x.rs:5` | — |"
        )
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("PASS", result.stdout)

    def test_strict_turns_drift_into_failure(self):
        doc = replace_once(CLEAN_DOC, "`src/x.rs:1` | `fn boot`", "`src/x.rs:3` | `fn boot`")
        with fixture(doc=doc) as root:
            result = run("--strict", doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("行号漂移", result.stdout)


# 两张图的 fixture：图 2 的边指向 `boot`，而 `boot` 只在图 1 里定义过。
TWO_GRAPHS_DOC = replace_once(
    CLEAN_DOC,
    "## 附录：逐图证据表",
    """## §3 第二张图

```mermaid
flowchart TD
    other[另一个]

    other --> boot
```

## 附录：逐图证据表""",
) + """
### 图 2 的节点

| 节点 id | 一句话 | 证据 | 符号 |
| --- | --- | --- | --- |
| `other` | 另一个 | `src/x.rs:1` | `fn boot` |
"""


class CrossCheckTest(unittest.TestCase):
    """C4 / C5（集合相等，按图分别比）、C6（id 唯一）、C7（边端点已显式定义）。"""

    def test_graph_node_without_table_row_fails(self):
        doc = replace_once(CLEAN_DOC, "    run[进入循环]\n", "    run[进入循环]\n    extra[多出来的]\n")
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("图 1 里有、证据表没有的节点：`extra`", result.stdout)

    def test_table_row_without_graph_node_fails(self):
        doc = replace_once(
            CLEAN_DOC,
            "| `run` | 进循环 | `src/x.rs:5` | `fn run` |",
            "| `run` | 进循环 | `src/x.rs:5` | `fn run` |\n"
            "| `extra` | 多出来的 | `src/x.rs:5` | `fn run` |",
        )
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("图 1 证据表有、图里没有的节点：`extra`", result.stdout)

    def test_duplicate_node_id_fails(self):
        doc = replace_once(CLEAN_DOC, "    run[进入循环]\n", "    run[进入循环]\n    boot[又一个启动]\n")
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("重复定义", result.stdout)

    def test_edge_endpoint_must_be_defined_in_the_same_graph(self):
        doc = replace_once(CLEAN_DOC, "    load -->|成功| run", "    load -->|成功| nowhere")
        with fixture(doc=doc) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("没有显式定义", result.stdout)

    def test_endpoint_defined_in_another_graph_still_fails(self):
        with fixture(doc=TWO_GRAPHS_DOC) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("图 2", result.stdout)
            self.assertIn("`boot` 没有显式定义", result.stdout)


class ReferenceTest(unittest.TestCase):
    """C3：文档仍被 README.md 引用。"""

    def test_readme_without_link_fails(self):
        with fixture(readme="# 索引\n\n这里没有指向文档的链接。\n") as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("孤儿", result.stdout)


class CommandLineTest(unittest.TestCase):
    """`--list`、用法错、文档不存在。"""

    def test_list_prints_parsed_nodes(self):
        with fixture() as root:
            result = run("--list", doc_path(root))
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("nodes:", result.stdout)
            self.assertIn("boot", result.stdout)

    def test_extra_argument_is_a_usage_error(self):
        with fixture() as root:
            result = run(doc_path(root), "another.md")
            self.assertEqual(result.returncode, 2, result.stdout + result.stderr)

    def test_missing_document_fails(self):
        with fixture(doc=None) as root:
            result = run(doc_path(root))
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn("文档不存在", result.stdout)


if __name__ == "__main__":
    unittest.main()
