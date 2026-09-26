# 收尾：删掉时钟的残留、对齐文档与手工清单

Type: implement
Status: done
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

**实现完成（2026-09-26）**。落点：`src/render/tui.rs`（删 `TICK`）、`src/render/wording.rs` / `src/render/{mod,width}.rs` / `src/render/tui.rs` 的注释、`scripts/tui-startup-check.py`（只改注释）、`docs/render.md`、`docs/tui-manual-checklist.md`、`README.md`、`docs/adr/0002-fullscreen-alt-screen-tui.md`、`.scratch/tui-sidebar/spec.md`。

### 1. `TICK` / `interval` 臂：**已删**

逐条确认过：`receiver.recv()`（广播）、`port.recv()`（console）、`keys.next()`（键盘）是三个**真的会唤醒**的来源；待答问题走 console port、事件走广播、重播走 console port，没有任何东西靠周期性唤醒才出现（`dirty` 由事件与按键置位）。所以 `TICK` 常量、`tokio::time::interval`、那条 `tick.tick() => {}` 臂与开头的 `tick.tick().await` 全部删除，`std::time::Duration` 的 import 也随之消失 —— 现在这个 `select!` 是三条来源、没有定时器，注释里写明它以前为什么在。`docs/render.md` 里「不等 120 ms tick」那句一并改掉。

### 2. 残留清理

`grep -rn "clock" src/render/` 只剩一条注释（讲它为什么被删）；`grep -rn "cwd" src/render/` 剩下的全是 CLI 的 `--cwd` 参数与 `sessions` 的措辞。`SessionFacts.cwd` 按票 02 的例外处理：**保留但改名 `session_dir`**，因为 `read_tool_body()` 真的在用它拉回溢出的工具输出（见 §8 的「例外」）。`cargo clippy --all-targets` 零告警，双向确认没有留下没人读的字段。顺手把模块文档里最后几处「四分区 / four-pane」的说法改成新外壳（`render/mod.rs`、`wording::help_interactive`、`draw_too_small` 的注释）。

### 3. 阶梯复核（`tests/render_layout.rs` 的矩阵 ↔ `prototype/geometry-table.md`）

逐档对过，**只有一处不同，而且是 spec 已经撤销的**：

- **100×24**：表里是 34 列（prototype 的中间档），实现是 **28** —— spec §2 明确撤销了 34（「不值得为它多占 6 列文本宽」），所以表里那一档连同 D 表的 `100/104/112/119` 四行都只作历史记录，代码是对的。
- 其余**逐条吻合**：40×10 隐藏 / 主列 38 / 转录 2 行 × 36 列；60×24 → 16 × 56；80×14 → 6 × 47；80×24 → 16 × 47；120×24 → 16 × 75；174×50 → 42 × 129（= 表 A）。草稿档（B）3 行 → 转录 14、12 行 → 转录 7 ✓。高度扫描（E）120 列下 `h = 10` 无身份行 + 5 字段、`h = 12` 文字身份、`h = 16` mark ✓。提示行（H）40 → 1 条 + 退出 + 就绪、80 → 2 条 + 退出、120 → **4 条 + 退出（放不下 `就绪`）**、174 → 5 条 + 退出 + 就绪 ✓（80 与 100 两档的**条数**与表一致；100 档主列从 63 涨到 69，仍放不下第 4 条，所以结论不变）。
- **两处「够不到」照旧成立**：状态行第 4 档（整行消失）仍然不可达（要主列内容宽 < 11，早于 40×10 地板）；F 表里宽度 10 那一行是 prototype 的 `status_row()` 行为，实现按 spec §2 改成**恒非空**、超窄交给 `truncate_columns`。
- **G 表最后一行（那个洞）已修**：单位 30、焦点 12 时 prototype 的帧里一格不亮，实现改成「窗口跟着焦点走」，回归测试逐停靠点断言「有且只有一格 `┃`」。

### 4. `scripts/tui-startup-check.py`

**只改注释**：`MARK_ROW` 的说明从「顶栏的标记」改成「左栏的标记（≥ 120 列才画）」，`BORDER_H` / `BORDER_V` 的说明从「每块一圈边框」改成「一圈外框 + 一条分隔线 + 主列三条横线」；docstring 里「the header's identity, the pane frames present」改成「the identity (the mark, in the sidebar), the frame and its divider present」。**锚点一个没动**（`MARK_ROW`、`ctrl-c`、`BANNER_ANCHOR`、`TEARDOWN`）。实测 `python3 scripts/tui-startup-check.py target/debug/fs-agent 1` → **4/4 GREEN**（三个手势 + 一个 `--continue`）。

### 5. 文档

- `docs/render.md`：把「The header」整节换成「**The shell**」——外框 + 全高左栏 + 主列的示意图、左栏三档与高度阶梯、tab 条的取舍、回合条的窗口规则、chrome 7 行的账、以及「cwd 与时钟不在屏幕上、但 `session_dir` 还要注入」。TUI 那一行表格与 `select!` 的描述跟着改。
- `README.md`：「TUI 长什么样」整节换成**真帧**（`draw_frame` 渲进 `TestBackend` 后 dump，120×24，不是手绘），下面重写左栏 / 主列 / 降级阶梯三段；仓库概览里那条「全屏四分区」也改掉。
- `docs/tui-manual-checklist.md`：⑩ 改成「左栏：mark / 文字身份 / 隐藏三档」（42×18 的旧阈值换成 120 / 80 的宽度档），⑨ 与 ④.3 的「右栏消失」改成「转录被压到 1 行」与新提示行宽度账，②/⑫ 里「中区块 / 右栏」的位置描述改到主列，⑭ 补一条「重开的历史里回合条已有格子」；**新增 ⑮「外壳改版：左栏 tab、状态行、回合条」**，逐条写怎么验与该看见什么（tab 点击与点填充线无效、tab 与弹窗的优先级、状态行三档、回合条焦点随滚动、点格跳转的落点、120 列提示行 4 条这一档、转录宽度账）。
  > **如实记一笔：这一轮的清单没有在真终端上跑过** —— 实现由 agent 完成、手上没有可交互的终端，所以 ⑩ 与 ⑮ 是**待人工过一遍**的条目，不是「已验证」。已在 ⑮ 的开头写明。
- `docs/adr/0002-fullscreen-alt-screen-tui.md`：追加一条「**外壳改版（后加，2026-09）**」，指明标题里的「四分区」与「顶部那块」一段自此只作历史记录、核心决定（alt screen / 转录自持缓冲 / 光标不依赖视口位置）不变；标题从「全屏四分区布局」改成「全屏布局」。

### 6. spec 回改（`.scratch/tui-sidebar/spec.md`）

四处，都带「实现期修正」的标记：
1. §1 的 `Regions` 字段表补 `main` 与 `sidebar_page`；
2. §4 的段首规则（「该单位之前最近的一条用户消息」→「该单位内第一条，取不到退到该单位首个源行」）；
3. §8 的删除清单：`SessionFacts.cwd` 的例外（改名 `session_dir`，因为 `read_tool_body` 真的在读它）+ `TICK` 标为已删；
4. §3 补一句：v1 的两条宽度退化路径**从外壳上够不到**（值列 21 / 33 列，缓存行 21 列刚好放下），只剩单元测试上的意义。

### 7. 基线

`cargo test` **721 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移（未顺手格式化）；`scripts/tui-startup-check.py` 4/4 GREEN。
