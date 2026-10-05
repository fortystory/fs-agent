#!/usr/bin/env python3
"""语义色板的两版对照 —— 对着真终端看（prototype，用完即弃）。

用法：

    python3 .scratch/tui-visual-language/prototype/palette-demo.py      # 两版都画
    python3 .scratch/tui-visual-language/prototype/palette-demo.py a    # 只看 A 版
    python3 .scratch/tui-visual-language/prototype/palette-demo.py b    # 只看 B 版

它不是真渲染器，只是把两版配色按 ANSI 打到终端上 —— 于是你在**自己的**终端里
看到的就是这两版真实的样子，包括 `DIM` 在你这台终端上到底兑不兑现（见
`research/01-terminal-capability-bounds.md` §1）。

色值按 crossterm 实际发出的字节给：命名色走 256 色形式（`Gray`=38;5;7、
`DarkGray`=38;5;8），不是 ratatui 文档表上的 37 / 90。
"""

from __future__ import annotations

import sys

# ── ANSI 片段 ────────────────────────────────────────────────────────────────

OFF = "\x1b[0m"
BOLD = "\x1b[1m"
DIM = "\x1b[2m"
ITALIC = "\x1b[3m"
REVERSE = "\x1b[7m"
DEFAULT_FG = "\x1b[39m"          # Color::Reset 的前景形态


def idx(n: int) -> str:
    """256 色索引前景 —— crossterm 给命名色发的就是这个形式。"""
    return f"\x1b[38;5;{n}m"


def rgb(r: int, g: int, b: int) -> str:
    """真彩色前景 —— crossterm 对 Color::Rgb 直发，不降级。"""
    return f"\x1b[38;2;{r};{g};{b}m"


# crossterm 的索引表（research/01 §3.2）：Red=1 Green=2 Yellow=3 Blue=4
# Magenta=5 Cyan=6 Gray=7 DarkGray=8 LightRed=9 LightGreen=10
# LightYellow=11 LightBlue=12 LightMagenta=13 LightCyan=14 White=15

# ── A 版「最小色数」 ─────────────────────────────────────────────────────────
# 颜色只回答两个问题：要不要注意（WARN / BAD）、有没有被选中（ACCENT）。
# 分类信息（谁说的、什么语法、标题还是正文）一律交给字形、结构与位置。

A = {
    "PLAIN": DEFAULT_FG,          # 正文
    "MUTED": idx(8),              # 退后一档（DarkGray，随主题）
    "CHROME": rgb(0x4A, 0x4A, 0x4A),  # 只做装饰线，不承载信息
    "ACCENT": idx(13),            # 选中 / 聚焦 / 记号
    "WARN": idx(3),               # 警告：诊断、hook 反馈、新内容
    "BAD": idx(1),                # 错误：工具失败、Severity::Bad
    "SPK1": idx(14),              # 角色五色（tui-ux 冻结项 9，不动）
    "SPK2": idx(13),
    "USER": idx(10),
    "EXEC": idx(11),
    "SYS": idx(7),
}

# ── B 版「分域归一」 ─────────────────────────────────────────────────────────
# 不追求减色，追求「同一域内每个语义各归其位」。分两个域：
#   · 界面域（chrome / 信号 / 角色）—— 色值不重复
#   · 内容域（markdown + 语法高亮）—— 自己一套，允许与界面域撞值，
#     因为代码块内部上下文明确，撞值不会被读混。
# 相比现状的两处实质改动：行内代码与语法数字不再用 Yellow（消灭与警告撞色）。

B = {
    "PLAIN": DEFAULT_FG,
    "MUTED": idx(7),              # 退后一档（Gray，跟随主题）
    "CHROME": rgb(0x4A, 0x4A, 0x4A),
    "ACCENT": idx(13),
    "MARKER": idx(12),            # 可点 / 记号：注入行、`/`、`❱` 之外的入口标记
    "WARN": idx(3),
    "BAD": idx(1),
    "GOOD": idx(2),               # 界面域里 Success 仍保留颜色
    "SPK1": idx(14),
    "SPK2": idx(13),
    "USER": idx(10),
    "EXEC": idx(11),
    "SYS": idx(7),
    # 内容域
    "MD_HEADING": idx(6),
    "MD_CODE": idx(7),            # ← 改：原本是 Yellow
    "MD_QUOTE": idx(7),
    "SYN_KEYWORD": idx(5),
    "SYN_FUNCTION": idx(4),
    "SYN_TYPE": idx(6),
    "SYN_STRING": idx(2),
    "SYN_NUMBER": idx(11),        # ← 改：原本是 Yellow
    "SYN_COMMENT": idx(8),
    "SYN_PUNCT": idx(7),
}


def paint(text: str, fg_code: str = "", *mods: str) -> str:
    """给一段文字上色。空 fg_code 表示不碰前景。"""
    prefix = "".join(mods) + (fg_code or "")
    return f"{prefix}{text}{OFF}" if prefix else text


# ── 一段有代表性的主列片段（两版共用结构，只换色板）────────────────────────

def frame(p: dict, title: str) -> str:
    out: list[str] = []
    add = out.append

    # 转录：用户消息（名字独占一行的排版，trace-tab 定的形态）
    add(paint("❯ " + "帮我把 render 里的颜色收一收", p["USER"]))
    add("")
    add(paint("kimi", p["SPK1"]))
    add(paint("我看了下，光 ", p["PLAIN"]) + paint("tui.rs", p["PLAIN"])
        + paint(" 一个文件就有 79 处 ", p["PLAIN"]) + paint("Color::", p["PLAIN"])
        + paint(" 字面量。", p["PLAIN"]))
    add("")
    # 工具行：▸ 是可点标记，摘要是「主」
    add("  " + paint("▸ ", p["MUTED"]) + paint("调用 grep ", p["PLAIN"], BOLD)
        + paint("pattern=", p["MUTED"]) + paint("Color::", p["PLAIN"]))
    add("  " + paint("▸ ", p["MUTED"]) + paint("调用 bash ", p["PLAIN"], BOLD)
        + paint("command=", p["MUTED"]) + paint("cargo test", p["PLAIN"]))
    add("  " + paint("失败：error[E0308]: mismatched types", p["BAD"]))
    add("")
    # 【旧】行内代码今天用 Yellow —— 与下面的警告同色，这是要收掉的那处撞色
    add("  " + paint("【旧】行内代码用 Yellow：", p["PLAIN"])
        + paint("`Color::DarkGray`", idx(3) if p is A else idx(3))
        + paint(" ← 与警告撞色", p["MUTED"]))
    add("  " + paint("【新】行内代码：", p["PLAIN"])
        + paint("`Color::DarkGray`", p["MD_CODE"] if "MD_CODE" in p else p["MUTED"]))
    add("")
    # markdown：标题、引用
    add("  " + paint("## 结论", p.get("MD_HEADING", p["PLAIN"]),
                     *() if "MD_HEADING" in p else (BOLD,)))
    add("  " + paint("> 这些灰大多可以退成同一档", p.get("MD_QUOTE", p["MUTED"])))
    add("")
    # 代码块：语法高亮的差别最大 —— A 版只留两个色 + 两个修饰符
    if "SYN_KEYWORD" in p:
        add("  " + paint("fn ", p["SYN_KEYWORD"]) + paint("name_style", p["SYN_FUNCTION"])
            + paint("() -> ", p["SYN_PUNCT"]) + paint("Style", p["SYN_TYPE"])
            + paint(" {", p["SYN_PUNCT"]))
        add("      " + paint("// 无名册时退回静音", p["SYN_COMMENT"], ITALIC))
        add("      " + paint("Style", p["SYN_TYPE"]) + paint("::", p["SYN_PUNCT"])
            + paint("new", p["SYN_FUNCTION"]) + paint("().", p["SYN_PUNCT"])
            + paint("fg", p["SYN_FUNCTION"]) + paint("(", p["SYN_PUNCT"])
            + paint("42", p["SYN_NUMBER"]) + paint(")", p["SYN_PUNCT"])
            + paint(" ", p["SYN_PUNCT"]) + paint("\"灰色名字\"", p["SYN_STRING"]))
        add("  }")
    else:
        add("  " + paint("fn name_style() -> Style {", p["PLAIN"], BOLD))
        add("      " + paint("// 无名册时退回静音", p["MUTED"], ITALIC))
        add("      " + paint("Style::new().fg(", p["PLAIN"])
            + paint("DarkGray", p["ACCENT"]) + paint(")", p["PLAIN"]))
        add("  }")
    add("")
    # 旁白与等待提示
    add("  " + paint("正在思考…", p["MUTED"]))
    add("")
    # 底部三条
    add(paint("─" * 64, p["CHROME"]))
    add(paint("kimi · ask · 上下文 42%", p["MUTED"]))
    add(paint("ctrl-c 取消   ctrl-o 左栏", p["MUTED"]))
    add(paint("❱ " + "把状态行也一起收了吧", p["PLAIN"]))
    return "\n".join("  " + line if line and not line.startswith("  ") else line
                     for line in out)


def dim_probe() -> str:
    """同一句话的三种「退后」写法 —— 让用户在自己的终端上判 DIM 到底行不行。"""
    line = "这一行在比什么：DarkGray / 默认前景+DIM / 默认前景"
    return "\n".join([
        "  " + paint(line, idx(8)),
        "  " + paint(line, DEFAULT_FG, DIM),
        "  " + paint(line, DEFAULT_FG),
    ])


def main(argv: list[str]) -> int:
    which = (argv[1] if len(argv) > 1 else "ab").lower()
    if which not in {"a", "b", "ab"}:
        print(__doc__)
        return 2

    if "a" in which:
        print()
        print(paint("═══ A 版「最小色数」：颜色只留给信号 ═══", "", BOLD))
        print(paint("PLAIN + MUTED + CHROME + ACCENT + WARN + BAD，其余靠 BOLD 与结构",
                    idx(8)))
        print()
        print(frame(A, "A"))
    if "b" in which:
        print()
        print(paint("═══ B 版「分域归一」：界面域 / 内容域各归其位 ═══", "", BOLD))
        print(paint("色数基本不变；改的是行内代码与语法数字不再用 Yellow", idx(8)))
        print()
        print(frame(B, "B"))

    print()
    print(paint("═══ DIM 实测（在你的终端上）═══", "", BOLD))
    print(paint("从上到下：DarkGray / 默认前景 + SGR 2 / 默认前景", idx(8)))
    print()
    print(dim_probe())
    print()
    print(paint("如果第 2 行与第 3 行看不出差别，就是 research/01 §1.2 说的那种终端 —— "
                "层级不能压在 DIM 上。", idx(8)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
