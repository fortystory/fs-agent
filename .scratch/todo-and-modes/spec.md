# 计划从「权限模式」改成「模型自己的待办工具」；模式三档可选

Status: ready-for-agent（一次 grilling 的折叠：九个决议由用户拍定；实现票 `01`–`04`）

- **来源**：用户提出「把 plan 模式从 `ask`/`auto`/`readonly` 中删除、添加 `auto`/`readonly` 入口；把 plan 做成一个内建工具提供给大模型、大模型必须用它管理计划；在左侧加一个 `todo` 标签显示待办列表」+ 两轮问答（九个决议）。
- **它推翻一条刻意的决定**：`.scratch/fs-agent-v1/spec.md` §13 写着「进出 plan 只由用户手势，**给模型一个 `exit_plan_mode` 等于把『能不能写』交回给模型**」。本 spec 把「能不能写」还回给用户（模式三档，用户选），于是那条论证失去对象 —— 但**推翻本身要留痕**，所以票 01 必须带一条 ADR（`docs/adr/0003-*`）。
- **术语**：`CONTEXT.md` 删 **硬计划模式（Plan mode）**，新增 **待办列表（Todo）** 与 **待办工具（`todo`）**；**询问（Ask）** 那条里的 `PlanConflict` 例子要一起改（它随 plan 模式退场）。
- **落点**：`src/permissions.rs`、`src/lib.rs`、`src/cli.rs`、`src/config.rs`、`src/tools/`（新工具）、`src/agent.rs`（规则段）、`src/render/{input,tui,layout,wording}.rs`、`src/context.rs`、`docs/`、`CONTEXT.md`、`README.md`、`.scratch/fs-agent-v1/spec.md`（§12/§13 回改）。

## Problem Statement

1. **两件事被绑在一档里。** 今天的 plan 模式同时承载「权限」（只读 + 一条写豁免）与「计划的载体」（`PLAN.md` + 一条钉住的注入指令 + 一套进出/撤回/重放修补的机制，牵动 18 个文件、119 处符号）。用户想「先想清楚再动手」时，被迫接受「连写都不能写」；反过来，想要一份能看进度的计划清单，也不得不进这一档。
2. **权限那一轴被浪费了。** 四档里 `readonly` 与 `auto` 实现完整、测试齐（`tests/permission_gate.rs` 里 `Mode::Auto` 33 处、`Mode::Readonly` 4 处），但**没有任何入口**（三处组装点硬编码 `Ask`），而 `plan` 却占了唯一的运行期手势（`Shift+Tab`）。用户要的「工作区里随便干、外面要问」也是被这一条卡住的（见 `.scratch/sandbox/seed.md`）。
3. **「计划」应该是模型自己的事。** 一份待办列表是模型的工作记录，不是权限问题：它该是一个它能随时读写的工具、在界面上看得见（侧栏），而不是一个把整套权限机制卷进来的模式。

## Solution

- **权限回到三档**：`readonly` / `ask` / `auto`，`plan` 整个退场（`Mode::Plan`、`PLAN.md` 写豁免、`/plan`·`/endplan`、冲突询问、钉住指令、`ModeChange` 的撤回用途、`docs/plan-mode.md`）。入口补齐：`[permissions] mode` 配置 + `--mode` 旗标 + `Shift+Tab` 在会话内循环三档。
- **计划变成工具**：内建工具 `todo(list)`，一次提交整份列表（每项 `{content, status}`，`status ∈ pending|in_progress|completed`）。列表就活在那条 `tool_call` 的 args / result 里 —— **零 schema 改动**（`ask_user_question` 已经证明这个形状），`--continue` / `replay` / 审计 / 侧栏重算全部免费。
- **规则段加一句**（模型可见文本，英文）：开工前先立待办、每完成一项更新它。这是**引导**不是强制（用户选的那一档）。
- **侧栏加 `todo` 标签**：会话里**一旦出现过非空列表就常驻**，页里显示列表与完成计数；主会话的列表与执行者各自的列表互不干扰。

## User Stories

1. 作为用户，我想让 `readonly` / `auto` 真的能选到（配置或旗标），以便我不用被迫待在「写都问」这一档。
2. 作为用户，我想在会话里用 `Shift+Tab` 循环三档，并在状态行看到当前档，以便边干边改权限。
3. 作为用户，我想让 `Shift+Tab` 不再切一个「计划模式」，以便「权限」与「计划」在我脑子里是两件事。
4. 作为用户，我想让模型自己维护待办列表（立、改、勾），以便我看得见它打算做什么、做到哪了。
5. 作为用户，我想让那份列表在左侧一个标签里看到，而不是翻转录，以便「还剩几项」一眼可见。
6. 作为用户，我想让那个标签**一旦出现过就不再消失**，以便它不会在我眼皮底下跳来跳去。
7. 作为用户，我想让执行者也能维护它自己的列表（不显示在侧栏、但转录里看得到），以便派出去的活也有步骤记录。
8. 作为用户，我想让 `--continue` 之后的列表**还在**（从流上重算），以便第二天回来接着干。
9. 作为用户，我想让老会话仍然能打开（老流里有 `PlanMode` 注入与 `ModeChange` 撤回），以便升级不砸掉我的历史。
10. 作为维护者，我想让这次推翻有一条 ADR，以便下一个人知道为什么「计划」从权限里搬了出来。

## Implementation Decisions

### §1 模式三档与入口（票 01）

- **`Mode` 只剩三档**：`Readonly` / `Ask` / `Auto`。`stance` 里的 `(Mode::Plan, …)` 三分支、`is_plan_write`、`PLAN_FILE_NAME`、`Asker::ask_plan_conflict` 与 `PlanConflict` 全删。
- **入口三处**：`config.toml` 的 `[permissions] mode = "readonly" | "ask" | "auto"`（默认 `ask`，未知值按 `Error::UnknownModel` 那种方式在启动时报错）；`--mode <档>` 旗标覆盖配置；`Shift+Tab` 在会话内**循环** `readonly → ask → auto → readonly`（TUI 的 `BackTab`；`FrontEndEvent::TogglePlan` 改名 `CycleMode`）。
- **循环是会话内的值**：模式仍是 `Session` 的策略值、**不进事件流**（现有语义）；`--continue` 回到配置里的模式；审计靠 `PermissionDecided`（它的 `reason` 已经点名模式）。
- **循环不注入任何东西**（选择）：切档会掉前缀缓存（换 `messages` 头上的注入就等于废掉缓存），而模型在第一次被拒时从 `PermissionDecided.reason` 里就读到了原因（「mode readonly: …」）。代价是模型不会事先知道档位变了 —— 记进 ADR 的后果一节。
- **headless / `probe` 也跟配置**：仍然是「无 answerer 时 `Ask → Deny`」那条在门外兜底（`src/cli.rs` 三处组装点改成读配置）。

### §2 `todo` 工具（票 02）

- **`todo(list)`**：参数 `{ items: [ { content: string, status: "pending" | "in_progress" | "completed" } ] }`，**一次提交整份**（缺省/空数组 = 清空）。`content` 非空、`status` 限三档，超限即工具错误（模型可读）。
- **结果**：一条简短的确认文本（英文，模型可见），例如 `todo: 5 items (2 completed)`；**列表本身以 args 为准**（唯一真相源是那条 `tool_call` 的 arguments，结果只回执）。
- **`effect()` = `ReadOnly`**：它不碰工作区（`Effect` 是工作区副作用的词表）。于是它可以并发，同一条助手消息里两次并发调用由 `seq` 定序、**后落地的那条是真相**（记进测试）。
- **挂载**：主会话与执行者都挂（`with_dynamic` 那一层的组装期决定，工具表是前缀的一部分，中途不能增删）；讨论者的会话也挂（它们就是主会话的形态）。**与 `ask_user_question` 的分界**：那个需要人、所以 headless 不挂；这个不需要人，所以**三个渲染器都挂**。
- **谁的列表是谁的**：列表从**该 agent 自己**最近的 `todo` 调用重算 —— 执行者的列表不显示在侧栏（转录里有它那一行，详情覆盖层里有 args）。

### §3 规则段（票 02）

- 在 `agent::agent_identity()`（每轮都在的模型可见前缀）里加一条：开工前先立待办、每完成一项就更新、全部完成时用一次全 `completed` 的更新收尾。
- **这是缓存前缀的改动**：ADR 0001 的规矩是「进 `messages` 的一侧只增不改」，所以措辞写死在代码里、用英文，并在 ADR 0003 里记一笔。
- **不强制**（用户选的那一档）：不做首轮 `ToolChoice::Tool("todo")`、不做门层强制。理由是前者每轮多一次往返、后者会把「列表」与「权限」重新绑在一起，正是这次要解开的东西。

### §4 侧栏 `todo` 标签（票 03）

- **`Tab` 加一个 `Todo` 变体**，标签文案 `wording::TAB_TODO`（`todo`）。`draw_tab_bar` 的写死三项数组变成条件列表：**只有当「本会话出现过非空列表」时才插入它**（位置：`调用量` 右边），一旦出现**不再消失**（全完成也留着）。
- **判据**：从流派生 —— 见过一条**非 `Executor`** 说话人的 `todo` 调用且 args 里的 `items` 非空，即置位；置位是渲染器状态（不进流，`--continue` 时由重放自然重建）。
- **页内容**：一行一项 —— 状态字形（`☐` / `▸` / `✓`，措辞层常量）+ 内容；顶部或尾部一行计数 `已完成 2/5`。**不做滚动**（侧栏页面今天只有 N 行）：行数由高度阶梯给，溢出时最后一行画 `＋3 项`。
- **阶梯**：`Todo` 页与 `调用量` 页共用同一套高度阶梯（`SidebarKind` 与字段数不变）；宽度两档（40 / 28）不变 —— `调用量│todo│轨迹│文件` 在 28 列下仍放得下（15 格标签 + 分隔 + 填充）。
- **点击**：标签仍**只给鼠标**（`Tab` 归 `/` 菜单、`Shift+Tab` 现在归模式循环）；命中矩形照旧「画了什么就记什么」。

### §5 删除清单与向后兼容（票 01、04）

- **删**：`Mode::Plan` 与它的 stance、`is_plan_write`、`PLAN_FILE_NAME`、`Asker::ask_plan_conflict` 与 `PlanConflict`（含 `render/input.rs` 的 `ask_plan_conflict`、`wording` 的冲突标题/正文/候选键与 plain 的冲突问答行）、`Harness::enter_plan_mode` / `exit_plan_mode` / `plan_conflict`、`Submission::{Plan,EndPlan}`、`FrontEndEvent::TogglePlan`（改名 `CycleMode`）、`context::plan_mode_instruction` 与它的钉住/撤回/`--continue` 修补、`docs/plan-mode.md`、`tests/plan_mode.rs`（12 条用例随实现删除或改写成模式选择与循环的用例）。
- **保留（向后兼容，硬约束）**：`ContextSource::PlanMode` 与 `HistoryReason::ModeChange` **两个枚举变体必须留下** —— 老会话的流里有 `ContextInjected { source: PlanMode }` 与 `HistorySuperseded { reason: ModeChange }`，删变体会让 `--continue` 在旧会话上反序列化失败。它们只是**不再被发射**；加一条测试钉住「老事件仍可反序列化」。
- **`PLAN.md`**：文件本身不动（用户的东西），只是不再有任何豁免 —— 它在 `readonly` 下不可写、在 `ask` 下要问、在 `auto` 下随便写。

## Testing Decisions

- **模式（`tests/permission_gate.rs` / `tests/config_profiles.rs`）**：三档的 stance 表；`plan` 相关用例删除；配置解析（三档合法、未知值报错、缺省 `ask`）；`--mode` 覆盖配置；`Shift+Tab` 循环三次回到原点（TUI 层，`TestBackend` 帧 + 状态行文案）。
- **工具（新 `tests/todo.rs`）**：合法调用回执与 args 形状；`items` 缺省/空 = 清空；`content` 空、`status` 非法被拒；`effect() == ReadOnly`；主会话与执行者的表里都有它、headless 的表里也有；端到端：假 provider 起真会话，模型调 `todo`，断言流上那条 `tool_call` 有且只有一条结果，且**侧栏读到的是 args 里的列表**。
- **规则段**：`tests/` 里断言 `agent_identity()` 含那条指令（并提醒它是缓存前缀，改动要配 ADR）。
- **侧栏（`tests/render_layout.rs`）**：无 `todo` 调用时标签**不出现**；一条非空列表后出现；全完成仍常驻；执行者的调用**不**让它出现；页内容（三档字形 + 计数）；溢出那行 `＋N 项`；28 档宽度下四个标签仍放得下；点击切页仍只认标签本身的格子。
- **向后兼容**：老事件（`ContextInjected{PlanMode}`、`HistorySuperseded{ModeChange}`）仍能反序列化；用 `tests/history_replay.rs` 那种「喂一段老流」的方式钉住。
- **手工清单**：`Shift+Tab` 循环三档在真终端里的观感（状态行变化、不再有模式弹窗）、`todo` 标签出现/常驻/切换、`＋N 项` 那一行在 28 档下读起来是否清楚。

## Out of Scope

- **沙箱 / `workspace` 模式**：`.scratch/sandbox/seed.md` 那份意向不动；这次只把模式**变成可选**，不新增档位。
- **待办列表的高级形状**：依赖关系、优先级、owner、截止时间、子任务、跨会话持久、`TODO.md` 落盘、编号与点击跳转、侧栏滚动。
- **强制**：不做首轮 `tool_choice` 强制、不做门层强制。
- **权限门的其它部分**：规则代数、断路器、`.env` 家族、cwd 限制、沿委派链传播，一条不改。
- **DSH 那套 upgrade + justification 审批**：属于沙箱那条线。
