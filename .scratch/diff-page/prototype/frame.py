#!/usr/bin/env python3
"""throwaway prototype：左栏「改动」页在两档宽度下的帧草图。

回答的是「长什么样」，不是「怎么实现」：列宽、截断、字形列、组标题、弹窗里的行号。
它按仓库里的真实几何算（数字与出处写在常量旁边），不是估的。

    python3 .scratch/diff-page/prototype/frame.py

用到的机械事实：
- 左栏三档宽度 `SIDEBAR_WIDE=40` / `SIDEBAR_NARROW=28`（`src/render/layout.rs:66-67`）
- 页区起点 = `identity.rows() + TAB_ROWS(3)`，页高 = 内容高 - 那一截（`layout.rs:462,533`）
- 120×24 宽档：块字收起来那版占 `LOGO_COMPACT_ROWS=5` 行 ⇒ 页区 40×15（`layout.rs:94,533` 的算式）
- 80×24 窄档：文字身份 `IDENTITY_ROWS=1` 行 ⇒ 页区 28×19（`layout.rs:105`）
- 详情覆盖层：框宽 = 屏宽 - `MODAL_MARGIN(4)`，封顶 `DETAIL_MAX_WIDTH(135)`；
  正文宽 = 框宽 - 2（边框）- 2（内边距）（`layout.rs:233-238,592-606`）
  120 列屏 ⇒ 框 116、正文 112；80 列屏 ⇒ 框 76、正文 72
- 中文按 2 列（`src/render/width.rs`）
"""

from __future__ import annotations

import unicodedata

# ── 假数据：一份典型的改动集（含长路径、中文路径、目录级未跟踪）─────────────────
CHANGES = [
    ("M", "src/render/tui.rs"),
    ("M", "docs/render.md"),
    ("M", "src/render/wording.rs"),
    ("A", "src/render/diff_page.rs"),
    ("D", "src/render/old_panel.rs"),
    ("??", ".scratch/diff-page/map.md"),
    ("??", "scripts/tmp_notes.txt"),
    ("??", "笔记/一次真机走查.md"),
]

GROUPS = [("已修改", "M"), ("新增", "A"), ("已删除", "D"), ("未跟踪", "??")]

# 页签条那一行（charting 已定的实算：四签在 28 列下占 21 列）
TABS = "调用量 ┆ todo ┆ 文件 ┆ 改动"


def cells(text: str) -> int:
    """这段文本占几列。"""
    return sum(2 if unicodedata.east_asian_width(ch) in "WF" else 1 for ch in text)


def fit(text: str, cols: int) -> str:
    """按列宽截断，尾部用 `…` 收尾（与文件页「名字超宽用 … 收尾」同一条口径）。"""
    if cells(text) <= cols:
        return text
    kept, used = "", 0
    for ch in text:
        step = 2 if unicodedata.east_asian_width(ch) in "WF" else 1
        if used + step + 1 > cols:  # 留一格给 …
            break
        kept, used = kept + ch, used + step
    return kept + "…"


def rule(cols: int) -> str:
    return "·" + "─" * (cols + 2) + "·"


def list_frame(cols: int, rows: int, path_style: str, glyph_cols: int) -> str:
    """页区一帧。`path_style` ∈ {"full","base"}；`glyph_cols` ∈ {1,2}。"""
    out = [f"页区 {cols} 列 × {rows} 行｜页签条：{fit(TABS, cols)}", rule(cols)]
    used = 0
    body = []
    for title, mark in GROUPS:
        rows_in = [p for m, p in CHANGES if m == mark]
        if not rows_in:
            continue
        body.append(fit(title, cols))
        for path in rows_in:
            name = path.split("/")[-1] if path_style == "base" else path
            glyph = path if False else ("??" if mark == "??" else mark)
            pad = " " * (glyph_cols - cells(glyph)) + " "
            body.append(fit(glyph + pad + name, cols))
    overflow = max(0, len(body) - rows)
    for line in body[:rows]:
        out.append("│" + line + " " * (cols - cells(line)) + "│")
        used += 1
    for _ in range(rows - used):
        out.append("│" + " " * cols + "│")
    if overflow:
        out.append(f"（还有 {overflow} 行没画出来 —— 这一页今天没有滚动）")
    return "\n".join(out)


DIFF = """diff --git a/src/render/diff_page.rs b/src/render/diff_page.rs
index 3f1a2b4..9c8d7e6 100644
--- a/src/render/diff_page.rs
+++ b/src/render/diff_page.rs
@@ -12,7 +12,9 @@ fn draw(&self, page: Rect) {
     let Some(page) = page else { return };
     let rows = match self.tab {
         Tab::Usage => self.panel.lines(page),
-        Tab::Files => self.files.lines(page),
+        Tab::Diff => self.diff.lines(page),
+        Tab::Files => self.files.lines(page),
     };
     note_rows(state, page, &rows, &[]);
 }
"""


def detail_frame(text_width: int, header_lines: bool) -> str:
    """弹窗正文一帧：行号列 + `+`/`-` 标记 + 内容。"""
    body = DIFF.splitlines()
    if not header_lines:
        body = [l for l in body if not l.startswith(("diff --git", "index ", "--- ", "+++ "))]
    # 行号：hunk 头 `@@ -a,b +c,d @@` 给两个区间；新增/上下文取新号，删除取旧号。
    old = new = 0
    numbered: list[tuple[str, str]] = []
    widest = 1
    for line in body:
        # 头行不给行号（它们不属于任何一侧的文件），也不剥首字符。
        if line.startswith(("diff --git", "index ", "--- ", "+++ ")):
            numbered.append(("-", line))
            continue
        tag = line[:1]
        # 整行原样画（`+`/`-`/空格那个首字符是 diff_tag 的判据，也是人眼的第一条线索）。
        if line.startswith("@@"):
            spec = line.split("@@")[1].strip()
            old = int(spec.split(" ")[0][1:].split(",")[0])
            new = int(spec.split(" ")[1][1:].split(",")[0])
            numbered.append(("-", line))
            continue
        if tag == "+":
            numbered.append((str(new), line)); new += 1
        elif tag == "-":
            numbered.append((str(old), line)); old += 1
        else:
            numbered.append((str(new), line)); old += 1; new += 1
        widest = max(widest, len(numbered[-1][0]))
    num_cols = widest + 1
    out = [f"正文 {text_width} 列｜行号列 {num_cols} 列（最长行号 {widest} 位 + 1）"]
    for number, text in numbered:
        prefix = " " * num_cols + " " if number == "-" else number.ljust(num_cols) + " "
        out.append(fit(prefix + text, text_width))
    return "\n".join(out)


def main() -> None:
    for cols, rows, tier in ((40, 15, "宽档 120×24"), (28, 19, "窄档 80×24")):
        for style in ("full", "base"):
            print(f"\n══ {tier} · 路径={style} · 字形列 2 格 " + "═" * 20)
            print(list_frame(cols, rows, style, 2))
    print("\n══ 40 列 · 字形列只占 1 格（`??` 撑破）" + "═" * 20)
    print(list_frame(40, 15, "full", 1))

    print("\n══ 弹窗正文" + "═" * 40)
    for width in (112, 72):
        print(f"\n-- 正文宽 {width} 列 · 留头行 --")
        print(detail_frame(width, True))
    print("\n-- 正文宽 112 列 · 不留头行 --")
    print(detail_frame(112, False))

    print("\n══ 截断口径对照（28 列下名字拿 25 列：28 - 字形 2 - 空格 1）" + "═" * 10)
    for path in [p for _, p in CHANGES]:
        print(f"  全路径 {fit(path, 25):<28} 基名 {fit(path.split('/')[-1], 25)}")


if __name__ == "__main__":
    main()
