# 提示行最末添一条 `ctrl-o 左栏`

Type: implement
Status: done
Blocked by: 01

> 规格：[`.scratch/sidebar-toggle/spec.md`](../spec.md) §4、`Testing Decisions`。
> 键位与几何归 [票 01](01-toggle-state-and-geometry.md)；本票只动措辞层与它的档位断言。
> **必须等票 01 落地**：在 `Ctrl-O` 真的能用之前，这条提示是在承诺一个什么都不做的键 ——
> 那是提示行绝不能做的事（`wording::EXIT_HINT_BUSY` 的 rustdoc 写着这条纪律）。

## 目标

`Ctrl-O` 有常驻可发现性：`KEY_HINTS` 末尾加一条 `ctrl-o 左栏`，其余五条的顺序与文案一个字不动。

## 现状（2026-10-02 量的，改前先复核）

- `KEY_HINTS` 在 `src/render/wording.rs:1057`，现在是 5 条：`enter 发送` / `ctrl-j 换行` /
  `esc 取消` / `shift+tab 模式` / `PgUp/PgDn 滚动`。
- 阶梯算法是 `hint_line`（`:1140`）：**从前往后填、超宽就停**，出口 `ctrl-c/ctrl-d 退出` 预留，
  状态词只在还装得下时才摆最前 —— 所以**位置就是优先级**，最末那条最先被窄档丢掉。
- 档位实测记在 `status_line` 的 docstring（`:1099-1102`）与 `tests/wording.rs:403`
  （`the_status_line_keeps_the_way_out_and_gives_up_the_state_word_when_narrow`）。
- `VIEWER_HINTS` 在 `:1091`（`esc 取消` / `PgUp/PgDn 滚动`）—— **本票不动它**。
- 反例参考：`[`seed.md`](../seed.md)` 里那处列宽推算已明确作废，别照抄。

## 落点

`src/render/wording.rs`、`tests/wording.rs`。必要时只改注释的 `scripts/tui-startup-check.py`。

## 具体行为

1. `KEY_HINTS` 改成 `[&str; 6]`，**末尾**追加 `"ctrl-o 左栏"`。文案写「左栏」不写「侧栏」
   （`CONTEXT.md` 的左栏词条把「侧栏」列在 `_Avoid_`）。
2. **不随状态换字**：收起时也照旧是 `ctrl-o 左栏`，不换成「显示左栏」。
3. **重取档位数字**：跑实测（`cargo test` 里那组断言或一个临时打印），把 `status_line` 的
   docstring 与断言里的档位更新成 6 条清单下的真实值 —— **不许沿用推算值**。
4. **不加进 `VIEWER_HINTS`**：一条键位只在一处被提示。

## 验证

`cargo test`（`tests/wording.rs`）+ `cargo clippy` + `cargo fmt`：

1. 最宽档位里 `ctrl-o 左栏` **出现**；中档 / 窄档里**不出现**（位置即优先级）。
2. 其余五条与出口的可见性与顺序**一个字不变**（除「最末那条挤掉了什么」这件事实本身）。
3. **`scripts/tui-startup-check.py` 先原样跑一遍**：它跑 260×30 宽屏，若脚本里有「提示行逐字
   相等」的断言就跟着改；没有就一行不动（别预改）。
4. 手工面写进 [票 03](03-docs-and-sweep.md) 的那一节（提示行在真终端里的可读性）。

## 实现完成（2026-10-02）

- `KEY_HINTS` 变成 `[&str; 6]`，末尾追加 `"ctrl-o 左栏"`；docstring 补上「位置就是优先级」。
- `status_line` 的阶梯 docstring 按实测重取：40 列三个条目、60 列四个、80 列六个、120 列八个
  （`ctrl-o 左栏` 只在最宽档出现）。
- `tests/wording.rs`：新增 `the_sidebar_switch_is_hinted_at_the_end_of_the_line`；
  `the_hint_ladder_is_the_one_the_prototype_measured` 的 `full` 串与 200/120/174 的读数按实测
  更新，而 40 / 60 / 80 三档**一个都没动** —— 那正是「排最末」的证据。
- `tests/render_layout.rs` 的渲染侧阶梯
  （`the_hint_row_gives_up_hints_before_it_gives_up_the_way_out`）：174 列的条目数 7 → 8，
  并新增一条「左栏开关是这一档多出来的那一条」。
- `scripts/tui-startup-check.py` **一行没动**：它的锚点是 `STATUS_ANCHOR = "ctrl-c"` 与
  `MARK_ROW`，都不受这条提示影响。**没有在真 pty 上重跑** —— 那一步归票 03 的走查。
