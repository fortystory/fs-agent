# grilling：测试与验证迁移

Type: grilling
Status: resolved
Blocked by: 02, 03, 04, 05, 06, 07
Part of: ../map.md

## Question

把本图所有决定的**可验证性**定下来：哪些进 `cargo test`、哪些只能进 pty 脚本、哪些只能进真终端手工清单；以及哪些既有测试与文档要改。沿用 `.scratch/tui-layout/issues/08`（测试与验证迁移）的先例。

## 需要定

1. **鼠标命中的自动测试**：`TestBackend` 下构造 `CtEvent::Mouse` 喂进渲染循环的先例已有（spec §Testing Decisions）。要定：点击覆盖层按钮 / 问卷选项 / 折叠提示行 / 详情滚动分别断言什么（buffer 文本变化？`TuiState` 的 pending 结果？`Questionnaire` 的 draft？）；`indicator` 命中已有的测试怎么扩。
2. **折叠与详情覆盖层的自动测试**：提示行文案与颜色的断言；详情覆盖层的滚动（`↑/↓` / `PgUp/PgDn` / 滚轮）；指针失效降级；`--continue` 重放出的「思考完成」行；这些需要什么样的 fixture（构造 `RenderEvent` / 假 provider / 临时会话目录里的 `outputs/*.txt`）。
3. **几何与降级**：删 airy 后的尺寸矩阵与逐宽度提示行条目表进 `tests/render_layout.rs`；Mark 间距的断言；`120×24` 等旧结论若被推翻，旧断言怎么改（**逐条列出要改的测试名**）。
4. **配色断言**：`TestBackend` buffer 的 `fg` 断言口径（`07` 的答案落地后）。
5. **`Ctrl-D`**：空闲确认、默认答案、忙时忽略、提示行恒在与降级的断言。
6. **既有基线与纪律**：开工前核实 `cargo test --all-targets` 的准确通过数（charting 时实测为 **633 passed / 0 failed**；票据 31 收尾时是 601，票据 32 落地后已涨）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只留 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移（**不要**顺手格式化）；新测试是否要遵守「断言看得见的东西，不断言私有函数」的既有口径。
7. **pty 与手工面**：`scripts/tui-startup-check.py` 是否需要新增锚点（删 airy 后首帧形状变了，`ctrl-c` / `fs-agent：` / `MARK_ROW` / 边框计数等锚点是否仍有效）；它的 docstring 明说「光标、鼠标、resize 留在手工清单」——点击作答与详情滚动只能手工验的边界要写清；`docs/tui-manual-checklist.md` 要新增哪些项（点击作答、`Ctrl-D`、折叠提示与详情、角色配色、删 airy 后的观感）。
8. **spec 回改清单**：把「哪些 tui-layout spec 章节要被这次改动回改」列成一张清单交给 `/to-spec`（§2 / §3 / §4 / §6 / §9 / §10，见 `map.md` Notes），并确认没有遗漏的不变量（ADR 0001 冻结的模型可见文本、plain/headless 不动）。

## 先读

- `tests/render_tui.rs` / `tests/render_layout.rs` / `tests/render_editor.rs` / `tests/wording.rs` / `tests/ask_user_question_tui.rs`
- `tests/support/`（既有 fixture 形态）
- `scripts/tui-startup-check.py`（docstring 与锚点）
- `docs/tui-manual-checklist.md`
- `.scratch/tui-layout/spec.md` 的 `Testing Decisions` 与 `Further Notes`（几何数字的唯一来源口径）
- `.scratch/tui-layout/issues/08-grilling-test-and-verification-migration.md`（先例）
- `docs/agents/issue-tracker.md`（本图产物与 `Part of` / `Blocked by` 约定）

## 答案落点

答案定到**可执行的清单级**：新增/改写的测试用例名与断言对象、pty 锚点增删、手工项编号与步骤、spec 回改清单。不要写实现代码。

## 进度

**100%** —— 完成。一轮 grilling、4 条决定；契约见 `## Answer`（分层、新增断言清单、既有测试逐条改动、pty 增删、手工项、基线、spec 回改汇总）。

**下一步**：无（已 resolved）。本图随 `08` 达到 **8/8**；实现按本票的清单落地，spec 由 `/to-spec` 回改。

## Answer

**验证契约（2026-09-23，4 条决定）。**

### §1 分层

- **行为 → `cargo test`**：合成 `CtEvent::Mouse` / `Key` 事件喂 `TuiState`，用 `TestBackend` 出帧断言。
- **终端归属 → `scripts/tui-startup-check.py`**：只有 pty 看得见的启动首帧与退出交还。
- **手感 → `docs/tui-manual-checklist.md`**：真配色、点击精度、滚动、Mark 字形。

### §2 `cargo test` 要新增的断言

- **鼠标命中（票 04 §8）**：中段覆盖层四种问题各点一次候选键区间 → 与按键结果一致；点正文 / 空白无动作；被裁的键不可点。问卷单选单击前进、**最后一题单击不提交**、多选单击只切换不前进；底部 `← 上一题` / `下一题 →` / `提交` 的可用性与动作；滚轮移高亮；点自定义行后光标出现、切题复位。失效：`modal()==None`、窗口滚动后、resize 后、`RunState{running:false}` 撤回后旧坐标不再生效。分派：详情打开时点击不影响转录；有待答问题时折叠提示不可点。
- **折叠与详情（票 02/03）**：提示行状态机（正在思考 → 正文到达**原地定格** → 思考完成；交错开新行；无 reasoning 不插行；合成器「有流无全文」）；文案与颜色（`▸`、`…`/`✓`、失败在行尾、post-hook 反馈留在折叠态）；详情覆盖层滚动（`↑/↓`、`PgUp/PgDn`、滚轮）、`Esc` / 再点关闭、打开时转录冻结、不可叠在问题上；指针降级（`outputs/<id>.txt` 读不到 → 预览 + `全文不可用`；超 200 000 字符 → `已截断`）。fixture：临时会话目录 + `outputs/*.txt`。
- **几何与降级（票 05）**：尺寸矩阵换成新数字；新增 `80×13/14`（右栏阈值）与 `42×17/18`（Mark 阈值）；Mark 间距三行的位置。
- **提示行（票 06）**：空闲行新梯子 40/60/80/120；忙时 / 查看行不变；`ctrl-c/ctrl-d 退出` 恒在。
- **配色（票 07）**：名字 cell 的 `fg` **精确等于**文档色（如讨论者 1 == `Color::LightCyan`），正文 cell 不是角色色；四种 speaker + 讨论者顺序 + 名册外兜底；无 speaker 的行不受影响。
- **Ctrl-D（票 06）**：空闲弹确认 / `y` 退出 / `Esc` 与其它键 = 否 / 忙时无反应 / 详情打开时关详情 / 有问题时忽略。

### §3 既有测试的逐条改动

- `tests/wording.rs`
  - `the_hint_ladder_is_the_one_the_prototype_measured` 与同文件里断言 `ctrl-c 退出` 的几处（约 415–495 行）：**空闲行**改成 `ctrl-c/ctrl-d 退出`，**忙时 / 查看行**不变。
  - `no_hint_ever_names_shift_enter`：不变（仍不得出现 `shift+enter`）。
- `tests/render_layout.rs`
  - `every_size_in_the_matrix_draws_the_regions_its_budget_allows`：删掉 airy 的两条断言（`40×12` 有 / `40×10` 没有），换成「任何尺寸都没有 airy」；新增 `80×14` 右栏出现。
  - `a_tall_draft_takes_the_panel_away_whole`：断言仍成立（面板消失），注释里「三行中段」改成「**输入上限 9、中段 1 行**」。
  - `the_mark_is_drawn_on_the_top_rows_and_is_lit_from_above`：`120×24` 仍成立；新增 `42×17`（文字 header）与 `42×18`（Mark）的边界断言。
  - `the_hint_row_gives_up_hints_before_it_gives_up_the_way_out` 与 `a_session_with_no_line_being_read_promises_only_what_the_keyboard_does`：按 §2「提示行」更新；查看行不变。
  - `a_floor_sized_terminal_still_draws_every_region`（`40×10`）：不变（已核实）。
- 其余测试不动——它们断言文本，本图不改文本口径。

### §4 pty 脚本

- `GESTURES` 加第三条 **`("ctrl-d", b"\x04y")`**（`Ctrl-D` 再按 `y`）。
- 锚点复核：`STATUS_ANCHOR = "ctrl-c"` 仍匹配（新文案含 `ctrl-c/ctrl-d`）；`BANNER_ANCHOR`、`MARK_ROW`、边框计数不变。判据不变：退出后不得留 raw mode / 鼠标报告 / bracketed paste / alt screen。

### §5 手工清单

- ④.3、④.5：`ctrl-c 退出` 文案更新；`120×24` 草稿 10 行的描述按新输入上限（9）改。
- ⑦ 退出后干净：加 `Ctrl-D → y` 路径。
- ⑩：**修既有漂移**——把 `41×19` 改成实测 **`42×18`**（宽 42 = 38 标记 + 2 边框 + 2 留白；高 18 由票 05 重算）。
- 新增项：折叠提示与详情覆盖层（点 `▸`、滚动、`Esc` / 再点关闭、打开时冻结）；鼠标点击作答（四种模态 + 问卷，含多选 / 翻页 / 提交）；角色配色（四种角色的真配色、浅色主题下的可读性）；`Ctrl-D` 的行为边界（忙 / 详情 / 问题）。
- 明确**不列** hover（any-motion 未开，见 map 的 Out of scope）。

### §6 基线与纪律

- 开工前核实：`cargo test --all-targets` 当前 **633 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只留 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移（**不**顺手格式化）。
- 新测试遵守「断言看得见的东西，不断言私有函数」；`fg` 是新增的断言口径。

### §7 给 `/to-spec` 的 spec 回改清单（汇总）

- **§2**：删 airy；几何表按 `.scratch/tui-ux/prototype/geometry/geometry-table.md` 的「新」列重写；`LOGO_MIN_HEIGHT = 18`；右栏最小 **80×14**；Mark 间距（logo 5 + 空 1 + 信息 1）；**并修正 Mark header 引入后遗留的 header 内容行列**。
- **§3**：工具输出折叠 + 思考提示行。
- **§4**：鼠标命中（覆盖层 / 问卷）+ `mouse()` 分派优先级。
- **§6**：键位表加 `Ctrl+D` 与其边界表。
- **§9**：详情覆盖层 + 四种问题的鼠标命中。
- **§10**：提示行两个 exit 常量与新梯子；角色配色（`speaker_color` / `DEBATER_PALETTE` / `SessionFacts.speaker_order`）。
- **两处既有漂移**（本图发现、不是本图引入）：spec §2 几何表早于 Mark header（归 `/to-spec`）；`docs/tui-manual-checklist.md` ⑩ 的 `41×19` vs 代码 `42×20`（归本票 §5 修）。

### §8 handoff

- 本图 8/8 resolved 且雾清空 ⇒ 路线 clear。**不要直接 `/implement`**：`/to-spec`（含 §7 回改）→ `/to-tickets` → 每票一次 `/implement`（fresh session、票间 `/clear`）→ `/code-review` 双轴。
