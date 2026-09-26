# fs-agent

自用 coding agent CLI（Rust，从零实现）。本文件只收录本项目**特有**的领域词汇——它是术语表，不是 spec，不含实现决策。

> **每条的格式是「中文名（English）」**：**中文是叙述、文档与讨论里的正式用词；英文是代码里的标识符 / 类型名**。两者指同一个概念，不是互为别名——所以写文档时说「讨论者」，写代码时写 `Debater`。
>
> `agent` 是泛称（程序名 `fs-agent`、"一个 agent 回合"），**不作为类型名**：类型名一律用下面的 **`Debater` / `Executor`**（即讨论者 / 执行者）。

## 名字

**分叉合成（Forked Synthesis，`fs`）**:
框架名，不是类型，也不是命令：`fs` 展开为 `Forked Synthesis`，指「异构讨论者分叉作答 → 合成器收束成共识 / 分歧 / 未决」这条机制；`fs-agent` 读作「分叉合成的 agent」。命令、crate 名与落盘路径（`~/.config/fs-agent/`、`~/.local/share/fs-agent/`）**一律保持 `fs-agent`**，不随框架名改写。
_Avoid_: 把 `fs` 读成 filesystem / file system；与 Out of Scope 的 `fork/rewind` 手势混用「分叉」（那个叫 fork，指回退，不是本词）

## 参与者

**讨论者（Debater）**:
参与同一会话轮次化讨论的对等 agent（一次讨论固定 2 个）。每个讨论者是配置里的一个**人物（persona）**：`[discussion] debaters` 池子里的一个「名字 + 模型 + 可选灵魂」条目——名字是它在流上的身份（转录标签、私有身份、模型看到的 `[轮 N · 名字]` 前缀都用它），所以必须唯一、且是不断开的单个词。池子至少两个；多于两个时每次讨论**抽两个**（`discussion::pick_pair`，可用 `--debaters A,B` 指定）。默认取两家不同厂商——异构是两个独立判断的来源；但**允许**同厂商甚至同一个模型（一个订阅到期不该让讨论不可用），那时多样性只剩采样噪声，`Config::debaters_share_a_vendor` 会在这一对上判定、前台明说。**灵魂（soul）** 是这个人物自己的性格 / 立场，用用户的原话写：它是唯一无法从名字推出来的东西，所以**落在流上**（`ContextInjected { source: Persona(名字) }`，署名是那个讨论者自己），由投影**只发给它自己**——投影一律「别人的注入也变 user」，人物注入是唯一的例外（spec §5）。不进私有身份，是因为身份必须能从流重算（spec §15）。
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

## 提问

**询问（Ask）**:
**harness 发起**的问句，答案是**闸门**：权限门判定为「问」时的许可询问，以及进入硬计划模式时 `PLAN.md` 已存在的冲突。答案决定一次动作的去留（允许/拒绝、覆盖/追加/保留），因此它回答的是「要不要做」。走中段覆盖层、一行按钮；发起者是 harness 或渲染器，不是模型。执行者的询问沿委派链回到同一个键盘。
_Avoid_: 用户提问（那是模型的）、prompt、确认框

**用户提问（User Question）**:
**模型发起**的问句，答案是**上下文**：模型调 `ask_user_question` 工具把一批问题交给用户，用户作答（或显式跳过），答案是那条 `tool_call` 的**唯一结果**（`{"answers":[{"id","selected","custom"?}]}` 的 JSON 文本）。与**询问（Ask）**的分界是「答案决定去留」还是「答案是模型继续干活的输入」——所以这是**第三类发起者**，`Asker` 那条接缝不扩展。只有主会话能问（执行者的工具表里没有它）；TUI 接管**底部输入区**（一屏一问、分页、每题必须作答或跳过），plain 逐行问答，headless 根本不挂这个工具。
_Avoid_: 询问（那是 harness 的闸门）、question（类型名用 `UserQuestion`）、prompt

**问卷（Questionnaire）**:
模型一次 `ask_user_question` 调用里的**整批问题**，以及前端为它持有的键盘状态（TUI 的 `Questionnaire`）。它不是事件、不落流：唯一持久痕迹是那条 `tool_call` 的 args 和它的唯一结果（spec §7）。TUI 里接管底部输入区、一屏一问；plain 逐行问答。
_Avoid_: 表单、form、wizard（那是多步配置流程，不是模型的问题）

**问卷请求（QuestionnaireRequest）**:
端口把一批问题交给前端的那个值（`src/render/input.rs`），带一条一次性回复通道 `reply: Result<UserAnswers, String>`；drop 掉 sender 等于「没有答案」（取消或输入结束），所以工具不会挂住。它与 `AskRequest` 并列——第三类发起者不扩展 `Asker` 接缝，就落在这里。
_Avoid_: 把它当成 `Ask` 的一个变体（答案类型不同，通道也不同）

**作答草稿（QuestionDraft）**:
问卷里**一道题**的作答状态：已选 `selected`、自定义文本 `custom`、高亮下标 `highlight`、是否跳过 `skipped`。它活在键盘那一侧（TUI 的 `TuiState`），每题一份、可来回翻页；`skipped` 是显式的「不作答」，无论之前打过什么字都编码成 `selected: []` 且无 `custom`。
_Avoid_: 答案（那是编码后的 `UserAnswer`）、答题卡

**问题选项（questions::Choice）**:
模型在 `questions[].options[]` 里给的一个选项：`label`（原串就是答案值，`(Recommended)` 只做显示）加可选 `description`。与 `wording::Choice`（一个键 + 一句中文标签，权限 / 计划冲突覆盖层的按钮词汇）**同名不同物**——前者是模型给的选项数据，后者是前端按钮的键位定义，共同点只有「都可被选中」；保留两个名字是更小的改动，各自的层各自命名。
_Avoid_: 把两者互相当别名；按钮（那特指 `wording::Choice`）

**问卷文案（`questionnaire_*`）**:
`render::wording` 里问卷一族人类可见文案的命名前缀（`questionnaire_hint`、`questionnaire_option`、`questionnaire_plain_*` 等）。同一条规则：TUI 与 plain 共享同一个生成器，模型面文本不经过这里。
_Avoid_: 在渲染器里内联问卷中文

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

**token**:
模型的计量单位：输入 / 输出、以及缓存命中与未命中各自计数。**不给它中文名** —— UI 与统计行一律写 `token`（左栏的标签、`token_pair`）。
_Avoid_: 词元、令牌、字

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
编号在**一次会话内唯一**：讨论的轮次号接在流上已有的轮次之后（`discussion::last_round`），所以在同一个会话里跑第二次 `/discuss` 会从「第 4 轮」接着数，而不是重新从「第 1 轮」开始——`round_attendance`、`sessions show --round N` 都靠这个唯一性分辨是哪一场讨论。
_Avoid_: iteration、pass

**回合（Turn）**:
单个 agent 的一次完整回合：投影 → 调 provider → 权限门 → 执行工具 → 追加事件。
_Avoid_: step、call

## 工具

**动态工具（CustomTool）**:
在 `config.toml` 的 `[tools.<命名空间>.<工具>]` 里声明的外部命令，线级名是 `custom__<命名空间>__<工具>`。声明语法的字段就是 provider 收到的声明（JSON Schema 原样），**没有**副作用类别字段，所以 `effect()` 恒为 `Exclusive`；`command` 是 argv 模板，参数按**整个 argv 元素**替换、不经 shell。内建名永不含 `__`，因此「名字含 `__`」⟺「来自配置」是**词法可判定**的。
_Avoid_: 插件、外部工具、MCP 工具

## 渲染

**渲染器（Renderer）**:
启动时选定的**唯一**渲染实现（headless / plain / TUI 三选一、互斥），消费组装期创建的同一条广播通道。三模式共享同一条事件序列，plain 与 TUI 共用 `Transcript` 的 `Block` 呈现层，只有绘制方式不同。
_Avoid_: 显示层、UI 组件、renderer 实现类

**转录（Transcript）**:
把事件流（含旁路的增量文本）转成展示单元 `Block` 的共享层：工具调用与它的结果归成一个块（**由结果绘制**），后置 hook 的反馈另成一个块，增量文本原样透传。**不是** `project()`——那个产出的是给模型的 `messages`。
_Avoid_: 日志、输出、render

**终端端口（Console）**:
前端与循环之间的键盘接缝：循环按需请求一行或一个问题，前端（TUI 或 plain 的行读取）回答；取消 / 计划手势经 `ConsoleEvents` 主动上行。权限门走同一条通道（`ConsoleAsker`），因为两者都来自同一个键盘。
_Avoid_: stdin、输入流、prompt

**左栏（Sidebar）**:
TUI 左侧的**全高**一栏：身份（宽档画 `fs` 标记、窄档退成一行 `fs-agent <版本>`）+ **页签条** + 选中页的内容。**去留只由宽度决定**（≥ 120 列 40 列宽、80–119 列 28 列宽、更窄整栏隐藏），**内容由自己那一栏的高度决定**（先丢标记、再丢身份行、最后从尾部丢字段）。页签是点出来的、不给键位（`Tab` 归 `/` 菜单、`Shift+Tab` 归计划模式）：`调用量`（会话读数，默认页）/ `轨迹` / `文件`，后两页还没实现、只画一句占位。
_Avoid_: 侧栏、信息面板（那只是它的一个页）、面板

**输入区（InputArea）**:
主列最下面那块打字的地方，在状态行与提示行之间，**最少 3 行、最多 10 行**：草稿折行数不足 3 时它照样占 3 行（空的两行是留白，草稿从**顶部**写起），超过 3 才继续长高。它**没有自己的边框**——上下的分隔线由外壳画，界面里只有一圈外框。地板之间有一条优先级：**转录的最后一行比输入区的最小高度优先**，所以 40×10 下输入区只拿 2 行。
_Avoid_: 输入框（听起来像带边框的控件）、命令行、prompt（那是它第一行的 `> `）

**忙碌脉冲（Pulse）**:
一次运行在飞时左栏标记的颜色相位：整块标记按 `PULSE_PALETTE` 那条 12 帧的色环（六个色相的明/暗两支）走，`PULSE_FRAME` 100 ms 一帧、1.2 秒一圈，停下就回到静止时的品红渐变。它是**纯渲染器状态**，不进事件流；计时器**只在忙碌时 arm**，空闲时那台时钟根本不存在。标记只在宽档（≥ 120 列）且够高时画，所以这是一种**宽屏可见**的信号。
_Avoid_: 动画、loading、spinner、进度条（那讲的是进度，它只讲「还在跑」）

**状态行（StatusRow）**:
输入区上方那一行：`模型 X │ 模式 Y │ 上下文 Z%`。按宽度先丢**模型**、再丢**模式**、最后只剩上下文占比（仍带标签），**这一行永远在场**——让它消失需要的宽度比 40×10 的终端地板还窄。三段都不可点。
_Avoid_: 提示行（那是它下面那一行，讲键位）、顶栏（那是它取代掉的旧东西）

**回合条（TurnRail）**:
转录右缘最右 1 列的竖条，**一格 = 会话的一个单位**：交互会话数**回合**（`TurnEnded`），讨论会话数**轮次**（`RoundEnded`），判定用组装期注入的 `SessionFacts.speaker_order`。格子贴底排（最新的最下），焦点那格**粗 + 亮**、其余**细虚线 + 暗**，被裁掉时最上一格 `⋮`；点一格跳到那一格单位的段首。
_Avoid_: 轮次条（交互会话里那格是回合、讨论里才是轮次，用它会读成只数轮次）、滚动条（那是行位置，在它左边一列）

**焦点回合（FocusedTurn）**:
回合条上被高亮的那一格，含义是「**正在看的那一段**」，**不是最新那一回合**。它是**派生量** = 视口顶端那一行所属的单位，不存状态——存下来的话，用户一滚动它就和视口漂移了。吸底时它恰好等于最新单位。
_Avoid_: 当前回合（会被读成「最新回合」）、选中回合（会被读成一个存起来的选项）

## 安全

**打码（Redactor）**:
入流前的**值级、best-effort** 替换：把配置里解析出的密钥值换成 `[redacted]`，于是**流上的文本 == 模型看到的文本**，而工具执行仍拿真值；范围含消息正文与工具参数，`outputs/<tool_call_id>.txt` 打码、`<tool_call_id>.before` 不打码（它是 `/undo` 的字节级还原源）。
_Avoid_: 脱敏、掩码、mask、sanitize
