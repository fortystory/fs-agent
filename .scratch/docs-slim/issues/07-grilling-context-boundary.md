# grilling：`CONTEXT.md` 的词条边界

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: 02

## 问题

[`CONTEXT.md`](../../../CONTEXT.md) 自称**不含实现决策**，但实测密布实现指称（`Config::debaters_share_a_vendor`、`discussion::pick_pair`、`spec §5`）。它是「每次上手读一次」的那一层，也是**术语的唯一来源** —— 所以剥过头会把「只有这里定义过」的东西弄丢。

本票要拍（按 [票 02](02-research-context-implementation-details.md) 的清点结果）：

1. **剥到哪条线**：候选
   - (a) **严格按它自己的声明**：只留「这个词是什么」加格式必需的类型名，实现细节与 `spec §` 引用全走（用 `docs/` 的链接替代）；
   - (b) **留指针**：保留「细节见 `docs/xxx.md`」这一类链接，删掉展开的实现叙述；
   - (c) **改声明**：承认它就是「术语 + 它怎么实现」，把开头那句声明改掉。
   （`/domain-modeling` 的立场是 `CONTEXT.md` 必须完全不含 implementation details —— 那是 skill 的默认，本仓库可以有自己的判断。）
2. **`spec §` 引用怎么办**：它们指向被排除在 scope 外的 `.scratch/*/spec.md`（冻结项 3）。术语表里出现 § 号，读者要跨三个目录才能落到定义 —— 换成 `docs/` 的链接，还是干脆删掉？
3. **剥离与铁律的关系**：`CONTEXT.md` 里那些「不做 / 永远不 / 一律」的硬约束（例如「模型文本按值打码」「hook 只能收紧」）算不算「实现决策」？它们是术语定义的一部分，还是该搬去 `docs/`？这是本票最需要判断的一处。
4. **它要不要也受段落 ≤500 管**：它在 scope 内，实测最长段 574。
5. **三个「格式必需」的名字在 `src/` 查无此名**（票 02 实测）：`PromptHue` / `FallingDash` / `StatusRow` 的真实形态是 `PROMPT_HUE_PER_SECOND` / `DASH_FALL` / `wording::status_row` —— 它们由 `.scratch/tui-input-pulse/spec.md` 那轮为 spec 立的概念名。**术语表里的英文槽位该对代码、还是对 spec**？这是 `CONTEXT.md` 开头「英文是代码里的标识符 / 类型名」那条规矩第一次被证伪。
6. **`CONTEXT.md` 被上游 spec 当术语落点主动加词条**：`.scratch/tui-input-pulse/spec.md`:6 明写「术语：`CONTEXT.md`（§渲染）**新增** 提示符色相 / 脉冲 / 下落短横」—— 这是**仍在生效的流程**，不只是历史残留，与本图 `尚未明确` 的「流程本身在制造胖点」是同一个病根。**本票要不要顺带定一条「新术语往哪写」的规矩**？也可以明确留给那片雾。

**依赖**：本票要读 [票 02](02-research-context-implementation-details.md)（`Status: resolved`）的清点表。它的关键输入：72 个词条里纯实现细节 **246 处指称 + 193 条描述**、格式必需 **78 处**；被剥内容 **92% 在 `docs/` 逐面或 `.scratch/*/spec.md` 已有一份**（66 个 full + 4 个 partial，2 个 none）；剥完 **14 个词条会空掉或只剩标题**（渲染节占 8 个）。

## 需要人拍板的点

- 剥到哪条线（a/b/c）。
- `spec §` 引用换成链接还是删掉。
- 硬约束句算不算术语定义。

## 作答

**结论：剥到「一句定义 + 可选 `docs/` 指针」；6 个「别处没有」的词条的独有内容搬进 `docs/`；9 处 `§N` 换成 `docs/` 链接；硬约束句全部当定义留下；`CONTEXT.md` 与其余 35 份同受单元 ≤500 管；四个「造名」槽位保留、改第 10 行的规矩；新术语的落点当场定死。**（2026-10-04 与维护者的 live exchange，七问。**Q4 与推荐不同**——维护者选了「硬约束句全留」而不是「按区分性拆开」，答案按前者落。）

### 1. 剥到哪条线 = 一句定义 + 可选指针（Q1 = b）

每个词条的目标形态：

> **中文名（English）**: <这个词是什么，一到两句>。细节见 [`docs/xxx.md`](../../../docs/xxx.md)。

删掉的是 **①235 + ②9 + ③2 = 246 处指称**与 **④193 条实现行为描述** —— 机制名（`discussion::pick_pair`、`Config::debaters_share_a_vendor`、`PULSE_FRAME`）、内部类型 / 事件变体、调用顺序、参数与阈值。**指针必须落到一份真实存在的来源**：票 02 §7.3 是按词条抽查过的落点表（讨论者 → [`docs/discussion.md`](../../../docs/discussion.md) L108-122；权限三兄弟 → [`docs/permissions.md`](../../../docs/permissions.md) L11-18 / L27-32 / L49-56；渲染 → [`docs/render.md`](../../../docs/render.md) L97-140；工具层 → [`docs/custom-tools.md`](../../../docs/custom-tools.md) / [`docs/mcp.md`](../../../docs/mcp.md) / [`docs/web.md`](../../../docs/web.md)；流程 → [`docs/agents/issue-tracker.md`](../../../docs/agents/issue-tracker.md)；安全 → [`docs/sandbox.md`](../../../docs/sandbox.md) / [`docs/credentials.md`](../../../docs/credentials.md)）。

**14 个「会空掉」的词条不许只剩标题**：票 02 §6 的 A 档（提示符色相 / 脉冲 / 下落短横 / 状态行 / 输入区 / 选项区 / 焦点回合 + 会话桶 / 翻页 / 阻塞边 / 问卷 / 询问 / 第四档权限模式 / 回合条）里，前 8 个是渲染节。spec 要求实现者给每个补**一句纯领域定义**（它是什么、读者凭什么认出它），再接指针 —— 术语表的职责是给概念立名，剥完只剩一个标题就是失职。

**与 `/domain-modeling` 的分道要写进 spec**：那条 skill 的默认立场是 `CONTEXT.md` 完全不含 implementation details；本仓库的选择是「定义 + 指针、不含机制」，两者兼容但不等同 —— 值得在 spec 里记一句，免得下一个读 skill 的会话把它当违规改回去。

### 2. 6 个「别处没有」的词条：独有内容搬进 `docs/`（Q2 = a）

| 词条 | coverage | 独有内容搬去哪 |
| --- | --- | --- |
| 问卷请求（`QuestionnaireRequest`） | **none** | [`docs/render.md`](../../../docs/render.md) 的问卷一节 |
| 问题选项（`questions::Choice`） | **none** | 同上 |
| 作答草稿（`QuestionDraft`） | partial | 同上 |
| 问卷文案（`questionnaire_*`） | partial | 同上 |
| 价目表（`PriceTable` / `Pricing`） | partial | [`docs/observability.md`](../../../docs/observability.md) |
| 落点（`LandingPoint`） | partial | [`docs/discussion.md`](../../../docs/discussion.md) |

搬完 `CONTEXT.md` 只留定义 + 指针。**不新增文件**（冻结项 2 的 scope 就是这 36 份，增补落在既有文件里）。

### 3. `spec §` 引用 = 换成 `docs/` 链接（Q3 = a）

9 处全部处理，**换完 `CONTEXT.md` 里不再出现任何 `§N`**：

- `spec §5` / `spec §15`（讨论者那条）→ [`docs/discussion.md`](../../../docs/discussion.md)（票 02 确认这两个 § 号指的是 `.scratch/fs-agent-v1/spec.md` 的 L297 / L453）。
- `spec §7`（**存疑**：两个候选 spec 都不贴合）→ **删掉**，不留链接。
- 6 处裸 `§N`（都点了路径：`sidebar-toggle/spec.md §2`、`tui-chrome/spec.md §1`、`tui-input-pulse/spec.md §2b`、`exit-gesture/spec.md §6`、`questionnaire-keys/spec.md §5` ×2）→ 各自换成对应的 `docs/` 逐面文档（渲染四处在 [`docs/render.md`](../../../docs/render.md)，其余按票 02 §7.3 的落点）。

### 4. 硬约束句全部留下（Q4 = a —— 与推荐不同）

票 02 列的「不做 / 永远不 / 一律 / 绝不」句**算术语定义的一部分，全部保留在词条里**（例：「挂起是真暂停，不是画布让位、回合继续跑」「讨论者绝不是落点」「升级不做父目录提升」）。理由由维护者拍定：这些句子划的是概念的**外延**，而 `CONTEXT.md` 是术语的唯一来源。

**可执行的边界**（Q1 与 Q4 合起来才完整）：按**句子性质**分，不按区分性分 ——

- **留**：**规范性**句子（它**应当**怎样：不做 / 一律 / 绝不 / 不是），哪怕它同时在说机制；
- **删**：**描述性**句子（它**事实上**怎么搭起来：由谁调用、参数是什么、存在哪个字段、实现在哪个文件），交给指针。

一句里两半都有的（例如升级那条的「只这一次调用、只给一次重试」+「门里给 `Ask`（一次）、粒度是声明的那个路径本身」），**留规范的那半、删描述的那半**。这条判据写进 spec，实现者不必逐句回来问。

### 5. 与其余 35 份同受单元 ≤500 管（Q5 = a）

不豁免。本轮实算它在单元口径下有 **8 项超长违规，是全 scope 最多的文件**：L110 647、L324 637、L24 622、L217 609、L114 568、L106 547、L148 539、L259 535（票面写的「最长段 574」是整块口径，与 647 的差别在清单项合并，按脚本口径走）。剥离机制后其中多数自然变短，剩下的按顶层项 / 段落拆开；**基线初值以脚本首跑实测为准**，清完显式收紧。

### 6. 四个槽位保留概念名，改第 10 行的规矩（Q6 = b）

`PromptHue` / `FallingDash` / `StatusRow`（连同 `Pulse`）**保留为槽位**；[`CONTEXT.md`](../../../CONTEXT.md) 第 10 行的规矩改成：

> 英文槽位是代码里的标识符 / 类型名；**若这个概念只在 spec 与文档里立了名、代码里没有对应实体，槽位就写那个已被文档采用的写法**。

事实附注（**不进 `CONTEXT.md` 正文**，随指针去 `docs/`）：它们的代码形态是 `PROMPT_HUE_PER_SECOND`（+ `hue` 局部量）、`DASH_FALL`（+ `identity_falling()` / `mark_lines()`）、`wording::status_row()`，由 [`.scratch/tui-input-pulse/spec.md`](../../tui-input-pulse/spec.md):6 那一轮立名。

### 7. 新术语的落点当场定死（Q7 = a）

规矩进 spec，与冻结项 8 的「同一规则两处维护」同一个判据：

> **新术语：定义进 `CONTEXT.md`（一句 + 可选指针），机制进 `docs/` 逐面。不许在 `CONTEXT.md` 里写机制。**

它治的正是 [`.scratch/tui-input-pulse/spec.md`](../../tui-input-pulse/spec.md):6 那条**仍在生效**的流程——不加这条，剥完会以同样的方式长回来。map 的 `尚未明确` 里那半条（「术语落点」）随本票毕业；**更宽的那半**（每次 feature 收尾往入口文档追加逐日经过）仍留在雾里。

### 交给 `/to-spec` 的执行要点

- **改动面**：`CONTEXT.md`（15 节 72 词条全部过一遍）+ 三份 `docs/` 的增补（[`docs/render.md`](../../../docs/render.md) 问卷四条、[`docs/observability.md`](../../../docs/observability.md) 价目表、[`docs/discussion.md`](../../../docs/discussion.md) 落点）。没有第四份。
- **不碰**：词条的中文名与格式（第 10 行的规矩只改英文槽位那半句）、`_Avoid_` 行、以及第 14 行那条「`agent` 是泛称、类型名用 `Debater` / `Executor`」的规矩。
- **增补 `docs/` 时必须重跑 [`scripts/check-language.py`](../../../scripts/check-language.py)**：`docs/observability.md` 的中文占比余量只有 **2.53 点**（按「只加不含中文的字符」算约 **163 字符**额度）、[`docs/render.md`](../../../docs/render.md) 余量 1.87 点（约 543 字符）。增补要用**中文叙述包住标识符**，别把代码清单直接贴进去。
- **验收**：`CONTEXT.md` 里 `§N` 归零、246 处指称与 193 条描述按 §1 / §4 的边界剥完、14 个空壳词条各补一句定义、6 条 coverage none/partial 的内容在 `docs/` 落地；然后跑票 05 的 `scripts/check-doc-size.py`（基线从 8 显式收紧）。
