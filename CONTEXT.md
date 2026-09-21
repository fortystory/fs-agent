# fs-agent

自用 coding agent CLI（Rust，从零实现）。本文件只收录本项目**特有**的领域词汇——它是术语表，不是 spec，不含实现决策。

> **每条的格式是「中文名（English）」**：**中文是叙述、文档与讨论里的正式用词；英文是代码里的标识符 / 类型名**。两者指同一个概念，不是互为别名——所以写文档时说「讨论者」，写代码时写 `Debater`。
>
> `agent` 是泛称（程序名 `fs-agent`、"一个 agent 回合"），**不作为类型名**：类型名一律用下面的 **`Debater` / `Executor`**（即讨论者 / 执行者）。

## 参与者

**讨论者（Debater）**:
参与同一会话轮次化讨论的对等 agent（v1 固定 2 个、异构：KIMI + DeepSeek）。
_Avoid_: 辩论者、discussant、agent（作类型名时）

**执行者（Executor）**:
由讨论者派出、带自己的 `parent_id` 与独立预算的子 agent，用来实际执行任务。
_Avoid_: 子代理、执行器、subagent、worker

**发言归属（Speaker）**:
一次发言的归属（事件上的 `speaker_id`）；投影靠它判定一条发言相对当前 agent 是 `assistant` 还是 `user`。
_Avoid_: 说话人、发言者、author、role

**合成器（Synthesizer）**:
讨论收尾时把各方发言合成产出的**那一次调用**——不是 agent（无工具、无回合、不参与轮次）。产出是「共识 / 分歧（含各自成立的前提）/ 未决」三档，而不是一个新答案。
_Avoid_: 聚合器、aggregator、judge、裁判

## 事件与状态

**事件（Event）**:
事件流里一条不可变记录；事件流是会话的唯一真相源。
_Avoid_: 消息（那是 provider 层的 `messages`）、log line

**事件流（EventLog）**:
只追加的事件序列；每个讨论者 / 执行者的 `messages` 都是它的一次投影。
_Avoid_: 事件日志、transcript、history、bus

**投影（Projection，`project()`）**:
纯函数 `(EventLog, SpeakerId, provider 能力) → messages`，把事件流转成某个 agent 这一次调用要重放的 `messages`。住在 provider 适配器侧。
_Avoid_: 渲染、render、format

**会话（Session）**:
持有 `EventLog` + 名册 + 预算 + config 的那个值；是唯一持有可变状态的结构。执行者用带 `parent_id` 的嵌套会话。
_Avoid_: conversation、thread、context

## 会话存储

**会话目录（StoredSession）**:
一个会话在磁盘上的可搬运单元：JSONL 事件流 + `outputs/`；与 cwd 绑定、默认 `0700`，`--continue` 与 `prune` 都以它为单位。
_Avoid_: 会话文件、存档、checkpoint

**会话桶（bucket）**:
按会话 cwd 分出的目录；`--continue` 只扫本桶（cwd 的 slug 只用于分桶，权威 cwd 在 `SessionStarted` 里）。
_Avoid_: 索引、registry

## 控制

**取消（Cancel）**:
一次用户手势（Esc），中断在飞的 provider 调用与在跑的工具；`tool_call` 已开始的那些各补一条合成结果，当前回合以 `Aborted` 收尾。手势本身**不进事件流**（与 `/undo` 的写回、`/plan` 的覆盖同规矩），并且**只沿委派链向下**：`CancelSignal`（信号端，只有前端持有）发起，`CancelObserver`（观察端）在回合与执行者手里逐层克隆——回合手里没有能发起取消的东西，所以「执行者被取消」不会把派发者的回合记成失败。
_Avoid_: abort、stop、interrupt（`Aborted` 是收尾原因，不是手势名）

**硬计划模式（Plan mode）**:
`readonly` 加**一条**写豁免的权限模式（`Mode::Plan`）：可以读，唯一能写的是项目根 `PLAN.md`；进出**只由用户手势**决定（`/plan` / `/endplan`），模型那侧没有对应的工具。派出去的执行者沿用同一模式。
_Avoid_: planning mode、计划态（叙述里的「硬 plan 模式」指的就是它）

## 上下文与技能

**技能（Skill）**:
按需披露的指令包：描述每轮都在场（便宜），全文只在模型调用 `skill` 时取（贵，但只在真需要时付）。
_Avoid_: 技巧、插件、工具、能力

**技能清单（SkillsCatalog）**:
技能的「名字 + 描述」列表，与 `AGENTS.md` 同处钉住的首条 `user` 消息、逐轮不变；模型据此决定要取哪个技能的全文。
_Avoid_: 技能索引、技能目录

**仓库地图（RepoMap）**:
按需取的仓库符号地图：`repo_map(focus?)` 工具一次返回「哪些文件定义了哪些函数 / 类型 / trait / 模块 / 宏」，按会话相关度排序、固定预算。产物是工具结果、**不注入**（注入版一刷新就把整段历史挤出缓存前缀）。
_Avoid_: 符号表、codebase map、repo index

## 成本与预算

**预算（Budget）**:
会话级的**累计 token 上限**：闸门看 `UsageRecorded` 的求和（讨论者、执行者、合成器共用一个额度，各端 config 必须一致，不然组装期报错），撞顶时**降级收尾**而不是中断。与**窗口**（`context::usable_input`：按 agent **各自**模型算的单次输入上限，靠丢弃旧内容满足）是两回事。
_Avoid_: 配额、quota、上下文预算

**价目表（PriceTable / Pricing）**:
按 model id 配的单价，单位是每百万 token 的 USD（`cached` / `miss` / `output` 三档，**命中与未命中分开计价**）。**费用只作显示**：闸门只数 token；未登记的 model 显示为「无价格」，不是 0。
_Avoid_: 费率表、计费表、cost table

**落点（LandingPoint）**:
允许改派更弱模型的**位置**，全系统只有两个：合成器与执行者（可配置覆盖）。**讨论者绝不是落点**——异构是讨论里最强的多样性杠杆。
_Avoid_: 分流点、路由目标、routing target

**日账本（DayLedger）**:
按 **UTC 日**聚合的 token 用量，**从会话文件派生**（由事件自己的时间戳决定归属哪一天），不新增状态文件；用来回答「供应商的滚动窗口这半天花了多少」。
_Avoid_: 账本文件、ledger.json、用量统计

## 讨论

**轮次（Round）**:
讨论协议的一步（独立首轮 → 揭示 → 定向第二轮 → 合成）。具体的一次读作「第 N 轮」。
_Avoid_: iteration、pass

**回合（Turn）**:
单个 agent 的一次完整回合：投影 → 调 provider → 权限门 → 执行工具 → 追加事件。
_Avoid_: step、call

## 渲染

**渲染器（Renderer）**:
启动时选定的**唯一**渲染实现（headless / plain / TUI 三选一、互斥），消费组装期创建的同一条广播通道。三模式共享同一条事件序列，plain 与 TUI 共用 `Transcript` 的 `Block` 呈现层，只有绘制方式不同。
_Avoid_: 显示层、UI 组件、renderer 实现类

**转录（Transcript）**:
把事件流（含旁路的增量文本）转成展示单元 `Block` 的共享层：工具调用、结果与后置 hook 反馈归成一个块，增量文本原样透传。**不是** `project()`——那个产出的是给模型的 `messages`。
_Avoid_: 日志、输出、render

**终端端口（Console）**:
前端与循环之间的键盘接缝：循环按需请求一行或一个问题，前端（TUI 或 plain 的行读取）回答；取消 / 计划手势经 `ConsoleEvents` 主动上行。权限门走同一条通道（`ConsoleAsker`），因为两者都来自同一个键盘。
_Avoid_: stdin、输入流、prompt

## 安全

**打码（Redactor）**:
入流前的**值级、best-effort** 替换：把配置里解析出的密钥值换成 `[redacted]`，于是**流上的文本 == 模型看到的文本**，而工具执行仍拿真值；范围含消息正文与工具参数，`outputs/<tool_call_id>.txt` 打码、`<tool_call_id>.before` 不打码（它是 `/undo` 的字节级还原源）。
_Avoid_: 脱敏、掩码、mask、sanitize
