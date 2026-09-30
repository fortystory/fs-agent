# 外框离场：内容区就是终端

Type: implement
Status: done

> 规格：`.scratch/tui-chrome/spec.md` §1。
> 本票推翻 `.scratch/tui-sidebar/spec.md` §1 的「一圈外框 + 内部分隔线」的**上半句**——分隔线留下，字形留给票 03。同时删掉 §1 里那套端点交叉符（`┬ ┴ ├ ┤`）。

## 目标

外壳不再画那圈 `Block`。内容区就是终端本身：左栏从第 0 列起、主列拿到不再被外框吃掉的那两列，转录拿到那两行。

## 落点

`src/render/layout.rs`（`plan`、`main_width`、`BORDER_COLUMNS` 的 rustdoc、`Regions` 新增 `screen`）；`src/render/tui.rs`（`draw_shell`、`draw_border` 删除、`draw_divide`、`paint_rule`、`draw_tab_bar` 左端）；`tests/` 下所有用 `TestBackend` 画整帧的文件（`render_layout.rs`、`render_tui.rs`、`ask_user_question_tui.rs`、`history_replay.rs`）里以边框字符或外框内缩为锚点的断言。

## 具体行为

1. **`draw_border` 整个删除**，`draw_shell` 不再调用它。外壳剩下三笔：`draw_divide`、`draw_sidebar`、主列的横线。
2. **`layout::plan(area, draft_rows)` 的内容区就是 `area`**，不再 `let inner = inner(area)`。于是：
   - `sidebar` 的 `y` 是 `area.y`、高度是 `area.height`（原来各少 1/2）；
   - `main` 的 `y` 是 `area.y`、高度是 `area.height`；
   - `transcript.y` 是 `area.y`（外框没了，第 0 行就是转录的第一行）。
3. **`main_width(width) = width − 左栏总宽`**，不再减 `BORDER_COLUMNS`。
4. **`layout::inner()` 保留原义**（一个带框浮层的内容矩形 = 四边各内缩一格），问题覆盖层、`/` 菜单、详情覆盖层继续用它。把 `BORDER_COLUMNS` 的 rustdoc 从「外壳花掉的行列」改写成「**一个带框浮层**花掉的行列」，并说明外壳已经不用它。
5. **`Regions` 新增 `screen: Rect`**，装 `plan` 的入参 `area`。票 04 会用它当详情居中的基准；本票只把它填上。
6. **端点交叉符取消**：
   - `draw_divide`：整列一律 `│`（原来的 `┬` / `┴` 分支删掉）；
   - `paint_rule`：整行一律 `─`（原来的 `├` / `┤` 两端删掉），签名可以简化成「把 `left..right` 填满」；
   - 主列横线的右端从 `area.right() − 1` 变成 `area.right()`；
   - 页签条两条线的左端从 `sidebar.x − 1` 变成 `area.x`。
7. **`MIN_WIDTH` / `MIN_HEIGHT`（40×10）与左栏三档宽度（40 / 28 / 隐藏）不动**。变的只是它们之下那些矩形的坐标。
8. **`CHROME` 在本票里先减 2**：`7 → 5`（外框上下那两行的账没了；状态行上方那条线留给票 02，本票不动它）。

## 测试

- 改写 `tests/render_layout.rs` 里以外框为锚点的断言：
  - `TRANSCRIPT_TOP` 从 `1` 变 `0`；
  - `transcript_rows()` 的探针（找 `┤`）在票 02 之前仍然可用（横线还在），但**左端已不是 `├`**，要跟着改；
  - `a_wide_terminal_draws_the_mark_the_sidebar_and_the_main_column` 里 `│` 与 `├`/`┤` 的读法；
  - `the_wide_sidebar_is_forty_columns_and_centres_the_mark` 的 `(41, 0) == "┬"` 与 `(41, 23) == "┴"` 两条断言：端点符号没了，改成断言分隔列在 `(41, y)` 上是 `│`（`y` 取帧内任意一行）；
  - `the_sidebar_has_two_widths_and_a_hidden_third` 里 80 列下 `(29, 0) == "┬"` 同理。
- **新增**：整帧里 `┌ ┐ └ ┘` 与「最外一列/一行的框字符」一个都不出现；最外层行/列装的是内容（转录第一行从第 0 行开始、左栏第一列从第 0 列开始）。
- **新增**：120×24 下转录行数 = `24 − 5 − 3 = 16`（在票 02 之前；票 02 之后变 17，届时改这条数）。
- 40×10 地板：左栏隐藏、主列拿到 40 列（原来 38）。

## 验收

- [ ] `cargo test` 全绿（含 `tests/render_tui.rs`、`tests/ask_user_question_tui.rs`、`tests/history_replay.rs` 里画整帧的那些）。
- [ ] `cargo clippy --all-targets` 无新增告警。
- [ ] `cargo fmt --check` 只留既有的那两处漂移。
- [ ] 真机：`cargo run` 起 TUI，四边没有框线，左栏与主列之间仍有一条竖线分得开。

## Comments

- 2026-10-01 落地，**与票 02 同批** —— 两条改动动的是同一份纵向算术（`CHROME` 与 `plan()` 里那几行），分两次改会在同一批断言上反复返工；票 03 紧随其后，因为端点交叉符一删，字形与颜色紧接着就该换。
- **实现期修正（算术）**：`layout::inner()` 是「外框内缩一格」，所以内容矩形的 `bottom()` 是**排他的** —— 状态行下面那条线正好落在 `status.bottom()` 自己那一行上，输入区从 `status.bottom() + 1` 起。spec §2 与票 02 原先写的是 `+ 2`，多算了一行（渲染时越界才发现），两处都已改掉。
- 测试里那套以 `│ ─ ├ ┤ ┬ ┴` 为锚点的断言随外框一起改：`TRANSCRIPT_TOP` 从 1 变 0，`MAIN_LEFT_AT_120` 42→41、`RAIL_AT_120` 118→119、`SCROLLBAR_AT_120` 117→118、`TRANSCRIPT_TEXT_RIGHT_AT_120` 117→118；`transcript_rows()` 的探针原来找行尾的 `┤`，现在找「延伸到屏幕右缘的那条虚线」（`ends_with('┄')`），并减去状态行那一行。
- 顺手挖出一处**不可达**：`draw_modal` 那三个提前返回的分支（`inner == 0`、`rows_available == 0`、`panes.modal()` 返 `None`）在 40×10 地板之上够不到 —— `main.width` 至少 40、`main.height` 至少 10，而模态高度总被 `rows_available` 夹在 `main.height` 之内。票 05 因此没有为「模态画不出来」写用例。
- 验收：`cargo test --all` 全绿；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `tests/ask_user_question.rs` 那处既有漂移；`python3 scripts/tui-startup-check.py` 12/12 绿（脚本锚点的改动见票 06）。
