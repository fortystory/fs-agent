# 17: 可观测性 CLI

**What to build:** 事后能问三个问题——**这次为什么停**、**谁在哪一轮改了这个文件**、**有没有静默降级**——而且这些都是从事件流 group-by 出来的，不是新埋点。

Blocked by: 12, 14

Status: done

**参考:** spec §18（可观测性）、§11（会话目录）

- [x] 四个动词：`sessions ls` / `show <id>`（`--round` / `--speaker` / `--kind` / `--tool` / `--only-error`）/ `replay <id> --speaker X --round N`（**投影重算**，调试投影 bug 的唯一手段）/ `stats <id>`
- [x] **不建索引**（`seq` 就是行号，顺序读 + 内存过滤；索引会是第二个真相源）；输出表格或 `--json`；**stdout 只放结果、诊断走 stderr**
- [x] 默认视图**按轮次分组**，工具调用与结果归成一处，前缀与实时同形
- [x] `--files` **文件改动史**（唯一按「工作区对象」而非时间索引的视图，且是**推导**不是新字段）
- [x] 内置指标含两个**静默**量：**单侧缺席率**、**编辑匹配梯降级分布**；另有 per-agent token / 花费 / 命中率、每轮调用数、执行者失败原因分布、read-before-edit 拒绝与 read set 失效、权限决策分布、hook 三类结果、分歧率与停法分布
- [x] 编辑匹配那类诊断量以**约定文本**落在既有 payload 上，**渲染与解析共用一个格式常量**（漂移会让统计悄悄变成 0）；不加新变体
- [x] 这是给**人**用的 CLI，**不新增 agent 工具**
- [x] 验收（最重要的一条）：用 `replay` 重算出来的投影 **== 当时实际发给 provider 的 `messages`**——这条是「事件流是唯一真相源」的验收

## Comments

实现落点：`src/agent/replay.rs`（`replay` / `ReplayError`：把一件事重算出来）、`src/agent.rs`（新增 `build_messages`，`run_turn` 与 `replay` 共用这一条流水线；`scoped_events_slice` 让窗口过滤能吃一个显式切片）、`src/session/observe.rs`（`list` / `summarize` / `timeline` / `Filter` / `file_history` / `stats` / `CostModel`）、`src/cli.rs`（`run_sessions` 与四个动词、表格与 `--json` 渲染）、`src/tools/registry.rs`（新增 `READ_BEFORE_WRITE_PREFIX`）、`src/lib.rs`（`Harness::events`）。测试：`tests/replay.rs`（7 例）、`tests/observe.rs`（9 例）、`tests/observe_cli.rs`（7 例）。文档：`docs/observability.md`（新增）。

实现期把票面留白写实的几处（**第 1 条要回改 spec §18**）：

1. **`replay` 复现的是「某一次调用」，不是「这一轮的最终状态」。** 已完成的流里有那次调用的回答，直接投影整条流会把回答也算进去。切口取该说话者在该轮的**最后一个 `TurnStarted`**：循环在 append 它**之前**取快照（`agent.rs` 的 `let events = scoped_events(...)` 在 `emit(TurnStarted)` 之前），所以 `seq < TurnStarted` 恰好就是那次请求看到的全部；再叠上既有的 `TurnScope`，轮次窗口一字不差。推论：一个多迭代的工具回合里，`replay` 复现的是**该轮最后一次调用**，更早的迭代在 v1 不可寻址（票面签名没有逐迭代的旗子）。票 10 折回里那句「replay 必须复现轮次窗口规则」由此写实。
2. **流水线是共用的，不是两套今天恰好一致。** `agent::build_messages(events, speaker, caps, identity, trim_policy)` 是「投影 → 前置私有身份 → 裁剪」的唯一实现，`run_turn` 与 `replay` 都调它；验收因此是**构造性质**：只要流一样、身份一样、能力表一样，`messages` 必然逐字节相等，不靠两处代码自觉。
3. **身份从流的形状推**：它从不进流（spec §15），所以 `replay` 按 `SpeakerId` 与「这一流有没有 `RoundStarted`」推——讨论者 + 有轮次 → `debater_identity(name)`；执行者 → 执行者常量；单 agent（无轮次）→ 无身份。合成器不走这条：它的请求本来就不是投影（见下）。
4. **合成器是另一种形状**，`replay` 单独复现：身份 + `discussion::synthesis_prompt(question, 流)`；`question` 从合成轮的 `RoundStarted` 之前最后一条 `MessageCompleted{role: User}` 找回。它没有 `TurnStarted`，所以切口规则对它不适用——它的调用被合成轮的 `RoundStarted` 夹住。
5. **能力表跟着 `[routing]` 走。** 被路由的合成器 / 执行者可能在另一张 `ModelCaps` 上作答（例如 `requires_reasoning_replay` 不同），用讨论者的 caps 重算就是**静默说谎**。`replay` 先用 `CostModel::new(..).with_routing(&config.routing)`，再用 `CostModel::model_for(speaker)` 取模型 → `caps_for`。`CostModel` 内部就是 `SessionConfig` + `model_for(LandingPoint)`，没有第二份路由规则（spec §17「唯一一条规则」）。
6. **钱需要一个被点名的 model**：`UsageRecorded` 上没有 model 字段，所以 `stats` 只在 `--model`（缺省取 `config.default_model`）时计价，且**人读视图里写明按哪个 model 计价**——不把猜测伪装成事实。`replay` 因为要 caps，同样需要一个 model。
7. **两个静默量读的是生产者的原文**：`MATCH_LEVEL_PREFIX`、`WROTE_PATH_PREFIX`、新加的 `READ_BEFORE_WRITE_PREFIX`，以及失败匹配那句——它由 `edit_file` 以 `"<path>: {error}"` 落流（`tools/file.rs:303`），所以判据是 `error.ends_with(&EditError::NoMatch.to_string())`，而不是相等。**两轴 review 抓到的真 bug**：实现初稿写成相等，于是 `edits.failed_matches` 与 `guards.invalidated_reads` 恒为 0，而测试用裸的 `EditError` 文本伪造了输入，绿着把 bug 藏住了——两处一起改（判据 + 测试用带路径前缀的真实文本）。
8. **`--files` 从结果行的 `wrote:` 推导**（不是从调用参数）：`hook.pre` 可能改写参数，结果才是「实际写了哪个文件」的唯一记录（与票 11 的 executor 元数据同源）；成功的 `write_file` / `edit_file` 才计。
9. **`sessions` 也接受会话目录路径**（`show /path/to/session`），因为会话目录是可搬运单元、也是测试与手工诊断最顺手的入口；按 id 查找则先扫 `--cwd` 桶、再扫全库（没有索引，`seq` 只是行号）。
10. **`Harness::events()`**（单 agent harness）与既有的 `DiscussionHarness::events()` 同形，是验收测试与对人的 CLI 共用的读取口。

**没改的（judgement call，写下来免得下次又提）**：`EventPayload` 的穷尽匹配点（`entry_of` / `Entry::kind` / `Entry::speaker` / `Entry::is_error` / `stats` / `render_entry`）不合并成一张表——spec §2/§5 明确要求「没有 `_` 分支」，加一个 payload 变体就得多处表态是**这条标准本身**，不是 shotgun surgery。`Count` / `TimelineGroup` 已按 review 改名；`Entry::{Hook,History,Context,Usage}` 的 `kind()` 返回事件名是对的（`--kind HookExecuted` 要能用）。逐轮游走的三处小循环（`last_call_cut` / `file_history` / `stats`）各自体不同，没抽通用 walker（那是 speculative generality）。

**两轴 review（Standards / Spec）与处理**：

*Standards*：`AgentStats` 违反 `CONTEXT.md:7`「`agent` **不作为类型名**」→ 改名 `SpeakerStats`，字段 `per_agent` → `per_speaker`；补齐 9 个公开 struct 的文档；`cli.rs` 的模块 `//!` 陈旧（还说「不碰 session store」）已改写；`cli::observe_kind` 与 `observe::normalize_kind` 是同一形状 → 收成公开的 `observe::canonical_kind`（`--kind` 过滤与「这是不是 usage 行」从此同一个归一化）；`session_cost` 改为直接求和已算好的 `SpeakerStats::cost`（原来重算一遍，还自相矛盾地宣称「不是第二份估算」）；`CostModel` 改为持 `SessionConfig` 并复用 `model_for(LandingPoint)`；删掉 `identity_for` 里已不可达的 `System` 分支与三个没人用的 `err` 参数；测试里硬编码的 `EditError` 文本改成用生产者自己的 `Display`。

*Spec*：一条真缺陷（第 7 条，已修 + 测试换了能抓到它的输入）；一条缺失（**分歧率**）已补 `Stats::divergence_rate`、人读视图与 JSON 都有；一条部分（`--files` 只认 `--round`/`--speaker`）已让它同时认 `--tool`，并在 `--help` 与文档里写明它不认 `--kind`/`--only-error`（那两者是「条目」形状的过滤，对一条文件改动没有意义）；一条实现风险（`replay` 不跟 `[routing]`）已按第 5 条修掉。
