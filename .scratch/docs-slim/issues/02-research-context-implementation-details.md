# research：`CONTEXT.md` 的实现细节清点

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## Question

[`CONTEXT.md`](../../../CONTEXT.md) 开头自己立了界：

> **它不含实现决策**（那是 `docs/` 逐面文档与 `.scratch/` 的 spec 的事）

但正文里密布只跟实现对得上的东西，例如「讨论者」词条里的 `discussion::pick_pair`、`Config::debaters_share_a_vendor`、`ContextInjected { source: Persona(名字) }`、以及 `spec §5` / `spec §15` 这类引用。这份文件 348 行 / 44KB，属于本图 Notes 里「每次上手读一次」的那一层 —— 违反自己的界就是双重代价。

要查清并写下来的：

1. **逐词条清点**：`CONTEXT.md` 有 15 个小节（名字 / 参与者 / 事件与状态 / 会话存储 / 目标 / 控制 / 提问 / 上下文与技能 / 成本与预算 / 讨论 / 工具 / 渲染 / 安全 / 流程 / 文档）。每个词条里出现多少次下列四类东西，**逐条列出原文片段**：
   - **代码标识符**（`Config::…`、`discussion::…`、`ContextInjected { … }`）
   - **`spec §` 引用**（`spec §5`、`spec §15`）
   - **源文件路径**（`src/…`）
   - **实现行为描述**（不是「这个词是什么」，而是「它怎么被实现」）
2. **分两类**：哪些是 `CONTEXT.md` 自己的格式要求的（开头那段「中文名（English）」的规矩让类型名必须出现），哪些是纯粹的实现细节。这个区分是决定的前提，不要混在一起报。
3. **剥离后会怎样**：若按「只留术语定义 + 格式必需的类型名」来剥，哪些词条会**空掉**、哪些会**只剩一句话** —— 这是边界感的输入，不是判断。
4. **它与 `docs/` 的重叠**：被剥掉的内容是否在 `docs/` 逐面文档或 `.scratch/*/spec.md` 里**已经有一份**（这直接接上冻结项 4 的「只删别处已有一份的复述」）。

产物：一张按小节分的表（词条 / 四类指称各几处 / 是否格式必需 / 别处是否已有一份）。

**本票只清点，不做「剥到哪」的决定** —— 那个决定归 [票 07](07-grilling-context-boundary.md)。

## Answer

清点了 `CONTEXT.md` 15 节 / 72 个词条的实现指称；完整 findings（逐词条四类片段表、格式必需 vs 纯实现细节的分类、剥离三档、别处 coverage 与抽查路径）在 [`../research/02-context-implementation-details.md`](../research/02-context-implementation-details.md)。

关键数字：① 代码标识符 **235 处**、② spec 节号引用 **9 处**（`spec §N` 3 + 裸 `§N` 6；3 处 `spec §N` 都没写是哪个 spec —— §5 / §15 可确定为 `.scratch/fs-agent-v1/spec.md`，§7 存疑；裸 `§N` 6 处则都点了路径）、③ `src/` 路径 **2 处**、④ 实现行为描述 **193 条**。词条标题槽位的英文名（格式必需）**78 处**；其余 **246 处指称 + 193 条描述**属纯实现细节。

别处 coverage：**66 个 full**（`docs/` 逐面或 `.scratch/*/spec.md` 已有一份）、**4 个 partial**（作答草稿 / 问卷文案 / 价目表 / 落点）、**2 个 none**（问卷请求 `QuestionnaireRequest`、问题选项 `questions::Choice` —— `docs/` 与 `.scratch/*/spec.md` 均无，只在 `src/`）。「只删别处已有一份的复述」能覆盖 92% 的词条。

剥离后果（输入，不是判断）：**14 个词条会空掉或只剩标题**（渲染节占 8 个：PromptHue / Pulse / FallingDash / StatusRow / Input Zone / Options Zone / FocusedTurn / TurnRail），约 **22 个只剩一句话**。

给票 07 的两条事实：**(1)** `PromptHue` / `FallingDash` / `StatusRow` 三个「格式必需」槽位在 `src/` 里查无此名（真实形态是 `PROMPT_HUE_PER_SECOND` / `DASH_FALL` / `wording::status_row`），它们是 `.scratch/tui-input-pulse/spec.md` 那一轮为 spec 立的概念名；**(2)** 该 spec 第 6 行明写「术语：`CONTEXT.md`（§渲染）新增 提示符色相 / 脉冲 / 下落短横」—— spec 把 `CONTEXT.md` 当术语落点，这是仍在生效的流程，不只是历史残留。

本票只清点，未做「剥到哪」的决定；未改 `CONTEXT.md` / `docs/` / `map.md`。
