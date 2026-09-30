#!/usr/bin/env python3
"""ADR 0004 与 ADR 0005 的护栏：散文一律中文，英文只留给**不是散文**的东西
（标识符、schema 值与协议标记、路径与命令、`docs/research/` 的一手引文）。

四条检查，任一条不过就以非零码退出：

① **模型可见 / 进流那一侧的两条棘轮**（ADR 0005 起）。`FROZEN_FILES` 就是这一侧的
   清单 —— 工具声明与描述、工具结果与错误、`AgentError.message`、`SessionError.detail`
   这些既进 `messages`、又进转录与详情弹窗的散文所在处。方向相反的两条棘轮：
   **中文串数只许上升**（`MODEL_TEXT_FLOOR`：守「翻过的地方不许被改回英文」），
   **英文散文串数只许下降**（`ENGLISH_PROSE_CEILING`：守「不许再往这一侧新增英文散文」）。
   迁移期间两条都按实测值收紧，收尾时提到实测值。测试模块（文件末尾的 `#[cfg(test)]`
   之后）不在这一侧内：那是测试数据，不是模型可见文本。

② **`docs/**/*.md` 与 `AGENTS.md` 的中文占比下限**：设计文档是散文，读者是人。
   `docs/research/` 是上游文档的引文，`docs/highlight.md` 本来就是中文（ADR 0001 的
   例外），两者不查。`docs/adr/*.md` 也在内 —— ADR 是散文。`AGENTS.md` 虽然是
   模型可见的注入文本，正文照样用中文（ADR 0004 的「后加」一节），只有它那五个
   小标题是技能工具链的锚点、留英文。

③ **ADR 的标题与小标题必须是中文**：ADR 的标题也是散文（ADR 0004 的「后加」一节），
   所以 `## Consequences` 那类英文小标题报红。整条都是行内代码的标题（如 `# `bash``）
   剥完是空的，放过；ADR 的**文件名**仍是标识符，留英文。

④ **`src/` 与 `tests/` 注释的中文行数下限**：棘轮，只许上升 —— 防止翻过的地方
   被改回英文。

用法：`python3 scripts/check-language.py`（在仓库根目录跑）。加 `--list` 会打印
① 的两条棘轮现在盯的文件与数字，用来核对清单本身。
"""

from __future__ import annotations

import glob
import os
import re
import sys

CJK = re.compile(r"[\u4e00-\u9fff]")
# 断言/panic 消息所在的行：那些字面量是散文，不在冻结面内。
ASSERTION = re.compile(r"\b(assert|assert_eq|assert_ne|debug_assert|panic|expect)\b")

# --- ① 模型可见 / 进流那一侧的两条棘轮（ADR 0005） -------------------------
# 模型可见（工具声明与描述 / 工具结果 / 身份与轮前缀 / 投影）与进流（reason / detail /
# summary / 协议标记）的散文所在处。这一侧按 ADR 0005 走中文，不再按 ADR 0001 冻在英文。
FROZEN_FILES = [
    "src/tools/ask_user.rs",
    "src/tools/bash.rs",
    "src/tools/custom.rs",
    "src/tools/edit.rs",
    "src/tools/file.rs",
    "src/tools/mod.rs",
    "src/tools/paths.rs",
    "src/tools/process.rs",
    "src/tools/registry.rs",
    "src/tools/repo_map.rs",
    "src/tools/skill.rs",
    "src/tools/task.rs",
    "src/tools/todo.rs",
    "src/tools/tool.rs",
    "src/permissions.rs",
    "src/discussion.rs",
    "src/discussion/protocol.rs",
    "src/agent.rs",
    "src/agent/executor.rs",
    "src/provider/mod.rs",
    "src/provider/projection.rs",
    "src/context.rs",
    "src/context/skills.rs",
    "src/context/repo_map.rs",
    "src/events.rs",
    "src/questions.rs",
    "src/hooks.rs",
    # 混住文件（ADR 0004 说的「混住」那一类）：模型可见 / 进流的串与给人看的串住在同一份
    # 文件里。整份文件不会被算成「这一侧」（那会把给人看的中文与该留英文的旗标一起算进
    # 来），但**这几份里确实有这一侧的散文**，所以照样逐条字面量盯住它们。
    "src/agent/history.rs",
    "src/render/input.rs",
    "src/render/tui.rs",
    "src/cli.rs",
]

# 两条方向相反的棘轮（ADR 0005），数字都是**实测值**：
#
# - **中文串数只许上升**：迁移期每翻完一批就往上涨，收口时提到实测值。确实要删代码、
#   连带删掉中文串时，往下调是一次**显式动作** —— 在提交信息里写明理由，别让它悄悄漂。
# - **英文散文串数只许下降**：判据是「≥3 个英文词、且含空格」的字面量，它守的是
#   「不许再往这一侧新增英文散文」。断言所在那一行的字面量不计（测试的断言消息是散文，
#   但它属于测试，不属于模型可见文本 —— ADR 0004）；只剩一两个词的标识符、路径、
#   schema 值也被上面那条判据天然排除。
#
# 进度（ADR 0005 的第 ②–⑤ 批落地后，2026-09-30）：中文 **219**、英文散文 **27**。
# 剩下的 27 条**一条散文都没有**，全是判据的假阳性，逐条记在 ADR 0005 的「进度与收口」
# 一节里：`fs-agent: {message}` 这类程序名前缀（25 条）与两条纯 `format!` 骨架
# （`→ {tool_name}({rendered})`、`{text}{separator}{display}: {}`）。再往下收就要动
# CLI 输出那 25 处的标点（ASCII 冒号换全角），那是另一件事，不在这一步里。
#
# 两条棘轮都是**实测值**，不留余量：任何一条串被改回英文（或新写一条英文散文）都会报红。
# 每翻完一批就把下限提到新的实测值、把上限收到新的实测值。

MODEL_TEXT_FLOOR = {
    "src/tools/ask_user.rs": 16,
    "src/tools/bash.rs": 6,
    "src/tools/custom.rs": 1,
    "src/tools/edit.rs": 7,
    "src/tools/file.rs": 26,
    "src/tools/paths.rs": 3,
    "src/tools/process.rs": 14,
    "src/tools/registry.rs": 4,
    "src/tools/repo_map.rs": 3,
    "src/tools/skill.rs": 3,
    "src/tools/task.rs": 4,
    "src/tools/todo.rs": 14,
    "src/permissions.rs": 20,
    "src/discussion.rs": 8,
    "src/agent.rs": 38,
    "src/agent/executor.rs": 5,
    "src/provider/mod.rs": 6,
    "src/provider/projection.rs": 1,
    "src/context.rs": 6,
    "src/context/skills.rs": 5,
    "src/context/repo_map.rs": 2,
    "src/events.rs": 1,
    "src/hooks.rs": 1,
    "src/agent/history.rs": 7,
    "src/render/input.rs": 3,
    "src/render/tui.rs": 2,
    "src/cli.rs": 13,
}
ENGLISH_PROSE_CEILING = 27

# --- ③ docs 的中文占比下限（百分数） ----------------------------------------
# 翻译完成后按**实测值减 2 个百分点**逐份收紧（2026-09-27 量：最低 31.2% 是
# `docs/agents/domain.md`，最高 45.5% 是 `docs/observability.md`；`docs/adr/*.md`
# 是 2026-09-30 ADR 中文化那一轮加进来的，实测 42.6–46.4%；`AGENTS.md` 同一天翻的，
# 实测 25.8% —— 它字节少、标识符密，比例天然低）。留 2 点余量是因为文档里必然有英文
# 标识符、代码路径、引用与命令，插一段代码块就会拉低比例；但再往下掉 —— 也就是有人把
# 整段散文翻回英文 —— 必须报红。
DOCS_MIN_RATIO = {
    "AGENTS.md": 23,
    "docs/adr/0001-chinese-ui-frozen-model-text.md": 44,
    "docs/adr/0002-fullscreen-alt-screen-tui.md": 40,
    "docs/adr/0003-plan-leaves-the-permission-modes.md": 41,
    "docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md": 43,
    "docs/adr/0005-model-visible-text-in-chinese.md": 36,
    "docs/bash.md": 32,
    "docs/credentials.md": 39,
    "docs/custom-tools.md": 30,
    "docs/discussion.md": 42,
    "docs/executor.md": 38,
    "docs/observability.md": 43,
    "docs/render.md": 42,
    "docs/repo-map.md": 43,
    "docs/skills.md": 41,
    "docs/agents/domain.md": 29,
    "docs/agents/issue-tracker.md": 29,
    "docs/agents/triage-labels.md": 32,
}

# --- ⑤ 注释中文行的棘轮 -----------------------------------------------------
# 数字是「已翻成中文的注释行数」的下限，**只许上升**：它守的是「翻过的地方不许被改回
# 英文」。2026-09-27 迁移收尾时提到实测值（`src` 5,209 / `tests` 2,423）。确实要删代码、
# 连带删掉中文注释行时，往下调这个数字是一次**显式动作** —— 请在提交信息里写明理由，
# 别让它悄悄跟着漂。
COMMENT_FLOOR = {
    "src": 5209,
    "tests": 2423,
}


# --- ④ ADR 的标题与小标题必须是中文 -----------------------------------------
# ADR 的标题也是散文（ADR 0004 的「后加」一节），所以下一个 ADR 不许再写出
# `## Consequences`。检查前先挖掉代码块与行内代码：`# `bash`` 那种标题剥完是空的，
# 放过；ADR 的文件名仍是标识符，留英文。
ADR_DIR = "docs/adr"
HEADING = re.compile(r"^#{1,6}[ \t]+(.+?)[ \t]*$", re.M)
FENCE = re.compile(r"^```.*?^```", re.S | re.M)


def check_adr_headings() -> list[str]:
    """每份 ADR 的每个标题，剥掉代码后都要含中文。"""
    problems = []
    paths = sorted(glob.glob(os.path.join(ADR_DIR, "*.md")))
    if not paths:
        problems.append(f"{ADR_DIR}/: 一份 ADR 都没有（清单该更新了）")
    for path in paths:
        text = FENCE.sub("", open(path, encoding="utf-8").read())
        for heading in HEADING.findall(text):
            body = re.sub(r"`[^`]*`", " ", heading)  # 行内代码不算散文
            body = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", body)  # 链接留文字
            if body.strip() and not CJK.search(body):
                problems.append(
                    f"{path}: 标题里一个中文字都没有：{heading.strip()!r}"
                    "（ADR 的标题与小标题也是散文，见 ADR 0004）"
                )
    return problems


def literals(src: str):
    """真正的字符串字面量（处理转义、raw string、字符字面量、注释）。

    产出 `(行号, 内容, 该行源码文本)`：第三项只用来判断这个字面量是不是断言消息。
    """
    out, i, n, line = [], 0, len(src), 1
    line_start = 0
    while i < n:
        ch = src[i]
        if ch == "\n":
            line += 1
            line_start = i + 1
            i += 1
            continue
        if ch == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            i = n if j < 0 else j
            continue
        if ch == "/" and i + 1 < n and src[i + 1] == "*":
            j = src.find("*/", i + 2)
            line += src.count("\n", i, j)
            i = n if j < 0 else j + 2
            continue
        if ch == "'" and i + 2 < n:
            # 字符字面量（`'x'`、`'\n'`）：跳过去。生命周期写作 `'a`（没有收尾的
            # 单引号），所以只在两个引号之间没有换行、且长度 ≤ 4 时才算。
            j = i + 3 if src[i + 1] == "\\" else i + 2
            if j < n and src[j] == "'" and "\n" not in src[i:j]:
                i = j + 1
                continue
        if ch == "r" and i + 1 < n and src[i + 1] in '#"':
            k, hashes = i + 1, 0
            while k < n and src[k] == "#":
                hashes += 1
                k += 1
            if k < n and src[k] == '"':
                end = '"' + "#" * hashes
                j = src.find(end, k + 1)
                body = src[k + 1: j] if j > 0 else ""
                if body:
                    out.append((line, body, src[line_start:src.find(chr(10), line_start) if src.find(chr(10), line_start) > 0 else n]))
                line += src.count("\n", i, (j + len(end)) if j > 0 else n)
                i = n if j < 0 else j + len(end)
                continue
        if ch == '"':
            j, buf = i + 1, []
            while j < n:
                if src[j] == "\\":
                    buf.append("\\" + (src[j + 1] if j + 1 < n else ""))
                    j += 2
                    continue
                if src[j] == '"':
                    break
                buf.append(src[j])
                j += 1
            body = "".join(buf)
            if body:
                out.append((line, body, src[line_start:src.find(chr(10), line_start) if src.find(chr(10), line_start) > 0 else n]))
            line += src.count("\n", i, j)
            i = j + 1
            continue
        i += 1
    return out


def code_only(src: str) -> str:
    """文件里 `#[cfg(test)]` 之前的部分（测试模块不在冻结面内）。"""
    at = src.find("#[cfg(test)]")
    return src if at < 0 else src[:at]


def count_model_text() -> tuple[dict[str, int], list[tuple[str, int, str]]]:
    """这一侧的两样东西：每个文件的**中文串数**，以及**英文散文串**的清单。"""
    chinese: dict[str, int] = {}
    english: list[tuple[str, int, str]] = []
    for path in FROZEN_FILES:
        if not os.path.exists(path):
            continue
        src = code_only(open(path, encoding="utf-8").read())
        n = 0
        for line, body, line_text in literals(src):
            if ASSERTION.search(line_text):
                continue  # 断言消息属于测试，不属于模型可见文本（ADR 0004）
            if CJK.search(body):
                n += 1
                continue
            flat = body.replace("\\n", " ").strip()
            if len(re.findall(r"[A-Za-z]{2,}", flat)) >= 3 and " " in flat:
                english.append((path, line, flat[:90]))
        chinese[path] = n
    return chinese, english


def check_model_text(list_all: bool) -> list[str]:
    """① 两条棘轮：中文串只许上升、英文散文串只许下降（ADR 0005）。"""
    problems = []
    for path in FROZEN_FILES:
        if not os.path.exists(path):
            problems.append(f"{path}: 清单里的文件不存在（清单该更新了）")
    if problems:
        return problems
    chinese, english = count_model_text()
    if list_all:
        print("这一侧的中文串数（下限 = 棘轮）：")
        for path in FROZEN_FILES:
            print(f"  {chinese[path]:3d}  {path}   （下限 {MODEL_TEXT_FLOOR.get(path, 0)}）")
        print(f"\n这一侧的英文散文串：{len(english)} 条（上限 {ENGLISH_PROSE_CEILING}）")
        for path, line, body in english:
            print(f"  {path}:{line}: {body}")
        return []
    for path, floor in sorted(MODEL_TEXT_FLOOR.items()):
        if chinese.get(path, 0) < floor:
            problems.append(
                f"{path}: 中文串 {chinese.get(path, 0)} 条 < 下限 {floor}"
                "（这一侧的散文按 ADR 0005 走中文，翻过的地方不许被改回英文）"
            )
    if len(english) > ENGLISH_PROSE_CEILING:
        problems.append(
            f"模型可见 / 进流那一侧的英文散文 {len(english)} 条 > 上限 "
            f"{ENGLISH_PROSE_CEILING}（不许再往这一侧新增英文散文，见 ADR 0005）"
        )
    return problems


def check_docs() -> list[str]:
    problems = []
    for path, floor in sorted(DOCS_MIN_RATIO.items()):
        if not os.path.exists(path):
            problems.append(f"{path}: 清单里的文档不存在（清单该更新了）")
            continue
        text = open(path, encoding="utf-8").read()
        ratio = 100 * len(CJK.findall(text)) / max(len(text), 1)
        if ratio < floor:
            problems.append(f"{path}: 中文占比 {ratio:.1f}% < 下限 {floor}%（散文该是中文）")
    return problems


def check_comments() -> list[str]:
    problems = []
    for root, floor in COMMENT_FLOOR.items():
        chinese = 0
        for dirpath, _, files in os.walk(root):
            for name in files:
                if not name.endswith(".rs"):
                    continue
                for line in open(os.path.join(dirpath, name), encoding="utf-8"):
                    stripped = line.strip()
                    if stripped.startswith("//") and CJK.search(stripped):
                        chinese += 1
        if chinese < floor:
            problems.append(f"{root}/: 中文注释行数 {chinese} < 下限 {floor}（只许上升）")
    return problems


def main() -> int:
    list_all = "--list" in sys.argv[1:]
    if list_all:
        return 0 if not check_model_text(True) else 1

    problems = (
        check_model_text(False)
        + check_docs()
        + check_adr_headings()
        + check_comments()
    )
    if problems:
        print("check-language: 不通过\n")
        for problem in problems:
            print(f"  - {problem}")
        print(
            "\n这四条来自 ADR 0004 与 ADR 0005：散文（注释 / docs / ADR / 断言消息 / 给人看的"
            "错误 / 模型可见与进流的文本）用中文，只有标识符、schema 值与协议标记、路径与命令"
            "留英文。"
        )
        return 1
    print(
        "check-language: OK（模型可见 / 进流那一侧的两条棘轮未回退、docs 与 ADR 是中文散文、"
        "注释中文行数未回退）"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
