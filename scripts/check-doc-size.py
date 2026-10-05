#!/usr/bin/env python3
"""文档密度的护栏：36 份活文档的「单元 ≤500」、入口三份的体量预算、中文占比余量。

来源是 `.scratch/docs-slim/spec.md`（§1 单元口径、§2 护栏契约）。它守的是**不许恶化**，
不是「瘦身做完了没」——装上就是绿的，存量欠账按文件的**违规计数基线**记着。

四条检查：

① **单元 ≤500**。单元 = 散文段，或一个顶层清单项连同它的续行与嵌套子项；**表格单元格
   单独算一个单元**。>500 才进豁免判定：R1（引文，`>` 行字符 ≥50%）与 R2（纯指针枚举，
   链接跨度 ≥50% 且 `·`/`、` ≥4 个，只对清单项与单元格）**自动放行**；R3/R4/R5 命中只打
   一行 `review:`，**不改退出码**。失败判据是「某文件的违规数 > 该文件的基线」——存量清理
   不被一条红命令堵住，而新增的超长单元立刻报红。每一项违规的位置永远打印。

② **入口三份的体量预算**：非空白字符数 ≤ 上限、行数 ≤ 上限，两张硬编码表。
   「行数不得增」不成立（拆段落必然加行），所以是两条并列的「不得超过」。

③ **中文占比余量**：逐份算 `100 × CJK 数 / 字符数`，相对 `check-language.py` 的
   `DOCS_MIN_RATIO` 下限余量 < 0.5 个百分点时打一行 `warn:`，**不改退出码**。下限只有
   一处事实源（那边），所以这里加载它的字典，不复制。

④ **清单自检**：清单里的文件不存在 → 非零退出；`docs/` 那一层存在但不在清单里的 `.md`
   → 打印一行提示、不非零退出（避免重演 `DOCS_MIN_RATIO` 悄悄漏掉两份 ADR 的事）。

用法（在仓库根跑）：

    python3 scripts/check-doc-size.py            # 判定
    python3 scripts/check-doc-size.py --list     # 打印全部单元与每条规则的当日命中数
"""

from __future__ import annotations

import argparse
import glob
import importlib.util
import os
import re
import sys
from dataclasses import dataclass

# --- 清单：38 份活文档 -------------------------------------------------------
# scope 与 `.scratch/docs-slim/research/03` 一致：入口三份 + `CONTEXT.md`、`docs/` 逐面
# 18 份、`docs/adr/` 12 份、`docs/agents/` 4 份。**不含** `docs/research/`（一手引文）、
# `.scratch/*/issues/`、`.scratch/*/spec.md`、`.scratch/*/research/`。
DOC_FILES = [
    "README.md",
    "CONTEXT.md",
    "AGENTS.md",
    ".scratch/README.md",
    "docs/bash.md",
    "docs/credentials.md",
    "docs/custom-tools.md",
    "docs/discussion.md",
    "docs/executor.md",
    "docs/goals.md",
    "docs/grep.md",
    "docs/highlight.md",
    "docs/lifecycle.md",
    "docs/mcp.md",
    "docs/observability.md",
    "docs/permissions.md",
    "docs/render.md",
    "docs/repo-map.md",
    "docs/sandbox.md",
    "docs/skills.md",
    "docs/tui-manual-checklist.md",
    "docs/web.md",
    "docs/adr/0001-chinese-ui-frozen-model-text.md",
    "docs/adr/0002-fullscreen-alt-screen-tui.md",
    "docs/adr/0003-plan-leaves-the-permission-modes.md",
    "docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md",
    "docs/adr/0005-model-visible-text-in-chinese.md",
    "docs/adr/0006-sandbox-by-bubblewrap.md",
    "docs/adr/0007-workspace-permission-mode.md",
    "docs/adr/0008-markdown-parsing-by-pulldown-cmark.md",
    "docs/adr/0009-goals-are-files-and-progress-is-derived.md",
    "docs/adr/0010-questionnaire-keys-dispatch-by-zone.md",
    "docs/adr/0011-diagrams-in-mermaid.md",
    "docs/adr/0012-input-tokens-are-atomic.md",
    "docs/agents/commits.md",
    "docs/agents/domain.md",
    "docs/agents/issue-tracker.md",
    "docs/agents/triage-labels.md",
]

# 自检扫的那三层 `docs/` 目录（非递归，所以 `docs/research/` 天然在外）。
DOC_DIRS = ["docs", "docs/adr", "docs/agents"]

# --- ① 单元与豁免 ------------------------------------------------------------
LIMIT = 500  # 硬线：单元长度 >500 才进豁免判定（冻结项 5）

FENCE = re.compile(r"^\s*(```|~~~)")
HEADING = re.compile(r"^#{1,6}[ \t]")
TABLE_ROW = re.compile(r"^\s*\|")
LIST_TOP = re.compile(r"^([ \t]*)([-*+]|\d+[.)])[ \t]")
NUMBERED_ITEM = re.compile(r"^\s*\d+[.)]\s")
SEPARATOR_CELL = re.compile(r"^[\s:|-]*$")
LINK = re.compile(r"\[[^\]]*\]\([^)]*\)")
CJK = re.compile(r"[\u4e00-\u9fff]")
RULES = ("R1", "R2", "R3", "R4", "R5")


@dataclass(frozen=True)
class Unit:
    """一个待量的单元：`path:line` 是它的起点，`kind` 决定哪些豁免规则适用。"""

    path: str
    line: int
    kind: str  # prose / item / heading / cell
    text: str

    @property
    def size(self) -> int:
        return len(self.text)

    @property
    def key(self) -> tuple[str, int]:
        return (self.path, self.line)


def cells_of(line: str) -> list[str]:
    """一行表格拆出的单元格文本（首尾的空格与竖线都剥掉）。"""
    body = line.strip()
    if body.startswith("|"):
        body = body[1:]
    if body.endswith("|"):
        body = body[:-1]
    return [cell.strip() for cell in body.split("|")]


def split_block(path: str, start: int, block: list[str]) -> list[Unit]:
    """一个非空块的切分（单元定义的第 ② 步）。

    - 块首就是清单项：每个顶层清单项（连同它的续行与嵌套子项）算一个单元；
    - 块首是散文、块里又出现清单项：**块首到第一个清单项之间的散文另算一个单元**，
      后面的清单项各自算一个；
    - 整块里一个清单项都没有：整块算一个单元。
    """
    head = next((i for i, line in enumerate(block) if LIST_TOP.match(line)), None)
    if head is None:
        return [Unit(path, start + 1, "prose", "\n".join(block))]
    units: list[Unit] = []
    if head > 0:
        units.append(Unit(path, start + 1, "prose", "\n".join(block[:head])))
    items = block[head:]
    indent = len(LIST_TOP.match(items[0]).group(1))
    current: int | None = None

    def flush(end: int) -> None:
        if current is not None:
            units.append(
                Unit(path, start + head + current + 1, "item", "\n".join(items[current:end]))
            )

    for index, line in enumerate(items):
        match = LIST_TOP.match(line)
        if match and len(match.group(1)) <= indent:
            flush(index)
            current = index
    flush(len(items))
    return units


def split_units(path: str, text: str) -> list[Unit]:
    """按「散文段 / 顶层清单项 / 标题 / 表格单元格」切出全部单元。

    切块：空行分隔的连续非空行；遇到表格行、围栏代码块、`#` 标题即断。围栏代码块整体
    跳过（代码不算散文，它自己就是边界）。
    """
    lines = text.split("\n")
    units: list[Unit] = []
    index, total = 0, len(lines)
    while index < total:
        line = lines[index]
        if not line.strip():
            index += 1
            continue
        if FENCE.match(line):
            index += 1
            while index < total and not FENCE.match(lines[index]):
                index += 1
            index += 1
            continue
        if TABLE_ROW.match(line):
            if not (SEPARATOR_CELL.match(line) and "-" in line):
                for cell in cells_of(line):
                    if cell:
                        units.append(Unit(path, index + 1, "cell", cell))
            index += 1
            continue
        if HEADING.match(line):
            units.append(Unit(path, index + 1, "heading", line))
            index += 1
            continue
        start = index
        while (
            index < total
            and lines[index].strip()
            and not FENCE.match(lines[index])
            and not TABLE_ROW.match(lines[index])
            and not HEADING.match(lines[index])
        ):
            index += 1
        units.extend(split_block(path, start, lines[start:index]))
    return units


def quote_exempt(unit: Unit) -> bool:
    """R1 引文：以 `>` 开头的行占单元字符数 ≥50%。"""
    quoted = sum(len(line) for line in unit.text.split("\n") if line.lstrip().startswith(">"))
    return quoted / unit.size >= 0.5


def pointer_exempt(unit: Unit) -> bool:
    """R2 纯指针枚举：链接跨度 ≥50% 且 `·`/`、` ≥4 个（只对清单项与单元格）。"""
    if unit.kind not in ("item", "cell"):
        return False
    span = sum(match.end() - match.start() for match in LINK.finditer(unit.text))
    separators = unit.text.count("·") + unit.text.count("、")
    return span / unit.size >= 0.5 and separators >= 4


def review_rules(unit: Unit) -> list[str]:
    """R3 / R4 / R5：形态对、但自动判据只能当筛子 —— 命中只提示，不放行也不报红。"""
    hits: list[str] = []
    if unit.kind == "prose":
        sentences = [s for s in re.split(r"[。！？]", unit.text) if s.strip()]
        # R3 单一长句：最长句子 ≥0.8×len，且按次级停顿再切也切不出 ≤500 的片。
        if sentences and max(map(len, sentences)) / unit.size >= 0.8:
            clauses = [s for s in re.split(r"[；、]", unit.text) if s.strip()]
            if clauses and max(map(len, clauses)) > LIMIT:
                hits.append("R3")
        # R4 不可断因果链：句子 ≥3 且 ≥60% 以指代 / 连接词开头。
        lead = re.compile(r"^(这|那|它|该|此|因此|所以|于是|结果|前者|后者|代价是|好处是)")
        if len(sentences) >= 3 and sum(1 for s in sentences if lead.match(s.strip())) / len(sentences) >= 0.6:
            hits.append("R4")
    # R5 次序操作序列：单条编号项、≥3 个次序词、且不含 `；`。
    if unit.kind == "item" and NUMBERED_ITEM.match(unit.text):
        markers = ("先", "再", "然后", "接着", "之后", "最后", "直到")
        if sum(unit.text.count(word) for word in markers) >= 3 and "；" not in unit.text:
            hits.append("R5")
    return hits


# --- ② 入口三份的体量预算 -----------------------------------------------------
# 上限**初始值 = 首次运行的实测**，只许降；收紧是一次显式动作，在提交信息里写理由
# （与 `MODEL_TEXT_FLOOR` / `COMMENT_FLOOR` 同风格）。**放宽是同一类显式动作** —— 维护者要它
# 长就跟着长，理由同样写在提交信息里，并在这里记一条带日期的注（见末尾那条 `AGENTS.md`）。
# **一类常规的上调例外**：`.scratch/README.md` 是 feature 索引，它天然随 feature 增长 —— 每加一个
# feature 就在上面那几次的先例里显式上调一次并记日期（`input-tokens` / `trace-tab` /
# `tui-visual-language`），直到撞上 ≤13,500 / ≤100 那个终点。其余两份照旧只许降。
# **2026-10-04 票 10 落地后收紧到实测**：三份都进了票 04 定的终点（`.scratch/docs-slim/issues/10-entry-docs-rewrite.md`：
#   `.scratch/README.md` ≤13,500 字符 / ≤100 行、`README.md` ≤18,500 / ≤300、`AGENTS.md` ≤950 / ≤30）。
# **2026-10-04 票 12 收口时又按实测收紧一次**：把删自 `docs/render.md` 的三处信息
#   （`--tui` 与互斥、`--config`、`--continue` 的查找顺序）并回「跑」一节之后，`README.md`
#   是 16,993 / 292 —— 仍远低于终点，棘轮跟着实测走。
# **2026-10-05 新增 `input-tokens` 索引行时再跟着实测调一次**：`.scratch/README.md`
#   8,203 / 53 → 8,344 / 54 —— 与票 12 同一种「先加信息、再跟着调棘轮」的显式动作。
#   这一档与 `README.md` 那两次不同：那条是**改写**挤出来的空间，这条是 feature 索引
#   天然随 feature 增长，所以它只会往上走，直到撞上 ≤13,500 / ≤100 那个终点。
# **2026-10-05 需求池两条注记时再跟一次**：`context-injection-detail` 落成 `mcp-support`
#   票 19 之后，它的索引行与需求池那段各加了一句注记（同类注记的另一条是 `ask-user-question`，
#   早就在表里），`.scratch/README.md` 8,513 / 54 —— 与 `input-tokens` 那次同一种显式动作。
# **2026-10-05 新增 `trace-tab` 决策图索引行时再跟一次**：wayfinder 的图也是 feature 索引的一行
#   （它的子票住在 `.scratch/trace-tab/issues/`，不占这份入口文档），`.scratch/README.md` 8,724 / 55
#   —— 含票数一路改到「6 resolved + 8 ready-for-agent」（`/to-tickets` 拆出八张实现票之后）。
# **2026-10-06 新增 `tui-visual-language` 索引行时再跟一次**：同一类显式动作，`.scratch/README.md`
#   8,724 / 55 → 9,060 / 56（feature 索引天然随 feature 增长，与 `input-tokens`、`trace-tab` 两次同因）；
#   同日该 feature 九张实现票落地、索引行也跟着改到「10 resolved + 9 ready-for-walkthrough」，
#   9,060 → **9,201 / 56** —— 同一类显式动作，不另起一条注。
# **2026-10-06 新增 `time-mcp` 索引行时再跟一次**：同一类显式动作，`.scratch/README.md`
#   9,201 / 56 → **9,420 / 57**（feature 索引天然随 feature 增长，与 `input-tokens`、`trace-tab`、
#   `tui-visual-language` 三次同因；同日该 feature 三张实现票落地，索引行的票数直接写 `3/3 done`）。
# **2026-10-06 新增 `tui-feedback` 索引行时再跟一次**：同一类显式动作，`.scratch/README.md`
#   9,420 / 57 → **9,630 / 58**（与上一条同因；六张实现票同日全部落地，索引行直接写 `6/6 done`）。
#   同日该目录又收两条走查反馈（拖选的手感、`Shift+Enter`），票数与索引行的描述一起改：
#   9,630 → **9,677 / 58** —— 同一类显式动作，行数没动，不另起一条注。
# 行数比票 04 的「现在」高是**预期**的：拆段落必然加行，票 04 承认「行数不得增」不成立，
# 行数上限是「拆完之后的新上限」，此后拦住「再往入口文档追加」。
# **2026-10-06 维护者放宽 `AGENTS.md`（第一份被放宽的入口文档）**：它当时 904 / 25，贴着
#   905 / 26 的棘轮，维护者的原话是「想加点啥有点费劲」—— 追加一条约定的代价因此高到要把别处
#   的句子削掉（`docs/agents/commits.md` 那一轮就是这么落成的）。新上限 **5,000 / 140**、终点
#   **5,500 / 150**：按现有密度（约 36 字符/行）折算是六倍体量。放宽与收紧同一条规矩 ——
#   显式动作、理由写进提交信息，决定也记进 `.scratch/docs-slim/issues/05` 的 `## 评论`。
#   其余两份照旧只许降。
ENTRY_BUDGET = {
    "README.md": {"chars": 16993, "lines": 292, "target_chars": 18500, "target_lines": 300},
    ".scratch/README.md": {"chars": 9677, "lines": 58, "target_chars": 13500, "target_lines": 100},
    "AGENTS.md": {"chars": 5000, "lines": 140, "target_chars": 5500, "target_lines": 150},
}
# 口径：字符数 = 剥掉全部空白后的 `len`；行数 = `text.count("\n") + 1`（与票面的实测同口径，
# 比 `wc -l` 多半行，因为末行没有换行符也算一行）。

# --- 违规计数基线（按文件） ---------------------------------------------------
# 初值 = 2026-10-04 首次运行的实测（25 项、口径差 1–3 项，以脚本输出为准）：存量欠账记在这里，
# `> 基线` 才报红。清完一份就把该文件的数字降到新实测值 —— 一次**显式动作**，在提交信息里
# 写明理由（与 `MODEL_TEXT_FLOOR` / `COMMENT_FLOOR` 同风格）。
# **2026-10-04 本轮收口后 36 份全为 0**（票 09 / 10 / 11 / 12 各清各的），字典随之清空 ——
# 从今往后任何一份文档新增一个 >500 的单元都会立刻报红，不再有存量欠账兜着。
VIOLATION_BASELINE: dict[str, int] = {}

# 各份文档的占比下限从 `check-language.py` 那一处事实源取（不复制字典）。
RATIO_WARN_MARGIN = 0.5  # 余量 < 0.5 个百分点 → 打一行 warn:，不改退出码


def ratio_floors() -> dict[str, int]:
    """`check-language.py` 的 `DOCS_MIN_RATIO`（唯一的占比下限事实源）。"""
    here = os.path.dirname(os.path.abspath(__file__))
    spec = importlib.util.spec_from_file_location(
        "check_language_floors", os.path.join(here, "check-language.py")
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules["check_language_floors"] = module
    spec.loader.exec_module(module)
    return module.DOCS_MIN_RATIO


@dataclass
class Report:
    """一次扫描的全部产物。`problems` 非空即非零退出，`notes` 与 `warnings` 只打印。"""

    problems: list[str]
    notes: list[str]
    warnings: list[str]
    units: list[Unit]
    violations: dict[str, list[Unit]]
    rule_hits: dict[str, list[Unit]]


def scan(root: str) -> Report:
    problems: list[str] = []
    notes: list[str] = []
    warnings: list[str] = []
    units: list[Unit] = []
    violations: dict[str, list[Unit]] = {}
    rule_hits: dict[str, list[Unit]] = {name: [] for name in RULES}

    # ④ 清单自检：缺文件非零退出；多出来的 `.md` 只提示。
    for path in DOC_FILES:
        if not os.path.exists(os.path.join(root, path)):
            problems.append(f"{path}: 清单里的文档不存在（清单该更新了）")
    listed = set(DOC_FILES)
    for directory in DOC_DIRS:
        for found in sorted(glob.glob(os.path.join(root, directory, "*.md"))):
            rel = os.path.relpath(found, root).replace(os.sep, "/")
            if rel not in listed:
                warnings.append(f"{rel}: `docs/` 下有清单外的 md（要护栏管它就加进 DOC_FILES）")
    if problems:
        return Report(problems, notes, warnings, units, violations, rule_hits)

    # ① 单元与豁免；③ 占比余量。
    floors = ratio_floors()
    for path in DOC_FILES:
        with open(os.path.join(root, path), encoding="utf-8") as handle:
            text = handle.read()
        for unit in split_units(path, text):
            units.append(unit)
            if unit.size <= LIMIT:
                continue
            if quote_exempt(unit):
                rule_hits["R1"].append(unit)
                continue
            if pointer_exempt(unit):
                rule_hits["R2"].append(unit)
                continue
            hits = review_rules(unit)
            if hits:
                for name in hits:
                    rule_hits[name].append(unit)
                notes.append(
                    f"review: {path}:{unit.line}: 单元 {unit.size} 字符命中"
                    f" {'、'.join(hits)}（形态对，交人定，不改退出码）"
                )
                continue
            violations.setdefault(path, []).append(unit)
        if path in floors:
            ratio = 100 * len(CJK.findall(text)) / max(len(text), 1)
            margin = ratio - floors[path]
            if margin < RATIO_WARN_MARGIN:
                warnings.append(
                    f"{path}: 中文占比余量 {margin:.2f} 点"
                    f"（{ratio:.2f}% / 下限 {floors[path]}%）—— 删中文散文前先看这里"
                )

    # ② 入口三份的体量预算。
    for path, budget in ENTRY_BUDGET.items():
        full = os.path.join(root, path)
        if not os.path.exists(full):
            continue
        with open(full, encoding="utf-8") as handle:
            text = handle.read()
        chars = len(re.sub(r"\s", "", text))
        lines = text.count("\n") + 1
        if chars > budget["chars"]:
            problems.append(
                f"{path}: 非空白字符数 {chars} > 上限 {budget['chars']}"
                f"（终点 ≤{budget['target_chars']}，见票 10）"
            )
        if lines > budget["lines"]:
            problems.append(
                f"{path}: 行数 {lines} > 上限 {budget['lines']}"
                f"（终点 ≤{budget['target_lines']}，见票 10）"
            )

    # 违规计数基线：每一项违规的位置永远打印，失败只看「某文件的违规数 > 该文件的基线」。
    for path, items in sorted(violations.items()):
        for unit in items:
            notes.append(f"{path}:{unit.line}: 单元 {unit.size} 字符（{unit.kind}）> {LIMIT}")
        baseline = VIOLATION_BASELINE.get(path, 0)
        if len(items) > baseline:
            problems.append(f"{path}: 违规 {len(items)} 项 > 基线 {baseline}（不许恶化）")

    return Report(problems, notes, warnings, units, violations, rule_hits)


def print_list(report: Report) -> None:
    """`--list`：全部单元按长度排序，外加每条规则的当日命中数与位置。"""
    hit_of: dict[tuple[str, int], list[str]] = {}
    for name in RULES:
        for unit in report.rule_hits[name]:
            hit_of.setdefault(unit.key, []).append(name)
    print(f"全部单元 {len(report.units)} 个，按长度排序（硬线 {LIMIT}）：")
    for unit in sorted(report.units, key=lambda u: -u.size):
        marks = hit_of.get(unit.key, [])
        if not marks and unit.size > LIMIT:
            marks = ["违规"]
        label = "、".join(marks) if marks else ("合格" if unit.size <= LIMIT else "")
        print(f"  {unit.size:6d}  {unit.path}:{unit.line}  {unit.kind}  {label}")
    print("\n每条豁免 / 复核规则今日的命中数与位置：")
    for name in RULES:
        items = report.rule_hits[name]
        print(f"  {name}: {len(items)} 项")
        for unit in items:
            print(f"      {unit.path}:{unit.line}  {unit.size} 字符")


def main() -> int:
    parser = argparse.ArgumentParser(description="36 份活文档的密度护栏（单元 ≤500 / 入口预算 / 占比余量）")
    parser.add_argument("--list", action="store_true", help="打印全部单元与每条规则的当日命中数")
    args = parser.parse_args()

    # 相对路径一律相对**当前目录**（仓库根）；`--list` 也不例外，少一个旗标就少一条契约外的口子。
    report = scan(".")
    for warning in report.warnings:
        print(f"warn: {warning}")
    if not args.list:
        for note in report.notes:
            print(f"note: {note}")
    if report.problems:
        print("check-doc-size: 不通过\n")
        for problem in report.problems:
            print(f"  - {problem}")
        if args.list:
            print_list(report)
        else:
            print(
                "\n单元 = 散文段 / 顶层清单项（含续行与嵌套子项）/ 表格单元格；>500 才进豁免判定，"
                "R1（引文）与 R2（纯指针枚举）自动放行，R3–R5 只提示。"
            )
        return 1
    if args.list:
        print_list(report)
        return 0
    print(
        f"check-doc-size: OK（{len(DOC_FILES)} 份文档、单元 ≤{LIMIT}，入口三份在预算内；"
        f"豁免 R1 {len(report.rule_hits['R1'])} / R2 {len(report.rule_hits['R2'])} 项）"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BrokenPipeError:
        # `--list | head` 这类下游提前关掉管道不是错误：把 stdout 指到空设备，
        # 免得退出时刷新再抛一次。
        os.dup2(os.open(os.devnull, os.O_WRONLY), sys.stdout.fileno())
        sys.exit(0)
