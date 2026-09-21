# 14: 成本与预算

**What to build:** 花钱看得见、撞顶收得住——会话累计到一个上限时**降级并收尾**（把讨论推到合成、不再派新执行者），而不是半路急刹。

Blocked by: 11

Status: done

**参考:** spec §17（成本与预算）、§10（累计是派生值）

- [x] 三个量各管一段：**窗口**按 agent 的模型、**轮数**（讨论 2 / 执行者 25）、**token 累计是会话级且讨论者与执行者共享**（由 usage 事件**求和派生**，不是状态）
- [x] 硬停行为 = 降级并收尾：不再开第二轮、**直接进合成**（合成是唯一不能省的调用）；不再派新执行者、**已在跑的跑完**；`RoundEnded { reason: BudgetExhausted }` 落流
- [x] 闸门看 token，**费用只作显示**；价目表可配，且 `cached` 与 `miss` **分开计价**
- [x] 前置估算的阈值用**剩余额度的比例**（不用绝对值，否则估算不准会频繁误拒）
- [x] 弱模型分流**只有两个落点**（合成器 + 执行者，可配覆盖）；**讨论者绝不换**；v1 默认全用讨论者模型
- [x] 日账本按 UTC 日聚合、**从今天的会话文件派生**（不新增状态文件）
- [x] `BudgetExhausted` 与 `Completed` / `MaxIterations` / `RoundsExhausted` 在显示上**可区分**
- [x] e2e：把累计上限调到很低 → 断言讨论不开第二轮、直接进合成、落 `BudgetExhausted`

## Comments

落地（2026-09-21），已按 `/code-review` 的两轴复审改过一轮；机制细节折回 spec §17（票 14 段）。

- **配置面**：`[budget] session_tokens` / `estimate_margin`、`[pricing.<model-id>] miss_input / cached_input / output`（三者都必填，缺一等于把一类 token 悄悄按 0 计价）、`[routing] synthesizer_model / executor_model`。model id 未登记一律启动报错（价格、路由同理）。`Config::session_config()` 是「文件配置 → 每个 agent 的注入值」的唯一一处，probe 与以后的会话组装都走它。
- **闸门有三处，只读求和**：`agent::run_turn` 每次迭代开头（撞顶 → `TurnEnded { BudgetExhausted }`）+ 裁剪后的前置估算（`Budget::admits_estimate`，阈值 = 剩余 × 1.5）；`run_discussion` 轮次循环开头（撞顶 → 关掉「没结束辩论的那一轮」并直接进合成）；`process_call` 的 `task` 分支（撞顶 → 该调用补一条错误结果，不派新执行者）。`run_single_shot` **故意不设闸**：合成是唯一不能省的调用。
- **执行者的回合免检**（复审改）：闸门只在「派不派」上管执行者，已在跑的执行者跑到自己的轮数上限为止，所以 `ExecutorFinished` 拿不到 `BudgetExhausted`（§16 的失败四值不变）。它的花费照常计入累计，于是**派发者**的下一个回合会被停住——这条测试钉在 `tests/executor.rs`。
- **判定两处判断**（已折回 spec §17）：
  1. 「阈值用剩余额度的比例」读作**宽容**方向——`estimate <= remaining * margin`（默认 1.5），因为「估算 > 剩余」正是 spec 说要避免的频繁误拒；真正的硬停仍是观测到的累计值。`margin = 1.0` 即那条被替代的严格比较，有测试钉住。
  2. 日账本**只聚合 token、不算钱**：`UsageRecorded` 上没有 model 字段，费用归属需要名册（在配置里，不在流上），硬算就是编。费用由知道 model 的视图（`probe`，以后的 `sessions stats`）用 `PriceTable::cost` 显示。
- **零轮次不补 `RoundEnded`**：额度在开赛前就没了时讨论跑 0 轮直接进合成，没有开过的轮次不关（§15 的推论），`BudgetExhausted` 由 `DiscussionOutcome.reason` 与诊断承担——这是对验收项「`RoundEnded { reason: BudgetExhausted }` 落流」的一处明确边界，不是漏项。
- **证据**：纯规则、`exhausted_note` 与日账本在 `tests/budget.rs`；讨论侧 e2e 在 `tests/discussion.rs`（不开第二轮 / 零额度也进合成 / 合成器换模不动讨论者 / 预算不一致组装报错）；回合闸与前置估算在 `tests/e2e_single_turn.rs`；执行者两闸在 `tests/executor.rs`；配置面（价格 / 预算 / 路由 / 报错）在 `tests/config_profiles.rs`。

