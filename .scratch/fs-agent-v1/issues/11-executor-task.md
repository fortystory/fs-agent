# 11: 执行者 `task`

**What to build:** 讨论者能派一个执行者去真实干活——执行过程不污染讨论，结论经派发者自己的发言回到讨论，且「派个子 agent 去写」这条路径**绕不过**已经定下的拒绝规则。

Blocked by: 04, 10

Status: done

**参考:** spec §16（执行者）、§12（继承）、§5（投影过滤）

- [x] 内建 `task` 工具派执行者：**嵌套 `Session`** + `parent_id`、独立轮数预算默认 **25**、事件落**同一个流**（`ExecutorSpawned` / `ExecutorFinished{reason}`）
- [x] 权限**继承拒绝、不继承允许**（`propagate` 按动作定：`Deny`/`Ask` 真、`Allow` 假）⇒ 沿链只可能变严
- [x] **递归深度 1**，且第一道防线是执行者的工具集里**没有** `task`（不是「有工具但拒绝」）
- [x] read set **不继承**，且任一方向都不流动
- [x] 回传 = **摘要 + 元数据**（token、改动的文件从流上**推导**）；执行者事件**不进**讨论者投影（`ExecutorFinished` 也不进），而**执行者自己的投影是全量的**
- [x] 同一批里的多个 `task` 可**并发**（上限默认 5）；`effect()` 判为「不碰工作区」⇒ 不参与写互斥；真正的写互斥靠**共享** `PathLocks`
- [x] 失败四值（`Error` / `MaxIterations` / `Aborted` / `MistakeLimit`）表现为**一条错误内容的工具结果**，讨论**不因此中断**
- [x] `task` 同 Turn 内**阻塞**到执行者结束（因果紧，不需要异步投递）
- [x] `/undo` 对执行者造成的编辑**同样有效**（同一会话目录、同一命名约定）

## Comments

**落地（票 11）**。代码：`src/tools/task.rs`（`task` 外壳 + `TASK_TOOL`）、
`src/tools/tool.rs`（`ExecutorSpawner` 端口 + `Tool::delegable`）、
`src/tools/registry.rs`（`Registry::for_executor`、`PendingCall.executor`）、
`src/agent.rs`（`TurnScope::Executor`、`process_call` / deferred 批、`run_deferred`、
`finish_call`、唯一的 `append_event` 写入路径）+ `src/agent/executor.rs`（`ExecutorPort` 与回传推导）、`src/provider/projection.rs`（brief 投影）、
`src/render.rs`（执行者一回合不上 stdout）、`src/permissions.rs`（`Mode::for_executor`）、
`src/events.rs`（`usage_of`）。测试：`tests/executor.rs`（11 个）+ `tests/discussion.rs`
里「讨论者派执行者，只有摘要入讨论」那一个。人读的说明：`docs/executor.md`。

**spec 留白在这里写实（已折回 spec §16/§5/§12/§19，见正文的「票 11 落地」块）**：

1. **执行者的窗口** = 钉住的注入 + 它自己的事件；brief 由 `ExecutorSpawned` 投影成
   执行者自己的第一条发言消息（该事件归执行者自己，`parent` 记派发者；这条消息**带 name**，
   所以不会被当成钉住 head 的一部分——否则 plan 模式的注入会并进 brief）。这样嵌套会话里
   `流 + 规则 → messages` 依然成立，且长讨论不会被重放进每个执行者的上下文。
2. **执行者的权限 = 父级的模式 ∪ 父级 `propagate` 的规则 ∪ 空 read set** ⇒ 权限是派发者的
   **子集**：`auto` 的子级能写（否则 `task` 在最需要它的无人值守场景里沦为只读），`readonly`
   的子级不能写，「总是允许」这类 `Allow` 不传播。headless + `ask` 父级下子级写不了，那是
   §12「无交互 ⇒ Ask→Deny」的既有语义，不是执行者特有的。
3. **执行者的模型可覆盖**（`SessionConfig::executor_model`，默认继承）——§17 弱模型分流留的
   机制；client 不换，所以覆盖值须是同一 provider profile 能服务的 model。

另外几处细节：**执行者 id = `<派发者>-<n>`**（从流上计数，恢复的会话不重发旧 id）；
**同一批多个 `task`** 的做法是「先按批内顺序逐个过 hook/门，已授权的 `task` 延迟，
再用 `buffered(max_parallel_executors)` 一起跑，结果按批内顺序落流」——hook 在后续调用上
`Stop` 时，已延迟的 `task` 各补一条错误结果，不变量「每个 `tool_call` 恰好一条结果」在新
交错下仍成立（有测试）；**回传里「改了哪些文件」读成功结果里的路径行**
（`tools::file::WROTE_PATH_PREFIX`）而不是 `ToolCallStarted` 的参数——`hook.pre` 可能已改写
那次调用（有测试）。`/undo` 那一格验的是**机制**（同一会话目录、同一命名约定，`.before`
落盘内容为实际被替换的区段）；`/undo` 手势本身归票 12。

**两轴 review 的处理**（`/code-review`）：Standards 轴指出「唯一写流者」的说法已过时、
`agent.rs` 混入了整个执行者域、`ExecutorPort::emit` 与 `emit_returning` 重复 ⇒ 抽出
`src/agent/executor.rs`（`agent` 的子模块，不是新边界），并把两条写入路径合成唯一的
`agent::append_event`（`Session::append` 随之删除）；`docs/discussion.md`、`session.rs`、
`tools/task.rs`、`tools/tool.rs` 的措辞同步。Spec 轴指出的三处已逐个修掉并补了回归测试：
模型覆盖缺失、`changed_files` 读错来源、brief 会吸收后续注入。
