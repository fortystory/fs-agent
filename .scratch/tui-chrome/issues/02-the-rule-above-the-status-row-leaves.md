# 状态行上方那条线离场，那一行还给转录

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/tui-chrome/spec.md` §2。
> 依赖票 01：两条改动动的是同一份纵向算术（`CHROME` 与 `plan()` 里那几行），票 01 先把外框的两行账去掉，本票去第三条线的账。

## 目标

主列的横线从三条减到两条（输入区上方、提示行上方）。状态行上方那条不再画，而它占的那一行**还给转录**——不留空白行，状态行紧贴转录的最后一行。

## 落点

`src/render/layout.rs`（`CHROME`、`plan` 里 `status` / `input` / `hints` 的三行、`max_input_rows` 的 rustdoc）；`src/render/tui.rs`（`draw_shell` 那个 `for y in [...]` 的列表）；`tests/render_layout.rs` 里数转录行数与横线数的断言。

## 具体行为

1. **`draw_shell` 的主列横线只画两条**：`[panes.input.y - 1, panes.hints.y - 1]`。`panes.status.y - 1` 从列表里删掉。
2. **`CHROME` 从 `5`（票 01 之后的值）减到 `4`**：两条横线 + 状态行 + 提示行。rustdoc 里的账要跟着重写（并说明它从 7 一路减到这里的两笔账分别是谁）。
3. **`plan()` 的纵向排布改成**：

   ```rust
   let transcript = Rect::new(main.x, area.y, main.width, transcript_rows);
   let status = Rect::new(main.x, transcript.bottom(), main.width, 1);      // 紧贴，不隔行
   let input  = Rect::new(main.x, status.bottom() + 1, main.width, input_rows);
   let hints  = Rect::new(main.x, input.bottom() + 1, main.width, 1);
   ```

   两个 `+ 1` 都是「那条线占一行」：`bottom()` 是排他的，所以线正好落在 `status.bottom()` /
   `input.bottom()` 自己那一行上，内容从下一行起。**状态行是唯一不隔行的那个**——它上面那条
   线没了。
4. **`max_input_rows(height) = min(MAX_INPUT_ROWS, height − CHROME − 1).max(1)`** 不变（`CHROME` 变小，它自己跟着变）。**40×10 下输入区第一次拿满 3 行、转录留 3 行**（票 01 之后是输入区 3 行、转录 4 行；本票之后转录 3 行——因为那条线的行被 `CHROME` 收回，同时 `max_input_rows` 的余地上涨）。**把 40×10 的实测值钉成断言**，别让它随手漂。

   > 算一遍，免得实现时再推：`h = 10`、`CHROME = 4` → `max_input_rows = min(10, 10 − 4 − 1) = 5`；`ir = clamp(draft_rows, 3, 5) = 3`；`tr = 10 − 4 − 3 = 3`。转录 3 行（0..2）、状态行第 3 行、线第 4 行、输入区 5..7、线第 8 行、提示行第 9 行。
5. **状态行本身的内容与阶梯一动不动**（`wording::status_row`，三档）。

## 测试

- 改写「转录有多高」的那些断言：120×24 下 `tr = 24 − 4 − 3 = 17`（票 01 之后是 16）。
- **新增**：`plan` 出来的 `status.y == transcript.bottom()`，且 `status.y − 1` 那一行**不是**横线（它就是转录的最后一行）。
- **新增**：主列里含 `─` 的行**恰好两条**。
- **新增**：40×10 下输入区 3 行、转录 3 行（把上面算出来的那串坐标钉住）。
- `tests/render_layout.rs` 的 `transcript_rows()` 探针：它现在的语义是「找主列第一条横线」，那条线还在（输入区上方），**探针可以留**，但它量出的值要按新算术更新；顺带确认它没有被「状态行上方那条线没了」误导。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警；`cargo fmt --check` 只留既有漂移。
- [ ] 真机：转录与状态行之间没有横线，读起来不挤（若不挤不动它；挤的话记进票 06 的手工清单，别在本票里偷偷加回一行空白）。

## 评论

- 2026-10-01 落地，**与票 01 同批** —— 同一份纵向算术，见票 01 的 Comments（那里记着 `bottom()` 是排他的这条算术修正）。
- 实测：120×24 下转录 17 行（原 14）；80×24 下 17；80×14 下 7；174×50 下 43；40×10 下输入区 3 行、转录 3 行（原输入区 2 行、转录 1 行）。
- 那一行**还给了转录**，没有留成空行：状态行紧贴 `transcript.bottom()`，`status.y - 1` 就是转录的最后一行。`a_wide_terminal_draws_the_mark_the_sidebar_and_the_main_column` 里有一条 `!rows[16].contains('┄')` 钉住它。
- 左栏的内容高度也跟着从 `h − 2` 变成 `h`，于是「先退文字身份、再退到什么都不画、再丢字段」的后几档落到 40×10 地板以下、够不到了 —— `the_sidebar_gives_up_its_identity_then_its_fields_as_it_shrinks` 的档位表按可达的档重取（注释里写明了原因）。
