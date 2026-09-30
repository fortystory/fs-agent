# 详情覆盖层屏幕居中

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/tui-chrome/spec.md` §4。
> 依赖票 01：居中基准要用 `Regions::screen`，那是票 01 新加的字段。

## 目标

详情覆盖层从「主列里居中」改成**屏幕居中**。宽度上限与边距照旧，问题覆盖层**保持主列居中**（两者故意不同基准）。

## 落点

`src/render/layout.rs`（`Regions::detail`、`detail_width` 的 rustdoc）；`tests/render_layout.rs` 里读详情覆盖层位置的断言（`borders` 那两条）。

## 具体行为

1. **`Regions::detail()` 的基准从 `self.main` 换成 `self.screen`**：

   ```rust
   let x = self.screen.x + (self.screen.width.saturating_sub(width)) / 2;
   let y = self.screen.y + (self.screen.height.saturating_sub(height)) / 2;
   ```

   高度沿用 `self.screen.height − BORDER_COLUMNS − DETAIL_MARGIN_ROWS`（外壳不再内缩之后比今天高 2 行，正文因此多两行，这是预期）。
2. **`None` 的判据改成对着 `screen` 量**：`width <= BORDER_COLUMNS || self.screen.height <= BORDER_COLUMNS + DETAIL_MIN_ROWS`（原来读 `self.main.height`）。
3. **`detail_width()` 的宽度来源**：从 `self.main.width` 换成 `self.screen.width`。**它会变宽**（主列比整屏窄 28–41 列），所以 `DETAIL_MAX_WIDTH = 135` 与 `MODAL_MARGIN = 4` 之间哪个在封顶会在窄屏上换手——120 列下 `screen.width − 4 = 116 < 135`，仍然由边距封顶、与今天同宽；但 **80 列下今天是 `main.width(80) − 4`，改后是 `80 − 4 = 76`**，明显变宽。这是这条需求的一部分（「屏幕居中」意味着能用到整屏），把它当成预期值写进注释。
4. **`modal()` 与 `modal_width()` 一行不改**：问题覆盖层继续居主列。两处 rustdoc 各补一句「另一半在哪、为什么不同」。
5. **`draw_detail` 不用改流程**：它从 `panes.detail()` 拿矩形，`state.detail_rect` 与点击命中自然跟着走。

## 测试

- **改写** `tests/render_layout.rs` 里「这个框在主列里居中」的断言（`borders` 那两条，约 2311 行与 3232 行）：改成**在屏幕上居中**——`borders[0]` 与 `width − 1 − borders[1]` 的差 ≤ 1。
- **新增**：把「屏幕居中」与「主列居中」区分开的一次断言：在 120 列（左栏存在）下，详情框左边那条边框的列**小于**主列居中会给出的列。这条是这段改动的**回归锁**——没有它，将来有人抹平基准时测试不会响。
- **新增**：80 列下详情框宽度 = `80 − 4 = 76`（封顶的是边距不是 135）。
- 详情覆盖层的正文行数、页脚、`Esc` 关闭、点框外关闭这些既有用例不期望变，跑一遍确认。

## 验收

- [ ] `cargo test` 全绿。
- [ ] `cargo clippy --all-targets` 无新增告警；`cargo fmt --check` 只留既有漂移。
- [ ] 真机：左栏存在时打开一个详情覆盖层，肉眼看起来是屏幕正中，不是偏右。

## Comments

- 2026-10-01 落地。
- 实测：120×24 下详情覆盖层 116 列（`screen.width − MODAL_MARGIN`，135 那条上限到不了；改动前是 `主列 − 4 = 73`），200×40 下 135（撞上限）；120×40 下它横跨第 2 到 117 列、第 2 到 37 行。
- **代价如实记下**：屏幕居中之后，详情框横向压住左栏、纵向压住底部输入区 —— 120×40 下提示符落在框内、被 `Clear` 抹掉。`the_detail_overlay_ignores_every_key_but_its_own` 因此改成先 `Esc` 关掉覆盖层再看草稿（它的意图「可打印键到不了草稿」一点没变）。真机那一格在 `docs/tui-manual-checklist.md` ⑳ 第 5 条。
- 问题覆盖层一行没动，仍然居中于主列 —— 两者不同基准是写下来的决定，不是笔误；两条用例都钉了它（`a_permission_question_lands_in_the_middle_as_a_covered_overlay` 仍在主列里量）。
- `overlay_width` 那个测试辅助函数本来靠「框不贴第一列」来分辨覆盖层，现在两种框都不贴，注释改了、代码不用改。
