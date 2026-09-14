# 07: 上下文预算与裁剪

**What to build:** 上下文窗口不会被撑爆——超预算时按**固定顺序**丢最不黏的，日志一条不删；项目规则与 skill 清单永远在场，前缀缓存持续命中。

Blocked by: 06

Status: done

**参考:** spec §10（预算、截断、丢弃顺序）

- [x] `usable_input(agent) = context_window − min(20_000, max_output_tokens)`，**按 agent 各自模型算**，**没有**全局预算
- [x] `messages = trim(project(...), budget, policy)`：裁剪是投影**之后**的另一个纯函数
- [x] 单次工具结果超限时**在入流前**截断：超限部分落盘、事件里只放预览与指针；指针失效**降级为预览、绝不失败**
- [x] 超预算丢弃只发生在 `trim` 里且**只读**（事件流一条不删）
- [x] 丢弃顺序：**老普通工具结果 → 老 skill body → 老整轮 → 该回合硬失败**
- [x] `AGENTS.md` + skill 清单作为**第一条 user 消息**注入（身份 → 规则 → 历史），**逐轮不变**、**永不参与裁剪**、且记成流上一条事件（否则 `project` 不再是「流 + 规则」的函数）
- [x] 「当前预算」不是状态是函数 ⇒ 窗口层无需加锁；会话累计是从 usage 事件求和的派生值
- [x] v1 = 字符/4 估 token + 粗暴丢弃；compaction **后置**（形状已定，本票不建）
- [x] e2e：塞进超预算历史，断言丢掉的确实是那几类里的、且事件流条数不变

## Comments

实现落点：`src/context.rs`（重写：`usable_input` / `estimate_tokens` / `trim` / `truncate_result` / `load_agents_md`）、`src/config.rs`（`SessionConfig::max_tool_result_tokens`，默认 `DEFAULT_MAX_TOOL_RESULT_TOKENS = 25_000`）、`src/events.rs`（`total_usage` 查询）、`src/agent.rs`（`record_context_injection`、每回合投影后裁一次、`emit_completed` 入流前截断）、`src/lib.rs`（`assemble` 读 `AGENTS.md` 并记注入）。测试：`tests/context_budget.rs`（纯函数 17 例 + e2e 5 例）、`tests/event_log.rs`（累计 2 例）。

实现期把票面留白写实的几处（都不改 spec 的决定）：

1. **「丢」= 把 `Message::Tool` 的 body 换成 stub，不删消息。** 线级硬约束是一个 `tool_call` 对一条 `tool` 消息（spec §5），删掉结果消息当场炸；所以「日志一条不删」与「这次发出的 messages 少几条」落在「旧结果 body 变短」+「老整轮成组删除」上。老整轮删除把该轮的 assistant 与它的结果**一起**拿走，配对仍然成立（`pairing_is_intact` 有断言）。
2. **「老」= 不在当前回合（最后一条非钉住 `user` 之后）里的内容。** 当前回合永不丢：模型得有话可答。当前回合自己就装不下 ⇒ 硬失败——那正是票 06 说的「真爆窗」信号，也是将来 compaction 的触发点。
3. **硬失败用既有的 `StopReason::Error` 收尾**（`TurnEnded { Error }`，落流 + stderr 诊断）。spec 的第 9 个值 `BudgetExhausted` 是**成本**闸门（票 14），窗口溢出是另一个量，不混用。硬失败发生在调 provider **之前**，所以装不下的请求绝不会发出去。
4. **钉住判据 = 开头连续的 `Message::User { name: None }`。** 投影里 `ContextInjected` 正好长这样且永远在最前（spec §5）。投影对「自己那方的 `AgentError`」也用同一个形状，但日志里任何 `AgentError` 之前必有一条 user 消息，所以它不会落在开头这段前缀里；中途的 `AgentError` 只是普通历史，可以被裁。⚠️ **票 15 的计划注入是会话中途的 `ContextInjected`，不在这段前缀里**：那张票落地钉住时要么给投影产出 origin 元数据、要么扩这个判据，否则计划指令可能随老整轮一起掉。这里明确交接。
5. **skill body 类别按「前置 assistant 的 `tool_call.name == TrimPolicy.sticky_tool_names`（默认 `["skill"]`）」判定**，所以票 08 挂上 `skill` 工具后自动生效，不需要再动 `trim`。
6. **单次结果上限默认 25k 估 token**（Anthropic 文档的默认值），放在 `SessionConfig`、可配。预览 = head/tail，约上限的 1/10 token（下限 200 字符），带落盘指针。落盘失败 → `pointer: None`，预览里写「could not be spilled」，**绝不失败**；正文小到「预览 + 指针注记」比正文还长时**整段保留**（截断绝不把流撑大，上限是用来兜住大输出的、不是拿来凑数字的）。打码（票 16）按票 06 交接口径插在**截断之前**：打码 → 截断 → 落盘（本票只做后两步）。
7. **skill 清单注入留给票 08。** 本票建好机制：记一条 `ContextInjected` 事件 → 投影成首条 `user` 消息 → `trim` 永不动它；`assemble` 读 `<cwd>/AGENTS.md`（存在且非空才注入，空/不可读 = 不注入）。票 08 追加一条 `SkillsCatalog` 注入即可，机制不返工。
8. **会话累计 = `events::total_usage`**：对 `UsageRecorded` 求和（`reasoning_tokens` 有人报过才出现），是派生值不是状态——票 14 直接站在它上面做硬停。
9. 窗口溢出时 `trim` 返回 `Err(TrimError::OverBudget { budget, estimated })`；`agent` 把它翻成第 3 条的硬失败。`trim` 本身对日志零写：e2e 两条——一条断言超预算那一回合的事件条数增量恰好等于该回合应有的 5 条、且流上的工具结果仍是全文（类 1）；另一条断言「把旧结果全部 stub 掉仍不够」时**最老整轮成组消失**（类 3）、当前回合完好、流上两条结果都还在。
10. **交接票 12（会话恢复）**：注入在 `assemble` 里记一次。`--continue` 复用既有日志时**不要再记一条 `ContextInjected`**（流上已经有了；重复会让投影出现两条钉住 user 消息、也会动前缀缓存），照「不追加第二个 `SessionStarted`」同一条规矩办。
11. **身份段不在本票**：spec 的布局是「`System` 身份 → `User` 规则 → 历史」，其中身份是**私有的 system prompt**（票 06 第 5 条：不进流）。本票只负责「规则」（`ContextInjected`）；`project` 照旧不发 `Message::System`，等有 system prompt 组装的那张票再接。
12. `estimate_tokens` 用 `div_ceil`（向上取整），不是字面的 `chars / 4`：这样任何非空文本至少算 1 token，避免「很短的输入估成 0」。
