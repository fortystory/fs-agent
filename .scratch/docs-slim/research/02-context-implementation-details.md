# research：`CONTEXT.md` 的实现细节清点

- 日期：2026-10-04
- 对应票：[`../issues/02-research-context-implementation-details.md`](../issues/02-research-context-implementation-details.md)
- 一手材料：`CONTEXT.md`（348 行 / 44,169 字节）、`docs/*.md`、`docs/adr/*.md`、`docs/agents/*.md`、`.scratch/*/spec.md`、`src/**/*.rs`
- 本文件只清点，不做「剥到哪」的决定（那个决定归票 07）。所有数字都可由 §8 的复核命令重算。

## 1. 结论摘要

1. `CONTEXT.md` 的 15 个小节共 **72 个词条**。正文里指向实现的东西，按「不重复指称」计：
   - **① 代码标识符**（类型 / 函数 / 字段 / 事件变体 / 配置键 / 工具名 / 线级名 / CLI 旗标 / 枚举值）：**235 处**
   - **② spec 节号引用**：**9 处**（其中 `spec §N` 明写 3 处，裸 `§N` 6 处）
   - **③ `src/` 路径**：**2 处**
   - **④ 实现行为描述**（「它怎么被实现」的句子）：**193 条**
2. 词条标题槽位里的英文名（`**中文名（English）**`）是 `CONTEXT.md` 自己的格式要求，**共 78 处**（72 个词条；6 个词条有两个槽位，如「投影（Projection，`project()`）」）。这 78 处**不算泄露**，但要注意其中 3 个槽位在 `src/` 里查无此名（见 §5.3）。
3. 属于**纯实现细节**的指称是 ①+②+③ 全部 **246 处**，外加 ④ 的 193 条行为描述。也就是说：**每 1 处格式必需的名字，正文里平均跟着约 3.2 处实现指称**。
4. ④ 的 193 条里，密度最高的是「渲染」38 条、「工具」30 条、「控制」25 条、「提问」25 条、「流程」16 条 —— 与本图 Notes 判定的「两个大户」不完全重合：`CONTEXT.md` 自己的胖点在**渲染与工具**。
5. 第 4 件事（别处是否已有一份）按词条计：**66 个 full**、**4 个 partial**（作答草稿、问卷文案、价目表、落点）、**2 个 none**（问卷请求、问题选项）。**「只删别处已有一份的复述」这条口径能覆盖 92% 的词条**，剩下 6 个是 `CONTEXT.md` 独有的实现细节。
6. 一个结构性发现：`.scratch/tui-input-pulse/spec.md:6` 明写「术语：`CONTEXT.md`（§渲染）**新增** 提示符色相（PromptHue）、脉冲（Pulse）、下落短横（FallingDash）」—— **spec 把 `CONTEXT.md` 当术语落点主动往里加词条**。这不是历史残留，是仍在生效的流程（见 §7.3）。

## 2. 计数口径与分类判据

### 2.1 四类指称

| 类 | 判据 | 计法 |
| --- | --- | --- |
| ① 代码标识符 | 反引号内、指向**代码实体**的 token：类型 / trait / 函数 / 方法 / 字段 / 常量 / 事件变体 / 模块路径 / 配置键 / 工具名 / 线级名 / CLI 旗标 / 枚举值 / schema 字段 | 词条内**去重**后计数（同一 token 重复出现只计一次） |
| ② spec 节号引用 | `spec §N` 与裸 `§N`（在本文件里裸 `§N` 也只可能指 spec） | 出现次数 |
| ③ `src/` 路径 | 字面含 `src/` 的路径 | 出现次数 |
| ④ 实现行为描述 | 一句话在讲「**怎么实现**」而非「这个词是什么」：具体机制、参数、阈值、字段、调用顺序、存储形态、权限落点 | 按「机制组」计：同一个机制 / 参数族 / 流程的多个分句算一条；定义性的一句（「它是什么」）不计。§4 的摘要是逐分句列的，分句数 ≥ 条数是正常的 |

**不计入四类**（但明细里标出）：键盘键位（`Esc` / `Ctrl-O` / `j` / `→`）、UI 展示字面量（`❱ ` / `▀▀▀▀` / `调用量` / `todo：3 项（1 项已完成）`）、非 `src/` 的路径（`~/.config/fs-agent/`、`.scratch/*/spec.md`、`docs/*.md`、`.env`、`.git/config`）、以及 5 处 `ADR NNNN` 引用（那也是一种实现指称，但不属本票点名的四类）。

### 2.2 格式必需（F）vs 纯实现细节

- **F（格式必需）**：`CONTEXT.md` 第 10–12 行的格式规矩（「中文名（English）」+「流程词没有对应的代码标识符：英文槽位放它在磁盘上或 tracker 里的位置」）**要求出现**的名字，即**词条标题槽位**里的英文。含无中文名的先例（`goal_note`、`token`、`frontier`）与流程节的位置槽位（`spec.md`、`map.md`、`Status:`、`issues/NN-<slug>.md`）。
- **纯实现细节**：① ② ③ 全部，加 ④ 的行为描述。**不含** F。
- 正文里**重复**出现的同一个标题名（如「转录（Transcript）」正文里的 `Block`、`project()`）不进 F —— 那里它不是在立名，是在讲代码怎么接。
- `_Avoid_` 行里点为「别混用」的名字（`subagent`、`update_plan`、`allow-always`……）不计入 ①，它们是反例清单。

### 2.3 别处 coverage 口径

「别处」= `docs/*.md` 逐面（含 `docs/adr/*`、`docs/agents/*`）**或** `.scratch/*/spec.md`。
**不含** `.scratch/*/issues/`、`.scratch/*/research/`、`docs/research/`、`src/`（本图冻结项 3 与任务口径）。判级：

- `full`：被剥掉的实现细节在别处已有一份可读的完整来源；
- `partial`：有一份但不全（例如只覆盖概念、字段名或类型名只在源码）；
- `none`：文档层查无（可能只在 `src/` 里）。

## 3. 总览表（按小节）

| 小节 | 词条数 | ① | ② | ③ | ④ | F | 别处 full / partial / none |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 名字 | 1 | 2 | 0 | 0 | 1 | 2 | 1 / 0 / 0 |
| 参与者 | 4 | 10 | 2 | 0 | 6 | 4 | 4 / 0 / 0 |
| 事件与状态 | 4 | 6 | 0 | 0 | 3 | 5 | 4 / 0 / 0 |
| 会话存储 | 2 | 6 | 0 | 0 | 3 | 2 | 2 / 0 / 0 |
| 目标 | 4 | 17 | 0 | 0 | 14 | 4 | 4 / 0 / 0 |
| 控制 | 6 | 35 | 1 | 0 | 25 | 6 | 6 / 0 / 0 |
| 提问 | 9 | 29 | 1 | 1 | 25 | 9 | 5 / 2 / 2 |
| 上下文与技能 | 3 | 4 | 0 | 0 | 5 | 3 | 3 / 0 / 0 |
| 成本与预算 | 5 | 8 | 0 | 0 | 12 | 6 | 3 / 2 / 0 |
| 讨论 | 2 | 4 | 0 | 0 | 4 | 2 | 2 / 0 / 0 |
| 工具 | 8 | 53 | 0 | 1 | 30 | 8 | 8 / 0 / 0 |
| 渲染 | 11 | 19 | 5 | 0 | 38 | 11 | 11 / 0 / 0 |
| 安全 | 2 | 7 | 0 | 0 | 7 | 2 | 2 / 0 / 0 |
| 流程 | 9 | 31 | 0 | 0 | 16 | 11 | 9 / 0 / 0 |
| 文档 | 2 | 4 | 0 | 0 | 4 | 3 | 2 / 0 / 0 |
| **合计** | **72** | **235** | **9** | **2** | **193** | **78** | **66 / 4 / 2** |

## 4. 逐词条明细（15 节 × 72 词条）

格式：`词条 | ①n ②n ③n ④n | F：槽位名 | 别处`，下附四类原文片段。行号指 `CONTEXT.md`。

### 名字

**分叉合成（Forked Synthesis，`fs`）** L18-21 ｜ ①2 ②0 ③0 ④1 ｜ F：`fs`、`Forked Synthesis` ｜ 别处 full（`docs/credentials.md`、`README.md` 的路径；`docs/goals.md`）
- ①：`fs-agent`（命令 / crate 名）、`fork/rewind`（与 明确不做 的手势混用的反例名）
- ④：命令、crate 名与落盘路径一律保持 `fs-agent`，不随框架名改写

### 参与者

**讨论者（Debater）** L24-27 ｜ ①6 ②2 ③0 ④4 ｜ F：`Debater` ｜ 别处 full（`.scratch/fs-agent-v1/spec.md` §15 L481-484；`docs/discussion.md` L108-122）
- ①：`[discussion] debaters`、`[轮 N · 名字]`、`discussion::pick_pair`、`--debaters A,B`、`Config::debaters_share_a_vendor`、`ContextInjected { source: Persona(名字) }`
- ②：`spec §5`、`spec §15`
- ④：名字是流上身份、必须唯一且不断开；多于两个时抽两个、`--debaters` 可指定；默认两家不同厂商、同厂商时由 `Config::debaters_share_a_vendor` 在**这一对**上判定并在前台明说；soul 走 `ContextInjected { source: Persona(名字) }`、署名讨论者自己、投影只发给自己、是「别人的注入也变 user」的唯一例外、不进私有身份（因身份必须能从流重算）

**执行者（Executor）** L28-31 ｜ ①1 ②0 ③0 ④1 ｜ F：`Executor` ｜ 别处 full（`docs/executor.md`）
- ①：`parent_id`
- ④：由讨论者派出、带自己的 `parent_id` 与独立预算的子 agent

**发言归属（Speaker）** L32-35 ｜ ①3 ②0 ③0 ④1 ｜ F：`Speaker` ｜ 别处 full（`.scratch/fs-agent-v1/spec.md` §5 L297）
- ①：`speaker_id`、`assistant`、`user`
- ④：投影靠 `speaker_id` 判定一条发言相对当前 agent 是 `assistant` 还是 `user`

**合成器（Synthesizer）** L36-39 ｜ ①0 ②0 ③0 ④0 ｜ F：`Synthesizer` ｜ 别处 full（`docs/discussion.md`；`[routing].synthesizer_model`）
- ④：无（「不是 agent、无工具无回合」是定义边界，不计）

### 事件与状态

**事件（Event）** L42-45 ｜ ①1 ②0 ③0 ④0 ｜ F：`Event` ｜ 别处 full（`docs/lifecycle.md`）
- ①：`messages`

**事件流（EventLog）** L46-49 ｜ ①1 ②0 ③0 ④1 ｜ F：`EventLog` ｜ 别处 full（`docs/lifecycle.md` L393、L756）
- ①：`messages`
- ④：每个讨论者 / 执行者的 `messages` 都是它的一次投影

**投影（Projection，`project()`）** L50-53 ｜ ①2 ②0 ③0 ④1 ｜ F：`Projection`、`project()` ｜ 别处 full（`.scratch/fs-agent-v1/spec.md` §5；`docs/lifecycle.md` L432；ADR 0001 / 0005）
- ①：`(EventLog, SpeakerId, provider 能力) → messages`（纯函数签名）、`messages`
- ④：纯函数签名本身；住在 provider 适配器侧

**会话（Session）** L54-57 ｜ ①2 ②0 ③0 ④1 ｜ F：`Session` ｜ 别处 full（`docs/lifecycle.md` 的 `SessionConfig`；`docs/executor.md`）
- ①：`EventLog`、`parent_id`
- ④：持有 `EventLog` + 名册 + 预算 + config 的那个值、是唯一持有可变状态的结构；执行者用带 `parent_id` 的嵌套会话

### 会话存储

**会话目录（StoredSession）** L60-63 ｜ ①4 ②0 ③0 ④2 ｜ F：`StoredSession` ｜ 别处 full（`docs/credentials.md` L103 的 `0700`；`README.md` L190 的 `prune`；`docs/observability.md`）
- ①：`outputs/`、`0700`、`--continue`、`prune`
- ④：落盘形态是 JSONL 事件流 + `outputs/`；与 cwd 绑定、默认 `0700`；`--continue` 与 `prune` 都以它为单位

**会话桶（bucket）** L64-67 ｜ ①2 ②0 ③0 ④1 ｜ F：`bucket` ｜ 别处 full（`.scratch/continue-by-id/spec.md` §2 L27、L54-67）
- ①：`--continue`、`SessionStarted`
- ④：不带 id 的 `--continue` 只扫本桶；cwd 的 slug 只用于分桶、权威 cwd 在 `SessionStarted` 里；按 id 续会话时先扫本桶再全 store

### 目标

**目标（Goal）** L70-77 ｜ ①5 ②0 ③0 ④3 ｜ F：`Goal` ｜ 别处 full（`docs/goals.md`；ADR 0009 L5）
- ①：`/loop <名字>`、`GoalSelected`、`--continue`、`sessions replay`、`task`（`_Avoid_` 里的 `_Avoid_` 自指不计）
- ④：`/loop <名字>` 认领它并落一条 `GoalSelected`；当前目标永远是流上最后一条、切换就是再记一条；`--continue` 与 `sessions replay` 都从流派生；不进会话状态（没有状态文件、没有「当前目标」字段）

**目标清单（Manifest）** L78-86 ｜ ①6 ②0 ③0 ④4 ｜ F：`Manifest` ｜ 别处 full（`docs/goals.md` L87-94；`.scratch/goal-loop/spec.md`）
- ①：`- <id> <内容>`（行格式）、`completed`、`/goal-new <名字> <来源>…`、`<来源目录名>/<票号>`、`/goal-list`、`/goal-`（命令前缀）
- ④：定义住在 `~/.local/share/fs-agent/goals/<名字>.md`、一行标题加一列 `- <id> <内容>`；只有条目与 id、不带状态；开工前封闭；`/goal-new` 从一批 feature 目录 / 票文件生成、多来源时 id 按顺序重排并带 `<来源目录名>/<票号>` 凭证；这一族命令用连字符、`/goal-` 一个前缀就能在补全里列全族；id 是两位十进制

**翻页（rollover）** L87-93 ｜ ①2 ②0 ③0 ④3 ｜ F：`rollover` ｜ 别处 full（`docs/goals.md` L107-115；ADR 0009）
- ①：`compact_at`、`/clear`
- ④：结束当前会话、开一个新的、进程不重启（渲染器与终端留着）；上下文过 `compact_at` 时循环自己压缩 + 翻页；`/clear` 由键盘触发且不带压缩与摘要注入；压缩与翻页总是成对发生

**`goal_note`** L94-99 ｜ ①4 ②0 ③0 ④4 ｜ F：`goal_note`（无中文名，见 `token` 先例） ｜ 别处 full（`docs/goals.md` L163；`.scratch/goal-loop/spec.md`）
- ①：`tool_call`、`effect()`、`ReadOnly`、`todo`
- ④：与待办工具完全同构、真相是那条 `tool_call` 的参数；`effect()` 是 `ReadOnly`、不新增事件；消费者是目标收尾汇总的「新工作」一项；主会话、讨论者、执行者与 headless 都挂、但不进侧栏

### 控制

**取消（Cancel）** L102-105 ｜ ①5 ②0 ③0 ④4 ｜ F：`Cancel` ｜ 别处 full（`.scratch/fs-agent-v1/spec.md`；ADR 0003；`docs/lifecycle.md`）
- ①：`tool_call`、`Aborted`、`/undo`、`CancelSignal`、`CancelObserver`
- ④：`tool_call` 已开始的那些各补一条合成结果、当前回合以 `Aborted` 收尾；手势本身不进事件流（与 `/undo` 的写回、模式循环同规矩）；`CancelSignal` 发起、`CancelObserver` 在回合与执行者手里逐层克隆；回合手里没有能发起取消的东西（所以「执行者被取消」不会把派发者的回合记成失败）

**挂起（Suspend）** L106-109 ｜ ①2 ②0 ③0 ④4 ｜ F：`Suspend` ｜ 别处 full（`.scratch/suspend-gesture/spec.md`；`docs/render.md` 提 SIGTSTP）
- ①：`fg`、`Stopped`
- ④：`Ctrl-Z` 把整个进程用 SIGTSTP 停住、shell 拿回提示符；单下生效不做举手；只有 TUI 有这个手势（raw 模式把 SIGTSTP 吃成按键，得应用自己发信号），plain 由终端驱动直接处理、headless 没有键盘；手势不进事件流、不打回执；停着的那段时间里子进程与各种 deadline 照旧走

**举手（Gesture）** L110-113 ｜ ①3 ②1（裸 `§5`） ③0 ④4 ｜ F：`Gesture` ｜ 别处 full（`.scratch/exit-gesture/spec.md`、`.scratch/questionnaire-keys/spec.md` §5）
- ①：`0`、`130`（退出码）、`Gesture`（正文再出现的类型名）
- ④：槽位只有一个、里面举着的是哪一把由 `Gesture` 标出、两把互斥；第一把是退出（空闲 `Ctrl-C`/`Ctrl-D` 共用，运行中第一下 `Ctrl-C` 同时是取消）、半秒内第二下退出；退出码跟着怎么退的走（人主动 0、运行中被打断 130）；问卷 `Esc` 顺带把键盘送回选项区、第二下 drop 掉回复通道、模型继续跑；举手期间提示行的出口段换成「再按一次 … 退出」，问卷立着时那句话落在页脚；纯渲染器状态、不进事件流

**权限模式（Mode）** L114-117 ｜ ①11 ②0 ③0 ④5 ｜ F：`Mode` ｜ 别处 full（`docs/permissions.md` L11-18；`.scratch/todo-and-modes/spec.md`；ADR 0003 / 0007）
- ①：`readonly`、`ask`、`workspace`、`auto`、`--continue`、`PermissionDecided.reason`、`config.toml`、`[permissions] mode`、`--mode`、`readonly → ask → workspace → auto`（严格度序列）、`todo`
- ④：三个入口（`config.toml` 的 `[permissions] mode`、`--mode` 旗标、会话里 `Shift+Tab` 循环四档）；循环只改这个值、什么都不注入（模型第一次被拒时才知道档位变了）；不进事件流，所以 `--continue` 回到配置里的那一档、审计看 `PermissionDecided.reason`；执行者沿用派发者的档位；没有可用沙箱时 `workspace` 档在组装期被拒

**区外读（Outside Read）** L118-121 ｜ ①6 ②0 ③0 ④3 ｜ F：`Outside Read` ｜ 别处 full（`docs/permissions.md` §「区外读」L27-32）
- ①：`[permissions] outside_read = "deny" | "ask" | "allow"`、`"deny"`、`ask`、`"allow"`、`[sandbox] mode = "off"`、`workspace`
- ④：读目标落在会话 cwd 之外时给什么裁决；与档位正交、四档都认它、是策略级的地板；缺省 `"deny"` 不动摇、写下来才算放弃（与 `[sandbox] mode = "off"` 同一立场）；区外写没有对称的旋钮

**升级（Escalation）** L122-125 ｜ ①8 ②0 ③0 ④5 ｜ F：`Escalation` ｜ 别处 full（`docs/permissions.md` L49-56；`docs/sandbox.md` L43；`.scratch/workspace-mode/spec.md`；ADR 0007）
- ①：`EROFS`、`bash`、`escalation = { justification, writable_paths }`、`readonly`、`Ask`、`.env`、`.git/config`、`.git/hooks`（`allow-always` 在 `_Avoid_`，不计）
- ④：命令被沙箱拒（内核 `EROFS`）之后带理由与路径、把同一条命令原样重试一次；唯一持有者是 `bash`；形状两者都要非空、必须成对；门里除 `readonly` 档外一律给 `Ask`（一次）；批准后这次调用多几条可写根、粒度就是声明的那个路径本身（不做父目录提升）；只这一次调用、只给一次重试、不进规则 / 配置 / 会话状态、没有 `allow-always`；与询问共用同一条审批通道与同一对事件；写死的边界（遮罩目录、`.env` 家族、`.git/config`、`.git/hooks`）不给任何通道

### 提问

**询问（Ask）** L128-131 ｜ ①1 ②0 ③0 ④2 ｜ F：`Ask` ｜ 别处 full（`docs/permissions.md`；`docs/lifecycle.md` L443-445、L769）
- ①：`Ask → Deny`
- ④：走中段覆盖层、一行按钮；发起者是 harness（或渲染器自己的确认框）不是模型；执行者的询问沿委派链回到同一个键盘；无交互前端时 `Ask → Deny`（由循环在门外做）

**用户提问（User Question）** L132-135 ｜ ①5 ②0 ③0 ④3 ｜ F：`User Question` ｜ 别处 full（`.scratch/questionnaire-keys/spec.md`；`docs/lifecycle.md` L444；`docs/render.md`）
- ①：`ask_user_question`、`tool_call`、`{"answers":[{"id","selected","custom"?}]}`、`Asker`、`UserQuestion`
- ④：模型调 `ask_user_question` 把一批问题交给用户；答案是那条 `tool_call` 的唯一结果（JSON 文本）；`Asker` 那条接缝不扩展；只有主会话能问（执行者的工具表里没有它）；TUI 接管底部输入区、plain 逐行问答、headless 根本不挂

**问卷（Questionnaire）** L136-139 ｜ ①2 ②1 ③0 ④2 ｜ F：`Questionnaire` ｜ 别处 full（`.scratch/questionnaire-keys/spec.md`）
- ①：`ask_user_question`、`tool_call`
- ②：`spec §7`（**没写是哪个 spec**；两个候选 spec 都有 §7，见 §5.2）
- ④：它是前端为整批问题持有的键盘状态（TUI 的 `Questionnaire`）；不是事件、不落流，唯一持久痕迹是那条 `tool_call` 的 args 和它的唯一结果

**问卷请求（QuestionnaireRequest）** L140-143 ｜ ①4 ②0 ③1 ④3 ｜ F：`QuestionnaireRequest` ｜ 别处 **none**（`docs/` 与 `.scratch/*/spec.md` 均无此名；只在 `src/render/input.rs` / `src/render/mod.rs`）
- ①：`reply: Result<UserAnswers, String>`、`AskRequest`、`Asker`、`Ask`
- ③：`src/render/input.rs`
- ④：它是端口把一批问题交给前端的那个值、住在 `src/render/input.rs`；带一条一次性回复通道 `reply: Result<UserAnswers, String>`；drop 掉 sender 等于「没有答案」（取消、双击 `Esc`、输入结束），所以工具不会挂住；与 `AskRequest` 并列

**作答草稿（QuestionDraft）** L144-147 ｜ ①6 ②0 ③0 ④4 ｜ F：`QuestionDraft` ｜ 别处 partial（`.scratch/questionnaire-keys/spec.md` L51、L135-136、L224 讲答案形状；`highlight` 字段级只在源码）
- ①：`selected`、`custom`、`highlight`、`skipped`、`TuiState`、`UserAnswer`
- ④：字段构成是已选 `selected` / 自定义文本 `custom` / 高亮下标 `highlight` / 是否跳过 `skipped`；活在键盘那一侧（TUI 的 `TuiState`）、每题一份、可来回翻页；`skipped` 无论之前打过什么字都编码成 `selected: []` 且无 `custom`；`selected` 与 `custom` 可以同时成立、单选与多选一个形状

**选项区（Options Zone）** L148-151 ｜ ①1 ②0 ③0 ④4 ｜ F：`Options Zone` ｜ 别处 full（`.scratch/questionnaire-keys/spec.md` §1 L75-90；ADR 0010）
- ①：`Zone`
- ④：`j`/`k`/`Ctrl-N`/`Ctrl-P`/`↑`/`↓` 在这里移动高亮、越过两端就是进输入区；空格与回车在这里确认（全部题处理完时回车是提交）；可打印字符与 `Backspace` 在这里一律被吞掉；`Zone` 是问卷级状态（每题共用、翻页时回到选项区）不是每题一份

**输入区（Input Zone）** L152-155 ｜ ①0 ②0 ③0 ④4 ｜ F：`Input Zone` ｜ 别处 full（`.scratch/questionnaire-keys/spec.md`；ADR 0010）
- ④：可打印字符、空格、`j`/`k` 在这里进文本；`↑`/`↓` 从这里回到选项区并移动高亮；`Esc` 回选项区并且同时举起「退出这次询问」的手；不是编辑器：没有行内光标（`←`/`→` 仍是翻页）、没有历史、`Backspace` 只从末尾删；没有选项的题只有这一区

**问题选项（questions::Choice）** L156-159 ｜ ①5 ②0 ③0 ④1 ｜ F：`questions::Choice` ｜ 别处 **none**（`questions::Choice` / `wording::Choice` 在 `docs/` 与 `.scratch/*/spec.md` 均无命中；实体在 `src/tools/ask_user.rs`、`src/render/tui.rs`）
- ①：`questions[].options[]`、`label`、`(Recommended)`、`description`、`wording::Choice`
- ④：`(Recommended)` 只做显示、原串就是答案值；两个 `Choice` 同名不同物（模型给的选项数据 vs 前端按钮的键位定义）与保留两个名字的理由

**问卷文案（`questionnaire_*`）** L160-163 ｜ ①5 ②0 ③0 ④2 ｜ F：`questionnaire_*` ｜ 别处 partial（`.scratch/questionnaire-keys/spec.md` L199 点明文案进 `src/render/wording.rs` 的 `questionnaire_*` 一族；`docs/render.md` 无此名）
- ①：`render::wording`、`questionnaire_hint`、`questionnaire_option`、`questionnaire_plain_*`、`questionnaire_*`
- ④：它是 `render::wording` 里问卷一族人类可见文案的命名前缀；TUI 与 plain 共享同一个生成器，模型面文本不经过这里

### 上下文与技能

**技能（Skill）** L166-169 ｜ ①1 ②0 ③0 ④1 ｜ F：`Skill` ｜ 别处 full（`docs/skills.md`）
- ①：`skill`
- ④：描述每轮都在场（便宜）、全文只在模型调用 `skill` 时取

**技能清单（SkillsCatalog）** L170-173 ｜ ①2 ②0 ③0 ④2 ｜ F：`SkillsCatalog` ｜ 别处 full（`docs/skills.md` L25-26；`.scratch/fs-agent-v1/spec.md`）
- ①：`AGENTS.md`、`user`
- ④：它与 `AGENTS.md` 同处钉住的首条 `user` 消息、逐轮不变；描述由 `name: description` 行组成（`docs/skills.md` 同句）

**仓库地图（RepoMap）** L174-177 ｜ ①1 ②0 ③0 ④2 ｜ F：`RepoMap` ｜ 别处 full（`docs/repo-map.md` L3-35）
- ①：`repo_map(focus?)`
- ④：一次调用返回「哪些文件定义了哪些函数 / 类型 / trait / 模块 / 宏」；按会话相关度排序、固定预算；产物是工具结果、不注入

### 成本与预算

**预算（Budget）** L180-183 ｜ ①3 ②0 ③0 ④4 ｜ F：`Budget` ｜ 别处 full（`docs/goals.md` L126、L188；ADR 0009 L5；`.scratch/tui-layout/spec.md`）
- ①：`UsageRecorded`、`SessionConfig.carried_tokens`、`context::usable_input`
- ④：闸门看 `UsageRecorded` 的求和（讨论者 / 执行者 / 合成器共用一个额度、各端 config 必须一致，不然组装期报错）；撞顶时降级收尾而不是中断；有当前目标时求和范围是这个目标横跨的所有会话、`SessionConfig.carried_tokens` 承载「别处已经花掉的那一份」、翻页不重置额度；与窗口（`context::usable_input`，按 agent 各自模型算）是两回事

**token** L184-187 ｜ ①2 ②0 ③0 ④2 ｜ F：`token`（无中文名） ｜ 别处 full（`docs/observability.md` L32、L71-73）
- ①：`token`、`token_pair`
- ④：输入 / 输出、以及缓存命中与未命中各自计数；UI 与统计行一律写 `token`（左栏的标签、`token_pair`）

**价目表（PriceTable / Pricing）** L188-191 ｜ ①3 ②0 ③0 ④3 ｜ F：`PriceTable`、`Pricing` ｜ 别处 partial（`docs/observability.md` L32、L71-73 讲费用按 model id 与「无价格」；`PriceTable` 名与三档字段只在 `.scratch/fs-agent-v1/spec.md`、`.scratch/tui-layout/spec.md`）
- ①：`cached`、`miss`、`output`
- ④：按 model id 配的单价、单位是每百万 token 的 USD；`cached` / `miss` / `output` 三档、命中与未命中分开计价；费用只作显示（闸门只数 token）、未登记的 model 显示为「无价格」不是 0

**落点（LandingPoint）** L192-195 ｜ ①0 ②0 ③0 ④1 ｜ F：`LandingPoint` ｜ 别处 partial（`README.md` L87 的 `[routing]`；`docs/discussion.md` L123 一句；`LandingPoint` 名只在 `.scratch/fs-agent-v1/spec.md`）
- ④：全系统只有两个落点（合成器与执行者、可配置覆盖），讨论者绝不是落点

**日账本（DayLedger）** L196-199 ｜ ①0 ②0 ③0 ④2 ｜ F：`DayLedger` ｜ 别处 full（`docs/goals.md` L127 指向 `docs/observability.md`；ADR 0009 L34）
- ④：按 UTC 日聚合 token 用量；从会话文件派生（由事件自己的时间戳决定归属哪一天）、不新增状态文件

### 讨论

**轮次（Round）** L202-206 ｜ ①4 ②0 ③0 ④3 ｜ F：`Round` ｜ 别处 full（`docs/discussion.md`；`.scratch/fs-agent-v1/spec.md` §15 L484）
- ①：`discussion::last_round`、`/discuss`、`round_attendance`、`sessions show --round N`
- ④：讨论协议的一步是「独立首轮 → 揭示 → 定向第二轮 → 合成」；编号在一次会话内唯一、接在流上已有的轮次之后（`discussion::last_round`）、第二次 `/discuss` 从「第 4 轮」接着数；`round_attendance` 与 `sessions show --round N` 靠这个唯一性分辨是哪一场讨论

**回合（Turn）** L207-210 ｜ ①0 ②0 ③0 ④1 ｜ F：`Turn` ｜ 别处 full（`docs/lifecycle.md`）
- ④：一次完整回合的顺序是「投影 → 调 provider → 权限门 → 执行工具 → 追加事件」

### 工具

**动态工具（CustomTool）** L213-216 ｜ ①7 ②0 ③0 ④4 ｜ F：`CustomTool` ｜ 别处 full（`docs/custom-tools.md` L24-25、L37）
- ①：`config.toml`、`[tools.<命名空间>.<工具>]`、`custom__<命名空间>__<工具>`、`effect()`、`Exclusive`、`command`、`__`
- ④：声明住在 `config.toml` 的 `[tools.<命名空间>.<工具>]`、线级名是 `custom__<命名空间>__<工具>`；声明语法的字段就是 provider 收到的声明（JSON Schema 原样）、没有副作用类别字段所以 `effect()` 恒为 `Exclusive`；`command` 是 argv 模板、参数按整个 argv 元素替换、不经 shell；内建名永不含 `__`，所以「名字含 `__`」⟺「来自配置」是词法可判定的

**待办列表（Todo）** L217-220 ｜ ①11 ②0 ③0 ④6 ｜ F：`Todo` ｜ 别处 full（`docs/goals.md` L87-94；`docs/render.md` L40-43；`.scratch/todo-and-modes/spec.md` §2 / §4；ADR 0009 L20-21、L44）
- ①：`todo`、`{id?, content, status}`、`status`、`id`、`pending`、`in_progress`、`completed`、`tool_call`、`--continue`、`sessions replay`、`seq`
- ④：每项是 `{id?, content, status}`、`status` 取 `pending` / `in_progress` / `completed`；可选的 `id` 是两位十进制数字、引用目标清单里的那一条，存在性由循环在派生进度时判、越界的 id 被忽略但不静默；列表就是那条 `tool_call` 的 args（没有第二处存储，结果只是一句回执 `todo：3 项（1 项已完成）`）；`--continue`、`sessions replay` 与侧栏都从 args 重算；一次提交整份（replace-all，缺省或空数组就是清空）；同一条助手消息里两次并发调用按 `seq` 定序、后落地的那条是真相；它是某个 agent 自己的列表，上侧栏的是非执行者那一份

**待办工具（`todo`）** L221-224 ｜ ①9 ②0 ③1 ④5 ｜ F：`todo` ｜ 别处 full（`.scratch/todo-and-modes/spec.md` L50-56、L80-81；`docs/render.md` L40-43；`docs/executor.md` L55-61）
- ①：`todo`、`effect()`、`ReadOnly`、`content`、`status`、`ask_user_question`、`task`、`agent_identity()`、`tool_choice`
- ③：`src/tools/todo.rs`
- ④：代码住在 `src/tools/todo.rs`；`effect()` 是 `ReadOnly`、不碰工作区、权限门从不为它发问、两次调用可以并发；`content` 为空或 `status` 越界是模型可读的工具错误；挂载面是主会话 / 讨论者 / 执行者，headless 也挂、也不像 `task` 那样从执行者的表里被拿掉；规则段（`agent_identity()`）请模型开工前先立待办，但是引导不是强制（没有首轮 `tool_choice`、没有门层强制）

**搜索提供方（SearchProvider）** L225-228 ｜ ①6 ②0 ③0 ④3 ｜ F：`SearchProvider` ｜ 别处 full（`docs/web.md` L216-218；`.scratch/web-search-tool/spec.md`）
- ①：`url`、`title`、`snippet`、`published_at`、`deepseek`、`web_search`
- ④：一次一个查询、返回按排名排好的结构化来源（`url` / `title` / `snippet` / `published_at`）；结构化来自提供方的块、不来自某个 agent 的总结；今天只有 `deepseek`（Anthropic 兼容端点 + 原生 `web_search` 服务器工具，一次调用是一个完整的模型轮次）；去重 / 并发 / 轮询合并与错误码在服务层，schema / 上限与呈现在工具层

**抓取提供方（FetchProvider）** L229-232 ｜ ①2 ②0 ③0 ④2 ｜ F：`FetchProvider` ｜ 别处 full（`docs/web.md` L117、L131、L141）
- ①：`http`、`reqwest`
- ④：把一个 URL 变成有界的正文（最终 URL、状态码、HTML / 纯文本、截断标志）；今天只有 `http`（`reqwest` 加自己做的 SSRF 防护 —— 沙箱只管文件、不管网络）；非 2xx 是结果不是错误（404 是被抓资源的状态）

**元工具（Meta-tool）** L233-236 ｜ ①9 ②0 ③0 ④4 ｜ F：`Meta-tool` ｜ 别处 full（`docs/mcp.md`；`.scratch/mcp-support/spec.md`）
- ①：`mcp_list`、`mcp_call`、`mcp_resources`、`mcp_read`、`server`、`tool`、`with_mcp`、`with_web`、`[mcp] enabled`
- ④：四个固定名字；server 的工具永远不进工具表、模型按 `server` + `tool` 两个参数穿过这四个口子；它们由 `with_mcp` 在组装期一步加进表（照 `with_web` 的形状）；`[mcp] enabled`（缺省关）是共同的开关、四个同开同关

**原语（Primitive）** L237-240 ｜ ①2 ②0 ③0 ④2 ｜ F：`Primitive` ｜ 别处 full（`docs/mcp.md` L185、L219）
- ①：`ClientCapabilities`、`ServerCapabilities`
- ④：fs-agent 这一版四类都落了 —— tool 与 resource 走四个元工具、prompt 走 `/` 菜单（发起者是人）、elicitation 把 server 要的输入交给既有的问询端口；协议只谈 2026-07-28 的无状态形态，sampling / roots / logging 已 deprecated、不在范围

**MCP server（server）** L241-244 ｜ ①7 ②0 ③0 ④4 ｜ F：`server` ｜ 别处 full（`docs/mcp.md` L49-51、L112-113、L140）
- ①：`[mcp.servers.<名字>]`、`.mcp.json`、`trust_results`、`trust_effects`、`sandbox`、`false`、`McpService`
- ④：配置住在 `[mcp.servers.<名字>]` 或仓库根的 `.mcp.json`、名字就是四个元工具 `server` 参数认的那个键；默认不可信 —— 内容带那句外部内容标记、工具按最严的副作用处理、进程过沙箱且只拿到白名单里的环境变量；`trust_results` 与 `trust_effects` 各自默认关、`sandbox` 默认过（要写 `false` 才不过），三个位各自独立、互不牵连

### 渲染

**渲染器（Renderer）** L247-250 ｜ ①2 ②0 ③0 ④2 ｜ F：`Renderer` ｜ 别处 full（`docs/render.md` L14-15、L28）
- ①：`Transcript`、`Block`
- ④：启动时选定的唯一渲染实现（三选一、互斥）消费组装期创建的同一条广播通道；三模式共享同一条事件序列，plain 与 TUI 共用 `Transcript` 的 `Block` 呈现层，只有绘制方式不同

**转录（Transcript）** L251-254 ｜ ①3 ②0 ③0 ④3 ｜ F：`Transcript` ｜ 别处 full（`docs/render.md` L28-43）
- ①：`Block`、`project()`、`messages`
- ④：把事件流（含旁路的增量文本）转成展示单元 `Block`；工具调用与它的结果归成一个块（由结果绘制）、后置 hook 的反馈另成一个块、增量文本原样透传；不是 `project()` —— 那个产出的是给模型的 `messages`

**终端端口（Console）** L255-258 ｜ ①2 ②0 ③0 ④2 ｜ F：`Console` ｜ 别处 full（`docs/render.md`；`docs/lifecycle.md` L304、L408、L443-445、L771、L790）
- ①：`ConsoleEvents`、`ConsoleAsker`
- ④：循环按需请求一行或一个问题、前端（TUI 或 plain 的行读取）回答；取消 / 模式手势经 `ConsoleEvents` 主动上行；权限门走同一条通道（`ConsoleAsker`），因为两者都来自同一个键盘

**左栏（Sidebar）** L259-262 ｜ ①2 ②1 ③0 ④6 ｜ F：`Sidebar` ｜ 别处 full（`.scratch/sidebar-toggle/spec.md` §2；`docs/render.md` L97-139；`docs/tui-manual-checklist.md`）
- ①：`fs`（标记）、`fs-agent <版本>`（窄档退成的一行）
- ②：裸 `§2`
- ④：TUI 左侧全高一栏、顶上先留一行空行；身份 + 页签条 + 选中页的内容；去留 = 用户意愿 × 宽度档（`Ctrl-O` 收起与叫回、意愿只在这一次进程里、不落配置不跨会话）；宽度档 ≥120 列 40 列宽 / 80–119 列 28 列宽 / 更窄整栏隐藏、窄档下「叫回」静默无效；内容由自己那一栏的高度决定（先丢标记、再丢身份行、最后从尾部丢字段）；页签是点出来的、不给键位（`Tab` 归 `/` 菜单、`Shift+Tab` 归模式循环）；`todo` 页插在 `调用量` 右边且只在会话出现过非空列表之后才出现。`调用量` / `轨迹` / `文件` / `todo` 是 UI 字面量（不计 ①）

**输入区（InputArea）** L263-267 ｜ ①0 ②1 ③0 ④4 ｜ F：`InputArea` ｜ 别处 full（`.scratch/tui-chrome/spec.md` §1；`docs/render.md` L139-140）
- ②：裸 `§1`（正文最后一句点明 `.scratch/tui-chrome/spec.md` §1）
- ④：最少 3 行、最多 10 行；草稿折行数不足 3 时照样占 3 行、草稿从顶部写起；没有自己的边框（上面那条分隔线由外壳画，外壳自己也没有边框）；地板之间有一条优先级（转录最后一行比输入区最小高度优先），40×10 下输入区 3 行、转录 3 行

**提示符色相（PromptHue）** L268-271 ｜ ①1 ②1 ③0 ④5 ｜ F：`PromptHue` ｜ 别处 full（spec 层：`.scratch/tui-input-pulse/spec.md` §2b L89-94 与 L6 明写这个词条；`docs/render.md` L155 只有一句；**`src/` 里查无 `PromptHue` 这个名字**，真实形态是 `PROMPT_HUE_PER_SECOND` / `hue`）
- ①：`Color::Rgb`
- ②：裸 `§2b` 的分叉（正文写 `§2b`，脚本按 `§2` 计 1）
- ④：界面里唯一在动的东西、只在一次运行进行中时动，轮到用户打字时停在静止色（第 0 帧）；色相每秒走 0.3 圈（约 3.3 秒一轮）、饱和度以 0.55 为中心幅度 0.2 每秒 3.0 弧度地呼吸、明度固定 0.85；24 位真彩（`Color::Rgb`）；纯渲染器状态、不进事件流；参数是从用户自用的一条脚本搬来的

**脉冲（Pulse）** L272-275 ｜ ①3 ②2 ③0 ④5 ｜ F：`Pulse` ｜ 别处 full（`.scratch/tui-input-pulse/spec.md`；`.scratch/exit-gesture/spec.md` §6；`.scratch/questionnaire-keys/spec.md` §5）
- ①：`TuiState.pulse`、`PULSE_FRAME`、`GESTURE_WINDOW`
- ②：裸 `§6`、裸 `§5`（指两个 spec）
- ④：`TuiState.pulse` 由循环在一次运行进行中时才 arm 的定时器推进（`PULSE_FRAME` 60 ms 一帧）、运行结束归零；消费者是提示符色相；空闲时那台时钟不存在（没有唤醒、没有重画）；唯一例外是有界的举手 deadline（`GESTURE_WINDOW` 500 ms，只在举手期间武装、到点把举手作废、两把手势共用）

**下落短横（FallingDash）** L276-279 ｜ ①3 ②0 ③0 ④3 ｜ F：`FallingDash` ｜ 别处 full（`.scratch/tui-input-pulse/spec.md` §2、L6；`docs/render.md` L111-116；**`src/` 里查无 `FallingDash`**，真实形态是 `DASH_FALL` / `identity_falling` / `mark_lines`）
- ①：`mark_lines(Some(frame))`、`wording::DASH_FALL`、`identity_falling`
- ④：一条 `▀▀▀▀` 从上往下落、落到底再从上面出现；宽档落过 mark 自己的五行（`mark_lines(Some(frame))`）、窄档落过一行里能表达的三个高度；已退出屏幕（真机上看下来不好看），代码与它自己的单元测试都留着、别顺手接回渲染路径

**状态行（StatusRow）** L280-283 ｜ ①0 ②0 ③0 ④3 ｜ F：`StatusRow` ｜ 别处 full（`docs/render.md` L99、L124、L138；`src/render/wording.rs` 的 `status_row`；**`src/` 里查无 `StatusRow`**）
- ④：内容是 `模型 X │ 模式 Y │ 上下文 Z%`；按宽度先丢模型、再丢模式、最后只剩上下文占比（仍带标签）；这一行永远在场、三段都不可点

**回合条（TurnRail）** L284-287 ｜ ①3 ②0 ③0 ④3 ｜ F：`TurnRail` ｜ 别处 full（`.scratch/tui-sidebar/spec.md`；`docs/render.md` L135）
- ①：`TurnEnded`、`RoundEnded`、`SessionFacts.speaker_order`
- ④：转录右缘最右 1 列的竖条、一格 = 会话的一个单位（交互会话数 `TurnEnded`、讨论会话数 `RoundEnded`，判定用组装期注入的 `SessionFacts.speaker_order`）；格子贴底排、焦点那格粗 + 亮、被裁掉时最上一格 `⋮`、点一格跳到那一格单位的段首。`⋮` 是 UI 字面量（不计 ①）

**焦点回合（FocusedTurn）** L288-291 ｜ ①0 ②0 ③0 ④2 ｜ F：`FocusedTurn` ｜ 别处 full（`.scratch/tui-sidebar/spec.md`）
- ④：它是派生量 = 视口顶端那一行所属的单位、不存状态（存下来的话用户一滚动它就和视口漂移了）；吸底时它恰好等于最新单位

### 安全

**沙箱（Sandbox）** L294-297 ｜ ①3 ②0 ③0 ④4 ｜ F：`Sandbox` ｜ 别处 full（`docs/sandbox.md` L3-43；ADR 0006；`.scratch/sandbox/spec.md`）
- ①：`bash`、`EROFS`、`[sandbox] mode = "off"`
- ④：`bash` 与动态工具跑在其下的文件写边界、由 bubblewrap 提供；整台机器只读挂进来、只有会话 cwd `/tmp` 与配置里的可写根能写、区外的写由内核以 `EROFS` 打回；`~/.config/fs-agent`（key 在那儿）与 `~/.ssh` 被遮成空且只读；它只管文件、不管网络，也不决定「跑不跑」；bubblewrap 不可用时拒绝运行，另有 `[sandbox] mode = "off"` 显式关掉这层

**打码（Redactor）** L298-301 ｜ ①4 ②0 ③0 ④3 ｜ F：`Redactor` ｜ 别处 full（`docs/credentials.md` L40、L45-47、L97、L107、L124）
- ①：`[redacted]`、`outputs/<tool_call_id>.txt`、`<tool_call_id>.before`、`/undo`
- ④：值级、best-effort 替换 —— 把配置里解析出的密钥值换成 `[redacted]`；于是流上的文本 == 模型看到的文本，而工具执行仍拿真值；范围含消息正文与工具参数；`outputs/<tool_call_id>.txt` 打码、`<tool_call_id>.before` 不打码（它是 `/undo` 的字节级还原源）

### 流程

**feature 目录（`.scratch/<feature-slug>/`）** L304-307 ｜ ①0 ②0 ③0 ④1 ｜ F：`.scratch/<feature-slug>/`（位置槽位） ｜ 别处 full（`docs/agents/issue-tracker.md`；`.scratch/README.md`）
- ④：一个 feature 一个目录、它自己的 spec / 决策图与票全在目录里、feature 之间不共享文件；总清单是 `.scratch/README.md`

**seed（`seed.md`）** L308-311 ｜ ①0 ②0 ③0 ④1 ｜ F：`seed.md` ｜ 别处 full（`docs/agents/issue-tracker.md`）
- ④：目录里只有它就说明这件事还没被访谈、也没被画成图，所以里面没有票

**spec（`spec.md`）** L312-315 ｜ ①0 ②0 ③0 ④1 ｜ F：`spec.md` ｜ 别处 full（`docs/agents/domain.md`、`docs/agents/issue-tracker.md`）
- ④：它是实现票的来源 —— 票从它拆出来，所以票不重述理由、只指回它

**决策图（map，`map.md`）** L316-319 ｜ ①4 ②0 ③0 ④2 ｜ F：`map`、`map.md` ｜ 别处 full（`docs/agents/issue-tracker.md`；`.scratch/README.md`）
- ①：`笔记`、`已定的决定`、`尚未明确`、`## 任务清单`
- ④：正文由 `笔记` / `已定的决定` / `尚未明确` 加一份 `## 任务清单` 组成；图走完会被折成 spec 而图留着当决策记录，所以 `map.md` 与 `spec.md` 可以同时存在

**交棒（handoff）** L320-323 ｜ ①1 ②0 ③0 ④1 ｜ F：`handoff` ｜ 别处 full（`docs/agents/issue-tracker.md`）
- ①：`map.md`（正文再出现的文件名）
- ④：决策图走完那一刻把图上互相链接的决定收束成 spec，并在 `map.md` 里补一条带日期的「交棒已发生」加上产物指向

**票（ticket，`issues/NN-<slug>.md`）** L324-327 ｜ ①15 ②0 ③0 ④4 ｜ F：`ticket`、`issues/NN-<slug>.md` ｜ 别处 full（`docs/agents/issue-tracker.md`；`docs/agents/triage-labels.md`）
- ①：`01`（编号起点）、`Type:`、`research`、`prototype`、`grilling`、`task`、`implement`、`Status:`、`claimed`、`resolved`、`ready-for-agent`、`done`、`ready-for-walkthrough`、`ready-for-human`、`Blocked by:`、`Part of:`、`## 评论`、`## 作答`（去重后 15）
- ④：一票一个文件、从 `01` 编号、永远不把所有票合成一个文件；开头那几行就是全部元数据（`Type:` / `Status:` / `Blocked by:` / `Part of:`）；收尾状态分三套（决策票 `claimed` → `resolved`，实现票 `ready-for-agent` → `done`，以及只等人走查的 `ready-for-walkthrough`，它与分诊标签里的 `ready-for-human` 不是一回事）；每票自包含；讨论追加在 `## 评论` 之下、决策票的答案追加在 `## 作答` 之下

**阻塞边（`Blocked by`）** L328-331 ｜ ①4 ②0 ③0 ④3 ｜ F：`Blocked by` ｜ 别处 full（`docs/agents/issue-tracker.md` L33-45）
- ①：`Blocked by: NN, NN`（行格式）、`resolved`、`python3 scripts/wayfinder-check.py .scratch/<effort>/map.md`、`## 任务清单`
- ④：列出的每个文件都变成 `resolved` 之后这张票才解除阻塞；本地 markdown 没有原生依赖边、这条边只是约定；核对靠那条 `wayfinder-check.py` 命令（图的 `## 任务清单` 勾选项必须恰好等于 `issues/` 里的文件、每票那四行元数据都得在）

**frontier** L332-335 ｜ ①1 ②0 ③0 ④2 ｜ F：`frontier`（无中文名） ｜ 别处 full（`docs/agents/issue-tracker.md` L35）
- ①：`Type: implement`
- ④：扫 `.scratch/<effort>/issues/`、找 open 且没被阻塞也没被认领的那些、编号最小的当选；`Type: implement` 一律跳过；设计票全关掉图就算走完，哪怕实现票还开着

**分诊标签（triage label）** L336-339 ｜ ①6 ②0 ③0 ④1 ｜ F：`triage label` ｜ 别处 full（`docs/agents/triage-labels.md`）
- ①：`Status:`、`needs-triage`、`needs-info`、`ready-for-agent`、`ready-for-human`、`wontfix`
- ④：映射与颜色记在 `docs/agents/triage-labels.md` 与 `docs/agents/label-colors.json`（后者只有远程 tracker 用得上、运行期没人读）

### 文档

**逐面文档（`docs/*.md`）** L342-344 ｜ ①3 ②0 ③0 ④2 ｜ F：`docs/*.md`（位置槽位） ｜ 别处 full（`README.md` 的「文档」表；`docs/agents/domain.md`）
- ①：`bash.md`、`render.md`、`repo-map.md`（举例的篇名）
- ④：`docs/` 下一个面一篇、讲现在怎么工作，所以代码改了它就该跟着改；与 spec 的分工是时态（spec 是当初定下的计划、会过期，逐面文档是当前的样子）；README 的「文档」表逐篇索引它

**不可逆的决定（ADR，Architecture Decision Record）** L346-349 ｜ ①1 ②0 ③0 ④2 ｜ F：`ADR`、`Architecture Decision Record` ｜ 别处 full（`docs/adr/`；`README.md`；`docs/agents/domain.md`）
- ①：`0003-plan-leaves-the-permission-modes.md`（举例的文件名）
- ④：`docs/adr/` 下一条决定一个编号一个文件、标题就是那句决定本身；收录口径是「难回头、离开上下文会显得奇怪、当初又真有替代方案」，重头是「为什么这么定」与被否决的替代方案；在决定敲定的那一刻惰性写下一条、不预先攒；与既有 ADR 冲突时明着挑明、不静默推翻。正文另有 `ADR 0003` 引用（5 处 `ADR NNNN` 之一，不属四类）

## 5. 分类：格式必需 vs 纯实现细节

### 5.1 格式必需（78 处）

- 72 个词条的标题槽位各至少 1 个；6 个词条有 2 个：分叉合成（`fs` + `Forked Synthesis`）、投影（`Projection` + `project()`）、价目表（`PriceTable` + `Pricing`）、决策图（`map` + `map.md`）、票（`ticket` + `issues/NN-<slug>.md`）、ADR（`ADR` + `Architecture Decision Record`）。
- 流程 9 个词条里有 5 个的英文槽位是**位置**而不是代码名（`.scratch/<feature-slug>/`、`seed.md`、`spec.md`、`map.md`、`issues/NN-<slug>.md`）—— 这是第 12 行规矩明说的，仍算格式必需。
- 无中文名的先例（英文槽位就是正名）：`goal_note`、`token`、`frontier`、`seed.md`、`spec.md`。
- **不算格式必需**的同类名字：正文里重复出现的 `Block`、`project()`、`map.md`、`todo`、`Choice`；以及 `_Avoid_` 行里的反例名。

### 5.2 纯实现细节（① 235 + ② 9 + ③ 2 = 246 处指称，另加 ④ 193 条）

按「指称的性质」再分五组，供票 07 挑边界用：

| 组 | 处数（约） | 例子 | 备注 |
| --- | --- | --- | --- |
| 类型 / trait / 事件变体名 | 60 | `ContextInjected`、`CancelSignal`、`AskRequest`、`Zone`、`SessionFacts`、`TurnEnded`、`PermissionDecided` | 大多是内部类型，读者不写代码就用不上 |
| 函数 / 方法 / 字段 / 常量 | 70 | `discussion::pick_pair`、`Config::debaters_share_a_vendor`、`project()`、`effect()`、`agent_identity()`、`PULSE_FRAME`、`GESTURE_WINDOW`、`SessionConfig.carried_tokens` | 「机制名」，剥离收益最高 |
| 配置键 / 线级名 / CLI 旗标 / 工具名 | 55 | `[permissions] outside_read`、`custom__<ns>__<tool>`、`--debaters A,B`、`mcp_list`、`ask_user_question` | 一半是**用户/模型可见的接口**（不是纯内部），票 07 要单独判 |
| schema 字段 / 枚举值 / 退出码 | 45 | `{id?, content, status}`、`pending`/`in_progress`/`completed`、`010`/`130`、`cached`/`miss`/`output` | 同上 |
| 节号引用（②）/ `src/` 路径（③） | 11 | `spec §5`、裸 `§6`、`src/render/input.rs` | ② 的 3 处 `spec §N` **都没写是哪个 spec**。`spec §5` / `spec §15` 可确定为 `.scratch/fs-agent-v1/spec.md`（该文件 L297「发言归属与投影」、L453「讨论协议与轮次」正是对应内容）；`spec §7` **存疑** —— 两个候选 spec 都有 §7（fs-agent-v1 的「工具 trait、注册表与副作用」、questionnaire-keys 的「折行与滚动」），按上下文都不贴合。对照组：**裸 `§N` 的 6 处全都点了路径**（`sidebar-toggle/spec.md §2`、`tui-chrome/spec.md §1`、`tui-input-pulse/spec.md §2b`、`exit-gesture/spec.md §6`、`questionnaire-keys/spec.md §5` ×2），只有写「spec §N」的 3 处偷了懒 |

### 5.3 边界发现：3 个「格式必需」的英文槽位在 `src/` 里查无此名

`CONTEXT.md` 第 10 行说英文是「代码里的标识符 / 类型名」，但：

| 词条槽位 | `src/` 真实形态 | 在哪被点名 |
| --- | --- | --- |
| `PromptHue` | `PROMPT_HUE_PER_SECOND` + `hue` 局部量 | `.scratch/tui-input-pulse/spec.md` L6、L89 |
| `FallingDash` | `DASH_FALL` + `identity_falling()` + `mark_lines()` | `.scratch/tui-input-pulse/spec.md` L6、L95 |
| `StatusRow` | `wording::status_row()`（函数，小写） | `docs/render.md` L99、L124 |

这三个（连同 `Pulse`）是 `.scratch/tui-input-pulse/spec.md` 那一轮**为 spec 立的概念名**，不是代码类型名。它们**属格式必需**（在标题槽位里），但**不指向真实标识符** —— 这是「格式必需 vs 实现细节」之外的一类：文档造名。票 07 若按「只留格式必需的类型名」剥，这三个要单独处理。

## 6. 剥离后果（若只留「术语定义 + 格式必需的类型名」）

这是票 07 的输入，不是判断。按 72 个词条逐一试剥，分三档：

**A. 会空掉或只剩标题（正文几乎全是实现行为）—— 14 个**

| 词条 | 剥离后剩下什么 |
| --- | --- |
| 提示符色相（PromptHue） | 空（定义就是颜色参数） |
| 脉冲（Pulse） | 空（定义就是帧计数器与两个常量） |
| 下落短横（FallingDash） | 空（定义就是动画形态 + 已退出屏幕） |
| 状态行（StatusRow） | 空（定义就是那三段的字面内容） |
| 输入区（Input Zone） | 空（定义就是键位分派） |
| 选项区（Options Zone） | 空（同上） |
| 焦点回合（FocusedTurn） | 空（「派生量、不存状态」是定义兼实现） |
| 回合条（TurnRail） | 一句：转录右缘一列、一格一个单位 |
| 会话桶（bucket） | 一句：按会话 cwd 分出的目录 |
| 翻页（rollover） | 一句：结束当前会话、开一个新的 |
| 阻塞边（`Blocked by`） | 一句：票顶上那一行记的依赖边 |
| 问卷（Questionnaire） | 一句：一次 `ask_user_question` 调用里的整批问题 |
| 询问（Ask） | 一句：harness 发起的问句、答案是闸门 |
| 交付给第四档的「权限模式」（Mode） | 剩枚举值 —— 而枚举值本身又是配置接口 |

**B. 只剩一句话（定义句尚可保留）—— 约 22 个**
执行者、发言归属、事件、事件流、投影、会话目录、`goal_note`、取消、举手、区外读、升级、用户提问、作答草稿、问题选项、技能清单、仓库地图、落点、token、价目表、终端端口、动态工具、分诊标签。

**C. 仍有可读的领域定义（剥掉实现后不伤词条）—— 约 36 个**
合成器、目标、目标清单、事件流（定义部分）、讨论轮次、回合、待办列表（定义部分）、搜索提供方、抓取提供方、原语、渲染器、转录、左栏（定义部分）、沙箱（定义部分）、打码（定义部分）、流程与文档两节的多数词条（它们本来就是讲 tracker 的形态，不是讲代码）。

> 注意 B/C 的划分与「别处已有一份」不重合：例如「提示符色相」剥离后会空掉，但它在 spec 里已有一份（full）—— 所以「别处已有」能救文档厚度，「格式必需」救不了可读性。

## 7. 别处是否已有一份（第 4 件事）

### 7.1 汇总

| coverage | 词条数 | 词条 |
| --- | --- | --- |
| full | 66 | 其余全部（详见 §4 每条的「别处」行） |
| partial | 4 | 作答草稿、问卷文案、价目表、落点 |
| none | 2 | 问卷请求、问题选项 |

### 7.2 None / partial 的证据（这几条不能靠「删复述」解决）

| 词条 | 被剥内容 | `docs/` | `.scratch/*/spec.md` | 结论 |
| --- | --- | --- | --- | --- |
| 问卷请求（QuestionnaireRequest） | 端口值、`reply: Result<UserAnswers, String>`、drop sender 语义、与 `AskRequest` 并列 | 无（`QuestionnaireRequest` 0 命中） | 无（0 命中） | 只在 `src/render/input.rs` / `src/render/mod.rs`；CONTEXT 独有 |
| 作答草稿（QuestionDraft） | `selected`/`custom`/`highlight`/`skipped` 四字段、`skipped` 编码、单选多选同形状 | 无 | 部分：`.scratch/questionnaire-keys/spec.md` L51、L135-136、L224 讲答案形状；字段构成与 `TuiState` 只在源码 | partial |
| 问题选项（questions::Choice） | `questions[].options[]`、`label`/`description`、`(Recommended)`、与 `wording::Choice` 的分家 | 无 | 无（两个 `::` 名 0 命中） | 只在 `src/tools/ask_user.rs`、`src/render/tui.rs`；CONTEXT 独有 |
| 问卷文案（`questionnaire_*`） | `render::wording` 前缀族（`questionnaire_hint` 等） | 无（`docs/render.md` 有 `wording::` 但无此前缀） | 有：`.scratch/questionnaire-keys/spec.md` L199 | partial |
| 价目表（PriceTable / Pricing） | `cached`/`miss`/`output` 三档 | 部分：`docs/observability.md` L32、L71-73 讲费用与「无价格」 | 有：`.scratch/fs-agent-v1/spec.md`、`.scratch/tui-layout/spec.md` 有 `PriceTable` | partial |
| 落点（LandingPoint） | 「全系统只有两个落点、可配置覆盖」 | 部分：`README.md` L87 的 `[routing]`、`docs/discussion.md` L123 一句 | 有：`.scratch/fs-agent-v1/spec.md` | partial |

### 7.3 Full 里最吃重的几条（抽查路径，供票 07 引用）

| 词条 | 一份完整来源 |
| --- | --- |
| 讨论者（Debater） | `.scratch/fs-agent-v1/spec.md` §15 L481-484（`pick_pair`、`debaters_share_a_vendor`、`Persona` 注入、`last_round` 逐条都在）+ `docs/discussion.md` L108-122；§5 在 L297，§15 在 L453 —— 这正是 CONTEXT 里 `spec §5` / `spec §15` 的作者本意 |
| 权限模式 / 区外读 / 升级 | `docs/permissions.md` L11-18（四档真值表）、§「区外读」L27-32、L49-56（`escalation` 形状）；`docs/sandbox.md` L3-43、L69 |
| 提问的键位与区域 | `.scratch/questionnaire-keys/spec.md` §1 L75-90（`Zone`）、L199（`questionnaire_*`）；ADR 0010 |
| 渲染的 TUI 形态 | `docs/render.md` L97-140（外壳 / 状态行 / 输入区 / 回合条）、L111-116（下落短横）、L155（色相）；`.scratch/tui-input-pulse/spec.md` §2b L89-94；`.scratch/tui-sidebar/spec.md`（`TurnRail`/`FocusedTurn`）；`.scratch/sidebar-toggle/spec.md` §2 |
| 工具层 | `docs/custom-tools.md` L24-25、L37；`docs/mcp.md` L49-51、L112-113、L140、L185、L219；`docs/web.md` L117、L131、L141、L216-218；`.scratch/todo-and-modes/spec.md` L50-56 |
| 流程 / 文档层 | `docs/agents/issue-tracker.md` L11、L33-45；`docs/agents/triage-labels.md`；`docs/agents/domain.md` §「依赖边 / frontier」；`README.md` 的「文档」表 |
| 安全层 | `docs/sandbox.md` L3-43、`docs/adr/0006`；`docs/credentials.md` L37-47、L97-107、L124 |

### 7.4 一条反向证据

`docs/` 与 `.scratch/*/spec.md` **自己也在按 `§` 号引用 spec**（本图「会撞的既有决定」第 3 条：`scripts/tui-startup-check.py` 与 `docs/adr/*` / `docs/*.md` 按 `§` 号引用 `.scratch/*/spec.md`）。所以「② spec 节号引用」这类指称在 `CONTEXT.md` 里不是孤例，剥掉它不会制造新的维护面；但它**暗示**了 `CONTEXT.md` 的读者被默认成「已经在 `.scratch/` 里工作过的人」——与「每次上手读一次」的定位冲突。

## 8. 方法与复核

- 计数脚本（临时、未落仓）：正则扫 `CONTEXT.md`，按 `^## ` 切 15 节、按 `^\*\*…\*\*:` 切词条，统计反引号 token、`spec §\d+`、裸 `§\d+`、`src/[\w/\.]+`、配置键。**脚本的坑**：标题行里含 `*` 的词条（「逐面文档（`docs/*.md`）」「问卷文案（`questionnaire_*`）」）会被 `[^*]+` 漏掉、并被并进上一个词条；本文件里的数字是**修正后**的（那两个词条已拆开）。
- 「别处」核查：对 `docs/*.md`、`docs/adr/*.md`、`docs/agents/*.md`、`.scratch/*/spec.md` 做关键词命中（`pick_pair`、`debaters_share_a_vendor`、`usable_input`、`carried_tokens`、`CustomTool`、`questionnaire_`、`SessionFacts`、`PULSE_FRAME`、`Redactor`、`EROFS`、`triage` 等约 70 个），再读命中处上下文定 coverage。
- 类型名存在性：对 `src/**/*.rs` 逐个查 `StoredSession` / `LandingPoint` / `DayLedger` / `PriceTable` / `SessionFacts` / `TurnRail` / `QuestionnaireRequest` / `QuestionDraft` / `PromptHue` / `FallingDash` / `StatusRow` / `questions::Choice` / `wording::Choice`。
- 已知局限：
  1. ④「实现行为描述」是**判断题**，条数（193）取决于「机制组」怎么切（口径见 §2.1：同一机制的多个分句算一条；§4 的摘要逐分句列出，分句数 ≥ 条数）。可复核性靠 §4 的逐条摘要，而不是靠脚本。
  2. ① 用**去重**计数；若按出现次数（occurrence）会是 370 个反引号减去路径 / 键位 / UI 字面量后的更大数。两种口径都在 §4 可重算（反引号总数 370）。
  3. research skill 的「启动 background agent 并行调研」在本会话不可用（子代理深度上限 = 1，`subagent` 调用被拒），所以本文件由单会话完成；这**不影响**材料的一手性（全部读的是仓内原文）。
  4. 本文件只清点，不含「剥到哪」的建议；结论供票 07 使用。
