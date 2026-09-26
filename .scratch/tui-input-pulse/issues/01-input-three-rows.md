# 输入区最少三行

Type: implement
Status: done

> 规格：`.scratch/tui-input-pulse/spec.md` §1、`Testing Decisions`（用户故事 1–6）。

## 目标

输入区**最少 3 行**：草稿折行数不足 3 时它仍然占 3 行，超过 3 时照旧长到 10 行上限，交接处不加第二圈边框。

## 落点

`src/render/layout.rs`、`tests/render_layout.rs`（必要的话 `docs/render.md` 的几何段留给票 03）。

## 具体行为

1. `layout.rs` 新增 `MIN_INPUT_ROWS: u16 = 3`（写清它是什么：**输入区的行数下限**，不是「草稿的下限」），`plan` 改成
   `input_rows = draft_rows.max(MIN_INPUT_ROWS).min(max_input_rows(area.height))`。
2. `max_input_rows` 照旧 = `min(MAX_INPUT_ROWS, h − CHROME − 1).max(1)`。**地板优先级写进注释**：转录底线那 1 行 > 输入区最小高度 —— 40×10 下输入区是 2 行、转录 1 行，不是 3 行 0 转录。
3. `plan` 与 `CHROME` 的 rustdoc 里「转录行 = h − CHROME − 输入行数」照旧成立，但要注明输入行数的下限是 3（而那一档的例外由 `max_input_rows` 负责）。
4. 画笔侧**不动**：输入区仍然画在 `panes.input` 矩形里、草稿仍然顶部对齐、上下的共享分隔线仍然是外壳那三条横线（`draw_shell`）。不要给输入区加 `Block`。
5. 编辑器（`src/render/editor.rs`）**一个字节不改**：`view(width, height)` 拿到的 height 只是窗口高度，返回的行数仍然是草稿的真实折行数，余下的行是空行。

## 测试

`tests/render_layout.rs`：

1. 矩阵（`every_size_in_the_matrix_draws_the_regions_its_budget_allows`）的转录行数改成新账：`40×10 → 1`、`40/60/80/100/120×24 → 14`、`80×14 → 4`、`174×50 → 40`。**每行都在断言旁边写清它为什么是这个数**（输入区 3 行 / 地板档 2 行）。
2. `a_floor_sized_terminal_still_draws_the_main_column`：40×10 的转录从 2 行改成 1 行，并补一条「输入区在这一档拿 2 行 —— 转录底线赢」。
3. 改写 `the_input_area_grows_with_the_draft_and_the_transcript_gives_up_the_rows`：
   - **空草稿**：输入区 3 行、转录 14，第 1 行 `> `、后两行没有任何内容；
   - **3 行草稿**：转录**仍然是 14** —— 这条是本票的核心断言（几何不再随前 3 行变化）；
   - **5 行草稿**：转录 12（框还会长），三行以上按序可见；
   - 提示行照旧在输入区下面一行。
4. `a_tall_draft_costs_the_transcript_and_never_the_sidebar` 的 10 行草稿档（转录 7）保持不变，跑一遍确认没被算错。
5. 40×24 与 174×50 各补一条「输入区 3 行」的断言（覆盖「宽屏高终端」这一档）。

## 不做什么

输入区的独立边框；草稿底部对齐；`MAX_INPUT_ROWS` 的调整；任何编辑器行为（`Esc` / 粘贴 / 光标）；脉冲（那是票 02）。

## Comments

**实现完成（2026-09-26）**。落点：`src/render/layout.rs`（`MIN_INPUT_ROWS = 3` + `plan` 的 clamp + 三段 rustdoc）、`tests/render_layout.rs`。

1. **`plan`**：`input_rows = draft_rows.max(MIN_INPUT_ROWS).min(max_input_rows(h))`，`max_input_rows` 一个字节没改 —— 所以 40×10 那一档的输入区是 **2 行**、转录 1 行（转录底线赢），`h ≥ 12` 的每一档都是 3 行。这条优先级写进了 `plan` 与 `max_input_rows` 的 rustdoc。
2. **画笔侧零改动**：输入区仍然画在 `panes.input` 里、草稿仍然顶部对齐、上下分隔线仍然由 `draw_shell` 画 —— 没有新增 `Block`，`editor.rs` 一个字节没动。
3. **测试**：矩阵的转录行数改成新账（40×10 → 1、80×14 → 4、其余 24 行档 → 14、174×50 → 40），每行旁注都写清为什么；`the_input_area_grows_with_the_draft_and_the_transcript_gives_up_the_rows` 改名成 `the_input_area_holds_three_rows_before_it_grows_and_the_transcript_pays_for_it`，核心断言是**空草稿与 3 行草稿给同样的 14 行**，再补一条「第 4、5 行才继续吃转录」；120×24 的参考帧测试补上「后两行是空行、不是第二块」；40×10 那条补上「这一档输入区 2 行」。另外顺手修正了 `clicking_a_rail_cell_jumps_to_that_turns_question` 的偏移量（转录少了 2 行，回合条窗口随之移动：偏移 1 → 单位 17、偏移 5 → 单位 21）。
4. **基线**：`cargo test` **724 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
