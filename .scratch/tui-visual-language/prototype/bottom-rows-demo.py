#!/usr/bin/env python3
"""底部三条（状态行 / 输入区 / 提示行）的现状与候选 —— 对着真终端看。

用法：

    python3 .scratch/tui-visual-language/prototype/bottom-rows-demo.py

它按 `wording::hint_line` / `wording::status_row` 的算法**重算**两档宽度下的实际结果，
再按色板的真 ANSI 打出来 —— 于是"120 列屏到底看得见几条提示"这件事是**算出来的**，
不是估的。
"""

from __future__ import annotations

import unicodedata

MUTED = "\x1b[38;5;8m"          # palette::MUTED（DarkGray 索引 8）
PLAIN = "\x1b[39m"              # palette::PLAIN（默认前景）
CHROME = "\x1b[38;2;74;74;74m"  # palette::CHROME
ACCENT = "\x1b[38;5;13m"        # palette::ACCENT
DIMC = "\x1b[2m"
OFF = "\x1b[0m"

KEY_HINTS = [
    "enter 发送",
    "ctrl-j 换行",
    "esc 取消",
    "shift+tab 模式",
    "PgUp/PgDn 滚动",
    "ctrl-o 左栏",
]
EXIT_IDLE = "ctrl-c/ctrl-d 退出"

# 左栏宽度档（layout.rs:49-56）：≥120 → 40；80–119 → 28；<80 → 无
def sidebar_columns(screen: int) -> int:
    if screen >= 120:
        return 40
    if screen >= 80:
        return 28
    return 0


def main_columns(screen: int) -> int:
    side = sidebar_columns(screen)
    return screen if side == 0 else screen - side - 1  # 分隔列占 1


def cw(text: str) -> int:
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def hint_line(state: str, hints: list[str], exit_: str, width: int) -> tuple[str, int]:
    """复刻 `wording::hint_line`（wording.rs:1224-1249）。返回（文本, 用上的提示条数）。"""
    exit_w = cw(exit_)
    chosen = ""
    used = 0
    for hint in hints:
        candidate = hint if not chosen else f"{chosen} · {hint}"
        if cw(candidate) + 3 + exit_w > width:
            break
        chosen = candidate
        used += 1
    run = exit_ if not chosen else f"{chosen} · {exit_}"
    with_state = f"{state} · {run}"
    text = with_state if cw(with_state) <= width else run
    return text, used


def status_row(model: str, mode: str, share: str, width: int) -> str:
    """复刻 `wording::status_row`（wording.rs:1697-1713）的三档降级。"""
    seg = lambda t: f" {t} "
    full = f"{seg(f'模型 {model}')}│{seg(mode)}│{seg(share)}"
    if cw(full) <= width:
        return full
    two = f"{seg(mode)}│{seg(share)}"
    if cw(two) <= width:
        return two
    return seg(share)


def frame(screen: int, hint_width: int, layered_status: bool, gap_above: bool) -> list[str]:
    width = main_columns(screen)
    out: list[str] = []
    transcript = [
        "kimi",
        "我看了下，光 tui.rs 一个文件就有 79 处 Color:: 字面量。",
    ]
    out += [f"{MUTED}  {t}{OFF}" if i == 0 else f"  {t}" for i, t in enumerate(transcript)]
    if gap_above:
        out.append("")
    out.append(f"{CHROME}{'┄' * width}{OFF}")
    status = status_row("kimi", "询问", "上下文 42%", width)
    if layered_status:
        # 标签 MUTED、值 PLAIN、分隔符 CHROME
        painted = ""
        for i, part in enumerate(status.split("│")):
            if i:
                painted += f"{CHROME}│{OFF}"
            stripped = part.strip()
            label, _, value = stripped.partition(" ")
            if value:
                painted += f"{MUTED} {label} {OFF}{PLAIN}{value}{OFF}"
            else:
                painted += f"{PLAIN} {stripped} {OFF}"
        out.append(painted)
    else:
        out.append(f"{MUTED}{status}{OFF}")
    out.append(f"{ACCENT}❱ {OFF}{PLAIN}把状态行也一起收了吧{OFF}")
    out.append(f"{CHROME}{'┄' * hint_width}{OFF}")
    text, used = hint_line("就绪", KEY_HINTS, EXIT_IDLE, hint_width)
    out.append(f"{MUTED}{text}{OFF}")
    marker = " " * (2 + cw(text))
    out.append(f"{DIMC}{marker}↑ 提示行用到第 {used} 条；剩 {len(KEY_HINTS) - used} 条看不见{OFF}")
    return out


def show(title: str, note: str, rows: list[str]) -> None:
    print()
    print(f"{PLAIN}\x1b[1m{title}{OFF}")
    print(f"{MUTED}{note}{OFF}")
    print()
    for row in rows:
        print("  " + row)


def main() -> int:
    for screen in (120, 80):
        width = main_columns(screen)
        side = sidebar_columns(screen)
        text, used = hint_line("就绪", KEY_HINTS, EXIT_IDLE, width)
        show(
            f"{screen}×24 · 左栏 {side} 列 · 主列（= 提示行宽度）{width} 列 · 现状",
            f"提示行用上 {used}/{len(KEY_HINTS)} 条；看不见的是："
            + "、".join(KEY_HINTS[used:]) if used < len(KEY_HINTS) else "全部可见",
            frame(screen, width, layered_status=False, gap_above=False),
        )

    show(
        "候选：状态行分层（标签 MUTED / 值 PLAIN / 分隔符 CHROME）",
        "120×24；整行仍属过程档，只是行内分出主次。",
        frame(120, main_columns(120), layered_status=True, gap_above=False),
    )

    show(
        "候选：状态行与转录之间加一行空白（tui-chrome/spec.md:196 留的口子）",
        "120×24；代价是转录少一行。",
        frame(120, main_columns(120), layered_status=True, gap_above=True),
    )

    full = " · ".join(KEY_HINTS)
    need = cw(full) + 3 + cw(EXIT_IDLE) + cw("就绪 · ")
    show(
        "核心问题：提示行要多宽才装得下全部 6 条",
        f"要 {need} 列。120 列屏给提示行只有 {main_columns(120)} 列 —— 差 {need - main_columns(120)} 列，"
        "而左栏吃掉了 41 列。",
        [
            f"{MUTED}全部 6 条 + 出口 + 状态词 = {need} 列{OFF}",
            f"{MUTED}120 列屏的主列          = {main_columns(120)} 列{OFF}",
            f"{MUTED}左栏 + 分隔列吃掉的      = 41 列{OFF}",
            f"{ACCENT}→ 提示行跨整屏（{120} 列）才装得下{OFF}",
        ],
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
