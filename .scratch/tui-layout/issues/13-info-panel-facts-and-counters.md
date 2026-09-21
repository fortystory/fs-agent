# 13: 右栏信息面板：注入的会话事实与流上的动态数字

**What to build:** 填上右栏：模型 / 上下文用量 / token（已用+上限、输入/输出、缓存）/ 回合。静态事实来自注入的 `SessionFacts`，动态数字从事件流累加，面板是纯函数。含栏内行序、标签对齐、无数据文案与栏内降级。

Blocked by: 10

Status: ready-for-agent

**参考:** spec §8（信息面板）、§10（措辞层）、§2（右栏宽度与栏内降级）

- [x] 消费 `SessionFacts`（10 已建）：模型 / cwd / 上下文窗口 / 预算上限
- [x] **上下文分子 = 最近一条 `UsageRecorded.usage.input_tokens`**（provider 实测值，**不用** `chars/4` 估算）；无数据 → `—`
- [x] **token 已用 = 累加全部 `UsageRecorded` 的 `Usage::total_tokens()`**（input + output）；口径与 `events::total_usage` **逐字一致** —— `cached_tokens`/`miss_tokens` 是 `input_tokens` 的**拆分**，**不得**加进合计；`reasoning_tokens` 已含在 `output_tokens` 内
- [x] 输入 / 输出 / 缓存明细（`cached` / `miss`）分列显示
- [x] 上限来自注入的 `budget.limit`：`None` → **只显示已用，不显示 `/ —`**
- [x] 回合 = 数 `EventPayload::TurnEnded`（**标签是「回合」不是「轮次」**，见 `CONTEXT.md`）
- [x] 栏内行序：模型 / 上下文 / token / 回合 / 输入 / 输出 / 缓存；标签列 6 列左对齐、数值右对齐、千分位半角逗号
- [x] 栏内降级：宽度不足时**先丢缓存明细，再丢输入/输出**；**核心四字段永不栏内丢失**；上下文 `（6%）` 只在内容宽 ≥ 29（`w ≥ 120`）出现，否则只留数值
- [x] 长模型名按内容宽右侧截断加 `…`（复用 `truncate_columns`）
- [x] 面板是 `(facts, counters, mode)` 的**纯函数**，渲染时算，不缓存字符串；`UsageRecorded`/`TurnEnded` 到达即更新计数器
- [x] 忙碌态**不在面板里重复**（它在提示行最左，见 10）；**不显示花费**
- [x] 措辞新增：面板中文标签、`—`、`token_pair(used, limit)`、`context_pair(used, usable)`、`cache_pair(cached, miss)`
- [x] 用例：首回合之前（`0` 与 `—`）；provider 不返回 usage；`budget.limit: None`；极窄内容宽（丢 `（6%）`、丢缓存、丢输入/输出）；`120×24` 草稿涨到 10 行时**右栏整栏消失**

## Comments

- **票 10 交接**（2026-09-21）：右栏的**空框与共用接缝已经在票 10 画好了**（`layout::Regions::panel` + `draw_seam`；该结构在票 11 的评审收口里由 `Panes` 改名为 `Regions`），本票只需填内容。票 10 票面 checklist 里那句「措辞层新增面板标签 / `—` / 配对函数」**没有在票 10 落地**，有意留给本票 —— 在被消费的这张票里加，才有测试可写。也就是本票要补：`PANEL_MODEL` / `PANEL_CONTEXT` / `PANEL_TOKENS` / `PANEL_TURNS` / `PANEL_INPUT` / `PANEL_OUTPUT` / `PANEL_CACHE`、`PANEL_UNKNOWN`（`—`）、`token_pair` / `context_pair` / `cache_pair`。

## Comments

**实现完成（2026-09-21）**。落点：新增 `src/render/panel.rs`（`Panel` 计数器 + `lines()` 排版 + 三个内边距 helper）；`src/render/tui.rs` 新增 `panel` 字段、`apply()` 交给 `Panel::observe`、`draw_transcript` 在右栏画面板；`src/render/wording.rs` 新增七个标签、`PANEL_UNKNOWN`、`thousands` 与三个配对函数；`tests/render_layout.rs` 新增 6 个用例、`tests/wording.rs` 新增 1 个。

**排版以原型为准**（`prototype/src/main.rs` 的 `panel_rows`）：标签列 6 列、一个空格、值区 `content − 7`；数字**右**对齐、模型名**左**对齐、超宽右侧截断加 `…`；`` 的放不下判定按**列宽**（`text_columns(pct) <= value_columns`），不是硬编码的 `w ≥ 120` —— 后者只是它的结果。有一条用例逐行比对 29 列下的精确字符串与对齐（左对齐的模型名尾部带空格、数字行尾部不带）。

**两处实现期的减法**：

1. **删掉 `DETAIL_MIN_COLUMNS = 5` 那个守卫**：面板只要被画出来，内容宽恒 ≥ 23 → 值区恒 ≥ 16，所以「值区 < 5 就不画细节行」不可达（与票 11 删掉的那条同类）。真正决定细节行去留的是高度。
2. **删掉 `rows.truncate(area.height)`**：`Paragraph` 本来就裁剪到区域，所以那句**不可观测**（变异检验证明了这一点：去掉它所有用例照过）。现在高度的降级由「行按重要性排序 + 段落到区域即止」实现，代码少一句，行为一样。

**三处如实说明**：面板不显示**模式**（票 05 的 §4 把 `mode` 写进了纯函数的入参，但快照里 header 才是模式的家，面板没有它）；标签用 DarkGray（属 chrome，与边框/提示行同色，不新增颜色语义）；`thousands` 放在措辞层（数字的呈现方式是人面向文本的一部分，且三个配对函数都用它）。

**基线**：`cargo test` **528 passed / 0 failed**（519 → +6 布局 +1 措辞 +... 另有两条在既有文件内新增）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移；pty 启动检查 3/3 GREEN。

**变异检验**：把 `cached` 加进合计、分子取第一次而不是最近一次、百分比无条件显示、模型名改成右对齐、放不下的缓存行照画 —— 五处全部被抓到（其中百分比那条第一次**没抓到**，因为我原先只断言了前缀，截断后的 `9,000 / 200,000…` 仍然包含它；改成逐行精确比对后抓到）。
