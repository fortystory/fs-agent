# 02 — 提示行回到输入框下：只跨主列，左栏恢复全高

Type: implement
Status: ready-for-agent
Part of: ../spec.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §2。

## 目标

提示行（键位提示 + 出口）不再跨整屏，而是与输入区同列同宽、就在它正下方。左栏因此恢复全高、
分隔竖线贯通到屏幕底。用户看得见的是：左栏宽档下提示从输入框那一列起（不再从第 0 列起），
120×24 的左栏页区多回一行。

## 现状（改前先复核）

- `src/render/layout.rs:354` 的 `hints = Rect::new(area.x, input.bottom() + 1, area.width, 1)`
  是「跨整屏」那一条（`tui-visual-language` §16、决策 27）。
- 同文件 `plan` 里 `sidebar_rows = area.height.saturating_sub(SIDEBAR_TOP_GAP + 1)` —— 那个 `+ 1`
  就是让给提示行的。
- `src/render/tui.rs` 的 `draw_divide` 画分隔列到 `panes.hints.y` 为止；`draw_shell` 里
  `for y in [panes.input.y - 1, panes.hints.y - 1]` 两条横线从 `left`（分隔列右边一格）画到
  `area.right()`。
- 会红的断言：`tests/render_layout.rs:256`（竖线画到提示行上一行）、`:689`（提示行宽度 = 终端
  宽度）、`:1221` 与 `:2294`（左栏阶梯把提示行那一行让出来）、`:593` 起（40×10 的提示行位置）。

## 落点

`src/render/layout.rs`（`hints`、`sidebar_rows` 与它们上方的注释）、`src/render/tui.rs`
（`draw_divide` 的终点、`draw_shell` 两条横线的终点）、`tests/render_layout.rs`。

## 具体行为

1. `hints` 改成 `Rect::new(main.x, input.bottom() + 1, main.width, 1)`：起止列与 `input` 完全
   一致（两者都从 `main` 推，所以不会各算一遍）。
2. `sidebar_rows` 恢复成 `area.height - SIDEBAR_TOP_GAP`：左栏画到屏幕最后一行。注释里那笔
   「底下还要让出提示行那一行」删掉，改写成这次的结论（120×24 页区 14 → 15）。
3. `draw_divide` 画到 `area.bottom()`（原先是 `panes.hints.y`）。
4. `draw_shell` 里两条横线改成从主列左缘画到主列右缘（`panes.main.x .. panes.main.right()`），
   不再从分隔列右边一格起：左栏现在与它们无关。左栏不存在时主列就是整屏，行为与今天一致。
5. `CHROME` 不改（提示行照样花一行），问卷的页脚与回执跟着矩形一起搬家，代码不用动。

## 验收

- 一帧 120×24：提示行（含 `enter 发送` 的那一行）的起始列等于输入区提示符所在那一列；提示行
  的宽度等于输入区宽度。
- 同帧：分隔列的 `┆` 画到**最后一行**（屏幕底），第 22 行与第 23 行都有。
- 左栏页区在 120×24 下是 15 行（用现成的行数 helper 量，不写死在新断言里）。
- 40×10 那一档：输入区仍拿满三行、转录仍有一行、提示行读起来仍是出口那句。
- 问卷（`/ask` 或权限询问）期间页脚仍画在提示行上、按钮仍点得中（既有点击断言照旧绿）。
- `cargo test` 全绿（本票会改掉上面列的那几条断言）。
