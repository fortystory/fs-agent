# fs-agent 运行时生命周期：代码事实清单

本文件是 2026-10-03 charting 阶段一次**只读侦察**的产物（未修改任何文件、未跑构建），作为本图
所有子票的共同基线：图画得对不对，以这里的事实为准。每个论断带 `文件:行号`；未逐行读过的段落
在末尾 `## 存疑` 里写明。

## 1. 进程生命周期

| 阶段 | 一句话 | 关键证据 |
| --- | --- | --- |
| 入口 | `main.rs` 只调 `fs_agent::cli::main()`，无逻辑 | `src/main.rs:1-3` |
| root 拒绝 | 先于参数、先于 runtime 检查 euid，无旗标可绕 | `src/cli.rs:73-86` |
| 环境快照 | 进程变量收进 `EnvMap`、`argv` 去首参，注入 `run` | `src/cli.rs:87-88` |
| runtime | 多线程 tokio runtime，失败即退 | `src/cli.rs:89-102` |
| 子命令分派 | `--version`/`--help`/`probe`/`discuss`/`prune`/`sessions`，其余走交互式 | `src/cli.rs:105-129` |
| 交互式参数解析 | `--plain/--tui/-c/--continue/--session/--config/--model/--mode/--cwd`；两渲染器互斥 | `src/cli.rs:161-219` |
| 配置加载与合并 | 显式 `--config` 必须存在；否则默认路径存在就读、不存在就 `resolve(None)` 走纯默认 + 环境 | `src/cli.rs:2087-2100`、`src/config.rs:763-824` |
| 凭据与打码器 | 每条 provider key 解析一次；打码器持全部 key 值，只在 ≥8 字符时生效 | `src/config.rs:713-720`、`src/events.rs:611-630` |
| 模型校验 | 未登记模型是启动错误，不静默降级 | `src/cli.rs:255-269` |
| 会话 store 与工作目录 | store root 来自环境；cwd = `--cwd` 或进程 cwd | `src/cli.rs:270-287` |
| 选择会话 | `choose_session` 决定最新、指名或全新；跨工作区时 cwd 跟着会话走 | `src/cli.rs:289-297` |
| provider 装配 | `OpenAiProvider::build(&config,&model,...)`：reqwest client + retry policy | `src/cli.rs:299-305`、`src/provider/openai.rs:101-127` |
| provider 能力表 | `caps_for(model)` 再取一次（双保险），决定窗口与投影分叉 | `src/cli.rs:334-340`、`src/provider/capability.rs` |
| SessionConfig | 从 Config 拷模型参数、pricing、budget、redactor、迭代上限、沙箱、routing | `src/config.rs:696-706` |
| 目标目录 | `goals_dir(env)` 在会话目录旁；库不读环境 | `src/cli.rs:319`、`src/config.rs:866` |
| 起始权限档 | `--mode` 压过 `[permissions] mode`，一次定死给门/横幅/状态行 | `src/cli.rs:226-228`、`src/cli.rs:323` |
| 渲染器选定 | `Tui` 若 `--tui` 或（非 `--plain` 且 stdout 是终端），否则 `Plain`；headless 只由库调用方组装 | `src/cli.rs:327-372`、`src/render/mod.rs:161-200` |
| 控制台端口 | `render::console()` 造键盘两端：句柄 + 手势；同一个键盘兼作权限询问与问卷 | `src/cli.rs:327`、`src/render/mod.rs:204-207`、`src/render/input.rs:205` |
| asker / questions | 交互式总是两者都有，于是工具表提供 `ask_user_question` | `src/cli.rs:373-378`、`src/tools/mod.rs:90-92` |
| 工具表组装 | `with_web(with_dynamic(&config.tools, can_ask), web_service)`；组装期定死 | `src/cli.rs:386-389`、`src/tools/mod.rs:78-130` |
| 沙箱探测 | 组装期一次；`mode="off"` 或调用方已给结果则不探；`workspace` 档无沙箱即在组装期拒绝 | `src/lib.rs:315-322`、`src/lib.rs:398-414`、`src/tools/sandbox.rs:48-83` |
| assemble | `OpenedSession::open`：建/开日志、发现技能、读 `AGENTS.md`、起渲染器；随后 `start` | `src/lib.rs:217-275`、`src/lib.rs:448-470` |
| 会话骨架 | 新流记 `SessionStarted` + 沙箱状态 + `AGENTS.md` + 技能清单（钉住注入） | `src/lib.rs:366-386` |
| 恢复 | 继续的流**不**重记骨架：补记沙箱状态、收尾悬空 `tool_call`、重放历史给前端 | `src/lib.rs:330-345`、`src/agent/history.rs:40-61`、`src/cli.rs:417-419` |
| 横幅与菜单 | 横幅走渲染通道；`/` 菜单 = 内建命令 + 已发现技能 | `src/cli.rs:423-444` |
| 主循环 | `interactive_loop`：等一行或手势 → 分派 Submission → 兑现代码 | `src/cli.rs:1102-1313` |
| 退出 · 正常 | 空输入的 `None`、`/quit`、`Submission::Ignore` 之后的循环 | `src/cli.rs:1177-1187` |
| 退出 · `Ctrl-C` | 忙时第一下 = 取消（取消 token），第二下 = 记 `ExitRequest`，等回合收尾后以 130 退 | `src/cli.rs:1973-1979`、`src/cli.rs:1988-2016`、`src/cli.rs:2044-2052` |
| 退出 · 取消传播 | 手势沿 `CancelObserver` 向下传到每个执行者；从不向上 | `src/agent/cancel.rs:8-15`、`src/agent/executor.rs:84-86` |
| 退出 · 崩溃 | TUI 的 panic hook 还原鼠标/粘贴/标题；`Drop` 还原终端模式；`ratatui::restore()` | `src/render/tui.rs:471-472`、`src/render/tui.rs:526-560` |
| 收尾 | `harness.shutdown()` 排空渲染通道，然后 stderr 打一行可粘的续接回执 | `src/cli.rs:460-467`、`src/lib.rs:1161-1164` |

## 2. 一次 turn 的生命周期

入口三分：`TurnStart::Prompt` → `run_turn`；`TurnStart::Skill` → `load_skill` + `drive_turn`；
`TurnStart::Injected` → `run_injected_turn`（`src/cli.rs:2023-2037`、`src/lib.rs:658-682`）。

每回合起点：`cancel.reset()`（手势范围限一次运行），拿 `observer`（`src/lib.rs:667-682`）。
`run_turn` 主循环（`src/agent.rs:470-731`）：

1. **快照**：`scoped_events(session, speaker, scope)` —— `Whole` / `Round{before_seq}` / `Executor`
   三种窗口（`src/agent.rs:82-113`、`src/agent.rs:473`）。
2. **不变量 2 检查**：还有挂着的 `tool_call` 就绝不调 provider，直接 `TurnEnded{Error}`
   （`src/agent.rs:478-480`）。
3. **取消预检** → `TurnEnded{Aborted}`（`src/agent.rs:484-487`）。
4. **预算硬停**：`budget.exhausted_note(spent_tokens)`；执行者豁免（`gated`）
   （`src/agent.rs:496-510`、`src/agent.rs:211-220`、`src/config/cost.rs:98`）。
5. **迭代上限**：`iteration >= max_iterations` → `MaxIterations`（默认 100）
   （`src/agent.rs:512-521`、`src/config.rs:41`）。
6. **`TurnStarted{iteration}`** 落流（`src/agent.rs:523-531`）。
7. **上下文组装**：`build_messages` = `project(events, speaker, caps)` → 前置私有身份 `system`
   （从不进流）→ `context::trim(_, usable_input(caps), TrimPolicy)`（`src/agent.rs:181-205`、
   `src/provider/projection.rs:46-…`、`src/context.rs:138-180`）。裁剪顺序：旧普通工具结果 →
   旧技能正文 → 旧整轮 → 硬失败（`src/context.rs:124-128`、`src/context.rs:151-179`）。
8. **估预算预检**：`estimate_messages_tokens` + `budget.admits_estimate`（字符数/4 的粗估），
   明显装不下就不发（`src/agent.rs:548-560`、`src/context.rs:64-72`、`src/config/cost.rs:86`）。
9. **请求**：`ChatRequest{ model, messages, tools: session.tools().specs(), tool_choice: Auto,
   params, cache_key: session_id }`（`src/agent.rs:562-569`、`src/tools/registry.rs:91-93`）。
10. **发送**：`tokio::select!{biased}` 让取消压过 `provider.send`（`src/agent.rs:573-588`、
    `src/provider/openai.rs:235-249`）。
11. **流式增量**：`TextDelta`/`ReasoningDelta` 只走渲染通道（不进流）；`Usage` 立即 emit
    `UsageRecorded`；工具调用碎片由适配器拼装，循环只收完整调用（`src/agent.rs:597-660`、
    `src/provider/openai.rs:453-504`、`src/provider/openai.rs:622-…`）。
12. **流只有 `[DONE]` 才算完成单位**：失败/半截流什么都不落流，`TurnEnded{Error}`
    （`src/agent.rs:662-677`）。
13. **`MessageCompleted{role: Assistant, text, reasoning?}`**（`src/agent.rs:679-690`）。
14. **逐调用分发**（`src/agent.rs:701-721` → `process_call` `src/agent.rs:821-1132`）：
    - `ToolCallStarted` 落流，构造 `PendingCall`（含 paths/locks/skills/repo_map/bash limits/
      sandbox/questions）（`src/agent.rs:835-885`）。
    - `resolve_facts` 解析 effect / 写目标 / 读路径 / argv / escalation，只此一次 canonicalize
      （`src/agent.rs:890-894`、`src/tools/registry.rs:102-164`）。
    - **① `hook.pre`**：在门之前，能拦下一次询问；`Rewrite` 会重解析事实；`Skip`/`Stop` 各自
      合成一个结果；失败即挡下（`src/agent.rs:896-994`、`src/hooks.rs:169-181`）。
    - **② 权限门 ③ 询问**：`permissions::decide`（纯函数：断路器 → 模式立场 → 规则上确界 →
      地板/路径上限/升级/`.env`）与钩子约束取上确界 `Allow < Ask < Deny`；`Ask` 时经 asker 问；
      无 asker 降级为 `Deny`（`src/agent.rs:998-1007`、`src/agent.rs:1841-1998`、
      `src/permissions.rs:531-640`、`src/hooks.rs:99`）。四档由 `Mode::stance` 逐档给出
      default/floor/reason（`src/permissions.rs:127-186`），`next()` 循环四档
      （`src/permissions.rs:113-120`）。
    - 放行但曾越界：按方向放宽读/写解析再解析一次；升级批准的路径只活这一次调用
      （`src/agent.rs:1014-1040`）。
    - **④ dispatch**：`task` 先判预算、再构造 `ExecutorPort` 并**推迟**；其余就地
      `tools.dispatch`（取工作区锁 → 路径锁 → `tool.call`）（`src/agent.rs:1041-1112`、
      `src/tools/registry.rs:170-216`）。
    - 工具进行中的取消：future 被 drop，合成「结果未知」的结果（`src/agent.rs:744-748`、
      `src/agent.rs:1083-1096`）。
    - **⑤ 结果回灌**：`finish_call` 记读集合/失效、`emit_completed`（打码 → 截断/溢出落盘 →
      追加），然后 `hook.post`（只丢反馈不藏结果）（`src/agent.rs:1186-1243`、
      `src/agent.rs:2045-2088`、`src/agent.rs:2118-2176`）。
15. **推迟批并发**：`run_deferred` 用 `buffered(max_parallel_executors)`，仍按批内顺序记录
    （`src/agent.rs:1140-1179`）。
16. **回边**：`last_assistant_has_tool_calls` 为真 → `continue` 下一轮迭代；否则
    `end_turn{Completed}`（`src/agent.rs:726-730`、`src/agent.rs:2182-2192`）。
17. **渲染完成**：每条事件经 `append_event` → `render.logged`，增量经 `text_delta`；渲染器在
    自己的任务里消费同一广播通道（`src/agent.rs:2215-2227`、`src/render/mod.rs:104-145`、
    `src/render/mod.rs:184-199`）。

预算计数：`UsageRecorded` 累积是唯一账本，`total_usage`/`usage_of` 从事件求和，`carried_tokens`
补上「别处已花」（`src/events.rs:900-925`、`src/agent.rs:211-220`）。

## 3. 嵌套 / 委派生命周期

- **讨论者（debater）**：`assemble_discussion` 校验名册（恰好 2 人、身份互异、预算与打码器一致）
  → 一个 `OpenedSession` 开两次会话，只差发言身份、模型、私有身份（`src/lib.rs:477-559`、
  `src/lib.rs:566-605`、`src/lib.rs:611-654`）。`run_discussion`：先记用户问题与各讨论者的人物
  注入；每轮 `RoundStarted`，`join_all` 并发跑两个 `run_turn`（`TurnScope::Round`），轮次判定
  全部从流上查询（出席/一致/缺席），收敛或第二轮后走向合成器；取消时不靠近合成器
  （`src/agent.rs:1334-1585`、`src/discussion.rs:28-63`、`src/discussion/protocol.rs:158-…`）。
- **合成器（synthesizer）**：不是 agent、没有工具与回合；一次 `run_single_shot`，用量照记；
  预算不设闸（那一次绝不能跳过）（`src/agent.rs:1525-1573`、`src/agent.rs:1598-1600`、
  `src/lib.rs:594-603`）。
- **执行者（executor）**：`task` 调用被授权后 `ExecutorPort::new` 快照会话：新读集合、派生策略
  （父档位 + 只传播的规则，`Allow` 不传播）、模型经 `LandingPoint::Executor` 路由、
  `max_iterations = executor_max_iterations`（默认 25）、工具表 `for_executor()` 去掉不可委派者
  （`task` 因此结构性缺席）、`questions: None`、共享 provider/日志/锁/asker/hook/技能
  （`src/agent/executor.rs:90-140`、`src/agent/executor.rs:161-180`、`src/tools/registry.rs:70-78`、
  `src/tools/tool.rs:242-248`、`src/config/cost.rs:220-…`）。
- **执行者回合**：`run_turn(..., TurnScope::Executor, ...)` → `ExecutorSpawned`（简报，先记）/
  `ExecutorFinished`；报告 = 摘要 + 从流派生的改动文件 + 该执行者的用量
  （`src/agent/executor.rs:143-231`、`src/agent/executor.rs:278-323`）。
- **`--discuss` 与会话内 `/discuss` 的区别**：`--discuss` 是独立子命令 / 独立
  `assemble_discussion`，自己建会话、自己记骨架；会话内 `/discuss` 走 `Harness::discuss`，
  讨论者是**这场会话的兄弟**，共享其日志/工具/锁/策略/技能/渲染器并**继承上下文**，轮次追加在
  同一条流上（`src/lib.rs:684-720`、`src/cli.rs:545-…`、`src/cli.rs:950-1040`）。
- **裸 `/discuss`**：取会话里最后一条 `role: User` 的 `MessageCompleted` 作为问题
  （`src/lib.rs:722-740`、`src/cli.rs:967-980`）。

## 4. 跨阶段的基础设施回路

- **事件流是唯一真相源**：`EventPayload` 枚举是全部事件种类（会话骨架/注入/沙箱状态/目标/轮次/
  回合/消息/工具/用量/权限/钩子/执行者/错误/历史注销）（`src/events.rs:325-471`）；**唯一写路径**
  `append_event`：打码 → `EventLog::append` → `render.logged`（`src/agent.rs:2203-2227`）；
  `events` 模块零内部依赖（`src/events.rs` 全文无 `use crate::`）。
- **持久化**：JSONL，一行一事件，`{seq, at, speaker_id, payload}`，`seq` 即行号；新建即 `0600`；
  打开时修坏尾（`src/events.rs:931-1038`、`src/events.rs:1043-1091`）。
- **重建**：`--continue` 由「日志里有没有 `SessionStarted`」判定，续接补齐未答完的调用，不重注入、
  不重记骨架（保前缀逐字稳定）（`src/lib.rs:244-252`、`src/lib.rs:330-345`）；`sessions replay`
  用同一 `build_messages` 从流重算请求（`src/agent/replay.rs:55-85`、`src/cli.rs:2870`）。
- **投影**：`project(events, speaker, caps) → messages` 是纯函数；自己的回合成 assistant、别人的
  成 user、另一讨论者的工具只留摘要、钉住注入合并成一条（`src/provider/projection.rs:1-…`）；
  裁剪是投影之后的第二个纯函数（`src/context.rs:1-16`）。
- **goal 循环**：目标是文件（清单），进度、预算、归属、恢复**全部从流派生**；`/loop` 起循环，
  每回合边界重算进度，阈值触发一次提醒或「压缩 + 翻页」成对动作，翻页不重置额度，异常中断才自动
  续跑（`src/cli.rs:1546-1800`、`src/lib.rs:838-845`、`src/lib.rs:880-912`、`src/goals.rs`）。
- **`ask_user_question` 通道**：发起者是**模型**（工具调用），端口是会话级 `UserQuestions`；
  TUI/plain 实现它；headless 不挂端口、工具表也不宣告它；执行者的表里没有这个工具
  （`src/questions.rs:74-78`、`src/tools/ask_user.rs:206-216`、`src/tools/mod.rs:90-92`、
  `src/agent/executor.rs:173-175`）。**权限询问**是另一类发起者（harness），走 `Asker`
  （`src/permissions.rs:976-1010`、`src/render/input.rs:241-283`）。
- **渲染层**：三种后端（headless / plain / TUI）启动时互斥选定，共享一条广播通道与一个
  `transcript` 层；markdown → highlight 在 `src/render/markdown.rs:647` 调
  `highlight::highlight_code`，TUI 在 `src/render/tui.rs:4453` 调 `markdown::to_lines_indented`
  （`src/render/mod.rs:1-22`、`src/render/mod.rs:161-200`）。
- **web**：fs-agent **没有** web 服务子命令。`src/web/` 是 `web_search`/`web_fetch` 两个工具的
  后端（服务层合并去重 + 出网后端），由 `cli::web_service` 组装（`src/cli.rs:2306-2335`、
  `src/web/mod.rs:202-228`、`docs/web.md:11-36`）。GUI 那个 `dsh web` 属于 DSH harness，不在
  本仓库。

## 5. 图上非画不可的边

1. **事件流是唯一真相源，`messages` 永远重算、从不存**（`src/lib.rs:3-4`、`src/agent.rs:175-205`、
   `src/agent.rs:2203-2227`）。
2. **`append_event` 是唯一写路径**；打码在追加之前，于是「流上文本 == 模型看到的文本」
   （`src/agent.rs:2054-2070`、`src/agent.rs:2203-2227`）。
3. **工具表组装后不变**（动态工具与联网工具都在组装期入表；表是缓存前缀的一部分）
   （`src/cli.rs:386-389`、`src/tools/mod.rs:96-118`、`src/tools/registry.rs:88-93`）。
4. **权限门在沙箱之外，且是纯函数**；门答「跑不跑」，沙箱答「能碰到什么」
   （`src/lib.rs:389-397`、`src/permissions.rs:530`、`src/tools/process.rs:11-13`、
   `CONTEXT.md:283-284`）。
5. **每次工具调用的固定顺序：`hook.pre → 门 → [询问] → dispatch → hook.post → 追加`**；
   钩子只能收紧（`src/agent.rs:3-7`、`src/agent.rs:896-1131`、`src/hooks.rs:99`、
   `README.md:221-223`）。
6. **每个 `tool_call` 恰好一个结果；有挂起调用就不调 provider**（`src/agent.rs:9-13`、
   `src/agent.rs:478-480`、`src/agent.rs:773-791`、`src/agent.rs:2040-2043`）。
7. **取消沿 token 树向下传播、从不向上**；手势是运行级、不进流（`src/agent/cancel.rs:8-15`、
   `src/agent/executor.rs:84-86`、`src/lib.rs:667-670`）。
8. **委派深度为一，由工具表强制**（`task` 在 `for_executor` 中被过滤掉）
   （`src/tools/registry.rs:70-78`、`src/tools/tool.rs:242-248`、`docs/executor.md:70-76`）。
9. **下游只继承 `Deny`/`Ask`，`Allow` 不传播**（`src/permissions.rs:414-427`、
   `src/agent/executor.rs:98-111`、`README.md:213`）。
10. **`workspace` 档离开沙箱就不存在**，组装期即拒，`Shift+Tab` 里再跳过一次
    （`src/lib.rs:398-414`、`src/lib.rs:1146-1156`、`src/tools/sandbox.rs:48-83`）。
11. **讨论的两个讨论者共享同一条流且并发写**，独立性靠 `seq` 切窗而非时序
    （`src/events.rs:925-929`、`src/agent.rs:58-79`、`src/agent.rs:1414-1427`）。
12. **合成器那一次调用绝不跳过**（预算硬停把它降级进合成器，而不是越过它）
    （`src/agent.rs:1382-1398`、`src/agent.rs:1525-1548`、`src/agent.rs:1598-1599`）。

## 6. 已有覆盖情况

**图类型**：全仓库**没有** mermaid / graphviz / plantuml / sequence / state 图
（`grep -rn "mermaid\|flowchart\|sequenceDiagram\|stateDiagram" README.md CONTEXT.md docs/`
无结果）。已有的是 ASCII / 文本图，共 5 处：

| 位置 | 图讲什么 |
| --- | --- |
| `README.md:221-223` | 一次工具调用的固定流水线：`hook.pre → 权限门 → [询问] → dispatch → hook.post → 追加事件` |
| `README.md:147-172` | TUI 屏幕布局（左栏/主列/状态行/输入区），非流程 |
| `docs/render.md:95-103` | TUI 外壳几何（layout 的纯函数产物） |
| `docs/executor.md:10-21` | 一次 `task` 调用的流向（讨论者 → 延迟 → 执行者事件序列 → 那条唯一结果） |
| `docs/executor.md:91-97` | 执行者失败报告的形状 |
| `docs/credentials.md:22-29` | 入流流水线树：打码 → 截断 → 落盘 → 追加（标注函数名） |
| `docs/web.md:14-22` | 出网三层（工具层 / 服务层 / 后端） |

**文字已覆盖的生命周期段落**：进程启动与 CLI（`README.md:28-143`、`README.md:184-198`）；
turn 循环与固定顺序（`README.md:219-232`、`CONTEXT.md:208`）；嵌套与委派（`docs/executor.md`
全篇、`docs/discussion.md` 全篇）；事件流 / 投影 / 裁剪（`README.md:225-226`、
`docs/observability.md`）；goal 循环（`docs/goals.md` 全篇）；权限与沙箱（`docs/permissions.md`、
`docs/sandbox.md`、`README.md:200-217`）；渲染（`docs/render.md`）；出网（`docs/web.md`）；
动态工具与技能（`docs/custom-tools.md`、`docs/skills.md`）；`bash`（`docs/bash.md`）。

**结论**：现状是「一段流水线 + 若干局部树」，**没有任何一张跨阶段的端到端生命周期图**
（进程启动 → 组装 → turn → 委派 → 退出）。新图放在 `docs/` 顶层最自然，且不应重复
`docs/executor.md:10-21`、`docs/credentials.md:22-29`、`README.md:221-223` 那三张局部图
—— 引用它们即可。

## 存疑

1. **headless 渲染器的真实组装点**：`Renderer::headless` 存在且被导出（`src/render/mod.rs:171-173`），
   但 `cli.rs` 从不构造它 —— 说明它只服务库调用方 / 测试。谁在生产里用它，需看 `tests/` 与
   `src/render/headless.rs` 的调用方。
2. **hook 在交互式路径上永远是 `None`**（`src/cli.rs:396`），只有库调用方 / 测试能挂。若图要画
   hook 边，得标明它是「可注入但当前 CLI 不注入」。
3. **`docs/web.md` 与 `dsh web` 的关系**未在仓库内出现；按代码判定 `src/web/` 只做出网工具后端。
4. **`context/` 与 `config/` 目录**：实际是 `src/context.rs`（含 `context/` 子模块 repo_map、
   skills）与 `src/config.rs`（含 `config/cost.rs`），不是两个并列顶层模块；`src/lib.rs:28-42`
   的边界清单才是权威。
5. **未逐一读全**：`src/render/tui.rs`（5334 行）只按结构 grep + 关键段读；`render/editor.rs`、
   `render/panel.rs`、`render/pane.rs`、`render/todo.rs`、`tools/edit.rs`、`tools/file.rs` 未逐行
   读。若图要画编辑器的按键循环或 `edit_file` 的匹配梯降级，需补读这些文件与
   `docs/render.md:145-215`、`docs/bash.md`。
