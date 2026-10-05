#!/usr/bin/env python3
"""PROTOTYPE（throwaway）—— 轨迹视图在 40 / 28 列下的三种画法。

票 01 的问题：轨迹视图要把整条流画进左栏 40 列（宽档）或 28 列（窄档）。三种变体：

    A 照抄     ：前缀 `[名字] `、消息正文全文折行、markdown 照排（等于把转录搬窄）
    B 摘要     ：前缀照旧，消息只画**首行 + …**（其余进一个新详情）
    C 短前缀   ：前缀去方括号并截到 4 列，消息首行 + …，叙述行只留 `· ` 不带名字

跑：python3 .scratch/trace-tab/prototype/trace-pages.py
中文按 2 列算（真终端口径）；markdown 只做折行近似，不画表格与高亮。
这是 prototype，不要进 product。
"""

import unicodedata

WIDTHS = (40, 28)

# (种类, 发言者, 正文)。种类：inject / msg / think / tool / hook / end
SESSION = [
    ("inject", "注入", "AGENTS.md 每轮全文注入，共 42 行"),
    ("msg", "用户", "帮我修一下 render 的折行"),
    ("think", "助手", "思考"),
    ("msg", "助手", "问题在 wrap_pending：它在 push 时按宽度折，而宽度来自每帧的 view。"
                    "我改成在 ensure 时缓存折行结果，并按视图宽度分开缓存。"),
    ("tool", "助手", "read_file src/render/pane.rs"),
    ("tool", "助手", "edit_file src/render/pane.rs"),
    ("hook", "助手", "hook 拒绝：这条命令会写出工作区"),
    ("msg", "助手", "已修好。测试全绿：147 + 53。"),
    ("end", "助手", "回合结束：Completed"),
]


def cell(ch: str) -> int:
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


def dwidth(text: str) -> int:
    return sum(cell(ch) for ch in text)


def cut(text: str, width: int) -> str:
    out, used = "", 0
    for ch in text:
        step = cell(ch)
        if used + step > width:
            break
        out += ch
        used += step
    return out


def wrap(text: str, width: int, hanging: int = 0) -> list:
    """把一段折成若干行，续行缩进 hanging 列。"""
    rows, line, used = [], "", hanging
    prefix = " " * hanging
    for ch in text:
        step = cell(ch)
        if used + step > width:
            rows.append(line)
            line, used = prefix, hanging
        line += ch
        used += step
    rows.append(line)
    return rows


def pad(text: str, width: int) -> str:
    return text + " " * max(0, width - dwidth(text))


def prefix_for(speaker: str, variant: str) -> str:
    if variant == "C":
        return cut(speaker, 4)
    return f"[{speaker}]"


def body_first_line(text: str, room: int) -> str:
    head = cut(text, room)
    if dwidth(head) < dwidth(text):
        head = cut(head, room - 1) + "…"
    return head


def render(block, width: int, variant: str) -> list:
    kind, speaker, text = block
    pre = prefix_for(speaker, variant)
    if variant == "C" and kind in ("inject", "hook", "end"):
        pre = "·"
    lead = f"{pre} " if pre else ""
    if kind == "msg":
        room = width - dwidth(lead)
        if variant == "A":
            rows = wrap(text, width, hanging=dwidth(lead))
            rows[0] = lead + rows[0]
            return rows
        return [lead + body_first_line(text, room)]
    if kind == "tool":
        sep = "▸ "
    elif kind == "think":
        sep = "▸ "
    elif kind == "hook":
        return ["  " + cut(lead + text, width - 2)]
    else:
        sep = ""
    return [cut(lead + sep + text, width)]


def draw(title: str, width: int, lines: list) -> None:
    print(f"\n{title}")
    print("┌" + "─" * width + "┐")
    for line in lines:
        print("│" + pad(line, width) + "│")
    print("└" + "─" * width + "┘")


def main() -> None:
    print("PROTOTYPE — 轨迹视图 40 / 28 列三种画法（中文 2 列；markdown 只做折行近似）")
    for width in WIDTHS:
        for variant, name in (("A", "照抄（消息全文折行）"),
                              ("B", "摘要（消息首行 + …）"),
                              ("C", "短前缀 + 摘要 + 叙述行只剩 ·")):
            lines = []
            for block in SESSION:
                lines.extend(render(block, width, variant))
            draw(f"{width} 列｜变体 {variant}：{name}", width, lines)
    print("\n三种变体的差别只在两处：消息正文画几行、前缀占几列。")
    print("工具行与思考行在三种变体里都已经是**一行**（今天就是），叙述行也是。")


if __name__ == "__main__":
    main()
