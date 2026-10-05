#!/usr/bin/env python3
"""PROTOTYPE（throwaway）—— 左栏页高"撑满"的三档帧，含"今天 vs 决定后"两套阶梯对照。

要回答的问题：`sidebar_page.height` 从 `fields`（恒 6、地板 3）改成「撑满左栏剩余高度」
之后，调用量 / todo / 轨迹三页在 120×24、80×24、120×10、80×10 各档长什么样。

决定（2026-10-05，HITL）：
  * 页区**贴顶**，高度 = 内容行 − 身份行 − 页签条；
  * `SIDEBAR_FIELDS = 6` 不再决定页高，它那个数降级成阶梯地板 `SIDEBAR_MIN_FIELDS = 3`。

跑：

    python3 .scratch/trace-tab/prototype/frames.py

几何数字全部来自 `src/render/layout.rs` 的常量（`SIDEBAR_TOP_GAP=1`、`LOGO_ROWS=5`、
`IDENTITY_ROWS=1`、`TAB_ROWS=3`、`SIDEBAR_FIELDS=6`、`SIDEBAR_MIN_FIELDS=3`、
`LOGO_WIDTH=38`、两档 40 / 28、门槛 120 / 80）。框里的中文按 1 列画（真终端占 2 列），
所以这是**形状草稿**，不是像素稿。这是 prototype，不要进 product。
"""

TOP_GAP, LOGO_ROWS, IDENTITY_ROWS, TAB_ROWS = 1, 5, 1, 3
WIDE, NARROW, WIDE_FROM, NARROW_FROM, LOGO_WIDTH = 40, 28, 120, 80, 38
FIELDS, MIN_FIELDS = 6, 3

USAGE_ROWS = ["上下文   12,345 / 200,000（6%）", "token      45,678 / 100万", "回合                     12",
              "输入                 23,456", "输出                 22,222", "缓存                 10,000"]
TODO_ITEMS = ["☐ 01 拆出两个 Pane", "▸ 02 每视口一个绘制宽度", "☐ 03 点击与滚轮分派",
              "✓ 04 rail 只绑对话 pane", "☐ 05 过滤接上", "☐ 06 页高解耦",
              "☐ 07 轨迹页排版", "☐ 08 历史重播跟随"]


def tier_of(width: int):
    if width >= WIDE_FROM:
        return WIDE
    if width >= NARROW_FROM:
        return NARROW
    return None


def sidebar_kind(tier, content_rows: int, floor: int):
    """复刻 `layout::sidebar_content` 那条阶梯：身份先让，然后（今天的版本）逐字段让到 floor。"""
    if tier is None:
        return 0, floor
    kind = LOGO_ROWS if tier >= LOGO_WIDTH else IDENTITY_ROWS
    fields = floor
    while kind + TAB_ROWS + fields > content_rows:
        if kind == LOGO_ROWS:
            kind = IDENTITY_ROWS
        elif kind == IDENTITY_ROWS:
            kind = 0
        elif fields > MIN_FIELDS:
            fields -= 1
        else:
            break
    return kind, fields


def usage_lines(room: int):
    return [row for row in USAGE_ROWS[:room]] + ["" for _ in range(max(0, room - len(USAGE_ROWS)))]


def todo_lines(room: int):
    if room == 0:
        return []
    if room == 1:
        return ["已完成 4/8"]
    item_rows = room - 1
    out = []
    if len(TODO_ITEMS) <= item_rows:
        out.extend(TODO_ITEMS)
    else:
        out.extend(TODO_ITEMS[: item_rows - 1])
        out.append("＋%d 项" % (len(TODO_ITEMS) - (item_rows - 1)))
    out.append("已完成 4/8")
    return out


def trace_lines(room: int):
    return ["此页尚未实现（另有票在跟）"] + ["" for _ in range(room - 1)]


def draw(title, width, lines):
    print(f"\n{title}")
    print("  ┌" + "─" * (width + 2) + "┐")
    for text in lines:
        print(f"  │ {text:<{width}} │")
    print("  └" + "─" * (width + 2) + "┘")


def report(screen_w, screen_h):
    tier = tier_of(screen_w)
    content_rows = screen_h - TOP_GAP
    today_kind, today_fields = sidebar_kind(tier, content_rows, FIELDS)
    new_kind, _ = sidebar_kind(tier, content_rows, MIN_FIELDS)
    today_h = today_fields if tier is not None else 0
    new_h = content_rows - new_kind - TAB_ROWS if tier is not None else 0
    print(f"\n{'=' * 78}")
    print(f"终端 {screen_w}×{screen_h}｜左栏内容宽 {tier}｜内容行 {content_rows}")
    if tier is None:
        print("左栏整栏不存在（< 80 或 Ctrl-O 收起）→ 对话视图退回全量")
        return
    print(f"页区 y：今天 {TOP_GAP + today_kind + TAB_ROWS} → 决定后 {TOP_GAP + new_kind + TAB_ROWS}"
          f"（身份 {today_kind} 行 → {new_kind} 行）")
    print(f"页区高度：今天 {today_h} 行（= fields）→ 决定后 {new_h} 行（差 {new_h - today_h:+d}）")
    print(f"{'=' * 78}")
    draw(f"调用量页（{new_h} 行，贴顶）", tier, usage_lines(new_h))
    draw(f"todo 页（{new_h} 行，条目拿 {max(0, new_h - 1)} 行 + 计数行）", tier, todo_lines(new_h))
    draw(f"轨迹页（{new_h} 行，内部排版归票 01）", tier, trace_lines(new_h))


def main():
    print("PROTOTYPE — 左栏页高撑满的三档帧（中文按 1 列画，形状草稿）")
    print("今天 = sidebar_page.height == fields(6)；决定后 = 内容行 − 身份 − 页签条，地板 3")
    for screen in [(120, 24), (80, 24), (120, 10), (80, 10), (79, 24)]:
        report(*screen)
    print("\n充裕档是唯一真变化的档：120×24 6→15、80×24 6→19。")
    print("极矮档（120×10 / 80×10）决定后**保住文字身份**、页区 6→5（丢的正是最先让的「缓存」）；")
    print("MIN_HEIGHT 之下（80×9 之类）到不了这一支。79×24 整栏不存在。")


if __name__ == "__main__":
    main()
