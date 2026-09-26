# 收尾：删掉时钟的残留、对齐文档与手工清单

Type: implement
Status: ready-for-agent
Blocked by: 02, 03, 04

> 规格：`.scratch/tui-sidebar/spec.md` §8（不变量与删除清单）、§2（阶梯）、`Testing Decisions`。

## 目标

外壳改版之后把「时钟/顶栏」这条路上的残留清干净，把文档与手工清单改到与新界面一致，并且**用实测把 spec 里写的阶梯数字钉一遍**。

## 落点

`src/render/tui.rs`、`src/render/layout.rs`、`src/render/wording.rs`、`scripts/tui-startup-check.py`、`docs/render.md`、`docs/tui-manual-checklist.md`、`README.md`、`tests/render_layout.rs`、`tests/wording.rs`。

## 具体行为

1. **`TICK` / `interval` 臂**：先确认它现在是否还有任何职责（时钟已删；待答问题走 console port、事件走广播、重播走 console port）。**没有就删掉 `TICK` 常量与那条 `interval` 臂**；若发现别处依赖它，把原因写成注释，不要留着猜。
2. **删除残留**：`SessionFacts.cwd`、`TuiState.clock`、`wording::clock` / `clock_short`、左栏与状态行里任何还画 cwd/时钟的路径。用 `cargo clippy --all-targets` 与 `grep` 双向确认没有留下没人读的字段。
3. **阶梯复核**：把 `tests/render_layout.rs` 的矩阵数字与 `prototype/geometry-table.md` 的实测逐档对一遍（`40×10` / `60×24` / `80×14` / `80×24` / `100×24` / `120×24` / `174×50` + 草稿 3 / 12 行）。**任何不一致都要么改代码、要么在 spec 里写清为什么**（比如措辞层与几何层的口径不同）。
4. **`scripts/tui-startup-check.py`**：注释改到新布局（「header」→「左栏的 mark」；`BORDER_H` / `BORDER_V` 的注释不再是「每块一圈」）。**锚点不动**：`MARK_ROW`（260×30 下左栏 40 列画 mark）与 `ctrl-c`（提示行）必须仍然绿；跑一遍确认。
5. **文档**：`docs/render.md` 的「四分区 / The header」两节改写为新外壳（外框 + 左栏 + 主列、状态行、回合条、mark 在左栏、cwd/时钟不再显示）；`README.md` 里的界面截图/占位改到新帧（可从 `prototype/frames/` 选一张，或自己跑一遍真终端）；`docs/tui-manual-checklist.md` 补上本轮的手工项（mark 在左栏的字形对齐、tab 点击、回合条焦点随滚动变化、点格跳转的落点、120 列下提示 4 条这一档读起来够不够）。
6. **spec 回改**：把实现期与原写法不一致的地方改回 `.scratch/tui-sidebar/spec.md`（这个仓库的规矩是 spec 跟着实现走，而不是留在票里）。

## 测试

- `cargo test` 全绿；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只留 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 两处既有漂移（不要顺手格式化那两个文件）。
- `python3 scripts/tui-startup-check.py`（先 `cargo build`）三个手势 + 一个 `--continue` 全绿。
- 手工清单跑一遍，结果记进 `docs/tui-manual-checklist.md`。

## 不做什么

`轨迹` / `文件` 页的内容；任何键位新增；plain / headless。

## Comments

（实现时把偏离 spec 的地方记在这里）
