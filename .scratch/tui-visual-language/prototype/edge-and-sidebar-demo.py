#!/usr/bin/env python3
"""转录右缘与左栏页的对照 —— 对着真终端看（prototype，用完即弃）。

用法：

    python3 .scratch/tui-visual-language/prototype/edge-and-sidebar-demo.py

两件事需要真终端才判得准：右缘那两条一列宽的竖线挤不挤，以及占比条的底色到底看不看得见。
"""

from __future__ import annotations

import unicodedata

MUTED = "\x1b[38;5;8m"
PLAIN = "\x1b[39m"
CHROME = "\x1b[38;2;74;74;74m"
ACCENT = "\x1b[38;5;13m"
BG_MUTED = "\x1b[48;5;8m"
BG_BRIGHT = "\x1b[48;5;7m"
OFF = "\x1b[0m"
DIMC = "\x1b[2m"


def cw(text: str) -> int:
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def pad(text: str, width: int) -> str:
    return text + " " * max(0, width - cw(text))


def section(title: str, note: str = "") -> None:
    print()
    print(f"{PLAIN}\x1b[1m{title}{OFF}")
    if note:
        print(f"{MUTED}{note}{OFF}")
    print()


def main() -> int:
    section(
        "① 转录右缘：对话视图（滚动条列 + 回合条列）",
        "TRAILING_COLUMNS = 2 永远预留（注释：这样文字不会因为转录长高了而重新折行）。",
    )
    # ratatui Scrollbar(VerticalRight) 默认：track │，thumb █
    body = "我看了下，光 tui.rs 一个文件就有 79 处 Color:: 字面量。"
    rail = ["┊", "┊", "┃", "┊", "┊"]
    track_col = ["│", "│", "│", "│", "│"]
    thumb_at = 2
    print("    " + pad(body, 52) + MUTED + "│" + OFF + MUTED + "█" + OFF + MUTED + "┃" + OFF)
    for i in range(1, 5):
        side = "█" if i == thumb_at else "│"
        print("    " + " " * 52 + MUTED + side + OFF + MUTED + rail[i] + OFF)
    print()
    print(f"    {MUTED}↑ 正文 {OFF}{CHROME}│{OFF}{MUTED}滚动条 track{OFF}{CHROME}│{OFF}"
          f"{MUTED}thumb{OFF}{CHROME}│{OFF}{MUTED}回合条{OFF}  —— 一列靠一列")

    section(
        "② 右缘方案：滚动条只画滑块、不画轨道",
        "轨道与紧邻的回合条是同一族的细竖线，两条并排读起来是噪音；位置靠滑块本身就够。",
    )
    print("    " + pad(body, 52) + " " + MUTED + "█" + OFF + MUTED + "┃" + OFF)
    for i in range(1, 5):
        side = "█" if i == thumb_at else " "
        print("    " + " " * 52 + side + " " + MUTED + rail[i] + OFF)
    print(f"\n    {MUTED}右边只剩一条轨道感的线（回合条），滑块偶尔出现{OFF}")

    section(
        "③ 轨迹页（左栏 40 列）的右缘：什么都没有",
        "与对话视图不一致 —— 它有自己独立的滚动与贴底跟随（trace-tab 票 04），却没有位置指示。",
    )
    trace_body = "我看了下，光 tui.rs 一个文件就有 79 …"
    print("    " + pad(trace_body, 36) + "  ")
    print("    " + pad("▸ 调用 grep pattern=Color::", 36) + "  ")
    print("    " + pad("▸ 调用 bash command=cargo test", 36) + "  ")

    section(
        "④ 占比条的三条路（左栏 `调用量` 页）",
        "现状：标签 fg DarkGray，条是 bg DarkGray —— 同一个值，一个当字一个当底。",
    )
    label = "上下文"
    value = "42%"
    labels_w = 8
    val_w = 10
    filled = 4

    print(f"    {MUTED}A 现状（bg DarkGray，条不占列）{OFF}")
    head = " " * filled
    print("    " + f"{MUTED}{pad(label, labels_w)}{OFF}" + " "
          + f"{BG_MUTED}{head}{OFF}" + " " * (val_w - filled)
          + f" {PLAIN}{value}{OFF}")
    print()
    print(f"    {MUTED}B 字符条（结构承重：不靠颜色对比，也让标签保持自己的色）{OFF}")
    bar = "▓" * filled + "░" * (val_w - filled)
    print("    " + f"{MUTED}{pad(label, labels_w)}{OFF}" + " "
          + f"{MUTED}{bar}{OFF}" + f" {PLAIN}{value}{OFF}")
    print()
    print(f"    {MUTED}C 底色换个更亮的灰（Gray 索引 7）{OFF}")
    print("    " + f"{MUTED}{pad(label, labels_w)}{OFF}" + " "
          + f"{BG_BRIGHT}{head}{OFF}" + " " * (val_w - filled)
          + f" {PLAIN}{value}{OFF}")
    print()
    print(f"    {DIMC}注：C 与 05 的「静音只有一档」相冲 —— 它会引回第二个灰值。{OFF}")

    section(
        "⑤ 左栏身份标记（宽档 5 行）与页签条",
        "标记的品红坡道 LightMagenta → Magenta 不在色板里，是这次唯一没归属的色。",
    )
    ramp = ["▄▀▀█", "█▀▀▄", "█  █", "█▀▀▄", "▄▄▄▄"]
    shades = ["\x1b[38;5;13m", "\x1b[38;5;13m", "\x1b[38;5;5m",
              "\x1b[38;5;13m", "\x1b[38;5;5m"]
    for row, shade in zip(ramp, shades):
        print("    " + shade + f"  {row}" + OFF)
    print("    " + CHROME + "┄" * 38 + OFF)
    print("    " + ACCENT + " 调用量 " + OFF + MUTED + "┆ todo ┆ 轨迹 ┆ 文件" + OFF)
    print("    " + CHROME + "┄" * 38 + OFF)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
