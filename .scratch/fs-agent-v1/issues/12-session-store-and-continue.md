# 12: 会话存储与恢复

**What to build:** 会话可恢复、可搬运：中断不产生隐藏状态（也能被明确收尾），每次编辑能**字节级**退回，会话目录不会被同机其他人读到。

Blocked by: 03

Status: done

**参考:** spec §11（存储、拓扑、`--continue`）、§20（权限位）

- [x] 数据落 `~/.local/share/fs-agent/sessions/<cwd-slug>/<session-id>/`（事件流 + `outputs/`）；**会话是一个目录、一个可搬运单元**
- [x] 会话与 cwd 绑定（slug 只用于分桶，权威 `cwd` 在 `SessionStarted` 里）；`--continue` 只扫**本目录桶**、取 mtime 最新、**不维护全局索引**
- [x] 会话 id = `<UTC 时间戳>-<短随机后缀>`，`--continue` 之后**绝不改变**（前缀缓存的前提）
- [x] 恢复撞上**悬空 `tool_call`** → 补一条合成的失败结果（结果未知；**不丢弃、不重执行**），且**不追加第二个 `SessionStarted`**
- [x] 不变量成立：`流 + 落盘文件 + 投影规则 → messages`，且**投影不读落盘文件**（保持纯函数）
- [x] `/undo` 按 `outputs/<tool_call_id>.before` 还原**实际被替换的区段**；取与编辑**同一把** per-path 锁；**不碰用户的 git**、不做每次编辑自动 commit
- [x] 会话目录创建即 `0700` / 文件 `0600`；**不自动清理**，提供手动 `prune`
- [x] e2e：跑一次会话 → 杀掉 → `--continue` → 断言 id 不变、悬空调用被合成收尾、继续对话正常

## Comments

**落点。** `src/session/store.rs`（目录形状、id、权限、`latest`/`list`/`prune`）、`src/agent/history.rs`（悬空收尾 + `/undo`）、`src/tools/edit.rs::revert`（编辑梯的逆）。根目录由 CLI 从 `$XDG_DATA_HOME` / `$HOME/.local/share` 算出（`config::sessions_dir`）后**注入**，库仍然不读环境。

**fresh / resume 不用新开关。** 组装点按交给它的 `log_path` 是否存在决定 `EventLog::create` 还是 `EventLog::open`，并查询流里有没有 `SessionStarted` 来决定记骨架还是做恢复。`--continue` 因此是一条代码路径，而库不需要一个「continue」标志（spec §1 的注入原则）。

**`/undo` 的区域靠重放验证。** 流里没有字节偏移，所以 `revert()` 对每个候选还原位置**重放编辑梯**，只有能精确产出当前内容的那个才算命中；`replace_all` 的 `.before` 是 `old_string` 的重复，条数可数。**已知边界**：纯删除（`new_string` 为空）没有位置可找回，一律拒绝而不是猜（已写回 spec §11）。`HistorySuperseded { reason: Undo }` 只退役该次编辑的 `ToolCallStarted`/`ToolCallCompleted` 两个 seq；投影新增一条守卫：退役后**空掉**的 assistant 消息不发（线上协议没有这种形状）。

**`prune` 的默认形状**（spec 未定，本票定）：按 cwd 桶、保留最新 N、默认 1（即 `--continue` 会恢复的那个），另有 `--dry-run` 与 `--cwd`。

**`--continue` 的 CLI 旗标留给票 18。** 本票给的是机制（`SessionStore::latest` + 组装期的恢复路径）与 e2e 证明；`fs-agent` 目前只有 `probe` 与新的 `prune` 两个子命令，交互式入口是票 18 的事。
