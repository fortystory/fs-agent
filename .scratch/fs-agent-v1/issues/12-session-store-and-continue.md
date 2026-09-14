# 12: 会话存储与恢复

**What to build:** 会话可恢复、可搬运：中断不产生隐藏状态（也能被明确收尾），每次编辑能**字节级**退回，会话目录不会被同机其他人读到。

Blocked by: 03

Status: ready-for-agent

**参考:** spec §11（存储、拓扑、`--continue`）、§20（权限位）

- [ ] 数据落 `~/.local/share/fs-agent/sessions/<cwd-slug>/<session-id>/`（事件流 + `outputs/`）；**会话是一个目录、一个可搬运单元**
- [ ] 会话与 cwd 绑定（slug 只用于分桶，权威 `cwd` 在 `SessionStarted` 里）；`--continue` 只扫**本目录桶**、取 mtime 最新、**不维护全局索引**
- [ ] 会话 id = `<UTC 时间戳>-<短随机后缀>`，`--continue` 之后**绝不改变**（前缀缓存的前提）
- [ ] 恢复撞上**悬空 `tool_call`** → 补一条合成的失败结果（结果未知；**不丢弃、不重执行**），且**不追加第二个 `SessionStarted`**
- [ ] 不变量成立：`流 + 落盘文件 + 投影规则 → messages`，且**投影不读落盘文件**（保持纯函数）
- [ ] `/undo` 按 `outputs/<tool_call_id>.before` 还原**实际被替换的区段**；取与编辑**同一把** per-path 锁；**不碰用户的 git**、不做每次编辑自动 commit
- [ ] 会话目录创建即 `0700` / 文件 `0600`；**不自动清理**，提供手动 `prune`
- [ ] e2e：跑一次会话 → 杀掉 → `--continue` → 断言 id 不变、悬空调用被合成收尾、继续对话正常
