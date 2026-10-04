# fs-agent

自用 coding agent CLI（Rust，从零实现）。本文件收录**两类**词：

- **领域词汇**——本项目特有的运行期概念（事件流、投影、待办列表……）。写代码、写文档、写票之前先在这里对齐名字。
- **流程词汇**——本仓库自己的运转方式（feature 目录、spec、票、决策图……）。它讲 `.scratch/` 与 `docs/` 怎么组织、票怎么流转。

**它不含实现决策**（那是 `docs/` 逐面文档与 `.scratch/` 的 spec 的事），也**不收两类东西**：通用编程概念（timeout、error type、工具函数那种，即使本项目到处在用），以及 skills 工具链的名字（`/grill-with-docs`、`/wayfinder`、`/handoff`……）——后者是**外来名**，只在票与文档的正文里出现，本文件最多在某个词的 `_Avoid_` 里点它一下，不给它立条目。

> **每条的格式是「中文名（English）」**：**中文是叙述、文档与讨论里的正式用词；英文槽位是代码里的标识符 / 类型名 —— 若这个概念只在 spec 与文档里立了名、代码里没有对应实体，槽位就写那个已被文档采用的写法**。两者指同一个概念，不是互为别名——所以写文档时说「讨论者」，写代码时写 `Debater`。
>
> **流程词没有对应的代码标识符**：那一节的英文槽位放它在磁盘上或 tracker 里的位置（`spec.md`、`map.md`、`Status:`）；没有中文名的词就直接不给中文名，见**token** 那条的先例。
>
> `agent` 是泛称（程序名 `fs-agent`、"一个 agent 回合"），**不作为类型名**：类型名一律用下面的 **`Debater` / `Executor`**（即讨论者 / 执行者）。

## 名字

**分叉合成（Forked Synthesis，`fs`）**:
框架名，不是类型，也不是命令：`fs` 展开为 `Forked Synthesis`，指「异构讨论者分叉作答 → 合成器收束成共识 / 分歧 / 未决」这条机制；`fs-agent` 读作「分叉合成的 agent」。命令、crate 名与落盘路径**一律保持 `fs-agent`**，不随框架名改写。细节见 [docs/discussion.md](docs/discussion.md)。
_Avoid_: 把 `fs` 读成 filesystem / file system；与 Out of Scope 的 `fork/rewind` 手势混用「分叉」（那个叫 fork，指回退，不是本词）

## 参与者

**讨论者（Debater）**:
参与同一会话轮次化讨论的对等 agent，一次讨论固定 2 个。每个讨论者是配置里的一个**人物（persona）**（名字 + 模型 + 可选灵魂）：名字是它在流上的身份，必须唯一、且是不断开的单个词。

两个讨论者**本来就该**异构——默认取两家不同厂商，因为独立的第二个判断才是讨论的意义；但**允许**同厂商甚至同一个模型（一个订阅到期不该让讨论不可用），那时多样性只剩采样噪声，前台会明说。**灵魂（soul）** 是这个人物自己的性格 / 立场，用用户的原话写：它是唯一无法从名字推出来的东西，所以**落在流上**、署名讨论者自己，并由投影**只发给它自己**（「别人的注入也变 user」是投影的通例，人物注入是唯一的例外），且**不进私有身份**（身份必须能从流重算）。细节见 [docs/discussion.md](docs/discussion.md)。
_Avoid_: 辩论者、discussant、agent（作类型名时）

**执行者（Executor）**:
由讨论者派出、用来实际执行任务的子 agent，是**一次委派**的承担者。细节见 [docs/executor.md](docs/executor.md)。
_Avoid_: 子代理、执行器、subagent、worker

**发言归属（Speaker）**:
一次发言的归属，决定投影把一条发言相对当前 agent 读成 `assistant` 还是 `user`。细节见 [docs/discussion.md](docs/discussion.md)。
_Avoid_: 说话人、发言者、author、role

**合成器（Synthesizer）**:
讨论收尾时把各方发言合成产出的**那一次调用**——不是 agent（无工具、无回合、不参与轮次）。产出是「共识 / 分歧（含各自成立的前提）/ 未决」三档，而不是一个新答案。细节见 [docs/discussion.md](docs/discussion.md)。
_Avoid_: 聚合器、aggregator、judge、裁判

## 事件与状态

**事件（Event）**:
事件流里一条不可变记录；事件流是会话的唯一真相源。细节见 [docs/lifecycle.md](docs/lifecycle.md)。
_Avoid_: 消息（那是 provider 层的 `messages`）、log line

**事件流（EventLog）**:
只追加的事件序列。细节见 [docs/lifecycle.md](docs/lifecycle.md)。
_Avoid_: 事件日志、transcript、history、bus

**投影（Projection，`project()`）**:
把事件流转成某个 agent 这一次调用要重放的 `messages` 的**纯函数**。细节见 [docs/observability.md](docs/observability.md)。
_Avoid_: 渲染、render、format

**会话（Session）**:
持有事件流、名册、预算与配置的那个值，是唯一持有可变状态的结构。细节见 [docs/lifecycle.md](docs/lifecycle.md)。
_Avoid_: conversation、thread、context

## 会话存储

**会话目录（StoredSession）**:
一个会话在磁盘上的可搬运单元；与 cwd 绑定，`--continue` 与 `prune` 都以它为单位。细节见 [docs/observability.md](docs/observability.md)。
_Avoid_: 会话文件、存档、checkpoint

**会话桶（bucket）**:
按会话 cwd 分出的目录，是 `--continue` 找会话时的第一层范围。细节见 [docs/observability.md](docs/observability.md)。
_Avoid_: 索引、registry

## 目标

**目标（Goal）**:
**跨会话**的工作单位：一份具名的清单文件（定义）加上从各会话流派生的进度与花费；`/loop <名字>` 认领它，当前目标永远是流上最后一条，所以切换就是再记一条。**它不进会话状态**（没有状态文件、没有「当前目标」字段）。细节见 [docs/goals.md](docs/goals.md) 与 [ADR 0009](docs/adr/0009-goals-are-files-and-progress-is-derived.md)。
_Avoid_: 路由目标（那是**落点**那条 `_Avoid_` 里的东西，与这里无关）、任务（那是 `task`）、计划（那是**待办列表**）

**目标清单（Manifest）**:
一个目标的**定义**：一份具名的清单文件，只有条目与 id、不带状态。**开工前封闭**（执行中不能往里加条目，于是「所有条目都 `completed`」是稳定判据），此后与票各自独立。细节见 [docs/goals.md](docs/goals.md)。
_Avoid_: 目标状态（不在文件里）、`TODO.md`、任务清单

**翻页（rollover）**:
**结束当前会话、开一个新的**那条内部动作（进程不重启，渲染器与终端留着；旧会话的文件留在磁盘上）。压缩与翻页总是成对发生（不做「压缩后看空间够不够再决定翻不翻」那条分支）。细节见 [docs/goals.md](docs/goals.md)。
_Avoid_: 重启、新会话（那是它的产出）、压缩（那是翻页带过去的那件事）

**`goal_note`**:
模型记「执行中冒出来的新工作」的工具，与**待办工具**完全同构；消费者是目标收尾时那份汇总的「新工作」一项（清单是封闭的，新工作只能在那里交代）。主会话、讨论者、执行者与 headless 都挂，但它**不进侧栏**。细节见 [docs/goals.md](docs/goals.md)。
_Avoid_: 目标条目（那只在清单文件里）、待办列表（那是 `todo`）

## 控制

**取消（Cancel）**:
一次用户手势，中断进行中的 provider 调用与在跑的工具。**问卷立着时是例外**：那里的 `Esc` 只管「退出这次询问」，问卷期间要取消这次运行只有 `Ctrl-C`。手势本身**不进事件流**，并且**只沿委派链向下**——「执行者被取消」不会把派发者的回合记成失败。细节见 [docs/lifecycle.md](docs/lifecycle.md)。
_Avoid_: abort、stop、interrupt（`Aborted` 是收尾原因，不是手势名）

**挂起（Suspend）**:
一次用户手势：`Ctrl-Z` 把整个 fs-agent 进程停住、shell 拿回提示符，`fg` 回到原地继续——**真暂停**，不是「画布让位、回合继续跑」（后者要 daemon 化，是另一件事）。**单下生效**，不做举手；**只有 TUI 有这个手势**，plain 的同一按键由终端驱动直接处理、headless 没有键盘。手势本身**不进事件流**，也**不打回执**；停着的那段时间里子进程与各种 deadline **照旧走**，所以想连子进程一起停下时的动作是「先**取消**、再挂起」。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 暂停（听着像暂停某一次调用）、后台任务（那是 `background-services`）、休眠、`/suspend` 命令

**举手（Gesture）**:
「第一下只举手、第二下才真做」的那个中间状态。**槽位只有一个**，里面举着的是哪一把由它标出，所以两把**互斥**——举新的就把旧的作废。第一把是**退出**：空闲时 `Ctrl-C` 与 `Ctrl-D` 共用它，运行中第一下 `Ctrl-C` 也举它（那一下同时是**取消**），退出码跟着**怎么退的**走（人主动退是 `0`，运行中被打断而退是 `130`）。

第二把是问卷里的**退出这次询问**：在输入区按 `Esc` 时它顺带把键盘送回选项区，第二下丢掉回复通道、**模型继续跑**、这一回合不中止。它是**纯渲染器状态**，不进事件流。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 双击（那说的是按键次数，没说出「中间有个会过期的状态」）、确认框（那是弹窗，这条路刻意不做）

**权限模式（Mode）**:
会话对「写」的一档立场：`readonly`（非只读调用一律拒）/ `ask`（写要问、读放行，交互式默认）/ `workspace`（工作区内一律放行、**区外写问一次**）/ `auto`（默认放行，仍受规则与断路器约束）。**计划不是一档**：待办列表是模型自己的 `todo` 工具（见**待办列表**）。细节见 [docs/permissions.md](docs/permissions.md)。
_Avoid_: 权限档、permission level（词表里是「模式」）、plan 模式（已退场）

**区外读（Outside Read）**:
读目标落在会话 cwd 之外时给什么裁决的一档策略，**全局**生效、四档都认它，因为它是策略级的地板。缺省是拒绝且**不动摇**——这条地板保住的正是 provider key 所在的那条路，**写下来才算放弃**（与关掉沙箱同一个立场）。区外**写**没有对称的旋钮，它只有 `workspace` 档那个出口。细节见 [docs/permissions.md](docs/permissions.md)。
_Avoid_: 读权限（那是**读集合**那回事）、路径上限（那是这条旋钮管着的东西）

**升级（Escalation）**:
命令被**沙箱**拒之后，模型带一份理由与要放开的路径、把**同一条命令**原样重试一次的那条手势——唯一持有者是 `bash`。形状里理由与路径两者都要非空、必须成对；**批准的就是声明的那个路径本身**（**不做父目录提升**）；**只这一次调用、只给一次重试**——不进规则、不写配置、不进会话状态，也没有 `allow-always`。它与**询问（Ask）**共用同一条审批通道，但回答的是另一个问题：询问问「要不要跑这条命令」，升级问「要不要放开这一条路径」。写死的边界（遮罩目录、`.env` 家族、`.git/config`、`.git/hooks`）**不给任何通道**，门里直接拒。细节见 [docs/permissions.md](docs/permissions.md)。
_Avoid_: 提权（那是另一个词）、批准（那是一次询问的答案）、allow-always（这里刻意没有）

## 提问

**询问（Ask）**:
**harness 发起**的问句，答案是**闸门**：权限门判定为「问」时的许可询问，回答的是「要不要做」——一次写、一次 shell 要不要放行。答案决定这次动作的去留（允许 / 总是允许 / 拒绝）。执行者的询问沿委派链回到同一个键盘；无交互前端时一律**拒绝**。细节见 [docs/permissions.md](docs/permissions.md)。
_Avoid_: 用户提问（那是模型的）、prompt、确认框

**用户提问（User Question）**:
**模型发起**的问句，答案是**上下文**：模型把一批问题交给用户，用户作答（或显式跳过），答案是那条工具调用的**唯一结果**。与**询问（Ask）**的分界是「答案决定去留」还是「答案是模型继续干活的输入」——所以这是**第三类发起者**，`Asker` 那条接缝不扩展。**只有主会话能问**（执行者的工具表里没有它）；TUI 里接管**底部输入区**（一屏一问、分页、每题必须作答或跳过），plain 逐行问答，headless 根本不挂这个工具。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 询问（那是 harness 的闸门）、question（类型名用 `UserQuestion`）、prompt

**问卷（Questionnaire）**:
模型一次提问调用里的**整批问题**，以及前端为它持有的键盘状态。它不是事件、不落流：唯一持久痕迹是那条工具调用与它的唯一结果。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 表单、form、wizard（那是多步配置流程，不是模型的问题）

**问卷请求（QuestionnaireRequest）**:
端口把一批问题交给前端的那个值，带一条一次性回复通道；丢掉发送端就等于「没有答案」。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 把它当成 `Ask` 的一个变体（答案类型不同，通道也不同）

**作答草稿（QuestionDraft）**:
问卷里**一道题**的作答状态：已选、自定义文本、高亮下标与是否跳过四项，活在键盘那一侧、每题一份。**跳过是「没作答就走开了」**——它由回车或往前翻页记下，而**作答会把它撤销**（所以它记的是这道题此刻的处置，不是一次盖章）；键盘当前在哪个**区域**是问卷级的、每题共用，见**选项区** / **输入区**。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 答案（那是编码后的 `UserAnswer`）、答题卡

**选项区（Options Zone）**:
问卷里**高亮落在选项上的那一区**：键盘在这里移动高亮，**越过两端就是进输入区**，**空格**在这里确认高亮项（界面里唯一的选中键），而**回车是「处置这一题、往前走」**；**可打印字符与 `Backspace` 在这里一律被吞掉**——想打字得先让高亮落到输入区。它与**输入区**共同构成问卷的**区域（Zone）**：键盘任一时刻只在一个区域里，`Zone` 是**问卷级**的一个状态（每题共用，翻页时回到选项区），不是每题一份。`j`/`k` 只在选项区是移动、进了输入区就是文本，这条分界正是本词条的由来。细节见 [docs/render.md](docs/render.md) 与 [ADR 0010](docs/adr/0010-questionnaire-keys-dispatch-by-zone.md)。
_Avoid_: 列表（它没说出「键盘在这里移动」）、自定义栏（那只是绘制出来的标签）、焦点（见**焦点回合**，那是转录里的词）

**输入区（Input Zone）**:
问卷里**键盘让给文本的那一区**——问卷的两个区域之一，键盘落在它上面时输入的是答案的文本部分：可打印字符与 `j`/`k` 都在这里进文本，`Esc` 回选项区、**并且**同时举起「退出这次询问」的手（所以任何区域连按两下 `Esc` 都是退出询问）。它**不是编辑器**：没有行内光标、没有历史，`Backspace` 只从末尾删；没有选项的题只有这一区。

它与**输入区（InputArea）**同名不同物：那条说的是主列底部那块**地方**，这条说的是键盘落在问卷的哪一**区**。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 自定义栏（绘制标签）、文本框、编辑器、textarea（它没有编辑器该有的那些东西）

**问题选项（questions::Choice）**:
模型在一道题里给的一个选项（`label` 加可选 `description`）。它与前端按钮的 `wording::Choice` **同名不同物**。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 把两者互相当别名；按钮（那特指 `wording::Choice`）

**问卷文案（`questionnaire_*`）**:
TUI 与 plain 共用的、问卷一族人类可见文案的命名前缀。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 在渲染器里内联问卷中文

## 上下文与技能

**技能（Skill）**:
按需披露的指令包：描述每轮都在场（便宜），全文只在模型调用时取（贵，但只在真需要时付）。细节见 [docs/skills.md](docs/skills.md)。
_Avoid_: 技巧、插件、工具、能力

**技能清单（SkillsCatalog）**:
技能的「名字 + 描述」列表，逐轮在场、不变；模型据此决定要取哪个技能的全文。细节见 [docs/skills.md](docs/skills.md)。
_Avoid_: 技能索引、技能目录

**仓库地图（RepoMap）**:
按需取的仓库符号地图：一次返回「哪些文件定义了哪些函数 / 类型 / trait / 模块 / 宏」，按会话相关度排序、固定预算。产物是工具结果、**不注入**（注入版一刷新就把整段历史挤出缓存前缀）。细节见 [docs/repo-map.md](docs/repo-map.md)。
_Avoid_: 符号表、codebase map、repo index

## 成本与预算

**预算（Budget）**:
**累计 token 上限**：闸门看已记录用量的求和，撞顶时**降级收尾**而不是中断。**有当前目标时求和范围是这个目标横跨的所有会话**（翻页不重置额度，见**目标**）；没有目标时就是本会话的。与**窗口**（按 agent **各自**模型算的单次输入上限，靠丢弃旧内容满足）是两回事。细节见 [docs/goals.md](docs/goals.md)。
_Avoid_: 配额、quota、上下文预算

**token**:
模型的计量单位：输入 / 输出、以及缓存命中与未命中各自计数。**不给它中文名**——UI 与统计行一律写 `token`。细节见 [docs/observability.md](docs/observability.md)。
_Avoid_: 词元、令牌、字

**价目表（PriceTable / Pricing）**:
按 model id 配的单价，单位是每百万 token 的 USD，命中与未命中分开计价。**费用只作显示**。细节见 [docs/observability.md](docs/observability.md)。
_Avoid_: 费率表、计费表、cost table

**落点（LandingPoint）**:
允许改派更弱模型的**位置**，全系统只有两个：合成器与执行者。**讨论者绝不是落点**。细节见 [docs/discussion.md](docs/discussion.md)。
_Avoid_: 分流点、路由目标、routing target

**日账本（DayLedger）**:
按 **UTC 日**聚合的 token 用量，**从会话文件派生**（由事件自己的时间戳决定归属哪一天），不新增状态文件；用来回答「供应商的滚动窗口这半天花了多少」。细节见 [docs/observability.md](docs/observability.md)。
_Avoid_: 账本文件、ledger.json、用量统计

## 讨论

**轮次（Round）**:
讨论协议的一步（独立首轮 → 揭示 → 定向第二轮 → 合成）；具体的一次读作「第 N 轮」。编号在**一次会话内唯一**，会话里的第二次讨论接着数，`sessions show --round N` 靠这个唯一性分辨是哪一场。细节见 [docs/discussion.md](docs/discussion.md)。
_Avoid_: iteration、pass

**回合（Turn）**:
单个 agent 的一次完整往返：它取上下文、落到事件为止，是会话与讨论统计的单位。细节见 [docs/lifecycle.md](docs/lifecycle.md)。
_Avoid_: step、call

## 工具

**动态工具（CustomTool）**:
来自配置、在 `config.toml` 里声明的外部命令。声明语法的字段就是 provider 收到的声明（JSON Schema 原样），**没有**副作用类别字段，所以副作用一律按最严处理；`command` 是 argv 模板，参数按**整个 argv 元素**替换、**不经 shell**。内建名永不含 `__`，因此「名字含 `__`」⟺「来自配置」是**词法可判定**的。细节见 [docs/custom-tools.md](docs/custom-tools.md)。
_Avoid_: 插件、外部工具、MCP 工具

**待办列表（Todo）**:
一次待办调用提交的**整份**列表：每项是内容加一个状态（待办 / 进行中 / 已完成），可选的 id 引用**目标清单**里的那一条。**列表就是那次调用的参数**，没有第二处存储；**一次提交整份**，同一条助手消息里两次并发调用时**后落地的那条是真相**。它是**某个 agent 自己的**列表，互不干扰；上侧栏的是**非执行者**那一份，执行者的只进转录。细节见 [docs/goals.md](docs/goals.md)。
_Avoid_: 计划（那是它取代掉的那一档模式）、待办事项（词表里没有别的「事项」）、`TODO.md`（不落盘）

**待办工具（`todo`）**:
模型维护待办列表的内建工具。它是**只读**的——不碰工作区，列表活在调用自己的参数里——所以权限门从不为它发问。挂载面是**主会话、讨论者与执行者**，headless 也挂（它不需要人，这正是与**用户提问**的分界）。规则段请模型开工前先立待办，但那是**引导**、不是强制。细节见 [docs/goals.md](docs/goals.md)。
_Avoid_: plan 工具、`update_plan`、`exit_plan_mode`

**搜索提供方（SearchProvider）**:
出网三层里最下面那一层的搜索实现：一次一个查询，返回**按排名排好的结构化来源**（`url` / `title` / `snippet` / `published_at`），而不是一段文本——结构化来自提供方的块，不来自某个 agent 的总结。去重、并发、轮询合并与错误码在**服务层**，schema、上限与呈现在**工具层**——换后端不动工具声明。细节见 [docs/web.md](docs/web.md)。
_Avoid_: 搜索插件、搜索引擎（那是后端背后那家公司）、检索器

**抓取提供方（FetchProvider）**:
与服务层并列的另一半：把一个 URL 变成**有界的正文**（最终 URL、状态码、HTML / 纯文本、截断标志）。**非 2xx 是结果不是错误**：404 是被抓资源的状态。细节见 [docs/web.md](docs/web.md)。
_Avoid_: 爬虫（那是遍历整站的东西）、浏览器、下载器

**元工具（Meta-tool）**:
MCP 那一层的四个固定名字的工具。与内建工具、动态工具的分别在于：**server 的工具永远不进工具表**，模型按「哪一个 server」加「哪一个工具」两个参数穿过这四个口子去调外部能力——于是「工具表是缓存前缀的一部分」与「只有连上才知道 server 有什么」之间不再撞。它们**同开同关**。细节见 [docs/mcp.md](docs/mcp.md)。
_Avoid_: MCP 工具（那是 server 提供的那些）、代理工具、桥接工具

**原语（Primitive）**:
MCP 规范里 server 能提供的东西的四个类别：tool（可调用的动作）、resource（按 URI 读的数据）、prompt（由**人**挑的模板）、elicitation（server 向人要一个输入）。**原语**是这一层的分类词，不是某一条具体能力。协议只谈无状态形态——sampling / roots / logging 已 deprecated、不在范围。细节见 [docs/mcp.md](docs/mcp.md)。
_Avoid_: 能力（那是 `ClientCapabilities` / `ServerCapabilities` 那一侧的词）、功能

**MCP server（server）**:
被 fs-agent 连上的外部进程或服务（stdio 子进程或 Streamable HTTP 端点），名字就是四个元工具认的那个键。它**默认不可信**：内容带外部内容标记、工具按最严的副作用处理、进程过沙箱且只拿到白名单里的环境变量；三个信任位（结果可信 / 副作用可信 / 过沙箱）**各自独立、互不牵连**，缺省都取最保守的那一档。写纯中文散文时也不译，直接用 server。细节见 [docs/mcp.md](docs/mcp.md)。
_Avoid_: MCP 服务（容易与 `McpService` 混）、插件、连接器、MCP 服务器（那是跑 server 的那台机器）

## 渲染

**渲染器（Renderer）**:
启动时选定的**唯一**渲染实现（headless / plain / TUI 三选一、互斥）；三模式共享同一条事件序列，plain 与 TUI 共用**转录**的呈现层，只有绘制方式不同。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 显示层、UI 组件、renderer 实现类

**转录（Transcript）**:
把事件流转成展示单元的共享层：一次工具调用与它的结果归成一个块（**由结果绘制**），增量文本原样透传。**不是** `project()`——那个产出的是给模型的 `messages`。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 日志、输出、render

**终端端口（Console）**:
前端与循环之间的键盘接缝：循环按需请求一行或一个问题，前端回答；取消 / 模式手势主动上行；权限门与模型提问走同一条通道，因为两者都来自同一个键盘。细节见 [docs/render.md](docs/render.md)。
_Avoid_: stdin、输入流、prompt

**左栏（Sidebar）**:
TUI 左侧的**全高**一栏：身份、**页签条**与选中页的内容。**去留 = 用户意愿 × 宽度档**：用户能收起与叫回，但那只是意愿——宽度仍然决定那一栏画不画得下，而在窄档下「叫回」静默无效。页签是**点出来的、从不给键位**（`Tab` 归 `/` 菜单、`Shift+Tab` 归模式循环）；`todo` 页只在会话出现过非空列表之后才出现，此后常驻。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 侧栏、信息面板（那只是它的一个页）、面板

**输入区（InputArea）**:
主列最下面那块打字的地方，在状态行与提示行之间，高度随草稿增长、但有上下限。**它没有自己的边框**：终端就是边界。地板之间有一条优先级：**转录的最后一行比输入区的最小高度优先**。它与问卷的**输入区（Input Zone）**同名不同物：这条是屏幕上的地方，那条是键盘状态（见**选项区**）。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 输入框（听起来像带边框的控件）、命令行、prompt（那是它第一行的 `❱ `）

**提示符色相（PromptHue）**:
输入区提示符 `❱ ` 的颜色：**界面里唯一在动的东西**，而且**只在一次运行进行中时动**——轮到用户打字时它停在静止色。它是**纯渲染器状态**，不进事件流。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 光标色（那是终端的事）、主题色、语法高亮色

**脉冲（Pulse）**:
那个驱动**提示符色相**的帧计数器：由循环在**一次运行进行中时**才启动的定时器推进，运行结束**归零**；它是**纯渲染器状态**，不进事件流。空闲时那台时钟不存在——没有唤醒、没有重画；空闲时的**唯一例外**是那个有界的举手 deadline（到点就把举手作废，**两把手势共用它**）。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 动画、loading、spinner、进度条（那讲的是进度，它只讲「还在跑」）

**下落短横（FallingDash）**:
一条 `▀▀▀▀` 从上往下落、落到底再从上面出现的动画。**已退出屏幕**：真机上看下来不好看，用户决定先不动标记；代码与它自己的单元测试都留着，**别顺手接回渲染路径**。细节见 [docs/render.md](docs/render.md)。
_Avoid_: spinner（那是命令行语汇）、loading 动画

**状态行（StatusRow）**:
输入区上方那一行，内容是「模型 / 模式 / 上下文占比」三段。按宽度**先丢模型、再丢模式、最后只剩上下文占比**（仍带标签），**这一行永远在场**。三段都不可点。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 提示行（那是它下面那一行，讲键位）、顶栏（那是它取代掉的旧东西）

**回合条（TurnRail）**:
转录右缘最右 1 列的竖条，**一格 = 会话的一个单位**：交互会话数**回合**，讨论会话数**轮次**。格子贴底排（最新的最下），焦点那格**粗 + 亮**、其余细虚线 + 暗，被裁掉时最上一格 `⋮`；点一格跳到那一格单位的段首。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 轮次条（交互会话里那格是回合、讨论里才是轮次，用它会读成只数轮次）、滚动条（那是行位置，在它左边一列）

**焦点回合（FocusedTurn）**:
回合条上被高亮的那一格，含义是「**正在看的那一段**」，**不是最新那一回合**。它是**派生量**，不存状态——存下来的话，用户一滚动它就和视口漂移了。细节见 [docs/render.md](docs/render.md)。
_Avoid_: 当前回合（会被读成「最新回合」）、选中回合（会被读成一个存起来的选项）

## 安全

**沙箱（Sandbox）**:
`bash` 与动态工具跑在其下的**文件写边界**：整台机器只读挂进来，只有会话 cwd、`/tmp` 与配置里的可写根能写，区外的写由内核直接打回。**它只管文件、不管网络**，也不决定「跑不跑」——那是**权限模式**的事。它不可用时**拒绝运行**，另有一个显式关掉这层的开关。细节见 [docs/sandbox.md](docs/sandbox.md)。
_Avoid_: 容器（那是 namespace + 镜像那一整套）、隔离（太泛，而且网络不在这层）、权限模式（沙箱答「能碰到什么」，模式答「跑不跑」）

**打码（Redactor）**:
入流前的**值级、best-effort** 替换：把配置里解析出的密钥值换成 `[redacted]`，于是**流上的文本 == 模型看到的文本**，而工具执行仍拿真值；范围含消息正文与工具参数。细节见 [docs/credentials.md](docs/credentials.md)。
_Avoid_: 脱敏、掩码、mask、sanitize

## 流程

**feature 目录（`.scratch/<feature-slug>/`）**:
issue tracker 的存放单位：**一个 feature 一个目录**，它自己的 spec、决策图与票全在目录里，feature 之间不共享文件。总清单是 `.scratch/README.md`（**feature 索引**，一行一个 feature：形态、一句话、票数与完成度）。细节见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
_Avoid_: 不给它中文名（全仓库都写 feature）；项目、模块（那是代码里的东西，不是 tracker 的单位）

**seed（`seed.md`）**:
还没变成 spec 的**种子材料**：一个想法、一份意向、一段对话的折叠。目录里只有它就说明这件事还没被访谈、也没被画成图，所以里面**没有票**。细节见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
_Avoid_: 不给它中文名（一律写 `seed.md`）；草稿、草案（听着像临时文件，它长期留着当材料的来源）

**spec（`spec.md`）**:
一个 feature 的**构建计划**：一次访谈折出来的那几节决定（问题陈述、用户故事、实现与测试决定、明确的 Out of Scope）。它是实现票的**来源**——票从它拆出来，所以票不重述理由，只指回它。细节见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
_Avoid_: 不给它中文名（写 spec，不写「规格」）；需求文档、设计文档、RFC；把它当 ADR（spec 讲怎么做，ADR 只讲当初为什么）

**决策图（map，`map.md`）**:
一次 wayfinder effort 的正文：`Notes` / `Decisions so far` / `Not yet specified` 几节，外加一份 `## 任务清单`；底下的票全是**决策票**——产出是**决定，不是交付物**。图走完会被折成 spec（见**交棒**），而图**留着当决策记录**：所以 `map.md` 与 `spec.md` 可以同时存在。细节见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
_Avoid_: 路线图、roadmap（那是排期，这里的票没有时间轴）；把它当 spec 读（图里的决定还在动，spec 里的是定下来的计划）

**交棒（handoff）**:
决策图走完那一刻的动作：把图上**互相链接的决定收束成 spec**。绕开它直接开实现票，会丢掉那些决定之间的链接——所以图只有折完才算走完。细节见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
_Avoid_: `/handoff`（那是把上下文搬去新会话的技能，跟这一步无关）、交接（那是两班人之间的说法）

**票（ticket，`issues/NN-<slug>.md`）**:
tracker 的最小单位：**一票一个文件、从 `01` 编号**，永远不把所有票合成一个文件；开头那几行就是它的全部元数据（`Type:` / `Status:` / `Blocked by:` / `Part of:`）。**每票自包含**，所以做完一票就可以把它那份 context 丢掉。

收尾状态分三套：决策票 `claimed` → `resolved`，实现票 `ready-for-agent` → `done`，以及**只等人走查**的 `ready-for-walkthrough`（它与分诊标签里的 `ready-for-human` **不是一回事**）。讨论追加在文件底部的 `## Comments` 之下，决策票的答案追加在 `## Answer` 之下。细节见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
_Avoid_: issue（那是对外 tracker 的说法，本仓库的 tracker 就是 markdown）、工单、任务（`task` 是 `Type:` 的一个值，指那类要动手的决策票）

**阻塞边（`Blocked by`）**:
票顶上那一行记的依赖边：列出的每个文件都变成 `resolved` 之后这张票才解除阻塞。本地 markdown 没有原生依赖边，这条边只是**约定**。细节见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
_Avoid_: 依赖、前置（听着像代码里的依赖，这条边只决定「先做哪张票」）、blocker（那是边另一端的票，不是边本身）

**frontier**:
还没被认领的**下一批票**：扫一遍票据目录，找 open、没被阻塞、也没被认领的那些，**编号最小的当选**；`Type: implement` 一律跳过。设计票全关掉，图就算走完，哪怕实现票还开着。细节见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
_Avoid_: 不给它中文名（一律写 frontier）；队列、待办（那是**待办列表**的名单，不是这里的选择规则）

**分诊标签（triage label）**:
票顶上 `Status:` 里那套角色串，五个：`needs-triage`（等人评估）/ `needs-info`（等报告者补料）/ `ready-for-agent`（写全了，可以交给 AFK agent）/ `ready-for-human`（必须人来做）/ `wontfix`（不处理），映射记在 [docs/agents/triage-labels.md](docs/agents/triage-labels.md)。
_Avoid_: 优先级、排期（那是另一码事）；状态（`Status:` 是承载它的那一行，标签是行里的值）

## 文档

**逐面文档（`docs/*.md`）**:
`docs/` 下**一个面一篇**的现状文档：讲**现在怎么工作**，所以代码改了它就该跟着改。与 spec 的分工是**时态**——spec 是当初定下的计划（会过期，留着当记录），逐面文档是当前的样子。细节见 [docs/agents/domain.md](docs/agents/domain.md)。
_Avoid_: 参考手册、guide、wiki；把它当 spec（spec 讲当初怎么定的，它讲现在什么样）

**不可逆的决定（ADR，Architecture Decision Record）**:
`docs/adr/` 下的一条决定，一个编号一个文件，标题就是那句决定本身。只收**难回头、离开上下文会显得奇怪、当初又真有替代方案**的那种决定，所以它的重头是「为什么这么定」和被否决的替代方案——**现状不在这里**：现状在 `docs/` 的逐面文档。在决定敲定的那一刻惰性写下一条，不预先攒；文档与讨论里一律写 ADR 0003。与既有 ADR 冲突时**明着挑明**，不静默推翻。细节见 [docs/agents/domain.md](docs/agents/domain.md)。
_Avoid_: 架构决策记录（标准译名，但本仓库只收「不可逆」那一档，口径就用那个中文名）、decision log（那是清单，不是一条决定）、spec / 票 / 逐面文档（它们讲现状或怎么做，ADR 只讲当初为什么——别拿它们当同义词）
