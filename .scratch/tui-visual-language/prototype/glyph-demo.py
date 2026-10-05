#!/usr/bin/env python3
"""字形语法的两档对照 —— 对着真终端看（prototype，用完即弃）。

用法：

    python3 .scratch/tui-visual-language/prototype/glyph-demo.py

它不接真渲染器，只把候选字形按原样打到终端上：于是"角改用哪种"、"小节线归哪档"
这些事，你在**自己的**终端与字体下就能判。最后一段是宽度标尺 —— 那几行要是没对齐，
说明你的字体/终端对这些码位的宽度处理与预期不同（见
`research/01-terminal-capability-bounds.md` §2）。
"""

from __future__ import annotations

import unicodedata

# ── 已定的两档（见 prototype/glyph-grammar.md）───────────────────────────────
FRAME_H = "┄"   # U+2504 TRIPLE DASH HORIZONTAL —— 与 ratatui LIGHT_TRIPLE_DASHED 同码位
FRAME_V = "┆"   # U+2506 TRIPLE DASH VERTICAL
CONTENT_H = "─"  # U+2500
CONTENT_V = "│"  # U+2502


def dwidth(text: str) -> int:
    """显示宽度：East Asian Wide/Fullwidth 记 2，其余记 1。"""
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def pad(text: str, width: int) -> str:
    return text + " " * max(0, width - dwidth(text))


def frame(tl: str, tr: str, bl: str, br: str, body: list[str], width: int = 34,
          h: str = FRAME_H, v: str = FRAME_V) -> str:
    inner = width - 2
    top = tl + h * inner + tr
    bot = bl + h * inner + br
    mid = [v + pad(line, inner) + v for line in body]
    return "\n".join([top, *mid, bot])


def section(title: str, note: str = "") -> None:
    print()
    print(f"\x1b[1m{title}\x1b[0m")
    if note:
        print(f"\x1b[38;5;8m{note}\x1b[0m")
    print()


def main() -> int:
    section(
        "① 浮层的角：三个方案",
        "横竖都已经是三重虚线（与外壳同一码位），差的只有四个角。",
    )
    body = ["模态：要跑这条命令吗？", "[y] 允许   [n] 拒绝"]
    indent = "    "
    print(indent + "A 实角（现状：角借了内容档的实线字符）")
    print("\n".join(indent + line for line in frame("┌", "┐", "└", "┘", body).split("\n")))
    print()
    print(indent + "B 空角（角是空格：线在四角断开，与虚线的断续同源）")
    print("\n".join(indent + line for line in frame(" ", " ", " ", " ", body).split("\n")))
    print()
    print(indent + "C 点角（用 · 补角；注意 U+00B7 在 Unicode 里也是 Ambiguous 宽度）")
    print("\n".join(indent + line for line in frame("·", "·", "·", "·", body).split("\n")))

    section(
        "② 详情里的小节线归哪档",
        "它在浮层内部，但分的是「内容」，与 markdown 的分隔线是同一件事。",
    )
    label_w = 14
    print("    " + pad("A 留实线（现状）", label_w) + CONTENT_H * 2 + " 思考 " + CONTENT_H * 2)
    print("    " + pad("B 改框架虚线", label_w) + FRAME_H * 2 + " 思考 " + FRAME_H * 2)
    print("    " + pad("markdown 分隔线", label_w) + CONTENT_H * 24 + "   ← 内容档")

    section(
        "③ 外壳与浮层：本来就是同一码位",
        "所以「框架一套虚线」这件事不需要换字符。",
    )
    print(f"    外壳横线 {FRAME_H}  U+2504   浮层横线 {FRAME_H}  U+2504   ← 同一个")
    print(f"    外壳竖线 {FRAME_V}  U+2506   浮层竖线 {FRAME_V}  U+2506   ← 同一个")
    print()
    print("    外壳（无角，手画，屏幕顶到底）：")
    print("      " + FRAME_V)
    print("      " + FRAME_V + "   …")
    print("    浮层（有角）：")
    print("\n".join("      " + line for line in frame(" ", " ", " ", " ", ["内容"]).split("\n")))

    section(
        "④ 散落的符号：现状与双义",
        "标 ← 的是同一个字符承担两个语义的地方。",
    )
    print("    ▸ [kimi] 调用 bash command=cargo test      ← 有折起来的内容（可点开）")
    print("    ▸ [kimi] 思考完成                          ← 同上")
    print("    ☐ 补测试            ← todo：待办")
    print("    ▸ 补测试            ← todo：进行中   ← 同一个 ▸，两个语义（靠区域区分）")
    print("    ✓ 补测试            ← todo：已完成")
    print("    ❱ 把状态行也收了吧  ← 提示符（输入区）")
    print("    ┃  ┊  ⋮             ← 回合条：焦点 / 普通 / 截断")
    print("    > [x] ● 选项        ← 问卷：光标 / 选中 / 单选")
    print("    …长消息被截断        ← 轨迹页的行内截断")
    print("    ┆                   ← 状态行里的三段分隔（与外壳竖线同码位，但语义不同）")

    section(
        "⑤ 宽度标尺",
        "每行 10 个字符。哪一行没对齐，就是你这台终端对这些码位的宽度处理不同。",
    )
    ruler = "".join(str(i % 10) for i in range(10))
    for name, ch in [
        ("CONTENT_H ─", CONTENT_H),
        ("FRAME_H   ┄", FRAME_H),
        ("FRAME_V   ┆", FRAME_V),
        ("CONTENT_V │", CONTENT_V),
        ("MIDDLE    ·", "·"),
        ("todo      ☐", "☐"),
        ("arrow     ▸", "▸"),
        ("rail      ┊", "┊"),
    ]:
        print(f"    {ruler}")
        print(f"    {ch * 10}   {name}")
        print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
