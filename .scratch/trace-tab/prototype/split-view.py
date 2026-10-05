#!/usr/bin/env python3
"""PROTOTYPE（throwaway）—— 120×24 整屏：对话视图与轨迹视图，轨迹页按轮次隔行底色。

体现维护者逐次追加的要求：
  1. 三类前缀各一色：注入 / 用户 / 助手；
  2. 对话视图里用户说的话右对齐（轨迹页仍左对齐）；
  3. **轨迹视图里不同轮次的信息用不同背景色区分**（单位级 zebra，不是屏幕行级）。

跑：
    python3 .scratch/trace-tab/prototype/split-view.py            # 带 ANSI 色
    python3 .scratch/trace-tab/prototype/split-view.py --plain    # 无色（贴进聊天看用）

中文按 2 列算；行数与几何是 120×24 的近似（精确值以 spec §4 与 layout 常量為准）。
这是 prototype，不要进 product。
"""

import sys
import unicodedata

W, SIDEBAR, DIVIDE = 120, 40, 1
MAIN = W - SIDEBAR - DIVIDE          # 79
TEXT = MAIN - 2                      # 77：右缘恒留滚动条 1 列 + 回合条 1 列
TRANSCRIPT_ROWS = 17

RESET = "\033[0m"
COLOR = {
    "inject": "\033[94m",      # LightBlue
    "user": "\033[92m",        # LightGreen
    "assistant": "\033[96m",   # LightCyan
}
# 轮次的两种背景（256 色里的两块深灰）。退化终端上不做底色。
ROUND_BG = ["\033[48;5;234m", "\033[48;5;236m"]
PLAIN = "--plain" in sys.argv

TINT_KEY = {
    "tool": "assistant", "think": "assistant", "hook_failed": "assistant",
    "error": "assistant", "usage": "assistant", "turn_start": "assistant",
    "turn_end": "assistant",
}

# 三个轮次、共 15 块 —— 正好是轨迹页一屏。
BLOCKS = [
    ("inject", "[上下文注入：AGENTS.md]"),
    ("user", "帮我修一下 render 的折行"),
    ("turn_start", "轮次 1"),
    ("tool", "read_file src/render/pane.rs"),
    ("assistant", "问题在 wrap_pending：它在 push 时按宽度折。"),
    ("turn_end", "回合结束：Completed"),
    ("user", "再把 28 列的表格看一眼"),
    ("turn_start", "轮次 2"),
    ("tool", "read_file src/render/markdown.rs"),
    ("assistant", "表格在 28 列下会挤成一团。"),
    ("turn_end", "回合结束：Completed"),
    ("user", "好，先提交"),
    ("turn_start", "轮次 3"),
    ("assistant", "已提交：abc1234。"),
    ("turn_end", "回合结束：Completed"),
]

CONVERSATION = {
    "user", "assistant", "error", "session_end", "permission",
    "hook_failed", "notice", "turn_start", "turn_end", "round_start", "round_end",
}


def rounds_of(blocks):
    """每块属于第几段：一个 `turn_end` 之后的块进下一段（第一个边界之前算第 0 段）。"""
    out, current = [], 0
    for kind, _ in blocks:
        out.append(current)
        if kind in ("turn_end", "round_end"):
            current += 1
    return out


ROUNDS = rounds_of(BLOCKS)


def cell(ch):
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


def dwidth(text):
    return sum(cell(ch) for ch in text)


def cut(text, width):
    out, used = "", 0
    for ch in text:
        if used + cell(ch) > width:
            break
        out += ch
        used += cell(ch)
    return out


def pad(text, width):
    return text + " " * max(0, width - dwidth(text))


def wrap(text, width, hanging=0):
    rows, line, used = [], "", hanging
    prefix = " " * hanging
    for ch in text:
        if used + cell(ch) > width:
            rows.append(line)
            line, used = prefix, hanging
        line += ch
        used += cell(ch)
    rows.append(line)
    return rows


def tint(kind, head, rest):
    """上色，返回**未闭合**的彩色串（外层负责 RESET）。"""
    key = TINT_KEY.get(kind, kind)
    if PLAIN or key not in COLOR:
        return head + rest
    return COLOR[key] + head + rest


def label(kind):
    if kind == "user":
        return "[用户] "
    if kind in ("assistant", "tool", "think", "hook_failed", "error", "usage",
                "turn_start", "turn_end"):
        return "[助手] "
    if kind == "tool_feedback":
        return "  "
    return ""


def render(block, width, idx, summary=False, right_align=False):
    """返回 [(plain, colored)]，colored 未闭合。"""
    kind, text = block
    lead = label(kind)
    bg = "" if PLAIN or not summary else ROUND_BG[ROUNDS[idx] % 2]
    if kind == "tool":
        return [(lead + "▸ " + cut(text, width - dwidth(lead) - 2),
                 bg + tint("assistant", lead + "▸ ", cut(text, width - dwidth(lead) - 2)))]
    if kind == "think":
        return [(lead + "▸ 思考", bg + tint("assistant", lead + "▸ ", "思考"))]
    if kind in ("assistant", "user") and summary:
        room = width - dwidth(lead)
        head = cut(text, room)
        if dwidth(head) < dwidth(text):
            head = cut(head, room - 1) + "…"
        return [(lead + head, bg + tint(kind, lead, head))]
    if kind in ("assistant", "user"):
        rows = wrap(text, width, hanging=dwidth(lead))
        rows[0] = lead + rows[0]
        out = []
        for row in rows:
            plain = row
            colored = tint(kind, row[: dwidth(lead)], row[dwidth(lead):])
            if right_align and kind == "user":
                gap = " " * max(0, width - dwidth(plain))
                plain, colored = gap + plain, gap + colored
            out.append((plain, colored))
        return out
    head = lead
    body = cut(text, width - dwidth(head))
    return [(head + body, bg + tint(kind, head, body))]


def rows_for(blocks, width, rows, summary=False, right_align=False):
    out = []
    for idx, block in enumerate(blocks):
        out.extend(render(block, width, idx, summary, right_align))
    return out[-rows:]


def sidebar_line(plain, colored, width):
    """把一行铺满左栏宽度（底色连同末尾留白一起铺）。"""
    if PLAIN:
        return pad(plain, width)
    return colored + " " * max(0, width - dwidth(plain)) + RESET


def build(sidebar_rows, main_blocks, right_align):
    left = ["", "   ▄▀▀█  fs", "   ▀▄▄▀  agent", "   ▀▀▀▀", "", ""]
    tabs = ["├" + "─" * (SIDEBAR - 2) + "┤", "│ 调用量 │ 轨迹 │ 文件" + " " * 4 + "│",
            "├" + "─" * (SIDEBAR - 2) + "┤"]
    page = sidebar_rows + [("", "")] * max(0, 15 - len(sidebar_rows))
    left_lines = [(pad(row, SIDEBAR),) * 2 for row in left + tabs]
    left_lines += [(sidebar_line(p, c, SIDEBAR),) * 2 for p, c in page]
    main = rows_for(main_blocks, TEXT, TRANSCRIPT_ROWS, right_align=right_align)
    main_lines = [("", "")] * (TRANSCRIPT_ROWS - len(main)) + main
    tail = [
        (pad(" 模型 deepseek-v4 │ 模式 ask │ 上下文 6%", MAIN),) * 2,
        ("", ""),
        (" ❱", " ❱"),
        ("", ""),
        (pad(" 就绪 · ctrl-o 左栏 · ctrl-c/ctrl-d 退出", MAIN),) * 2,
        ("", ""),
    ]
    return left_lines, [(p, c) for p, c in main_lines] + [(p, c) for p, c in tail]


def show(title, left, main, note):
    print(f"\n{'=' * W}\n{title}\n{'=' * W}")
    divide = "│" if PLAIN else "\033[90m│\033[0m"
    for i in range(24):
        lp, lc = left[i] if i < len(left) else (" " * SIDEBAR, " " * SIDEBAR)
        mp, mc = main[i] if i < len(main) else ("", "")
        print(lc + divide + mc + (RESET if not PLAIN else "")
              + " " * max(0, MAIN - dwidth(mp)))
    print(note)


def main():
    print("PROTOTYPE — 120×24 整屏（三类前缀上色 + 用户消息右对齐 + 轨迹页按轮次隔行底色）")
    print("颜色：注入 LightBlue ｜ 用户 LightGreen ｜ 助手 LightCyan")
    print("轨迹页底色：轮次 1 与轮次 3 用 #1c1c1c，轮次 2 用 #303030（256 色 234 / 236）")

    trace = rows_for(BLOCKS, SIDEBAR - 1, 15, summary=True)
    left_trace, _ = build(trace, BLOCKS, right_align=False)
    usage = ["", "   ▄▀▀█  fs", "   ▀▄▄▀  agent", "   ▀▀▀▀", "", "",
             "├" + "─" * (SIDEBAR - 2) + "┤", "│ 调用量 │ 轨迹 │ 文件" + " " * 4 + "│",
             "├" + "─" * (SIDEBAR - 2) + "┤",
             " 上下文   12,345 / 200,000（6%）", " token      45,678 / 100万",
             " 回合                     12", " 输入                 23,456",
             " 输出                 22,222", " 缓存                 10,000"]
    left_usage = [(pad(row, SIDEBAR),) * 2 for row in usage] + [("", "")] * (24 - len(usage))

    _, today = build([], BLOCKS, right_align=False)
    show("今天：一条流全在主列（左栏 = 调用量）", left_usage, today,
         "↑ 过程与对话混在一起；轨迹页还不存在")

    _, after = build(trace, [b for b in BLOCKS if b[0] in CONVERSATION], right_align=True)
    show("之后：主列 = 对话视图（用户右对齐），左栏 = 轨迹视图（三轮隔行底色）", left_trace, after,
         "↑ 轨迹页的色块按**轮次**走，不按屏幕行 —— 滚动时色块跟着内容走")


if __name__ == "__main__":
    main()
