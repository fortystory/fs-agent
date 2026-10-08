#!/usr/bin/env python3
"""帧草图：左栏 `todo` 页与它的详情弹窗在两档宽度下长什么样（PROTOTYPE —— 验完即弃）。

这份脚本不回答任何设计问题，它只把 04 号票的问题变成能看的帧，好让维护者对着帧拍板。
取定值与被丢掉的备选收在那一票的 `## 作答` 里，不在脚本里。

一条命令重画全部五块：

    python3 .scratch/todo-page/prototype/frame.py

只看一块：`--only page|wrap|indent|detail|variants`。

几何全部对着 `src/render/` 的真实常量核过（核到 2026-10-09 的 `40ec32c`）。
折行是 [`pane::wrap_line`](../../../src/render/pane.rs) 的最小复刻：**按字符硬切、数的是列
不是字节、续行从第零列起** —— `/to-tickets` 在真机上就是这样被劈成两行的。
"""

from __future__ import annotations

import sys
import unicodedata

# --- 几何（出处：src/render/layout.rs）-----------------------------------
WIDE, NARROW = 40, 28              # :66 SIDEBAR_WIDE / :67 SIDEBAR_NARROW（≥120 / ≥80 起）
PAGE_WIDE_ROWS = 15                # 120x24：MarkCompact(5) + TAB(3) + page(15)，:555 的算式
PAGE_NARROW_ROWS = 19              # 80x24：Text(1) + TAB(3) + page(19)
PAGE_FLOOR_ROWS = 3                # :111 SIDEBAR_MIN_PAGE_ROWS —— 只出现在宽档 120x12
PAGE_MIN_NARROW = 5                # 80x10：窄档最矮（再矮过不了 MIN_HEIGHT）
DETAIL_WIDE, DETAIL_NARROW = 112, 72  # :607 DETAIL_MAX_WIDTH=135 = min(屏宽-4,135)-边框2-内边2

SEP = " · "        # wording.rs:1951 ——「并列两段之间」那一个
DONE_MARK = "~"    # 帧里代表真机上的 MUTED 降暗（票 02 第 6 条：色板里没有 dim 这一档）

# 状态字形：`▸` 今天与转录里可折叠块的标记同形（wording.rs:1907），本票挑一个替身。
# `●` 是 2026-10-09 维护者当场挑的（备选：`◐` `▓`）。它与 `☐` `✓` 同属 Ambiguous 宽度，
# 帧里按一列算 —— 真机上要与 `☐` 一起核一遍同一个字体的显示宽度。
GLYPH = {"pending": "☐", "in_progress": "●", "completed": "✓"}


# --- 显示列 ---------------------------------------------------------------
def cw(ch: str) -> int:
    """一列还是两列。Ambiguous（`☐` `·` 这类）按一列算 —— 与本仓库现状一致。"""
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


def dw(text: str) -> int:
    return sum(cw(ch) for ch in text)


def wrap(text: str, width: int) -> list[str]:
    """按字符硬切到显示列宽（`pane::wrap_line` 的最小复刻：不保词）。"""
    lines, cur, used = [], "", 0
    for ch in text:
        step = cw(ch)
        if used + step > width and used > 0:
            lines.append(cur)
            cur, used = ch, step
        else:
            cur += ch
            used += step
    lines.append(cur)
    return lines


def pad(text: str, width: int) -> str:
    return text + " " * max(0, width - dw(text))


# --- 一行条目 -------------------------------------------------------------
def head_of(item: dict, mode: str) -> str:
    """一条的前缀。`aligned` 是推荐口径：照抄文件页「字形列每行都占满两格」那条规矩，
    再把 id 位补满，于是带 id 与不带 id 的两行内容严格同列。"""
    glyph = GLYPH[item["status"]]
    ident = item.get("id")
    if mode == "aligned":
        return f"{glyph} {(ident or '  ')} "
    if mode == "tight":
        return f"{glyph} {(ident + ' ') if ident else ''}"
    return f"{glyph} "  # no-id


def item_lines(item: dict, width: int, mode: str = "aligned", indent: bool = True) -> list[str]:
    """一条占几行（折行！）。真机上完成项的每一行都套 MUTED，帧里前面加 DONE_MARK。"""
    head = head_of(item, mode)
    body = wrap(item["content"], width - dw(head))
    cont = " " * dw(head) if indent else ""
    lines = [head + body[0]] + [cont + part for part in body[1:]]
    if item["status"] == "completed":
        lines = [DONE_MARK + line for line in lines]
    return lines


def ordered(items: list[dict]) -> list[dict]:
    """`in_progress` 置顶，其余保持提交时的原顺序（冻结项 4）。"""
    busy = [i for i in items if i["status"] == "in_progress"]
    rest = [i for i in items if i["status"] != "in_progress"]
    return busy + rest


# --- 页 -------------------------------------------------------------------
def progress_text(items: list[dict]) -> str:
    done = sum(1 for i in items if i["status"] == "completed")
    busy = sum(1 for i in items if i["status"] == "in_progress")
    head = f"{done}/{len(items)}"
    if busy:
        return head + SEP + f"{busy} 个在做"
    if items and done == len(items):
        return head + SEP + "完成"
    return head


def page_rows(
    items: list[dict],
    width: int,
    height: int,
    *,
    button: str = "详情",
    mode: str = "aligned",
    indent: bool = True,
    overflow: str = "＋{hidden} 项",
) -> list[str]:
    """页从上到下的行。放不下时末行报溢出（冻结项 4：不滚动）。"""
    head = progress_text(items)
    top = pad(head, max(1, width - dw(button))) + button if button else pad(head, width)
    rows = [top[:width]]

    todo = [i for i in items if i["status"] != "completed"]  # 完成项折掉
    room = height - 1
    lines = [line for i in ordered(todo) for line in item_lines(i, width, mode, indent)]
    if len(lines) <= room:
        rows += lines
    else:
        shown, used, take = [], 0, 0
        for item in ordered(todo):
            need = item_lines(item, width, mode, indent)
            if used + len(need) <= room - 1:  # 留一行报溢出
                shown += need
                used += len(need)
                take += 1
            else:
                break
        rows += shown + [overflow.format(hidden=len(todo) - take)]
    return rows[:height]


# --- 弹窗 -----------------------------------------------------------------
def detail_title(name: str, width: int, footer: str = "esc 关闭") -> str:
    """照 `wording::detail_section` 的 `── 名字 ──` 一族（wording.rs:351）。"""
    left = f"── {name} "
    right = f" {footer} "
    return pad(left + "─" * max(0, width - dw(left) - dw(right)) + right, width)


def detail_rows(
    items: list[dict],
    width: int,
    *,
    split_done: bool = False,
    indent: bool = True,
    heading: str = "── 已完成 ──",
) -> list[str]:
    """弹窗正文：进度行 + 进行中 + 待做 + 已完成（MUTED）。折行归调用方那条规矩照旧。"""
    rows = [detail_title("待办", width), progress_text(items), ""]
    for item in ordered([i for i in items if i["status"] != "completed"]):
        rows += item_lines(item, width, "aligned", indent)
    done = [i for i in items if i["status"] == "completed"]
    if done:
        rows.append("")
        if split_done:
            rows.append(heading)
        for item in done:
            rows += item_lines(item, width, "aligned", indent)
    return rows


# --- 打印 -----------------------------------------------------------------
def frame(title: str, width: int, rows: list[str], indent: str = "   ") -> None:
    print(f"{indent}┌─ {title} ── {width} 列 ─────────────┐")
    for row in rows:
        print(f"{indent}│{pad(row, width)}│")
    print(f"{indent}└{'─' * width}┘")


def legend() -> None:
    print(f"   {DONE_MARK} = 真机上那一行套 MUTED（色板里没有 dim，落点见票 02 第 6 条）")


def sample() -> list[dict]:
    return [
        # 提交顺序。注意第 5 条被标成 in_progress —— 页上要置顶。
        {"id": None, "status": "completed", "content": "摸清仓库现状：tracker 约定、现有 todo 设计与 spec"},
        {"id": "02", "status": "in_progress", "content": "接上命中区域与一种新详情，量清每条要动的行数与几处"},
        {"id": "03", "status": "pending", "content": "别的 coding agent 怎么呈现计划列表"},
        {"id": "04", "status": "pending", "content": "两档宽度下页与弹窗的帧草图"},
        {"id": "05", "status": "pending", "content": "极小页高与三种空态"},
        {"id": None, "status": "completed", "content": "两轮 grilling"},
        {"id": "06", "status": "pending", "content": "指针落点与覆盖层打架"},
        {"id": "07", "status": "pending", "content": "多 speaker 时页上那一份是谁的"},
        {"id": None, "status": "pending", "content": "折成 spec，再 /to-tickets 拆实现票"},
        {"id": None, "status": "pending", "content": "实现：进度行、折行、折叠完成项、弹窗（这一条故意写得很长，用来量 28 列下折几行）"},
        {"id": "10", "status": "pending", "content": "真机走查与收口"},
    ]


LONG = {"id": "11", "status": "pending", "content": "六十字" * 40}


def block_page() -> None:
    print("■ 页（推荐排版：前缀严格同列 + 按钮 `详情` + 续行缩进 + 完成项折掉）")
    items = sample()
    frame("120x24 · 宽档", WIDE, page_rows(items, WIDE, PAGE_WIDE_ROWS))
    frame("80x24 · 窄档", NARROW, page_rows(items, NARROW, PAGE_NARROW_ROWS))
    legend()


def block_wrap() -> None:
    print("■ 折行账：一条内容在两档下占几行（`wrap_line` 按字符硬切，不保词）")
    print(f"   {'':12}{'窄档 28 列':>22}{'宽档 40 列':>22}")
    for name, text in (
        ("短", "补测试"),
        ("中", sample()[1]["content"]),
        ("长", sample()[9]["content"]),
        ("更长", LONG["content"]),
    ):
        cells = []
        for width in (NARROW, WIDE):
            body = width - 5  # aligned 前缀 5 列
            cells.append(f"{len(wrap(text, body))} 行 / 正文 {body} 列")
        label = f"{name}（{len(text)} 字）"
        print(f"   {label:16}" + "".join(f"{cell:>22}" for cell in cells))
    print("   前缀 aligned = 字形 1 + 空格 1 + id 2 + 空格 1 = 5 列")


def block_indent() -> None:
    print("■ 续行缩进还是顶格 —— 这一条撞着 pane.rs:442 的既有纪律")
    print("   「续行从第零列起 —— 窗格是一份日志，缩进会主张一种折行后的文字并没有的结构」")
    items = [sample()[1]]
    for indent, label in ((True, "缩进到内容列（冻结项 5 的字面）"), (False, "续行顶格（wrap_line 的纪律）")):
        print(f"   · {label}")
        for row in item_lines(items[0], NARROW, "aligned", indent):
            print("       " + row)


def block_detail() -> None:
    print("■ 弹窗（正文折行归调用方；72 列是 80x24 的真实宽度）")
    items = sample()
    frame("80x24 · 正文 72 列", DETAIL_NARROW, detail_rows(items, DETAIL_NARROW))
    print(f"   （120x24 下正文 {DETAIL_WIDE} 列：同一份清单占 "
          f"{len(detail_rows(items, DETAIL_WIDE))} 行）")
    legend()


def block_variants() -> None:
    print("■ 变体对照（页一律取 28 列，只看差别）")
    items = sample()
    bare = {"id": None, "status": "pending", "content": "折成 spec，再 /to-tickets 拆实现票"}
    numbered = {"id": "07", "status": "pending", "content": "多 speaker 时页上那一份是谁的"}

    print("   A. 前缀口径（内容从第几列起）")
    for mode, label in (("aligned", "aligned：字形 + id 位补满，内容第 5 列起"),
                        ("tight", "tight：有 id 才占位，内容第 3 或 6 列起"),
                        ("no-id", "no-id：不留 id 列，内容第 3 列起（丢掉凭据）")):
        print(f"     · {label}")
        for row in item_lines(numbered, NARROW, mode) + item_lines(bare, NARROW, mode):
            print("       " + row)

    print("   B. 按钮的三个退路（窄档 28 列，进度文案占 16 列）")
    for button, label in (("详情", "退路 0：`详情`"),
                          ("···", "退路 1：只用一个 ASCII 记号"),
                          ("", "退路 2：没有按钮，整页可点"),
                          ("2/11", "退路 3：换更短的进度文案")):
        head = "2/11 · 1 个在做" if button != "2/11" else "2/11"
        gap = NARROW - dw(head) - dw(button)
        print(f"     · {label}：文案 {dw(head)} + 间隙 {max(0, gap)} + 按钮 {dw(button)} = {NARROW} 列")

    print("   C. 弹窗里完成项：连续降暗 vs 另起一段（只看完成项那一段）")
    joined = detail_rows(items, DETAIL_NARROW)
    split = detail_rows(items, DETAIL_NARROW, split_done=True)
    for rows, label in ((joined, "连续（推荐）"), (split, "另起一段")):
        cut = next(i for i, row in enumerate(rows) if row.startswith(DONE_MARK + "✓") or "── 已完成" in row)
        print(f"     · {label}")
        for row in rows[cut - 2:cut + 4]:
            print("       " + row)

    print("   D. 极小页区（3 行地板只在宽档 120x12；窄档最矮 80x10）")
    frame("120x12 · 40x3", WIDE, page_rows(items, WIDE, PAGE_FLOOR_ROWS))
    frame("80x10 · 28x5", NARROW, page_rows(items, NARROW, PAGE_MIN_NARROW))


BLOCKS = {
    "page": block_page,
    "wrap": block_wrap,
    "indent": block_indent,
    "detail": block_detail,
    "variants": block_variants,
}


def main(argv: list[str]) -> int:
    names = list(BLOCKS)
    if len(argv) > 1:
        if len(argv) != 3 or argv[1] not in ("--only", "only") or argv[2] not in BLOCKS:
            print(f"usage: {argv[0]} [--only {'|'.join(BLOCKS)}]")
            return 2
        names = [argv[2]]
    for name in names:
        BLOCKS[name]()
        print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))