# 05 — 拖选与反白：Down / Drag / Up 三段，选区归属一块文本区域

Type: implement
Status: ready-for-agent
Part of: ../spec.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §5。字符串的取值与复制是下一票（06），本票只做到「屏幕上选
> 得出来、看得见」。

## 目标

按住左键拖过一块区域，那段文字反白；松开手退出选择、**不**触发原来那一次点击的动作（页签不切、
覆盖层不关、链接不开）。没拖动的按下-松开仍然是一次普通点击，行为与今天逐字相同。

## 现状（改前先复核）

- `src/render/tui.rs` 的 `mouse()` 把一切压在 `MouseEventKind::Down(MouseButton::Left)` 上：
  详情覆盖层、问卷、页签 / 回合条 / 链接三支都在这一个分支里跑完。`Drag` 与 `Up` 今天被 `_ => {}`
  吃掉。
- `Regions`（同文件）是命中区域的记账：每帧 `clear`，绘制时 `push`，指针读它 —— 新的一层照这条
  纪律办。
- 各块区域的显示行来源：转录是 `Pane::view()` 交回来的 `Vec<Line>`（`pane::source_at(i)` 能判出第
  i 行是不是上一行的软折续行）；详情是 `DetailView::body`；左栏是 `draw_sidebar_page` 的 `rows`；
  问卷是 `questionnaire_lines()`；输入区是 `editor.view()` 的 `rows`。
- 绘制顺序（谁盖住谁）就是 `draw_frame` 里的调用顺序：外壳 → 转录 → 状态行 → 底部 → 菜单 →
  覆盖层 → 详情。

## 落点

`src/render/selection.rs`（新建，并在 `src/render/mod.rs` 注册 `pub mod selection;`）、
`src/render/tui.rs`（`mouse` 重构、五处记录点、`draw_frame` 末尾画反白）、
`tests/render_layout.rs`。

## 具体行为

1. **屏幕文本层**（`selection.rs`）：
   - `ScreenText`：每帧重建（`clear()` + 绘制时 `push`），按 push 顺序存 `TextBlock`。
   - `TextBlock { rect, kind, rows: Vec<TextRow> }`，`TextRow { text: String, folded: bool }`；
     `kind` 取 `Transcript | Detail | Sidebar | Questionnaire | Draft`。
   - `block_at(x, y)` 返回**最后 push 的**那块的编号（最后画的在最上层）。
2. **记录点**（TUI 侧，五个，都是「画那一行的同一个地方」）：
   - `draw_transcript`：两个视图各记各自的 `rows`，`folded[i] = source_at(top+i) == source_at(top+i-1)`；
   - `draw_detail`：记它这一帧画出来的主体窗口（含边框内的文字区矩形），`folded` 由
     `detail_body` 的折行信息推 —— 给它加一个「这一段折出了几行」的旁路（返回 `(Line, bool)` 的
     列表，或并列一个 `Vec<bool>`），**不改它的排版结果**；
   - `draw_sidebar_page`：记它交回来的 `rows`（`folded` 全假）；
   - `draw_bottom`：问卷分支记 `questionnaire_lines()` 画出来的窗口，编辑器分支记 `editor.view()`
     的行（`folded` 全假）。
3. **手势**：
   - `TuiState` 新增 `screen_text: selection::ScreenText` 与 `drag: Option<Drag>`，
     `Drag { block: usize, anchor: (u16, u16), head: (u16, u16), selecting: bool }`。
   - `Down(Left)`：先按 `selection::block_at` 找出所属区域；找得到就记 `anchor`（`selecting: false`），
     找不到也照记（block 用 `None` 表达，拖动不产生选区）。**不执行**任何既有动作。
   - `Drag(Left)`：与 `anchor` 的曼哈顿距离 ≥ 2 格才置 `selecting = true`（手抖不该变成一次选择），
     并把 `head` 更新为**夹在所属区域矩形内**的那个格子。
   - `Up(Left)`：`selecting` 为真 → 交给下一票的复制（本票先只清掉 `drag`）；否则把这一次点击
     交给**今天那套分派**——把那一段代码原样抽成 `fn click_at(&mut self, column, row)`，`Up` 里调它。
   - `Drag` / `Up` 在没有 `drag` 时不做事；滚轮与中右键照旧；`Esc` 与重放期间清掉进行中的选择。
4. **反白**：`draw_frame` 最后（`draw_detail` 之后）调 `selection::paint(frame, &state.screen_text, &state.drag)`：
   对选区矩形内每个缓冲单元加 `Modifier::REVERSED`。它只碰缓冲，不改任何绘制函数。

## 验收

- 新断言（帧层）：
  - `Down` 在正文行 + `Drag` 到右下方两格：那一带的单元带 `REVERSED`（读缓冲的 modifier）；
  - 同一串事件里 `Up` 之后：页签没切、覆盖层没开（拖选不吃掉点击之外的动作）；
  - `Down` + 同格 `Up`（无 `Drag`）：页签照旧切、`▸` 行照旧开详情（既有断言一条不改地照旧绿）；
  - 移动只有一格时不算选择，`Up` 照旧按点击处理。
- 详情覆盖层里按住拖动：覆盖层**不关**；同一次抬起的 `Up` 若没越过门槛则照旧按「框外点击关闭」
  那一支走。
- `cargo test` 全绿。
