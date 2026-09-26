# 左栏 tab：点击切换、两个占位页

Type: implement
Status: ready-for-agent
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

（实现时把偏离 spec 的地方记在这里）
