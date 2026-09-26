# 左栏 tab：点击切换、两个占位页

Type: implement
Status: done
Blocked by: 02

> 规格：`.scratch/tui-sidebar/spec.md` §3 / §6 / §7。帧：`prototype/frames/120x24-tab-trace.txt` / `-tab-files.txt`。

## 目标

让 tab 条**活起来**：三个标签都能点，选中页跟着变；`轨迹` / `文件` 这两页现在只有一句占位符（内容各自单独开票）。

## 落点

`src/render/tui.rs`（`draw_*` 左栏那一支 + `HitAction` + `mouse()` 分派）、`src/render/wording.rs`（tab 标签与占位符）、`src/render/layout.rs`（若 `Regions` 需要记住选中页的位置就能算清楚）、`tests/render_layout.rs`、`tests/render_tui.rs`。

## 具体行为

1. **选中态**：选中页的标签 `LightMagenta` + `BOLD`，未选中 `DarkGray`；选中是**渲染器状态**（`TuiState` 里的一个 `Tab` 枚举，默认 `调用量`），不是事件、不落流。
2. **命中矩形**：每个标签的文字范围一个矩形，这一帧画了什么就记什么（沿用 `state.regions`，新 `HitAction::SwitchTab(Tab)`）。`mouse()` 的分派顺序：**详情覆盖层 > 待答问题 > 回合条 > 左栏 tab > 转录**。
3. **占位页**：`轨迹` / `文件` 的内容区画一行 `wording::tab_placeholder()`（`此页尚未实现（另有票在跟）`，暗灰），左对齐在字段区第一行。**不画假数据、不画空表。**
4. **不给键盘**：不加任何键位（`Tab` 归 `/` 菜单、`Shift+Tab` 归计划模式，仓库不开键盘增强协议）。tab 只能点。
5. **切到占位页时面板读数消失**是接受的代价（状态行的 `上下文 n%` 成为唯一读数）——写一句注释在 `Tab` 枚举上，免得后来者以为是 bug。

## 测试

- 点「轨迹」→ 选中页变、面板字段消失、占位符出现；点「调用量」→ 字段回来。
- 命中矩形只在标签文字上（点 tab 条右侧的填充横线**不**切换）。
- 覆盖层/问题在场时点击 tab **不做任何事**（覆盖层优先）。
- 窄档（`w < 80`，左栏隐藏）时没有 tab 命中矩形。

## 不做什么

`轨迹` / `文件` 的真实内容；tab 的键位；tab 的键盘焦点。

## Comments

**实现完成（2026-09-26）**。落点：`src/render/tui.rs`（`TuiState.tab`、`HitAction::SwitchTab`、`draw_sidebar` 记命中矩形并按选中页画、`mouse()` 的第三段分派）、`tests/render_layout.rs`（5 个新用例）。

**这票挖出一个真 bug，已修并留了回归测试**：tab 标签的命中矩形和问题的按钮矩形记在**同一张** `state.regions` 表里（spec §7 就是这么定的），而 `question_click()` 原来在拿到任意一个 action 之后先把 `pending` **取走**再匹配 —— 于是「有弹窗时点 tab」不但不会切页（对），还会把**读者没回答的问题**当成 Dismiss 丢掉（工具被无声拒绝，界面看着像「点了没反应」）。修法：把不属于这个问题的 action **原样放回**（新增一条 `(pending, _) => self.pending = Some(pending)` 臂），问题照旧在场、照旧可答。回归测试 `a_question_keeps_the_tabs_from_answering` 断言三件事：弹窗还在、页面没切、问题**仍可作答**（按 `y` 仍能拿到 `Allow`）。把修复撤掉，这条测试立刻红。

**其余按票面落地**：选中 `LightMagenta` + `BOLD` / 未选中 `DarkGray`（两个配色各有断言）；命中矩形只在标签正文上（点分隔线 `│` 与右端填充的 `─` 都不切换，有用例）；`Tab` 不给键盘（`Tab` 仍归 `/` 菜单、`Shift+Tab` 归计划模式，键位表零改动）；窄档（60 列）标签根本不画，点过去什么也不会发生。

**与票面不同的地方**：命中矩形是按「这一帧真的画了什么」逐标签算的（`used + width <= sidebar.width` 才记），所以被裁掉的标签不会有矩形。左栏最窄 28 列、三个标签加分隔共 16 列，实际永远裁不到 —— 但这条断言留着，免得以后加第四个标签时它悄悄出错。

**基线**：`cargo test` **713 passed / 0 failed**（708 → +5）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
