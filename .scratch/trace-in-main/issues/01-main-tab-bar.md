# 01 — 主列页签条与几何

Type: implement
Status: done
Part of: ../spec.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1、§2。

## 目标

主列顶部出现一条页签条（`对话` ┆ `轨迹`），点一下切页；左栏页签里不再有 `轨迹`。用户看得见：
120×24 的终端里，主列第一行是页签条，下面 14 行是当前那一页的内容；左栏页签条上只剩
`调用量` / `todo` / `文件`。

## 现状（改前先复核）

- `layout::plan` 是几何的唯一家：`CHROME = 4`（主列的两条分隔线、状态行、提示行），
  `transcript_rows = h − CHROME − 输入行数`；左栏页签条用 `TAB_ROWS = 3`、`TAB_RULE_ROWS = 1`。
- 主列页签条今天不存在。左栏页签条住在 `draw_tab_bar`，标签列表是建出来的（`Tab::Usage` /
  可选的 `Todo` / `Trace` / `Files`），每个标签画出来时记一个 `HitAction::SwitchTab` 命中矩形。
- 左栏轨迹页由 `draw_sidebar_page` 里 `state.tab == Tab::Trace` 那一个分支画
  （`panes.sidebar_page_text()` + `sidebar_page_scrollbar()`）。
- 选中档是 `ACCENT` + `BOLD`、未选是 `MUTED`，标签之间一个 `┆`、其余填 `┄`（页签条那两笔）。

## 落点

`src/render/layout.rs`（`CHROME`、`Regions::main_tabs`、删除只服务左栏轨迹页的两个方法）、
`src/render/tui.rs`（`Tab` 枚举、新的 `MainTab`、`draw_tab_bar` 抽出可复用的一行、`HitAction`、
`draw_sidebar_page` 去掉轨迹分支）、`src/render/wording.rs`（`TAB_CONVERSATION`）、
`tests/render_layout.rs` / `tests/wording.rs`。

## 具体行为

1. `CHROME` 从 4 涨到 7，算式与注释一起改：多出来的正是页签条那 3 行。120×24、草稿空时转录从
   17 行变 **14** 行。
2. `Regions` 新增 `main_tabs: Rect`（标签行），位置是主列第一行；`transcript` / `rail` 从它下面
   一行起。已存在的两条分隔线由 `input` / `hints` 推出来，不另写算式。
3. 主列页签条的画法与左栏页签条**同一套**：标签行上下各一条从 `main.x` 到屏幕右缘的
   `paint_rule` 虚线，标签之间 `┆`，行尾 `┄` 填满，选中 `ACCENT` + `BOLD`、未选 `MUTED`。
   两处共用同一段画标签行的代码（抽成一个按 `(标签列表, 选中项, 宽度, 命中动作构造器)` 工作的
   内部函数），而不是复制一遍。
4. `HitAction::SwitchMainTab(MainTab)` 与 `HitAction::SwitchTab(Tab)` 并列；主列标签的命中矩形
   只在真画出来时记（照 `draw_tab_bar` 那条纪律）。
5. `TuiState` 新增 `main_tab: MainTab`（`Conversation` / `Trace`），默认 `Conversation`；`mouse`
   的 `SwitchMainTab` 分支只改它。**不给键位**。
6. 左栏 `Tab` 枚举去掉 `Trace`；`draw_sidebar_page` 里那条轨迹分支整段删除（下一票把它接回
   主列），`Tab::Trace => Vec::new()` 那个兜底一并去掉。`layout::Regions::sidebar_page_text` 与
   `sidebar_page_scrollbar` 删除（只服务左栏轨迹页）。
7. `wording::TAB_CONVERSATION = "对话"`，与 `TAB_TRACE` / `TAB_USAGE` / `TAB_FILES` 并列；
   `tests/wording.rs` 那组常量断言补上它。

## 验收

- 一帧 120×24 的断言：屏幕上有一行同时含 `对话` 与 `轨迹`，且它**不在**左栏里（列号 > 分隔列）；
  左栏页签那一行含 `调用量` 与 `文件`、不含 `轨迹`。
- 一条断言：转录的内容行数从 17 变 14（用现成的行数 helper 量，不写死在新断言里）。
- 点击 `轨迹` 标签后 `state.main_tab == MainTab::Trace`（本票还不画轨迹内容，只切状态）。
- 窄终端（`< 80` 列，无左栏）里主列页签条照旧在，两条横线从屏幕左缘起。
- `cargo test` 全绿（本票会改掉若干既有断言的落点，见 `tests/render_layout.rs` 的 tab 组）。
