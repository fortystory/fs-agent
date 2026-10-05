# 12 — 轨迹页按轮次隔行底色

Type: implement
Status: done
Part of: ../map.md
Blocked by: 09

> 规格：[`../spec.md`](../spec.md) §3（按轮次隔行底色）；示例见 [`../prototype/split-view.txt`](../prototype/split-view.txt)（带色版，`cat` 可见底色）。

## 目标

轨迹页里每个**单位**（交互会话数回合、讨论会话数轮次）的所有行共用一块深背景，相邻单位交替 —— 像表格的隔行换色，但交替的单位是**轮次**而不是屏幕行。用户看得见：在轨迹页里一眼分得开「这一轮」与「上一轮」。

## 现状（改前先复核）

- 每源行 → 单位序号的索引**已经存在**（回合条用的那一份）；上一张 tracer 票已让轨迹 pane 也持有它自己那套平行表。
- 色板只用命名色；`Color::Indexed` 可用但仓库至今没用过。

## 落点

`src/render/tui.rs`（轨迹页的行生成期打上底色）。

## 具体行为

1. 单位判据与回合条**一致**（交互数 `TurnEnded`、讨论数 `RoundEnded`）。
2. 底色在**行生成期**按「源行 → 单位序号」打上 —— 不按屏幕行算，所以滚动时色块跟着内容走、不会重排。
3. 第一个边界之前的行（注入、第一次提问）归第一段。
4. **只在轨迹视图**铺底；对话视图不铺。
5. 两个深背景色值实现期对着真终端定（256 色的两块深灰起步）；退化终端**不做底色**。

## 验证

1. 帧断言：三轮会话在轨迹页里出现两块交替底色，边界落在「回合结束」之后。
2. 滚动到中间：色块仍跟着内容（不是跟着屏幕行）。
3. 对话视图没有任何底色；未着色的终端（或无底色开关打开时）画面与上一张票一致。

## 不做什么

- 不动前景色（三类前缀的颜色不变）。
- 不给超长单位做「内部再分档」——那条边界先记着，是以后的决定。

## 作答（2026-10-05）

- **单位号取自回合条那份记账**：`turn_rail.units()` 是**已完成**单位的个数 —— 它由
  `is_boundary` 驱动、与视图无关，所以正在建的那个单位拿到的是自己的号，第一个边界之前的行
  因此归第一段。底色在 `push_line(Viewport::Trace, ...)` 里当场设
  `line.style.bg = TRACE_STRIPES[unit % 2]`（`Color::Indexed(235)` / `Indexed(236)`）。
  底色因此住在行上，滚动时跟着内容走；折行（`pane::wrap_line`）与取景（`Pane::window`）都
  保留行样式。对话视图与 plain 一个字不改。
- **退化终端**：`TuiState::set_stripes(bool)` 是那个开关，`Tui::run` 组装时按通用约定
  `NO_COLOR` 关掉它；关/开都会清空轨迹视图并按当前宽度整批重排（底色已经在行上，不重排
  会留旧底色）。
- 新增四条帧断言：三个回合两块交替底色且边界落在回合结束之后、
  `问题 5` 滚动前后底色跟着它、主列每一行都没有底色、`set_stripes(false)` 之后整屏无底色
  而内容照旧。
- `cargo test` 全绿（render_layout 169 + 其余）、`cargo clippy --all-targets` 无警告、
  `cargo fmt --check` 干净。

**2026-10-05 `/code-review` 后的修正**（Spec 轴的两条发现）：

- **就地重写的思考行也补底色**：实时路径的 `settle_thinking` 走
  `trace.replace_last(line)`，绕开了 `push_line` 那一步打底；现在 `in_place` 分支自己设一次
  `bg`（重放路径本来就走 `push_line`），两条路同色。回归在
  `the_thinking_line_wears_the_stripe_too`。
- **`replay_trace` 改成重放两个视图**：轮次底色的单位号由回合条推进，而回合条只在重放**对话**
  视图时重建（`close_unit` 与对话目标绑定）—— 只重放轨迹会让整页打上同一个单位号、色块不再
  交替。那个只写不读的 `trace_units` 平行表随之删掉。回归在
  `turning_the_stripes_back_on_still_alternates_them`。

**2026-10-05 维护者把底色换成了分隔线**（推翻本票的做法）：

- `TRACE_STRIPES`、`stripes` / `set_stripes`、`trace_stripe`、`replay_trace` 与组装处的
  `NO_COLOR` 分支**全部删掉**；改成单位结束之后在轨迹页插一整行虚线（`trace_rule`，穿外壳
  同一种框架色 `CHROME_LINE`）。它同样是**行**，同样跟着内容滚动，两边都不再有底色。
- 测试换成 `the_trace_page_draws_a_rule_between_units`、
  `the_trace_rule_travels_with_the_content`、`neither_view_paints_a_stripe_background`；
  手工清单 ㉙ 第 7 条、`docs/render.md` 与 `CONTEXT.md` 跟着改。
