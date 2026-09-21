# 13: 右栏信息面板：注入的会话事实与流上的动态数字

**What to build:** 填上右栏：模型 / 上下文用量 / token（已用+上限、输入/输出、缓存）/ 回合。静态事实来自注入的 `SessionFacts`，动态数字从事件流累加，面板是纯函数。含栏内行序、标签对齐、无数据文案与栏内降级。

Blocked by: 10

Status: ready-for-agent

**参考:** spec §8（信息面板）、§10（措辞层）、§2（右栏宽度与栏内降级）

- [ ] 消费 `SessionFacts`（10 已建）：模型 / cwd / 上下文窗口 / 预算上限
- [ ] **上下文分子 = 最近一条 `UsageRecorded.usage.input_tokens`**（provider 实测值，**不用** `chars/4` 估算）；无数据 → `—`
- [ ] **token 已用 = 累加全部 `UsageRecorded` 的 `Usage::total_tokens()`**（input + output）；口径与 `events::total_usage` **逐字一致** —— `cached_tokens`/`miss_tokens` 是 `input_tokens` 的**拆分**，**不得**加进合计；`reasoning_tokens` 已含在 `output_tokens` 内
- [ ] 输入 / 输出 / 缓存明细（`cached` / `miss`）分列显示
- [ ] 上限来自注入的 `budget.limit`：`None` → **只显示已用，不显示 `/ —`**
- [ ] 回合 = 数 `EventPayload::TurnEnded`（**标签是「回合」不是「轮次」**，见 `CONTEXT.md`）
- [ ] 栏内行序：模型 / 上下文 / token / 回合 / 输入 / 输出 / 缓存；标签列 6 列左对齐、数值右对齐、千分位半角逗号
- [ ] 栏内降级：宽度不足时**先丢缓存明细，再丢输入/输出**；**核心四字段永不栏内丢失**；上下文 `（6%）` 只在内容宽 ≥ 29（`w ≥ 120`）出现，否则只留数值
- [ ] 长模型名按内容宽右侧截断加 `…`（复用 `truncate_columns`）
- [ ] 面板是 `(facts, counters, mode)` 的**纯函数**，渲染时算，不缓存字符串；`UsageRecorded`/`TurnEnded` 到达即更新计数器
- [ ] 忙碌态**不在面板里重复**（它在提示行最左，见 10）；**不显示花费**
- [ ] 措辞新增：面板中文标签、`—`、`token_pair(used, limit)`、`context_pair(used, usable)`、`cache_pair(cached, miss)`
- [ ] 用例：首回合之前（`0` 与 `—`）；provider 不返回 usage；`budget.limit: None`；极窄内容宽（丢 `（6%）`、丢缓存、丢输入/输出）；`120×24` 草稿涨到 10 行时**右栏整栏消失**

## Comments

- **票 10 交接**（2026-09-21）：右栏的**空框与共用接缝已经在票 10 画好了**（`layout::Panes::panel` + `draw_seam`），本票只需填内容。票 10 票面 checklist 里那句「措辞层新增面板标签 / `—` / 配对函数」**没有在票 10 落地**，有意留给本票 —— 在被消费的这张票里加，才有测试可写。也就是本票要补：`PANEL_MODEL` / `PANEL_CONTEXT` / `PANEL_TOKENS` / `PANEL_TURNS` / `PANEL_INPUT` / `PANEL_OUTPUT` / `PANEL_CACHE`、`PANEL_UNKNOWN`（`—`）、`token_pair` / `context_pair` / `cache_pair`。
