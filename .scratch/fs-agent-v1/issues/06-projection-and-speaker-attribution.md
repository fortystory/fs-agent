# 06: 投影与发言归属

**What to build:** 事件流能喂给多个发言者——每个 agent 这次调用要重放的 `messages` 都是从流**重算**的纯函数产物；他人的发言被正确降档，自己的推理与工具往返被正确回放。没有「agent 的记忆」这种隐藏状态。

Blocked by: 03

Status: ready-for-agent

**参考:** spec §5（投影规则）、§2（schema）

- [ ] `project()` 是纯函数 `(EventLog, SpeakerId, provider 能力) → messages`，住在 provider 适配器侧；逐字段差异是**数据**不是代码分支
- [ ] 「哪些事件进模型上下文」是一张**穷尽 match 表**（不放 payload 上）
- [ ] 他人回合三档：文本**保留**、工具调用**只留一行摘要**、结果正文与 `reasoning_content` **都不投**；他人一律投成 `user`，配对的 `tool` 结果一起改写或丢弃（否则线级配对校验会炸）
- [ ] 我自己的 `reasoning_content` **必须回放**；我自己的工具往返 + 后置 hook 反馈按 `seq` **合并成一条 `tool` 消息**
- [ ] 合并按 role 序列、**轮次是硬边界**、**无条数阈值**、严格按 `seq`；两条钉住的注入不参与合并
- [ ] `[轮 N · 名字]` 前缀**只加在他人发言上**；`name` 只发在 `user` / `assistant` 上、消毒到 `[A-Za-z0-9_-]`、**绝不依赖它**；`tool` 消息不发 `name`（DeepSeek 的 schema 里根本没有这个字段）
- [ ] 模型侧前缀与界面侧前缀是**两套、不共用生成器**
- [ ] e2e：同一批事件、两个不同发言归属，投影结果不同且各自快照稳定；`messages` 能被序列化成两家的线级形状
