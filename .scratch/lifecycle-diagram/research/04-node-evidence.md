# 生命周期图的节点/边 → `文件:行号` 证据表

本文件是票 04 的交付物：以 [`prototype/01-drafts.md`](../prototype/01-drafts.md) 定形的五张图为
准，**逐节点、逐边**给出 `文件:行号`，供 `docs/lifecycle.md` 的图旁证据表与
`scripts/lifecycle-check.py` 的对账使用（校验项见 [`02-lifecycle-check-design.md`](./02-lifecycle-check-design.md)）。

- **基础**：优先复用 [`01-runtime-lifecycle-facts.md`](./01-runtime-lifecycle-facts.md)。本表只
  在它没覆盖、或它自标「存疑」的地方回代码；行号以**本票回读时**的文件为准。
- **记号**：证据列用 `路径:行号`（区间 `a-b` 含两端）；符号列只写
  `fn`/`struct`/`enum`/`trait`/`const` 里的稳定名字，不用 `impl`（02 号设计 §1·C2）。
  标 **†** 的条目是转引 `research/01` 且本票未逐行回读；其余都逐行读过。
- 本票只读、未跑构建；未修改 `research/01`。

---

## 1. 图 1：鸟瞰总图

### 节点

| 节点 id | 一句话 | 证据 | 符号 |
| --- | --- | --- | --- |
| `boot` | 进程入口，`main` 只调 `cli::main()` | `src/main.rs:1-3`、`src/cli.rs:78-103` | `fn main` |
| `cmd` | 子命令分派：`--version`/`--help`/`probe`/`discuss`/`prune`/`sessions`，其余交互式 | `src/cli.rs:105-129` | `fn run` |
| `setup` | 组装：配置 · 凭据 · provider · 工具表 · 沙箱 | `src/cli.rs:234-411` | `fn interactive` |
| `sess` | 会话：新开或 `--continue` / `--session` 恢复 | `src/cli.rs:289-297`、`src/cli.rs:2633-2677` | `fn choose_session` |
| `render` | 渲染器选定 TUI / plain（headless 见 §8） | `src/cli.rs:327-372`、`src/render/mod.rs:161-200` | `fn tui`／`fn plain` |
| `loop` | 主循环：等一行或手势 | `src/cli.rs:1102-1313` | `fn interactive_loop` |
| `route` | 分派 `Submission` | `src/cli.rs:1183-1307`、`src/cli.rs:1380-1433` | `fn submission` |
| `turn` | 一次 turn | `src/lib.rs:658-682`、`src/agent.rs:443-732` | `fn run_turn` |
| `disc` | 讨论：`--discuss` 子命令与会话内 `/discuss` | `src/cli.rs:545-770`、`src/cli.rs:950-1040`、`src/agent.rs:1334-1585` | `fn run_discussion` |
| `goal` | 目标循环 `/loop` | `src/cli.rs:1546-1803` | `fn run_goal_loop` |
| `maint` | `probe` · `prune` · `sessions` 三条独立子命令 | `src/cli.rs:115-124`、`src/cli.rs:2102`、`src/cli.rs:2392`、`src/cli.rs:2550` | `fn run_sessions` |
| `tail` | 收尾：排空渲染通道 + 续接回执 | `src/cli.rs:460-467`、`src/lib.rs:1080-1081` | `fn finish_session`／`fn shutdown` |
| `ok` | 退出 0（空输入、`/quit`、`Ignore`、正常收尾） | `src/cli.rs:1177-1187`、`src/cli.rs:1973-1978` | `fn exit_code_after` |
| `aborted` | 退出 130（忙碌中被举手退出，等回合收尾后兑现） | `src/cli.rs:1310-1312`、`src/cli.rs:1988-2015` | `struct ExitRequest` |

### 边

| 边 | 证据 | 说明 |
| --- | --- | --- |
| `boot --> cmd` | `src/main.rs:1-3` | `main` 无逻辑，直接进 `cli::main` |
| `cmd -->\|交互式\| setup` | `src/cli.rs:127` | 没有子命令就是交互式 |
| `cmd -->\|--discuss\| disc` | `src/cli.rs:116` | `discuss` 子命令独立组装 |
| `cmd -->\|probe / prune / sessions\| maint` | `src/cli.rs:115-124` | 三条各自 `return` 自己的 `ExitCode` |
| `setup --> sess` | `src/cli.rs:289-297` | `choose_session` 决定开哪一场 |
| `setup --> render` | `src/cli.rs:327-372` | 渲染器在组装前选定并注入 |
| `sess/render --> loop` | `src/cli.rs:446-459` | `interactive_loop(...)` 被调 |
| `loop --> route` | `src/cli.rs:1183` | 拿到一行后交给 `submission` |
| `route -->\|一句提示\| turn` | `src/cli.rs:1297-1306`、`src/cli.rs:2033` | `Submission::Prompt` → `TurnStart::Prompt` |
| `route -->\|/discuss\| disc` | `src/cli.rs:1244-1253` | 走 `discuss_in_session` |
| `route -->\|/loop\| goal` | `src/cli.rs:1275-1295` | 走 `run_goal_loop` |
| `route -->\|/quit 或空行\| tail` | `src/cli.rs:1177-1187` | `None` 与 `Quit` 都 `ExitCode::SUCCESS` |
| `turn --> loop` | `src/cli.rs:1307` | 处理完回到循环顶 |
| `goal --> turn` | `src/cli.rs:1626` | 目标循环用 `TurnStart::Injected` 驱动每次 `run_one_turn` |
| `maint --> ok` | `src/cli.rs:115-124` | 见 §8：没有共同的 `ok` 收尾路径 |
| `tail --> ok` | `src/cli.rs:1973-1978` | `exit_code_after(false)` |
| `loop -->\|两下 Ctrl-C\| tail` | `src/cli.rs:1164-1168` | 空闲时收到 `Quit` 直接返回 130 |
| `loop -->\|取消后收尾\| aborted` | `src/cli.rs:2044-2052`、`src/cli.rs:1310-1312` | 忙时先取消，回合收尾后兑现 130 |

---

## 2. 图 2：进程启动与收尾

### 节点

| 节点 id | 一句话 | 证据 | 符号 |
| --- | --- | --- | --- |
| `main` | 只剩一句调用 | `src/main.rs:1-3` | `fn main` |
| `root` | 先于参数、先于 runtime 检查 euid | `src/cli.rs:73-86` | `fn root_refusal` |
| `refuse` | root 直接拒、无旗标可绕 | `src/cli.rs:83-86` | — |
| `env` | `EnvMap` + `argv`（去首参） | `src/cli.rs:87-88` | — |
| `rt` | 多线程 tokio runtime，失败即退 | `src/cli.rs:89-102` | — |
| `cmd` | 子命令分派 | `src/cli.rs:105-129` | `fn run` |
| `parse` | 交互式参数解析，两渲染器互斥 | `src/cli.rs:161-219` | `fn parse_interactive` |
| `cfg` | 显式 `--config` 必须存在，否则默认路径 | `src/cli.rs:247-253`、`src/cli.rs:2087-2100` | `fn load_config` |
| `red` | 每条 provider key 解析一次，打码器持全部值 | `src/config.rs:713-720`、`src/events.rs:611-630` | `fn redactor` |
| `prov` | provider 装配 + 能力表（双保险） | `src/cli.rs:299-305`、`src/cli.rs:334-340`、`src/provider/openai.rs:101-127`、`src/provider/capability.rs:83` | `fn build`／`fn caps_for` |
| `scfg` | `SessionConfig`：预算 · 迭代上限 · 沙箱 · 落点 | `src/cli.rs:306-312`、`src/config.rs:696-706` | `fn session_config` |
| `mode` | 起始权限模式：`--mode` 压过 `[permissions] mode` | `src/cli.rs:226-228`、`src/cli.rs:323` | `fn effective_mode` |
| `tools` | 工具表组装：内建 + 动态 + 联网，组装期定死 | `src/cli.rs:386-389`、`src/tools/mod.rs:96-130` | `fn with_dynamic`／`fn with_web` |
| `probe` | 沙箱探测：`mode="off"` 或已给结果则不探 | `src/lib.rs:315-322`、`src/tools/sandbox.rs:48-83` | `fn sandbox_availability`／`fn probe` |
| `open` | `assemble`：建/开日志 · 技能 · `AGENTS.md` · 起渲染器 | `src/lib.rs:217-275`、`src/lib.rs:448-470` | `fn open`／`fn assemble` |
| `kind` | 这条流有没有 `SessionStarted` | `src/lib.rs:244-252` | — |
| `skel` | 新流：`SessionStarted` + 沙箱状态 + 钉住注入 | `src/lib.rs:366-386` | `fn record_skeleton` |
| `resume` | 续流：补记沙箱状态 · 收尾悬空 `tool_call` · 重放历史 | `src/lib.rs:330-345`、`src/agent/history.rs:40-61`、`src/cli.rs:417-419` | `fn start`／`fn recover_pending_calls` |
| `banner` | 横幅走渲染通道；`/` 菜单 = 内建 + 已发现技能 | `src/cli.rs:423-444` | `fn banner` |
| `loop` | 主循环 | `src/cli.rs:1102` | `fn interactive_loop` |
| `quit` | 排空渲染通道，然后 stderr 打续接回执 | `src/cli.rs:460-467`、`src/lib.rs:1161-1164` | `fn finish_session`／`fn drain_renderer` |
| `ok` | 退出 0 | `src/cli.rs:1177-1187`、`src/cli.rs:1973-1978` | `fn exit_code_after` |
| `cc` | 退出 130：忙时第一下取消、第二下记 `ExitRequest` | `src/cli.rs:1988-2015`、`src/cli.rs:2044-2052` | `struct ExitRequest` |
| `panic` | TUI 的 panic 钩子 + `Drop` 还原终端（见 §8） | `src/render/tui.rs:526-563`、`src/render/tui.rs:470-471` | `struct TerminalModes` |

### 边

| 边 | 证据 | 说明 |
| --- | --- | --- |
| `main --> root` | `src/cli.rs:82-86` | euid 检查在任何别的事之前 |
| `root -->\|是\| refuse` | `src/cli.rs:83-85` | 断头路，直接 `ExitCode::FAILURE` |
| `root -->\|否\| env` | `src/cli.rs:87-88` | 收环境与 argv |
| `env --> rt` | `src/cli.rs:89-102` | 建 runtime |
| `rt --> cmd` | `src/cli.rs:102` | `block_on(run)` |
| `cmd -->\|交互式\| parse` | `src/cli.rs:127`、`src/cli.rs:239` | `interactive` 第一步 |
| `parse --> cfg` | `src/cli.rs:247` | `load_config` |
| `cfg --> red` | `src/config.rs:713-720` | 打码器随 `session_config` 同行 |
| `red --> prov` | `src/cli.rs:299` | provider 装配 |
| `prov --> scfg` | `src/cli.rs:306` | `session_config` |
| `scfg --> mode` | `src/cli.rs:323` | 三种消费者共用同一答案 |
| `mode --> tools` | `src/cli.rs:386-389` | 工具表在组装处定死 |
| `tools --> probe` | `src/lib.rs:457`、`src/lib.rs:279` | ⚠️ 见 §8：探测实际在 `open` 之后的 `session()` 里 |
| `probe --> open` | `src/lib.rs:459` | ⚠️ 同上，顺序被推翻 |
| `open --> kind` | `src/lib.rs:249-252` | 看日志里有没有 `SessionStarted` |
| `kind -->\|新开\| skel` | `src/lib.rs:331-332` | `!resuming` |
| `kind -->\|--continue\| resume` | `src/lib.rs:334-344` | 补沙箱状态 + 收尾 + 重放 |
| `skel/resume --> banner` | `src/cli.rs:417-429` | 重放先于横幅 |
| `banner --> loop` | `src/cli.rs:446-459` | 进主循环 |
| `loop -->\|/quit 或空行\| quit` | `src/cli.rs:1177-1187`、`src/cli.rs:462` | `harness.shutdown()` |
| `loop -->\|忙碌时两下 Ctrl-C\| cc` | `src/cli.rs:2044-2052` | 先等回合收尾，再兑现 130 |
| `quit --> ok` | `src/cli.rs:1973-1978` | `finish_session` 原样返回 `code` |
| `loop -->\|panic\| panic` | 未找到 | 见 §8：没有这条边 |

---

## 3. 图 3：一次 turn（`sequenceDiagram`）

### 参与者

| 参与者 | 一句话 | 证据 | 符号 |
| --- | --- | --- | --- |
| `human` | 键盘：提示、手势、权限询问与问卷共用 | `src/render/input.rs:127-140`、`src/render/input.rs:205-215` | `struct ConsoleHandle`／`fn console` |
| `loop` | 循环：驱动回合、盯手势 | `src/cli.rs:1102-1313`、`src/cli.rs:2023-2053`、`src/lib.rs:667-682` | `fn run_one_turn`／`fn drive_turn` |
| `prov` | provider：请求、流式返回 | `src/provider/mod.rs:34-42` | `trait Provider` |
| `gate` | 权限门：纯函数裁决 + 询问往返 | `src/permissions.rs:530-640`、`src/agent.rs:1841-1998` | `fn decide`／`fn authorize` |
| `tools` | 工具表：锁与派发 | `src/tools/registry.rs:170-216` | `fn dispatch` |

### 消息 / 边

| 消息 | 证据 | 说明 |
| --- | --- | --- |
| `human->>loop: 一句提示` | `src/render/input.rs:133`、`src/cli.rs:1158-1160` | `console.prompt()` |
| `loop->>loop: cancel.reset()` | `src/lib.rs:667-672` | 手势范围限一次运行 |
| `loop->>loop: 挂着的 tool_call？→ TurnEnded` | `src/agent.rs:478-480` | 不变量 2，直接 `StopReason::Error` |
| `loop->>loop: 预算 / 迭代上限 / 取消 三处预检` | `src/agent.rs:484-521` | 取消 → 预算 → 迭代上限 |
| `loop->>loop: TurnStarted 落流` | `src/agent.rs:523-531` | — |
| `loop->>loop: 组装 messages` | `src/agent.rs:535-542`、`src/agent.rs:181-205`、`src/provider/projection.rs:46`、`src/context.rs:138` | project → 私有身份 → trim |
| `loop->>loop: 估预算预检` | `src/agent.rs:548-560`、`src/context.rs:70` | `estimate_messages_tokens` + `admits_estimate` |
| `loop->>prov: ChatRequest（tools 表原样）` | `src/agent.rs:562-569`、`src/agent.rs:574-581` | `tools: session.tools().specs()` |
| `prov-->>loop: 增量文本（走渲染通道）` | `src/agent.rs:607-614` | `render.text_delta`，不进流 |
| `prov-->>loop: UsageRecorded 落流` | `src/agent.rs:631-638` | 唯一用量账本 |
| `prov-->>loop: [DONE]` | `src/agent.rs:639-649`、`src/agent.rs:672-677` | 只有 `[DONE]` 才算完成单位 |
| `loop->>loop: MessageCompleted 落流` | `src/agent.rs:679-690` | — |
| `loop->>tools: 逐条 tool_call：hook.pre` | `src/agent.rs:896-994` | 在门之前、由循环直接调 |
| `tools->>gate: 权限门 + 钩子约束取上确界` | `src/agent.rs:998-1007`、`src/hooks.rs:99` | `Allow < Ask < Deny`（见 §8：方向应是 loop→gate） |
| `gate->>human: Ask 时问一次` | `src/agent.rs:1904-1954`、`src/permissions.rs:974-1012`、`src/render/input.rs:251` | 无 asker 则降级 `Deny` |
| `human-->>gate: 允许 / 总是允许 / 拒绝` | `src/agent.rs:1954-1995`、`src/permissions.rs:996-1004` | `Answer` 三值 |
| `gate->>tools: dispatch（工作区锁 → 路径锁）` | `src/agent.rs:1078-1112`、`src/tools/registry.rs:178-185` | — |
| `tools-->>loop: 结果：打码 → 截断 → 追加` | `src/agent.rs:2053-2087` | `emit_completed` |
| `loop->>loop: hook.post` | `src/agent.rs:1229-1241`、`src/agent.rs:2118-2170` | 失败只丢反馈 |
| `loop->>loop: 还有 tool_call？→ 下一轮迭代` | `src/agent.rs:726-728` | `last_assistant_has_tool_calls` |
| `loop-->>human: TurnEnded` | `src/agent.rs:2182-2192`、`src/agent.rs:730` | `end_turn{Completed}` |

---

## 4. 图 4：委派与嵌套

### 节点

| 节点 id | 一句话 | 证据 | 符号 |
| --- | --- | --- | --- |
| `deb` / `deb2` | 讨论者：两个并发跑各自回合、共享一条流 | `src/lib.rs:566-605`、`src/cli.rs:900-948`、`src/agent.rs:1334-1427` | `fn discussion_participants` |
| `task` | `task` 工具调用：先判预算、再构造端口并推迟 | `src/agent.rs:1044-1076` | — |
| `pre` | `hook.pre`：在门之前，能拦下一次询问 | `src/agent.rs:896-994` | — |
| `gate` | 权限门 → 通过后决定推迟还是就地派发 | `src/agent.rs:998-1007`、`src/agent.rs:1035-1077` | `fn authorize` |
| `hooks` | 钩子实现：可注入，CLI 不注入 | `src/hooks.rs:169-181`、`src/lib.rs:94`、`tests/hook_mount_points.rs:297-298` | `trait Hook` |
| `pool` | 推迟批：按 `max_parallel_executors` buffered | `src/agent.rs:1140-1178` | `fn run_deferred` |
| `spawn` | `ExecutorPort::new` 快照会话 | `src/agent/executor.rs:90-140` | `fn new` |
| `policy` | 派生策略：只传播 `Deny` 与 `Ask` | `src/agent/executor.rs:107-111`、`src/permissions.rs:414-420` | `fn inherited_rules` |
| `table` | 执行者工具表：`task` 结构性缺席 | `src/agent/executor.rs:134`、`src/tools/registry.rs:70-78`、`src/tools/tool.rs:242-248` | `fn for_executor`／`fn delegable` |
| `budget` | 独立回合预算：`executor_max_iterations` | `src/agent/executor.rs:118`、`src/config.rs:41-45` | `const DEFAULT_MAX_ITERATIONS` |
| `run` | 执行者自己的回合 | `src/agent/executor.rs:184-192` | `fn run_turn`（`TurnScope::Executor`） |
| `spawned` | `ExecutorSpawned` 落流（简报先记） | `src/agent/executor.rs:148-159` | — |
| `finished` | `ExecutorFinished` 落流 | `src/agent/executor.rs:204-215` | — |
| `one` | 那次 `task` 调用唯一的结果：摘要 + 改动文件 + token | `src/agent/executor.rs:217-231`、`src/agent/executor.rs:305-324` | `fn executor_report` |
| `round` | 讨论轮次：`RoundStarted` + `join_all` 并发 | `src/agent.rs:1400-1427` | — |
| `syn` | 合成器：一次 `run_single_shot` | `src/agent.rs:1525-1548`、`src/agent.rs:1600` | `fn run_single_shot` |
| `out` | 共识 / 分歧 / 未决 | `src/agent.rs:1574-1584`、`CONTEXT.md:36-38` | — |

### 边

| 边 | 证据 | 说明 |
| --- | --- | --- |
| `deb/deb2 --> task` | `src/agent.rs:702-711` | 回合里逐条调 `process_call` |
| `task --> pre` | `src/agent.rs:890-891` | `resolve_facts` 后立刻 `hook.pre` |
| `pre --> gate` | `src/agent.rs:998-1007` | 门在钩子之后，取上确界 |
| `pre -.-> hooks` | `src/agent.rs:903-917`、`src/cli.rs:396` | CLI 恒 `None`，测试可注入 |
| `gate --> pool` | `src/agent.rs:1072-1076` | `task` 被推迟 |
| `pool --> spawn` | `src/agent.rs:1155-1176`、`src/agent/executor.rs:236-238` | `dispatch` → `ExecutorSpawner::spawn` |
| `spawn --> policy/table/budget` | `src/agent/executor.rs:107-134` | 三者并列派生，不是流水线 |
| `budget --> run` | `src/agent/executor.rs:184-192` | — |
| `spawn --> spawned` | `src/agent/executor.rs:148-159` | 做任何事之前先记 |
| `run --> finished` | `src/agent/executor.rs:204-215` | — |
| `finished --> one` | `src/agent/executor.rs:219-231` | 元数据从流派生 |
| `one --> deb` | `src/agent/executor.rs:226-230` | 成功是 `ToolOutput`，否则 `ToolError` |
| `deb2 --> round` | `src/agent.rs:1409-1417` | 每轮 `RoundStarted` 后 `join_all` |
| `round --> syn` | `src/agent.rs:1516-1533` | 取消时**不**靠近合成器 |
| `syn --> out` | `src/agent.rs:1567-1584` | — |

---

## 5. 图 5：基础设施回路

### 节点

| 节点 id | 一句话 | 证据 | 符号 |
| --- | --- | --- | --- |
| `emit` | `append_event`：唯一写路径 | `src/agent.rs:2215-2227` | `fn append_event` |
| `red` | 打码在追加之前，于是「流上文本 == 模型看到的文本」 | `src/agent.rs:2223`、`src/events.rs:513-530`、`src/events.rs:641-665` | `fn redact` |
| `log` | `EventLog::append`：JSONL 一行一事件 | `src/events.rs:1009-1038` | `fn append` |
| `disk` | 会话目录：JSONL + `outputs/` | `src/events.rs:948-1002`、`src/lib.rs:233-236` | `fn create`／`fn open` |
| `rlog` | `render.logged`：广播通道 | `src/render/mod.rs:104-145` | `fn logged` |
| `tui` | 渲染器 TUI | `src/cli.rs:353-361`、`src/render/mod.rs:194-197` | `fn tui` |
| `plain` | 渲染器 plain | `src/cli.rs:364-371`、`src/render/mod.rs:190-193` | `fn plain` |
| `headless` | 渲染器 headless（生产组装点只有 `probe`，见 §8） | `src/render/mod.rs:171-173`、`src/render/mod.rs:186-189`、`src/cli.rs:2238` | `fn headless` |
| `proj` | `project`：纯函数 → `messages` | `src/provider/projection.rs:46` | `fn project` |
| `trim` | `trim`：纯函数，只读、不删日志 | `src/context.rs:138-180` | `fn trim` |
| `req` | provider 请求 | `src/agent.rs:562-569` | — |
| `cont` | `--continue`：补骨架之外的状态 | `src/lib.rs:244-252`、`src/lib.rs:330-345`、`src/cli.rs:417-419` | `fn start` |
| `replay` | `sessions replay`：同一 `build_messages` 重算 | `src/agent/replay.rs:55-85`、`src/cli.rs:2870` | `fn replay` |
| `goal` | `/loop`：每回合边界重算进度 | `src/cli.rs:1546-1626`、`src/lib.rs:838-845`、`src/lib.rs:880-912` | `fn run_goal_loop`／`fn compact_and_rollover` |
| `roll` | 阈值判定：`remind_at` / `compact_at` | `src/cli.rs:1710-1716`、`src/goals.rs:629-640` | `fn threshold_step` |
| `compact` | 压缩 + 翻页：一个动作，成对发生 | `src/lib.rs:880-890`、`src/lib.rs:923-929` | `fn compact` |
| `ask` | 询问 Ask：harness 发起，答案是闸门 | `src/permissions.rs:974-1012`、`src/render/input.rs:223-256` | `trait Asker` |
| `uq` | 用户提问：模型发起，答案是上下文 | `src/questions.rs:70-78`、`src/render/input.rs:259-300`、`src/tools/ask_user.rs:201-220` | `trait UserQuestions` |
| `console` | 终端端口 Console：同一个键盘 | `src/render/input.rs:205-215` | `fn console` |

### 边

| 边 | 证据 | 说明 |
| --- | --- | --- |
| `emit --> red --> log` | `src/agent.rs:2223-2224` | 打码是追加前的最后一件事 |
| `log --> disk` | `src/events.rs:1009-1038` | 一行一事件、`seq` 即行号 |
| `emit --> rlog` | `src/agent.rs:2225` | 同一次追加叙述给渲染器 |
| `log --> proj` | `src/provider/projection.rs:46` | 投影只读流 |
| `proj --> trim` | `src/agent.rs:535-542` | 两个纯函数，串联 |
| `trim --> req` | `src/agent.rs:562-569` | — |
| `disk --> cont` | `src/lib.rs:244-252` | 由「日志里有没有 `SessionStarted`」判定 |
| `disk --> replay` | `src/cli.rs:2870` | 从会话目录读回 |
| `cont --> proj` / `replay --> proj` | `src/agent.rs:181-205`、`src/agent/replay.rs:80-85` | 同一 `build_messages` |
| `goal --> roll` | `src/cli.rs:1710-1716` | 只在回合边界判 |
| `roll -->\|是\| compact` | `src/cli.rs:1731-1741` | `compact_and_rollover` |
| `compact --> goal` | `src/cli.rs:1745-1762` | 重新认领目标、重摆清单、额度不重置 |
| `ask --> console` | `src/render/input.rs:251` | `ConsoleAsker` |
| `uq --> console` | `src/render/input.rs:278` | `ConsoleQuestions`（第三类发起者，不扩展 `Asker`） |
| `console --> emit` | `src/render/input.rs:133`、`src/cli.rs:1158-1160` | 提示经循环变成 `user` 消息 |
| `rlog --> tui/plain/headless` | `src/render/mod.rs:184-199` | 一个进程恰好一个渲染器 |

---

## 6. `research/01` §5 那 12 条不变量 → 图上哪条边

| # | 不变量 | 图上的边 | 证据 |
| --- | --- | --- | --- |
| 1 | 事件流是唯一真相源，`messages` 重算、从不存 | `log --> proj --> trim --> req`；`cont/replay --> proj` | `src/lib.rs:3-4`、`src/agent.rs:181-205`、`src/provider/projection.rs:46` |
| 2 | `append_event` 是唯一写路径；打码在追加之前 | `emit --> red --> log`；`emit --> rlog` | `src/agent.rs:2215-2227`、`src/agent.rs:2054-2070` |
| 3 | 工具表组装后不变 | `mode --> tools`（无回边） | `src/cli.rs:386-389`、`src/tools/mod.rs:96-118`、`src/tools/registry.rs:88-93` |
| 4 | 权限门在沙箱之外，且是纯函数 | `pre --> gate --> tools` 与 `tools` 内部的沙箱 | `src/lib.rs:389-397`、`src/permissions.rs:530`、`src/tools/process.rs:11-13`、`CONTEXT.md:281-284` |
| 5 | 每次调用固定顺序 `hook.pre → 门 → [询问] → dispatch → hook.post → 追加` | 图 3 六条相邻消息；图 4 `task → pre → gate` | `src/agent.rs:1-7`、`src/agent.rs:896-1131`、`src/hooks.rs:99`、`README.md:222` |
| 6 | 每个 `tool_call` 恰好一个结果；有挂起调用就不调 provider | 图 3 `loop->>loop: 挂着的 tool_call？`、`tools-->>loop: 结果` | `src/agent.rs:9-13`、`src/agent.rs:478-480`、`src/agent.rs:2040-2043` |
| 7 | 取消沿 token 树向下、从不向上；手势不进流 | `human->>loop` 的手势 → 执行者；`loop -->\|两下 Ctrl-C\| tail` | `src/agent/cancel.rs:9-12`、`src/agent/executor.rs:84-86`、`src/lib.rs:667-670` |
| 8 | 委派深度为一，由工具表强制 | `table`（`task` 缺席）；`spawn --> table` | `src/tools/registry.rs:70-78`、`src/tools/tool.rs:242-248`、`docs/executor.md:70-76` |
| 9 | 下游只继承 `Deny`/`Ask`，`Allow` 不传播 | `spawn --> policy` | `src/permissions.rs:414-420`、`src/agent/executor.rs:98-111`、`README.md:213` |
| 10 | `workspace` 档离开沙箱就不存在 | `probe` 与组装期拒绝；`Shift+Tab` 再跳一次 | `src/lib.rs:398-414`、`src/lib.rs:1146-1156`、`src/tools/sandbox.rs:48-83` |
| 11 | 两个讨论者共享同一条流且并发写，独立性靠 `seq` 切窗 | 图 4 `deb`/`deb2 --> round` 的并发与 `TurnScope::Round` | `src/events.rs:925-929`、`src/agent.rs:58-79`、`src/agent.rs:1414-1427` |
| 12 | 合成器那一次调用绝不跳过 | `round --> syn`；预算硬停降级进它 | `src/agent.rs:1382-1398`、`src/agent.rs:1525-1548`、`src/agent.rs:1598-1599` |

---

## 7. 复核 `research/01` 的 5 条存疑

1. **headless 渲染器的真实组装点 —— 已复核，并推翻「`cli.rs` 从不构造它」。**
   生产路径有：`probe` 子命令 → `probe_model`（`src/cli.rs:2150`）在 `src/cli.rs:2238` 构造
   `Renderer::headless`；这一段在 `#[cfg(test)]`（`src/cli.rs:3312` 起）**之外**，是真代码。
   测试/库调用方另有 20+ 处，如 `tests/e2e_single_turn.rs:52`、`tests/web_search.rs:176`、
   `tests/discussion.rs:1108`，以及 `src/cli.rs` 单测 `:4013`、`:4099`、`:4250`。
   结论：headless 是 **probe 子命令 + 测试** 的路径；图 5 的虚线语义（「可注入但 CLI 不注入」）
   对 headless 不再准确，应改注「CLI 交互式不构造；probe 与库调用方/测试构造」。
2. **hook 在交互式路径上恒为 `None` —— 已复核成立。**
   三个生产组装点全是 `hook: None`：`src/cli.rs:396`（interactive）、`src/cli.rs:715`
   （`discuss` 子命令）、`src/cli.rs:2232`（probe）。类型是 `Option<Arc<dyn Hook>>`
   （`src/lib.rs:94`）。可注入由测试证明：`tests/hook_mount_points.rs:297-298`（及同文件多处）
   传 `Some(...)`。结论：图 4 的 `pre -.-> hooks` 虚线注释「可注入但 CLI 不注入」是对的。
3. **`docs/web.md` 与 `dsh web` 的关系 —— 复核后维持原判定。**
   `grep -n "dsh\|GUI\|web 服务" docs/web.md` 无结果；按代码 `src/web/` 只做出网工具后端：
   `src/cli.rs:2306-2335`（`web_service`）与 `src/tools/mod.rs:118-130`（`with_web`），
   挂上 `web_search`/`web_fetch`。**仍未找到**仓库内任何把 `docs/web.md` 与 DSH harness 的
   `dsh web` 关联起来的文字或代码 —— 该说法应视为仓库外知识，不写进 `docs/lifecycle.md`。
4. **`context/` 与 `config/` 目录 —— 已复核成立。**
   是 `src/context.rs` + 子目录 `src/context/{repo_map.rs,skills.rs}`、`src/config.rs` + 子目录
   `src/config/cost.rs`，不是两个并列顶层模块。权威边界清单：`src/lib.rs:28-42` 的 15 个
   `pub mod`（`context`、`config` 各一行）。
5. **`src/render/tui.rs` 等未逐行读 —— 仍未逐行读，但不阻塞这五张图。**
   本票按图所需只回读了 TUI 的 panic hook（`src/render/tui.rs:537`）、`Drop for TerminalModes`
   （`:559-563`）与 `ratatui::restore`（`:471`）。`render/editor.rs`、`render/panel.rs`、
   `render/pane.rs`、`render/todo.rs`、`tools/edit.rs`、`tools/file.rs` 仍未逐行读 —— 这五张图
   **都没有画到**编辑器按键循环或 `edit_file` 的匹配梯降级，所以本表不含未验证的转引。

---

## 8. 图上画了、但代码里没找到 / 对不上的诚实清单

1. **图 2 `tools --> probe --> open` 的顺序不存在。** 组装期显式的沙箱检查只有
   `assemble` 里的 `workspace_needs_a_sandbox`（`src/lib.rs:457`），而它在非 `workspace` 档
   直接早退、根本不探（`src/lib.rs:403-405`）。真正的惰性探测在 `open`（`:459`）之后的
   `opened.session(...)`（`:460` → `:279` → `:315-322`）。所以 `probe` 不能画成 `open` 的
   前置节点，应画成 `open` 内部的一步。
2. **图 2 `loop -->|panic| panic` 这条边没有。** panic hook 只在 TUI 的
   `TerminalModes::enter` 里装一次（`src/render/tui.rs:537`），`Drop` 也只挂在
   `TerminalModes` 上（`:559-563`）；plain/headless 不装任何钩子。而且 panic 可以从任意点
   发生，不存在「主循环 → panic」这条可指认的控制流边。建议改注为「TUI 专属、进程任意点可
   触发」或撤掉这条边。
3. **图 1 `render{渲染器：TUI / plain / headless}` 三选一在交互式路径上不成立。**
   交互式只二选一（`src/cli.rs:328-372`，`use_tui` 非真即假）；`headless` 的生产组装点只有
   `probe`（`src/cli.rs:2238`）。三选一这句只对「三种前端总的枚举」成立
   （`src/render/mod.rs:161-168`），对「这一趟选定哪一个」不成立。
4. **图 1 `maint --> ok` 没有共同的 `ok` 收尾路径。** `probe`/`prune`/`sessions` 各自直接返回
   自己的 `ExitCode`（`src/cli.rs:115-124`），既不组装会话、也不走 `interactive` 的
   `shutdown → finish_session`（`src/cli.rs:460-467`）。三支应各自收在自己的终点。
5. **图 5 的 `roll{过 compact_at 了吗}` 不是一个可指认的节点/函数。** 判据是
   `goals::threshold_step(percent, remind_at, compact_at, reminded)`（`src/goals.rs:629-640`），
   唯一调用点在目标循环里（`src/cli.rs:1711-1716`）；`/clear` 走
   `clear_session`（`src/cli.rs:1869`）完全不判阈值。它属于目标循环内部的一步，不是跨阶段的
   基础设施节点。
6. **图 3 的 `gate` 参与者不是一次真实的对象往返。** 「询问 → 人答」经
   `asker.ask(&request)`（`src/agent.rs:1954`）到 `ConsoleAsker`（`src/render/input.rs:251`），
   裁决合成在 `authorize` 里（`src/agent.rs:1954-1995`）；没有一个叫 `gate` 的对象持有这次
   往返。作为 sequence 抽象可以，但证据表只能落到 `authorize`/`ConsoleAsker`。
7. **图 3 `tools->>gate: 权限门 + 钩子约束取上确界` 的方向不对。** 门由**循环**在
   `process_call` 里调（`src/agent.rs:998-1007`），工具表只在门之后被调
   （`src/agent.rs:1081-1097`）。应写成 `loop->>gate`，再把 `gate->>tools: dispatch`。
8. **五张图的节点 id 跨图撞名，会让 02 号设计的 C4/C5 判不了。** 重复的有：
   `cmd`（图 1、2）、`loop`（图 1、2、3）、`ok`（图 1、2）、`gate`（图 3、4）、`red`（图 2、5）、
   `goal`（图 1、5）；`tail`/`quit`、`turn`/`run` 语义也重叠。`02-lifecycle-check-design.md` §1·C4
   的比法是整个文档一个 `set[str]`、要求「有且仅有一行」，撞名必假差异。定稿前请给节点加图前缀
   （如 `g1_boot`）或让五张图用不相交的 id 空间 —— 这是**图与检查器对不上键**，不是代码缺失。
