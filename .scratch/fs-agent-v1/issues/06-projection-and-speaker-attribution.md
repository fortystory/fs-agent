# 06: 投影与发言归属

**What to build:** 事件流能喂给多个发言者——每个 agent 这次调用要重放的 `messages` 都是从流**重算**的纯函数产物；他人的发言被正确降档，自己的推理与工具往返被正确回放。没有「agent 的记忆」这种隐藏状态。

Blocked by: 03

Status: done

**参考:** spec §5（投影规则）、§2（schema）

- [x] `project()` 是纯函数 `(EventLog, SpeakerId, provider 能力) → messages`，住在 provider 适配器侧；逐字段差异是**数据**不是代码分支
- [x] 「哪些事件进模型上下文」是一张**穷尽 match 表**（不放 payload 上）
- [x] 他人回合三档：文本**保留**、工具调用**只留一行摘要**、结果正文与 `reasoning_content` **都不投**；他人一律投成 `user`，配对的 `tool` 结果一起改写或丢弃（否则线级配对校验会炸）
- [x] 我自己的 `reasoning_content` **必须回放**；我自己的工具往返 + 后置 hook 反馈按 `seq` **合并成一条 `tool` 消息**
- [x] 合并按 role 序列、**轮次是硬边界**、**无条数阈值**、严格按 `seq`；两条钉住的注入不参与合并
- [x] `[轮 N · 名字]` 前缀**只加在他人发言上**；`name` 只发在 `user` / `assistant` 上、消毒到 `[A-Za-z0-9_-]`、**绝不依赖它**；`tool` 消息不发 `name`（DeepSeek 的 schema 里根本没有这个字段）
- [x] 模型侧前缀与界面侧前缀是**两套、不共用生成器**
- [x] e2e：同一批事件、两个不同发言归属，投影结果不同且各自快照稳定；`messages` 能被序列化成两家的线级形状

## Comments

实现落点：`src/provider/projection.rs`（重写）、`src/provider/mod.rs`（`Provider::caps()`）、`src/provider/openai.rs`（trait 实现）、`src/session.rs`（`Session::log()`）、`src/agent.rs`（把能力交给投影）。测试在 `tests/projection_attribution.rs`（纯函数 13 例）与更新后的 `tests/e2e_single_turn.rs`。

实现期把票面留白写实的几处（都不改 spec 的决定）：

1. **能力从 provider 自己来。** 投影要按字段分叉，但 `agent` 只拿得到 `&dyn Provider`，而组装接缝里的假 provider 用 `fake-model`（不在能力表里，`caps_for` 会报错）。所以在 `Provider` trait 上加 `fn caps(&self) -> ModelCaps`：真实 client 返回自己建表时的值，假 provider 可显式 `with_caps(...)` 或默认 `deepseek-flash`。`agent` 每回合读一次、传 `&caps`。
2. **`project()` 收 `&EventLog`**（票面签名），不再收事件切片；`Session` 因此多一个只读的 `log()`（写入口仍是 crate-private 的 `append`）。
3. **分叉的「数据」具体是 `caps.requires_reasoning_replay`。** 自己的 `reasoning_content` 是否回放由它决定（两家都建模为 `true`，所以实际行为不变）；这条让「逐字段差异是数据不是代码分支」有可测的载体，测试用原地改一个字段来钉它。
4. **两条钉住的注入**：`ContextInjected` 一律自成一 `user` 消息、前后都截断合并块；除此之外，**第一条「发言类」`user` 消息也被钉住**（合并块只要遇到另一个发言者就先 flush，见 `push_other`），所以「首条 user 消息」与 `ContextInjected` 两条都不参与后续合并。`ContextInjected` 同时覆盖「AGENTS.md + skill 清单那条首条 user 消息」（ticket 07 会以 `ContextInjected` 落流）与「会话中途的 plan 注入」（ticket 15）。
5. **前缀只在轮次里出现。** `[轮 N · 名字]` 仅当 `RoundStarted` 之后、`RoundEnded` 之前（`N` 就是 spec 定的形状）；轮次外没有 `N` 可写，正文保持原样，归属由 `name` 承载——单 agent CLI 会话本来也只有一个「他人」（人类）。所以 `tests/e2e_single_turn.rs` 里首条 user 消息的断言只改 `name`：`"say hi", name: None` → `"say hi", name: Some("user")`。**没有**发明 `[名字]` 之类的第四种前缀。
6. **`name` 的取值**：直接取 `SpeakerId` 的 `Display` 再消毒（`executor:<id>` → `executor-<id>`），只保留 `[A-Za-z0-9_-]`、截到 64 字符、空串回退 `unknown`。合并块里有多个不同发言者时不发 `name`（正文前缀在轮次内永远是基线）。`tool` 消息结构上就没有 `name` 字段。
7. **他人的工具调用摘要形如 `→ read_file({"file_path":"src/lib.rs"})`**，参数渲染截到 160 字符加 `…`；`{}`/`Null` 参数只留 `→ 工具名`。结果正文与 `reasoning_content` 一律不投，配对的 `tool` 消息也随之丢弃（不发出 `assistant.tool_calls`，线级配对因此自洽）。
8. **穷尽 match 表里两处票面没提的落点**：`AgentError` 只对**出错的那一方自己**投成一条自成一体的 `user` 消息（spec §2 要求模型看到并纠正；别人的错不该进我的上下文），`SessionError` 不投；`HistorySuperseded.targets` 里的 `seq` 在扫描前被排除（spec §2 的「投影规则排除被替代区间」），`summary` 的插入留给真正建 compaction 的票。
9. **执行者可见性**：`SpeakerId::Executor(_)` 的事件对任何非它自己的发言者都不进上下文（含 `ExecutorFinished`）；执行者自己的投影是全量的。
10. **线级配对是投影的硬约束，不靠调用方的顺序。** 自己的 assistant 组只在它的 `tool_call` 都拿到结果后才落消息（`close_pending_if_settled`）：合法的回合里这不改变任何顺序，而一个交错了他人发言的日志（今天产生不了，但没人从类型上禁止）也不会漏出「有 `tool_call` 没有 `tool` 消息」的请求。测试 `an_interleaved_other_speaker_never_leaves_a_tool_call_without_its_result` 钉住它。


