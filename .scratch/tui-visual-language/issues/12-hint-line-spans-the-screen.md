# 12 — 提示行跨整屏、左栏让位、输入区去粗

Type: implement
Status: ready-for-walkthrough
Part of: ../map.md
Blocked by: 11

> 规格：[`../spec.md`](../spec.md) 实现决定 §16–§19。

## 目标

120 列屏上**六条键位提示全部看得见** —— 尤其是 `ctrl-o 左栏`：它按设计排在最末，所以到今天为止**从来没有出现过**（左栏自己的开关，是永远不出现的那一条）。代价明码标价：左栏页区少一行。

## 现状（改前先复核）

- 提示行拿到的是**主列宽**（`wording.rs` 那段注释自己写着：「宽屏带着左栏时提示行比屏窄 41 列」）。实测：120 列屏给 79 列 → 只显示 **4/6** 条；80 列屏给 51 列 → 只显示 **2/6** 条。
- **全部 6 条 + 出口 + 状态词要 111 列**，而 120 列屏只给 79 列。
- 左栏是**全高**的，左栏与主列之间那条竖虚线从屏幕顶贯通到底。
- 输入区整段 `BOLD`（`tui.rs` 的输入区绘制），注释没有解释为什么。

## 落点

- `src/render/layout.rs`：`Regions` 的提示行矩形、左栏的高度、`CHROME` 的账。
- `src/render/tui.rs`：`draw_shell` / `draw_sidebar` / `draw_divide`（竖虚线的收短）、输入区的样式。
- `src/render/wording.rs`：`hint_line` 的宽度来源与那段提示阶梯注释。

## 具体行为

1. **提示行的矩形改成整屏宽**。
2. **左栏画到提示行上一行为止**；左栏与主列之间那条竖虚线**同样收短**。**页区在 120×24 下从 15 行变 14 行**（页高的公式不变，变的只是剩余高度）。
3. **输入区草稿去掉整段 `BOLD`**：草稿归正文档，提示符 `❱` 仍是唯一的焦点。
4. 提示阶梯的数字与注释**重算重写** —— 「宽屏带着左栏时提示行比屏窄 41 列」那句作废。
5. **回改既有 spec**：[`sidebar-toggle`](../sidebar-toggle/spec.md) 与 [`trace-tab`](../trace-tab/spec.md) 写下的「全高左栏」；以及 `docs/render.md` 的外壳几何那几处。

## 验证

1. 逐格缓冲快照：120 列屏**六条提示全在**；80 列屏与 40 列屏各自的档位；左栏高度与竖虚线终点；草稿的样式不再带 `BOLD`。
2. `tests/render_layout.rs` 里与提示行宽度、左栏高度有关的断言按新事实改写。
3. `scripts/tui-startup-check.py` **不许红**（它的三个字形锚点本票一个都没动）。
4. 真终端：手工清单 **⑮ / ⑯ / ㉖**（左栏短一行的观感、提示行最末 `ctrl-o 左栏` 出不出现）。

## 不做什么

- 不改左栏的**宽度档**（40 / 28 / 隐藏）与 `Ctrl-O` 意愿那一层。
- 不改提示行的**键位顺序**（`ctrl-o 左栏` 仍排最末 —— 它现在只是真的看得见了）。
- 不动状态行（[11](11-palette-skeleton-and-status-row.md) 已经做完）。

## 实现记录（2026-10-06）

**已落地**（自动化部分全绿：`cargo test --all-targets` 1216 passed / 0 failed、`cargo clippy --all-targets` 与 `cargo fmt --check` 干净、`scripts/check-doc-size.py` 与 `check-language.py` 绿、`scripts/tui-startup-check.py` 15/15 绿）。

提示行跨整屏、左栏与竖虚线收短、页区 14 行、草稿去粗、提示阶梯重算，全在 `tests/render_layout.rs`。**真终端**：手工清单 ⑮ / ⑯ / ㉖。
