#!/usr/bin/env python3
"""ADR 0004 的护栏：散文用中文，标识符与「进 messages / 进流」的文本留英文。

三条检查，任一条不过就以非零码退出：

① **冻结面**：模型可见或要永久回放的字符串字面量里不得出现中文。哪些文件属于
   「冻结面」、以及**今天已经存在**的几条中文（三条身份提示、讨论轮前缀、上游供应商
   错误里的 `欠费` / `余额` / `额度` 匹配词）逐条列在下面的表里 —— 表是白名单，不是
   省略号：出现表外的中文串就是有人把进入 `messages` 或进入事件流的文本中文化了，
   那是 ADR 0001 与 ADR 0004 都禁止的事（缓存前缀 + 老流永久混排）。
   测试模块（文件末尾的 `#[cfg(test)]` 之后）不在冻结面内：那是测试数据。

② **`docs/**/*.md` 的中文占比下限**：设计文档是散文，读者是人。`docs/research/` 是
   上游文档的引文，`docs/highlight.md` 本来就是中文（ADR 0001 的例外），两者不查。

③ **`src/` 与 `tests/` 注释的中文行数下限**：棘轮，只许上升 —— 防止翻过的地方
   被改回英文。

用法：`python3 scripts/check-language.py`（在仓库根目录跑）。加 `--list` 会打印
冻结面里**允许**的那几条中文，用来核对白名单本身。
"""

from __future__ import annotations

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

# --- ② docs 的中文占比下限（百分数） ----------------------------------------
# 翻译完成后按实测值收紧；比例留出余量，因为文档里必然有英文标识符、代码路径、
# 引用与命令。
DOCS_MIN_RATIO = {
    "docs/bash.md": 30,
    "docs/credentials.md": 30,
    "docs/custom-tools.md": 30,
    "docs/discussion.md": 30,
    "docs/executor.md": 30,
    "docs/observability.md": 30,
    "docs/render.md": 30,
    "docs/repo-map.md": 30,
    "docs/skills.md": 30,
    "docs/agents/domain.md": 30,
    "docs/agents/issue-tracker.md": 30,
    "docs/agents/triage-labels.md": 30,
}

# --- ③ 注释中文行的棘轮 -----------------------------------------------------
# 数字是「已翻成中文的注释行数」的下限。翻译推进后往上提，绝不往下调。
COMMENT_FLOOR = {
    "src": 195,
    "tests": 93,
}


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
        return 0

    problems = check_frozen(False) + check_docs() + check_comments()
    if problems:
        print("check-language: 不通过\n")
        for problem in problems:
            print(f"  - {problem}")
        print(
            "\n这三条来自 ADR 0004：散文（注释 / docs / 断言消息 / 给人看的错误）用中文，"
            "标识符、模型可见文本与进流文本留英文。"
        )
        return 1
    print("check-language: OK（冻结面无新增中文、docs 是中文散文、注释中文行数未回退）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
