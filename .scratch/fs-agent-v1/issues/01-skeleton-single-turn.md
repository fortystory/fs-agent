# 01: 骨架与唯一接缝：一个回合

**What to build:** 从零建起这个 crate，并让**一条完整的最小闭环**跑通：一次会话、一个 agent、一个回合、headless 渲染。这一票的产物除了「agent 能回一句话」之外，还有**整张 spec 唯一那条 e2e 接缝**——库入口的组装函数接收注入的 provider，测试用脚本化假 provider 驱动，断言事件流与两个 sink。之后的每一票都往这条接缝加场景，不新开 mock 接缝。

Blocked by: None

Status: done

**参考:** spec §1（布局）、§2（事件流与 schema）、§3（回合循环）、§19（headless 渲染）

- [x] 单 crate：二进制入口薄、库入口公开，且库入口**不读环境**（provider、渲染 sink、配置值全部从参数注入）
- [x] `EventLog` 只追加：一会话一 JSONL、**单一写入者**、每行 flush 但**不 fsync**、容忍末行不完整；`seq` = 行号
- [x] 信封 = `{ seq, at, speaker_id, payload }`，`speaker_id` 必填（`Debater(id) | Executor(id) | User | System`）；`StopReason` 至少含五个单次循环值（`Completed` / `MaxIterations` / `Aborted` / `MistakeLimit` / `Error`）
- [x] 一个回合跑通：投影 → 调 provider → 完成单元落进流（`MessageCompleted`）→ `TurnEnded { reason }`；**增量文本不进流**（走旁路直达渲染）
- [x] 续行由循环自己算：最后一条 assistant 含 ≥1 个 `tool_call` 才继续；「有没有未出结果的 `tool_call`」是对事件流的**查询**，不是隐藏状态；结束只认 `[DONE]`，`finish_reason` 只作诊断
- [x] headless 渲染只写两个显式 sink：stdout **只放最终产物**（`TurnEnded{Completed}` 的 assistant 文本），其余全 stderr
- [x] 假 provider 能按调用序脚本化返回文本 / 推理 / usage，并能演成流式分片
- [x] 仓库里第一个 e2e 测试：从组装入口注入假 provider 跑完整闭环，断言 JSONL 事件流与两个 sink

## Comments

实现完成（agent）。落点：`src/{events,agent,render,session,lib,cli,provider}.rs`；测试在 `tests/{e2e_single_turn,event_log}.rs` 与 `tests/support/{fake_provider,capture}.rs`。产出提交 `5710978`，评审收口 `2d2dc8a`。

- **组装接缝**：`assemble(AssemblyParts)` 接收注入的 provider、两个 render sink、session 配置与工作目录，**库入口不读环境**（`cli` 是唯一读环境的地方），返回 `Harness` 驱动回合。
- **`EventLog`**：一会话一 JSONL、每行 `flush` 不 `fsync`；`seq` 就是行号，重开时 `next_seq = events.len() + 1`；只有**无法解析的末行**才当作撕裂写入丢弃，缺终止换行的完整事件保留并补换行。
- **单一写入者**：`Session::append` 是 crate-private，连 `SessionStarted` 也由 `agent` 记录，`Session` 不再当第二个写者。
- **回合收尾**：只有 `[DONE]` 结束一条消息——流报错或提前停止都不产生 `MessageCompleted`，直接 `TurnEnded { Error }`；续行与否、还有没有悬空 `tool_call` 都是对事件流的查询。
- **渲染**：增量文本走旁路，永不进流；stdout 只在 `TurnEnded { Completed }` 时落该回合的 assistant 文本，其余（含诊断与事件叙述）全走 stderr。
- **假 provider**：按调用序脚本化文本 / 推理 / usage / 分片，并支持流中途注入错误，是后续每一票唯一的验收手段。

评审（两轴）后另修：空工具参数落 `{}` 而非 JSON `null`；`AgentId` 改名 `ParticipantId`（`CONTEXT.md` 禁 `agent` 作类型名），三个 id newtype 统一走 `string_id!`；测试改断言磁盘 JSONL 而不是内存缓存。
