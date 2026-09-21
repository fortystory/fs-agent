# 10: 讨论协议

**What to build:** 两个异构讨论者就同一个问题各自作答、**只在结论真的冲突时**才互相回应，最后由合成器把「共识 / 分歧（含各自成立的前提）/ 未决」摆给用户——它画出选项空间，**不替用户收敛**。

Blocked by: 07

Status: done

**参考:** spec §15（讨论协议）、§2（轮次事件）、§17（预算撞顶）

- [x] 两个讨论者（KIMI + DeepSeek）**同一轮并发**发；**独立首轮互不可见**（保住多样性）
- [x] 每个讨论者在正文后**另起一行给一行结论**；归一化后**精确相等或子串包含**即判为一致（**零额外调用**、无假阴性）
- [x] **只在冲突时**开第二轮：明确告知「你看到了对方的回答、请针对分歧回应」；上限 2 轮（可配）
- [x] `RoundStarted{round, mode}` / `RoundEnded{round, reason}` / `DivergenceRecorded{round, topic, positions}` 落流；第三轮不需要新机制（模式槽位已有）
- [x] 合成器 = **一次独立单发调用**（不是 agent 身份、无工具、无回合），产出三档，落在 `speaker_id = System` 的 `MessageCompleted` 上；揭示发言**全文**、不揭示私有推理
- [x] 协议指令常驻**私有身份**、**不进流**（否则「第二轮的 messages 能从流重算」当场不成立）
- [x] 单侧失败 → 该方本轮**缺席、讨论继续**，且缺席**能从流上查出来**（该方自己的 `TurnEnded{Error}` + 该轮没有 `MessageCompleted`）；双侧失败 → `RoundEnded{Error}` + `SessionError`；**不重跑**
- [x] e2e（假 provider 数调用次数）：无分歧 = **3 次**调用、有分歧 = **5 次**
- [x] `RoundEnded` 四个值（含 `BudgetExhausted`）在渲染上可区分

## Comments

实现落点：`src/discussion.rs`（协议策略：`DEBATERS = 2` / `DEFAULT_MAX_ROUNDS = 2` / `plan_after_round` / `RoundPlan` / `debater_identity` / `synthesizer_identity` / `synthesis_prompt` / `position_of` / `divergence_topic`）、`src/discussion/protocol.rs`（新，纯函数：`CONCLUSION_MARKER` / `conclusion_of` / `normalize` / `answers_agree` / `round_attendance` / `round_outcome`）、`src/agent.rs`（`TurnScope` + `scoped_events` / `Debater` / `SingleShot` / `Discussion` / `DiscussionOutcome` / `run_discussion` / `run_single_shot` / `record_round_started|ended` + `record_divergence` + `record_session_error`）、`src/events.rs`（`pending_tool_calls_of`；`EventLog` 可变共享句柄）、`src/session.rs`（`Arc<Registry>`、共享 `Policy`、私有 `identity`）、`src/lib.rs`（`SessionScaffold` 抽取 + `assemble_discussion` + `DiscussionHarness`）、`src/render.rs`（轮次边界 + 合成产物独占 stdout）、`src/context.rs`（`pinned_len` 认首条 `system`）。测试：`tests/discussion.rs` 36 例（纯函数 + e2e 剧本）、`tests/support/fake_provider.rs` 增会合点 `meeting_at`。文档：`docs/discussion.md`（三层分工 + 轮次结构 + 两条不许破的规则 + 失败矩阵）。回改：spec §15 六条机制 + `Further Notes` 的「票 10 回改」+ 不变量 (2) 的作用域。

实现期把票面留白写实的几处（都不改 spec 的决定）：

1. **独立首轮 = 结构性投影窗口，不是时序假设**。第 R 轮某讨论者的 `messages` = 「`seq ≤ 该轮 RoundStarted`」∪「自己的后续事件」（`agent::TurnScope::Round`）。两个讨论者用 `join_all` 同轮并发，先返回的一方会立刻把发言写进共享流——靠「对方还没作答」判独立在假 provider 下当场破。窗口切在 `seq` 上，与交错顺序无关。**变异验证**：把窗口关掉，`the_first_round_hides_the_other_debater_and_the_targeted_round_reveals_it` 立刻变红。第二轮因此天然看到第一轮对方的作答（揭示），不需要额外机制。
2. **协议指令 = 常量的私有身份**（`debater_identity` / `synthesizer_identity`），经 `Session::identity` 作为请求首条 `system` 消息前置，**不进流**。定向第二轮的规则写在常量里而不是每轮改 system prompt：system 是缓存前缀的头，中途改会废掉整个前缀缓存（同 §4 的 `reasoning_effort`）。测试断言身份到达模型、且日志原文里没有它的字样。
3. **结论行用共用常量标记 `CONCLUSION:`**，取最后一条非空标记行；归一化 = 去空白/标点/装饰、转小写；一致 = 精确相等或**双向**子串包含。**不加最小长度阈值**（阈值会引入假阴性）；**无结论绝不判为一致**（空串是任何串的子串）。
4. **`RoundEnded` 只由结束辩论阶段的那一轮发**（外加每个合成轮，理由 `Completed`）。四个终局值因此总是终局理由，渲染/统计可直接分发；为开定向轮而结束的那一轮不发 `RoundEnded`，下一个 `RoundStarted` 是它的边界（投影侧同样以 `RoundStarted` 收口）。`BudgetExhausted` 的产生点留给票 14（在 `plan_after_round` 之前那道硬停），本票只保证它在渲染上可区分。
5. **单侧缺席取 `RoundEnded{NoDivergence}`**（主张最少的那个），缺席由该方 `TurnEnded{Error}` + 该轮没有 `MessageCompleted` 表达，查询走 `round_attendance`。**双侧无人作答**（`answers.is_empty()`）才是 `RoundEnded{Error}` + `SessionError`，且**不调合成器**。合成失败（provider 报错 / 未到 `[DONE]` / 产出为空）同样记 `RoundEnded{Error}` + `SessionError`、产物空串、不重跑。假 provider 的调用次数在三种剧本里分别是 3 / 5 / 3（单侧缺席）与 2（双侧失败）。
6. **合成器的输入由流上重算**：`synthesis_prompt(question, events)` 逐轮取 `round_attendance`，缺席方被写成「本轮缺席」。这是「只有一份回答被读成共识」那条危险误读的收口，测试直接断言提示词里出现缺席方名字与「缺席」。揭示的是发言全文，私有推理（`reasoning`）不进提示词。
7. **两块前置重构**（都为「两个讨论者并发写一条流」服务，行为不变、旧 16 个测试套兜底）：(a) `EventLog` 内部 `Arc<Mutex<_>>` 的共享句柄，`events()` 返回快照——`Mutex` 让每条事件仍原子落入，§2 的「单一写入者」仍是事实而非约定；`project()` 入参随之从 `&EventLog` 收窄成 `&[Event]`。(b) `Session` 的 `tools` 变 `Arc<Registry>`、`policy` 变 `Arc<Mutex<Policy>>`（A 记住的允许要能到 B）、新增 `identity`；组装层抽出 `SessionScaffold`，`AssemblyParts` / `DiscussionParts` 各嵌一块。
8. **并发是「同一 task 上交替推进」**：`join_all` 轮询两个回合，一方等 provider 流时另一方推进——对两次网络调用来说这就是并发。`meeting_at` 会合点假 provider 把这条钉住：顺序执行会在第一发上撞 5 秒超时而不是拿到流。**顺带修掉一个并发下的真 bug**：`pending_tool_calls` 原先是会话级的，对方在飞的 `tool_call` 会被读成自己欠着结果（不变量 2）——现在循环用 `pending_tool_calls_of(events, speaker)`，会话级那个留给 `--continue` 诊断。
9. **`discussion` 不碰 `provider` 的边界是活的**：身份文本、结论判定、轮次策略、合成提示全在 `discussion`（纯策略）；provider 调用、事件写入全在 `agent`（§3 的控制流归 agent 层 + 不变量 3 只有循环写流）。讨论者的会话是**长寿命**的（一块 scaffold 开两个 `Session`），所以 read set 与「本会话记住允许」跨轮累积。
10. **交接**：票 11 的 `task` 工具把执行者接进来时，执行者事件落同一条流、`round_attendance` 已按「只数 `Debater`」把它们排除（有测试）；票 14 的硬停在 `run_discussion` 的 `match plan_after_round` 之前加一道预算查询，把开定向轮换成 `RoundEnded{BudgetExhausted}` 后直接进合成；票 17 的 `sessions replay` 必须复现第 1 条窗口规则与常量身份，否则重算的 `messages` 与实际发出的不等；票 18/19 消费 `RoundEnded` 终局值做停法分布（不变量：它不再承载「非终局」）。
11. **已知留白**：`DiscussionHarness::discuss` 一个 harness 只跑一次讨论（轮次编号按讨论计，再问一次会从 1 重开）——已在方法文档写明；N ≠ 2 的讨论在组装期报错（N > 2 要先重开「N = 2 不裁决」那条决定）；`repo_map` 的会话相关度按讨论者自己的窗口重算，不掺对方发言；**合成请求不走 `trim`**（`synthesis_prompt` 把各轮作答全文拼起来，spec §15 要的正是「揭示全文」，所以这里没裁）——真跑长讨论时若撞窗口，接法是让票 14 的预算闸门在合成前兜住，而不是在提示词里偷偷截断。
12. **`agent.rs` 从 975 行长到 1,381 行**（轮次循环 + 单发调用各约 200 行）：分层理由写在 `agent.rs` 与 `discussion.rs` 的模块文档里（§3 的控制流归 agent 层、不变量 3 只有循环写流）。若将来还要往控制流里加东西，先按 `provider/projection.rs` 的先例拆 `agent/rounds.rs`，不要继续堆这一个文件。

### 两轴复查（`/code-review`，2026-09-21）

**Standards** 报 1 处硬违规 + 1 处约定 + 6 条 smell。已改：

- **硬违规**：`struct SingleShot` 与词表冲突（CONTEXT.md 定 合成器 的代码标识符是 `Synthesizer`，spec §1/§15 同）——已重命名为 `agent::Synthesizer`，一个概念一个名字。
- **约定**：仓储里 08/09 两票都为各自的领域写了 `docs/{skills,repo-map}.md`，本票补 `docs/discussion.md`。
- **重复代码**：`position_of` 的 fallback 与 `divergence_topic` 抽成 `first_non_empty_line`；测试里 `answered` 改为复用 `answer`；`Harness::shutdown` 与 `DiscussionHarness::shutdown` 抽成 `drain_renderer`。
- **Speculative Generality**：`round_outcome` 原来用 `windows(2)` 假装不假设人数，但组装期硬性拒绝 N ≠ 2——改回一次两两比较，并把「为什么是两个」写进文档。
- **Feature Envy / Message Chain**：`run_discussion` 里十处 `discussion.debaters[0].session` 收进 `Discussion::recorder()` / `Discussion::stream()`（现在 `debaters[0]` 只出现在 `Discussion` 自己的三个访问器里）。
- **Mysterious Name**：`Opened` → `OpenedSession`。
- **有意保留**（judgement call，改的是更小的一侧）：`debate_rounds` 与 `round_attendance` 各自扫一遍轮次边界（两处各两行 match，且投影里本来就有第三份，抽公共迭代器会引入下标而不减规则）；`config + provider` 在组装结构里成对出现（嵌套一层 `ModelParts` 会给调用方多一层间接，不值得）。

**Spec** 报 1 处真漂移 + 1 处死代码 + 1 处健壮性 + 2 处（已记录的）越界。已改：

- **真漂移**：不变量 (2) 的作用域从「会话级」收窄成「发起调用的那个 agent」（`pending_tool_calls_of`）。这是并发讨论的必要条件，但 spec 明说三条不变量不许绕——**已回改 `Further Notes` 的不变量 (2)**，写清作用域与会话级查询的用途（`--continue` 悬空恢复）。
- **死代码**：`DEFAULT_MAX_ROUNDS` 原来没人读。改成 `DiscussionParts::max_rounds: Option<u32>`，`None` 取默认 2、`Some` 覆盖、`Some(0)` 报错；新增 `omitting_the_round_cap_takes_the_protocol_default` 钉住默认值真的接上了。
- **健壮性**：`conclusion_of` 原来只认字面前缀，`**CONCLUSION:** x` / `- CONCLUSION: x` 会被读成「无结论」——那是假阴性（白买一轮）。现在容忍标记的装饰（`MARKER_LEAD` + 抽取后再 `trim_matches(IGNORED)`），有测试。
- **越界但有意保留**：(a) 两个讨论者共享权限策略——用户批准方案时就包含这条，现已把理由写进 spec §15 回改（§12 的「不继承允许」讲的是委派链向下，兄弟会话不在那条链上）；(b) `position_of` 在无结论时退回作答首行——写入 `## Comments` 第 6 条，为的是分歧记录不静默丢掉破协议的那一方。
- **可辩护、已记录**：非终局轮不发 `RoundEnded`（下一个 `RoundStarted` 是边界）、合成轮以 `Completed` 收尾、单侧缺席取 `NoDivergence`（主张最少的理由，缺席另有可查证据）。
