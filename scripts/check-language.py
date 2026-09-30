#!/usr/bin/env python3
"""ADR 0004 的护栏：散文用中文，标识符与「进 messages / 进流」的文本留英文。

五条检查，任一条不过就以非零码退出：

① **冻结面**：模型可见或要永久回放的字符串字面量里不得出现中文。哪些文件属于
   「冻结面」、以及**今天已经存在**的几条中文（三条身份提示、讨论轮前缀、上游供应商
   错误里的 `欠费` / `余额` / `额度` 匹配词）逐条列在下面的表里 —— 表是白名单，不是
   省略号：出现表外的中文串就是有人把进入 `messages` 或进入事件流的文本中文化了，
   那是 ADR 0001 与 ADR 0004 都禁止的事（缓存前缀 + 老流永久混排）。
   测试模块（文件末尾的 `#[cfg(test)]` 之后）不在冻结面内：那是测试数据。

② **混住文件里必须保持英文的字面量**：`agent/history.rs`、`render/input.rs`、
   `render/tui.rs`、`cli.rs` 这些文件里，模型可见 / 进流的串与给人看的串住在一起，
   所以不走①（整文件冻结会拦住该翻的那半），改正面查这几条串还在、且不含中文。

③ **`docs/**/*.md` 的中文占比下限**：设计文档是散文，读者是人。`docs/research/` 是
   上游文档的引文，`docs/highlight.md` 本来就是中文（ADR 0001 的例外），两者不查。
   `docs/adr/*.md` 也在内 —— ADR 是散文。

④ **ADR 的标题与小标题必须是中文**：ADR 的标题也是散文（ADR 0004 的「后加」一节），
   所以 `## Consequences` 那类英文小标题报红。整条都是行内代码的标题（如 `# `bash``）
   剥完是空的，放过；ADR 的**文件名**仍是标识符，留英文。

⑤ **`src/` 与 `tests/` 注释的中文行数下限**：棘轮，只许上升 —— 防止翻过的地方
   被改回英文。

用法：`python3 scripts/check-language.py`（在仓库根目录跑）。加 `--list` 会打印
冻结面里**允许**的那几条中文，用来核对白名单本身。
"""

from __future__ import annotations

import glob
import os
import re
import sys

CJK = re.compile(r"[\u4e00-\u9fff]")
# 断言/panic 消息所在的行：那些字面量是散文，不在冻结面内。
ASSERTION = re.compile(r"\b(assert|assert_eq|assert_ne|debug_assert|panic|expect)\b")

# --- ① 冻结面 ---------------------------------------------------------------
# 模型可见（工具声明 / 工具结果 / 身份与轮前缀 / 投影）与进流（reason / detail /
# summary / 协议标记）的代码所在处。
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
]

# 冻结面里**已经存在**的中文，逐条点名（前缀匹配）：它们不是漏网之鱼，是这一侧的
# 既有设计 —— harness 对模型说话用的是中文（身份、轮前缀），另有三条是上游供应商
# 错误文本里的匹配词，与语言无关。
ALLOWED = [
    # agent.rs：单 agent 的 system 身份，四段拼成一条（模型可见，冻结）
    ("src/agent.rs", "你是 fs-agent"),
    ("src/agent.rs", "你直接读写文件"),
    ("src/agent.rs", "docs/adr、.scratch"),
    ("src/agent.rs", "不要自称是"),
    # discussion.rs：讨论者的身份与作答规则、用户给的 soul 的框法、合成器的身份，
    # 以及合成器那一侧的「材料」包装（模型可见，冻结）
    ("src/discussion.rs", "你是本次讨论中的一位讨论者"),
    ("src/discussion.rs", "你的性格设定"),
    ("src/discussion.rs", "你是本次讨论的合成器"),
    ("src/discussion.rs", "问题："),
    ("src/discussion.rs", "## 第 {round} 轮"),
    ("src/discussion.rs", "### {speaker} 的作答"),
    ("src/discussion.rs", "### {speaker}"),
    ("src/discussion.rs", "请按「共识 / 分歧"),
    # provider/projection.rs：投影写进 messages 的轮前缀（模型可见，冻结）
    ("src/provider/projection.rs", "[轮 {round} · {speaker}]"),
    # provider/openai.rs 的三条是**匹配词**（去认上游的欠费错误），不是散文；该文件
    # 不在冻结面清单里，列在这里以免将来有人把它们当成注释一起翻掉。
    ("src/provider/openai.rs", "欠费"),
    ("src/provider/openai.rs", "余额"),
    ("src/provider/openai.rs", "额度"),
]

# 混住文件里那几条**必须保持英文**的字面量（前缀匹配）：模型可见或要永久回放。
#
# 为什么需要这一条：①「冻结面无新增中文」是按**文件**判的，而 `agent/history.rs`、
# `render/input.rs`、`render/tui.rs`、`cli.rs` 这些文件里，模型可见的串与给人看的串住在
# 一起 —— 整文件冻结会连带拦住该翻的那半，整文件放开又没人看着该留的那半。所以这两类
# 文件用**正面**检查：这几条串必须存在，且里面一个中文都没有。
FROZEN_LITERALS = [
    # 被杀死的进程没写下的那个工具结果（模型可见）
    ("src/agent/history.rs", "the session was interrupted while this call was in flight"),
    # 被杀死的回合里那些合成的工具结果（模型可见；const 名见 src/agent.rs）
    ("src/agent.rs", "hook stopped the turn: the tool did not run"),
    ("src/agent.rs", "the turn was cancelled: the tool did not run"),
    ("src/agent.rs", "the turn was cancelled while this call was in flight"),
    ("src/agent.rs", "session token budget exhausted: no new executor was"),
    # 会话级失败的 detail（进流、永久回放）
    ("src/agent.rs", "no debater answered this round"),
    # 问卷端口给模型的错误文本（`ask_user_question` 的结果）
    ("src/render/input.rs", "no questionnaire answerer is connected"),
    ("src/render/input.rs", "the questionnaire was left unanswered"),
    ("src/render/input.rs", "input ended before the questionnaire was answered"),
    ("src/render/tui.rs", "a questionnaire needs at least one question"),
    # probe 发给模型的提示词（模型可见）
    ("src/cli.rs", "The quick brown fox jumps over the lazy dog"),
    ("src/cli.rs", "Ignore the filler below"),
]

# --- ③ docs 的中文占比下限（百分数） ----------------------------------------
# 翻译完成后按**实测值减 2 个百分点**逐份收紧（2026-09-27 量：最低 31.2% 是
# `docs/agents/domain.md`，最高 45.5% 是 `docs/observability.md`；`docs/adr/*.md`
# 是 2026-09-30 ADR 中文化那一轮加进来的，实测 42.6–46.4%）。留 2 点余量是因为
# 文档里必然有英文标识符、代码路径、引用与命令，插一段代码块就会拉低比例；但再往下掉
# —— 也就是有人把整段散文翻回英文 —— 必须报红。
DOCS_MIN_RATIO = {
    "docs/adr/0001-chinese-ui-frozen-model-text.md": 44,
    "docs/adr/0002-fullscreen-alt-screen-tui.md": 40,
    "docs/adr/0003-plan-leaves-the-permission-modes.md": 41,
    "docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md": 43,
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


def check_frozen(list_allowed: bool) -> list[str]:
    if list_allowed:
        for path, prefix in ALLOWED:
            print(f"  允许：{path}  {prefix!r}")
        return []
    problems = []
    for path in FROZEN_FILES:
        if not os.path.exists(path):
            problems.append(f"{path}: 冻结面清单里的文件不存在（清单该更新了）")
            continue
        src = code_only(open(path, encoding="utf-8").read())
        for line, body, line_text in literals(src):
            if not CJK.search(body):
                continue
            head = re.sub(r"^(\\n|\\t|\s)+", "", body)
            if any(path == p and head.startswith(prefix) for p, prefix in ALLOWED):
                continue
            # 断言消息是给人看的散文（ADR 0004），不是模型可见文本，也不是进流的文本：
            # `assert!` / `panic!` / `expect(…)` 所在那一行的字面量不查。
            if ASSERTION.search(line_text):
                continue
            head = body[:60].replace("\n", " ")
            problems.append(f"{path}:{line}: 冻结面里出现了中文串 {head!r}…")
    return problems


def check_frozen_literals() -> list[str]:
    """那几条必须保持英文的串：还在，且没有变成中文。"""
    problems = []
    for path, prefix in FROZEN_LITERALS:
        if not os.path.exists(path):
            problems.append(f"{path}: 清单里的文件不存在（清单该更新了）")
            continue
        src = code_only(open(path, encoding="utf-8").read())
        found = [body for _, body, _ in literals(src) if prefix in body]
        if not found:
            problems.append(
                f"{path}: 找不到该保持英文的字面量 {prefix[:48]!r}…（改写或删除了？）"
            )
            continue
        for body in found:
            if CJK.search(body):
                problems.append(
                    f"{path}: 模型可见 / 进流的字面量被翻成了中文：{prefix[:40]!r}…"
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
    list_allowed = "--list" in sys.argv[1:]
    if list_allowed:
        print("冻结面允许的中文（白名单）：")
        check_frozen(True)
        print("\n必须保持英文的混住字面量：")
        for path, prefix in FROZEN_LITERALS:
            print(f"  {path}  {prefix!r}")
        return 0

    problems = (
        check_frozen(False)
        + check_frozen_literals()
        + check_docs()
        + check_adr_headings()
        + check_comments()
    )
    if problems:
        print("check-language: 不通过\n")
        for problem in problems:
            print(f"  - {problem}")
        print(
            "\n这五条来自 ADR 0004：散文（注释 / docs / ADR / 断言消息 / 给人看的错误）用中文，"
            "标识符、模型可见文本与进流文本留英文。"
        )
        return 1
    print(
        "check-language: OK（冻结面无新增中文、混住文件里的模型可见 / 进流串仍英文、"
        "docs 与 ADR 是中文散文、注释中文行数未回退）"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
