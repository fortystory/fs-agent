# 10: 讨论协议

**What to build:** 两个异构讨论者就同一个问题各自作答、**只在结论真的冲突时**才互相回应，最后由合成器把「共识 / 分歧（含各自成立的前提）/ 未决」摆给用户——它画出选项空间，**不替用户收敛**。

Blocked by: 07

Status: ready-for-agent

**参考:** spec §15（讨论协议）、§2（轮次事件）、§17（预算撞顶）

- [ ] 两个讨论者（KIMI + DeepSeek）**同一轮并发**发；**独立首轮互不可见**（保住多样性）
- [ ] 每个讨论者在正文后**另起一行给一行结论**；归一化后**精确相等或子串包含**即判为一致（**零额外调用**、无假阴性）
- [ ] **只在冲突时**开第二轮：明确告知「你看到了对方的回答、请针对分歧回应」；上限 2 轮（可配）
- [ ] `RoundStarted{round, mode}` / `RoundEnded{round, reason}` / `DivergenceRecorded{round, topic, positions}` 落流；第三轮不需要新机制（模式槽位已有）
- [ ] 合成器 = **一次独立单发调用**（不是 agent 身份、无工具、无回合），产出三档，落在 `speaker_id = System` 的 `MessageCompleted` 上；揭示发言**全文**、不揭示私有推理
- [ ] 协议指令常驻**私有身份**、**不进流**（否则「第二轮的 messages 能从流重算」当场不成立）
- [ ] 单侧失败 → 该方本轮**缺席、讨论继续**，且缺席**能从流上查出来**（该方自己的 `TurnEnded{Error}` + 该轮没有 `MessageCompleted`）；双侧失败 → `RoundEnded{Error}` + `SessionError`；**不重跑**
- [ ] e2e（假 provider 数调用次数）：无分歧 = **3 次**调用、有分歧 = **5 次**
- [ ] `RoundEnded` 四个值（含 `BudgetExhausted`）在渲染上可区分
