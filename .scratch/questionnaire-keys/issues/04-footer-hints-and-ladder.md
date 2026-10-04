# 04 — 问卷页脚：键位提示、举手回执与三档宽度阶梯

Type: implement
Status: done
Blocked by: 03

> 来源：[`../spec.md`](../spec.md) §6。它同时补上一条既有的手感缺口：问卷期间 `Ctrl-C` 举起的
> 那只手**今天是看不见的**。

## 目标

- **排版**（`draw_questionnaire_footer`，[:3845-3904](../../../src/render/tui.rs)）：
  `1 / 3   ←上一题  下一题→  提交   j/k 移动 · 空格选中 · esc 退出询问`。
  可点按钮**紧跟进度**（`hint_region` 越界就丢掉点击区域，[:3907-3915](../../../src/render/tui.rs)），
  键位提示垫最后。
- **举手期间最后那一段被替换**（不是追加）：「再按一次 esc 退出询问」 /
  「已取消 · 再按一次 ctrl-c 退出」——与 exit-gesture §2「替换出口段」同构。
  **这补上了今天那个缺口**：`draw_bottom` 在问卷分支提前 return（[:3716-3731](../../../src/render/tui.rs)），
  页脚 render 到 `panes.hints`（[:3903](../../../src/render/tui.rs)），提示行整行被替换，
  所以问卷期间 `Ctrl-C` 的举手没有回执。
- **新造一条三档宽度阶梯**（今天没有），按「先丢教学性的」排序：
  1. 丢 Emacs 别名（`ctrl-n/ctrl-p`）；
  2. 键位提示砍到只剩 `esc 退出询问`；
  3. 只留进度与按钮。
  **举手回执在任何宽度下都保**。
- 文案进 [`src/render/wording.rs`](../../../src/render/wording.rs)（`questionnaire_*` 一族）。

## 现状（2026-10-02 核实，改前先复核）

- 页脚今天只有「进度 + 三个 availability-gated 按钮」，**没有宽度阶梯**；有阶梯的是普通状态行的
  `hint_line`（[`src/render/wording.rs:1121-1144`](../../../src/render/wording.rs)，40/60/80/120 四档），
  可以照它写，但别接错对象。
- 进度文案是 `wording::questionnaire_progress`（[`src/render/wording.rs:839-841`](../../../src/render/wording.rs)），
  今天画在 hints 行开头（[:3903](../../../src/render/tui.rs)）。
- 举手状态由票 03 引入（同一槽位 + `Gesture` 标签）；本票只读它。

## 测试

- [`tests/render_layout.rs`](../../../tests/render_layout.rs)（今天问卷页脚的两条在 `:4169`、`:4418`）：
  - 三档宽度下各自画出什么、丢掉什么，且**举手回执在每一档都在**；
  - 举手期间那一段是回执而不是键位提示；
  - 键位提示加进来之后，三个可点按钮的命中区域仍与画出来的一致。
- [`tests/wording.rs`](../../../tests/wording.rs)：键位提示与两句举手回执的文案。
- `cargo test` 全绿、`cargo clippy --all-targets` 干净。

## 不做什么

- 不动提示行本身（那是全局的，问卷外 `esc 取消` 仍然对）。
- 不动按钮的可用性规则与它们的点击动作。
- 不给窗口加可配时长（票 03 的窗口是常量）。

## 评论

- **落地（2026-10-02）**：`wording::questionnaire_status` 与 `questionnaire_hint(ready)` 删掉 ——
  它们只被测试引用，TUI 页脚今天压根不画键位提示。换成 `questionnaire_hint(room)`（三档：
  完整含 Emacs 别名 / 去掉别名 / 只剩 `esc 退出询问` / 整段不画）与两条举手回执
  （`questionnaire_decline_raised`、`questionnaire_exit_raised`）。阈值是 48 / 28 / 12 列，
  量的是**留给提示的列数**（第一版按 96/72/40 定得太苛刻，宽终端反而看不到别名）。
- **`draw_questionnaire_footer`** 在按钮之后补最后那一段：举手时是回执（**任何宽度都保** ——
  问卷期间提示行整行被页脚替换，那句话只有落在这里才看得见），否则是键位提示，按剩余列数降级。
  回执是**替换**段尾提示，不是追加（与 exit-gesture §2 同构）。
- **先红后绿**：新增 wording 一条（阶梯与两句回执）、render_layout 三条（键位提示与回执替换、
  `Ctrl-C` 的回执落在页脚、窄终端先丢教学性的那部分）。
- **顺手核出的一条**：页脚的按钮按可用性增删，所以**没作答完时根本没有「提交」按钮** ——
  窄终端测试要先让那一题有着落，否则断言的是一个不存在的按钮。
- **验收**：`cargo test` 全绿（render_layout 128 条、wording 63 条）、`cargo clippy --all-targets` 干净。
