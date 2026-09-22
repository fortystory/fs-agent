# fs-agent：自用 · 可扩展核心 · 多 agent 讨论（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 `docs/agents/issue-tracker.md`
前身：`.scratch/v1-architecture/`（**已封存**，作为证据源；其目的地建立在「极简」上，已被用户推翻）

## Destination

一份 **spec-ready 的边界级 Rust 架构决策集**，交给 `/to-spec` 折叠成可建计划。

**判据**：`自用` ∧ `核心闭环 + 可扩展接缝`。原判据里的「**极简**」已由用户去掉 —— 凡是自用场景真正需要的就做，不再用「极简」预先排除。

**核心特征**：多个 agent 在**同一会话中轮次化讨论**（**异构**模型），并**可派出子 agent 执行任务**。

本图产出**两组接缝**：fs-agent 的核心接缝（票 01–07，从旧图承继并按新判据适配），加上多 agent 引入的新接缝与因判据变化而**重开**的条目（票 15–21）。

## Notes

**✅ 本图已完成（2026-09-13）**：**25 张票（含 5 张 research）全部 `resolved`**，`Not yet specified` 为空 ⇒ **没有剩下的决定，路线已 clear**。下一步是 **`/to-spec`**——把这份决策集折叠成可建计划；`/to-tickets` 与 `/implement` 在它之后。**本图本身不执行任何事**（见 `Out of scope`）。

> **审计（2026-09-13，三片并行）**：修掉 **24 处**正文与交接块不一致 / 术语别名，并收口 **5 处 schema 缺口**——其中唯一的**新决定**是补上「**四个内置权限模式**」的语义（票 20 §1b：`readonly` / `ask` / `auto` / `plan`，此前被五个票使用却从未被定义）。**元发现**：这 24 处里约一半的根因是「先写答案、后来由别的票修正它，但**没回改正文**」这个写法本身——用这种方式建的图，票数一多必然开始自相矛盾。**有意留下的别名**（不会让人读错，故未清）：票 16 §2 标题的「聚合器」、票 14/22 的「子 agent / subagent」。

- **领域**：`fs-agent` —— 自用 coding agent CLI，Rust，从零实现。
- **设计输入**：
  - `docs/research/coding-agent-features.md` —— 940 行综述，10 个 coding agent。**警告：它的多 agent 章节讲的是「隔离」架构，与本图目的地相反，不能当依据用。**
  - `.scratch/v1-architecture/` —— 封存的前身图。它的 ratify 记录是「哪些砍掉项是被『极简』砍的、哪些是被证据砍的」的**主要来源**。
  - `.scratch/multi-agent-architecture/research/01-debate-conformity-and-speaker-attribution.md`（731 行）
  - `.scratch/multi-agent-architecture/research/02-provider-call-surface.md`（761 行，Kimi/DeepSeek 逐字段一手参考）
- **已退休的票**：旧图的 **08（research：Rust 生态）已解**，结论并入本 Notes 的「Rust 生态事实」，不再单独开票。**本图编号在 08 处有一个空洞**，旧编号全部保留，以防票内交叉引用断裂。
- **原样承继、未改写的票**：03、04、09、10、11、12。其余（01、02、05、06、07、13、14）因判据或目的地变化已改写。

### 多 agent：已定约束（Chart 期定下）

- **介质 = 共享只追加事件流 + 每个 agent 自己的窗口**。事件流是唯一真相源；每个 agent 的 `messages` 是它的一次**投影**。身份（system prompt）**不进流**，永远私有。
- **「轮次化」不是设计选择，是 API 的物理性质**：每次调用无状态，A 说话时 B 的那次调用还没发生。所以「实时相互讨论」不可能，只能每轮把发言重放给每个人。
- **轮次结构 = 独立首轮 → 揭示 → 定向第二轮 → 合成**。第 1 轮各自独立作答（互不可见，保住多样性）；**只在结论冲突处**开第二轮（把调用数压到接近最小，并回避「无差别多轮共享」这个从众主因）。
- **讨论者 = 2 个，异构**：KIMI + DeepSeek。**不允许加同模型的第三个角色** —— 见下条。
- **多样性只能靠异构模型**。Kimi 的 `temperature`/`top_p` **不能传**（传了报错）、`top_k`/`seed` 两家都没有、penalties 在 Kimi 报错 / 在 DeepSeek 静默无效。所以「用采样参数造多样性」不可用。
- **归属编码 = 「合并 + 正文前缀」为基线，`name` 只作增强**。理由：**两家对「连续多条同 role」的行为都未文档化**，不能把 `name` 当唯一保障；而主动合并顺带对齐 Anthropic 的**已文档化**行为，一份投影跨三家可用。
- **讨论的目的 = 探索 / 发散**（用户选）。**由此产生一个未解的张力**：合成/聚合是收敛动作，与「要多样观点」相反 → 见 `Not yet specified`。

### Provider 硬事实（2026-09-12；逐字段细节见 `research/02`）

- **`n` 两家都不能用**（Kimi 固定 1，`n>1` → 400；DeepSeek 文档无此字段）→ 独立首轮**必然 = N 次调用**，所以**异构不额外花钱**，异构严格优于同构。
- **两家都显式 stateless**（Kimi Responses 的 `store`/`previous_response_id`/`conversation` 恒 false/null；无 Assistants/Threads）→ **状态必须 fs-agent 全量持有、每次重放**。
- **没有 `seed`** → 不可复现 → e2e 验收**必须**用 mock provider（旧图切片 9）。
- **Kimi 限流 = 并发 + RPM/TPM/TPD**（按充值分档）；DeepSeek 只有并发 2500/500 → **N agent × 多轮 × 递增上下文会撞 Kimi 的 TPD**。
- `parallel_tool_calls` 在 DeepSeek 是 **Ignored（恒开）** → **写串行化只能由 harness 做**。
- **usage chunk 形状不同**（Kimi 单独发 `choices: []` 统计 chunk；DeepSeek 搭在最后一个 content chunk，且 `stream_options` 与 `stream:false` 同用 → **400**）。
- **错误码不同**：欠费 Kimi **429 quota** / DeepSeek **402** → 适配器不能统一按 429 处理。
- **Kimi OpenAPI 漏声明 `tool_calls[].index`，但 guide 依赖它** → Rust 结构体**必须给 `index` 留位**。
- **Kimi 平台 key 域隔离**：`platform.kimi.ai` 的 key 与其他区域站混用返回 **401** → `config.toml` 里 **`base_url` 必须与 key 来源同域**。
- `tool_choice: required` 只有 `kimi-k3` 支持（K2.6 / K2.7-code 传了报错）；DeepSeek thinking 下 `required` 与具名选择 **400**。

### Rust 生态事实（来自已解票 08）

- **`async fn` in trait 不 dyn-compatible**（Rust Reference 明确）→ `Provider` / `Tool` **不能直接 `dyn`**；替代：`async-trait` / `trait-variant` / `dynosaur` / 泛型单态化。
- **hook 挂载点的惯用表达是「trait + typed enum」，不是 channel**；`rig-agent` 的 `AgentHook` 是最近先例。
- **JSONL 无官方 API**；`std` 的 `OpenOptions::append` **不保证**跨线程追加不交错。
- 产物：`.scratch/v1-architecture/research/08-rust-ecosystem.md`。时点提醒：调研日 Rust stable 已是 **1.98.1**，本机 `rustc` 是 **1.94.0**。

### 判据变更的后果：砍掉项分两类

- **证据型 → 保持砍掉**：AST / tree-sitter 编辑、unified diff 编辑格式、原生多 provider 协议、MCP client、SWE-bench 跑分。理由与被去掉的「极简」无关（零采用 / 只有一个实现用 / 一个 OpenAI-compatible 覆盖 90%，而你的两家都兼容）。
- **极简型 → 重开**：OS 沙箱、权限 DSL、可观测性、成本上限与模型路由、会话树、秘密打码、todo 半边。**其中成本上限与可观测性已被新事实顶开**（Kimi TPD 的乘法增长；N agent × 多轮产生读不动的 transcript）。
- **不在被砍之列的相邻项**：硬 plan 模式、每次编辑自动 git commit + `/undo`、完整 transcript 落盘、system prompt 前缀稳定。

### 其他约定

- **术语格式 = 「中文名（English）」**（2026-09-13 定）：`CONTEXT.md` 的正式用词是**中文**，英文是**代码里的标识符 / 类型名**。所以叙述里写 **讨论者 / 执行者 / 发言归属 / 合成器 / 事件 / 事件流 / 投影 / 会话 / 轮次 / 回合**，而代码里写 **`Debater` / `Executor` / `SpeakerId` / `EventLog` / `Session` / `Round` / `Turn`**——两者是同一个概念，不是互为别名。**票里的英文标识符不算违反这条**（它们描述的是代码）。
- **架构深度 = 边界级**：crate/模块划分 + 模块边界 + 关键 trait 签名。不要定到逐个公开函数签名，更不要定文件名与函数职责。
- **配置**：`~/.config/fs-agent/config.toml`（TOML）；**不**自动加载项目 `.env`；优先级 **`config.toml` > 已导出 env > 内置默认**。
- **每张票的答案必须自足**：`/implement` 会在 `/clear` 之后的新会话里读它，看不到本图的对话。
- **扩展点：将来要加"参与者"时，加到哪、代价是什么**（2026-09-13 记，是既有决定的**推论**，不是新决定）：
  - **再一个讨论者**（比如第三个模型）= **又一个 `Debater(AgentId)`**，因为 `AgentId` 是 id 不是枚举（票 15）⇒ **零 schema 改动**。代价只有三处：配置、重算成本式（`N + (冲突 ? N : 0) + 1`，票 18 的 3/5 是 N=2 代入后的值）、把"两行结论"的比较推广成 N 行（"全部归一化后相等或互为子串 ⇒ 无分歧"）。⚠️ **并且必须重开票 16 的"N=2 不裁决"**——那条的依据是"N = 2 没有多数投票"，N ≥ 3 时依据失效（不是错，是依据消失）。
  - **一个裁判**（票 16 已用证据否决过一次：MT-Bench 位置一致性 GPT-4 65.0% / Claude-v1 23.8%、75% 偏向第一个；Anthropic 自己发现多裁判更差）——**若将来真要做，它不需要新的参与者种类**：它的形状就是**合成器那种"单发、非 agent、不参与轮次、读发言产出结论"的调用**。⚠️ 但选它之前要先补一个洞：**非 agent 的产出落在哪条事件上**（合成器已经撞上这个毛刺——枚举里没有它的家）。
  - ⇒ **给这两者预留枚举空位是纯亏**：空变体没有语义（投影的穷尽 match 要为它写一个没东西可判的分支），而将来真要加时的成本**与现在完全相同**。这与票 04/11/14/17/18/21 反复立的那条（"前向兼容靠**机制**，不靠预留字段/抽象"）一致。
  - 术语提醒：`裁判` 现在是 `合成器` 的 `_Avoid_`（票 16 登记）；真要引入裁判，名字得另取（例如 **仲裁者 / Arbiter**）。
- **本图只产决策。**「做」发生在 `/to-spec` → `/to-tickets` → `/implement`。那种「干脆直接写起来」的冲动，是走到地图边缘的信号。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里，本文件不复述。按名字引用，不写裸编号。 -->

- [research：多 agent 编排框架的轮次管理与终止条件](issues/22-research-orchestration-frameworks.md): 详情在 `research/03-orchestration-frameworks-turn-management.md`。**最重要的一条是核实出来的缺失**——「只在结论不一致时才继续」在任何被查框架里都**没有**内建机制，最接近的 5 个官方构造全是「失败 / 停滞才继续」，所以用户的「定向第二轮」是**设计而非选型**。另有三条直接喂票 16 / 17：AutoGen 0.4+ 的终止条件是**对增量消息求值的可组合一等对象**（与事件流天然吻合）；`selector_func` 返回 `None` 的语义在 0.2 与 0.4+ **相反**；AutoGen 把上下文策略做成**视图级**，印证投影设计。裁判偏见有硬数字（位置一致性 65.0% / Claude-v1 23.8%），Anthropic 官方实践是**单次调用 + rubric**，且发现多裁判更差。
- [Crate 与模块布局](issues/01-crate-and-module-layout.md): 单 crate（`main.rs` 薄 + `lib.rs` 公开，组装入口注入 provider，为 mock provider e2e 留门）；**12 个顶层边界** events/config/provider/tools/permissions/hooks/context/agent/discussion/session/render/cli，依赖只沿 DAG 向下、`events` 零内部依赖、投影是 `provider::projection` 子模块、`discussion` 不碰 provider；`Session` 是唯一可变状态，Executor = 带 `parent_id` 的子 Session + 独立预算 + 向父流追加事件；`EventLog` 追加自加锁。词汇落进根目录 `CONTEXT.md`（新建）。

- [Provider trait、多模型、能力表与流式 chunk](issues/02-provider-trait-multi-model-capability-table.md): `async-trait` + `Box<dyn Provider>`（dyn 是为注入 e2e 的 mock，不是为多 provider——Kimi/DeepSeek 是同一 client 的两个 profile）；trait **只流式**，输入是已投影的 `ChatRequest`，事件含 `TextDelta`/`ReasoningDelta`/`ToolCall{Started,Completed}`/`Usage`/`Finished`——**tool_call 分片拼接与两家 usage 形状由 adapter 归一**，上层看不到 `index` 缺口；`reasoning_content` 必须建模并回放（DeepSeek 带 tools 不回传即 400）；配置两级 `[providers.*]`(base_url+key) → `[models.*]`(引用+覆盖)，结构上堵死 Kimi 跨域 401；能力表按 `model id` 建、**只建模两家**、`#[non_exhaustive]` + 未登记即报错、**adapter 是执行点**（丢弃用户显式参数时告警）；默认 **100 Turn**；终止**自己算**（最后一条 assistant 含 tool_call 才继续，pending 是对 `EventLog` 的查询而非隐藏状态，`FinishReason` 只作诊断、结束只认 `[DONE]`）；`ProviderError` 六分类（`QuotaExhausted` 与 `RateLimited` 必须分开：Kimi 429 / DeepSeek 402）+ adapter 拥有传输级重试；`prompt_cache_key` = 会话 id、不发 `user_id`。

- [工具 trait、注册表、副作用标记与写串行化](issues/03-tool-trait-registry-side-effects.md): `Tool` = `async-trait` + `Box<dyn Tool>`，签名 `spec()` / `effect(args) → {ReadOnly|WritePaths|Exclusive}` / `async call(ctx, serde_json::Value) → Result<ToolOutput, ToolError>`；args 类型擦除是为票 11 的动态工具；内建集合 `builtin()` 是**函数**、注册表是**运行期值**（无全局 static、随 Session 走）、`register` 是票 11 的挂载点；**v1 串行但调度器此刻就按 `effect` 分流**（模型一定会批量发 tool call，所以并行只读是接线而非重构）；`read-before-edit` 与 **per-path 写互斥**都在工具层 dispatch 集中强制——read set 归 Session、锁表是组装期注入的 `PathLocks`（跨 Executor 有效，每 Session 一份等于没锁）；`Result` + **循环一处保证每个 tool_call 恰好一条结果**。

- [编辑策略的可插拔点与降级匹配分层](issues/04-edit-strategy-fallback-layers.md): 线级契约 `edit_file(file_path 绝对路径, old_string, new_string, replace_all?)` / `write_file`；**匹配梯**是 `tools` 内部的有序 `Matcher` 列表（first-success-wins，非新顶层边界）——v1 三层 exact → 行尾空白无关 → 每行 trim，**命中级别必须报出**否则降级是静默的；三样护栏都进 v1（唯一性强制、匹配过大拒绝、占位符拒绝且判据是**注释形状短语**而非裸 `...`，免伤 Rust 的 `..`/`..=`）；编辑格式 v1 固定 search-replace、**不加 `edit_format` 预留字段**（换格式的挂点用既有的"注册表在组装期按 profile 构造"）；**匹配全线失败即失效该路径的 read set**（读→写间隙无锁，这是不加内容哈希也拿到的同等安全性）。

- [上下文预算接缝、工具输出截断与丢弃的分界](issues/06-context-budget-seam.md): 公式 `usable_input(agent) = caps.context_window − min(20_000, caps.max_output_tokens)` **住 `context`**、按 agent 各自模型算、无"全局预算"；`messages(agent) = trim(project(log, speaker, caps), budget, policy)`——**投影只做归属，裁剪是投影之后的另一个纯函数**（票 07 的"可重算"不变量因此成立）；两个机制不同时刻不同数据：**单次结果截断在入流前**（超限落盘 + 事件里只放预览与指针）、**超预算丢弃在 `trim` 里且只读**（日志一条不删），丢弃顺序 工具结果 → 整轮 → 该 Turn 硬失败；**"当前预算"不是状态是函数 → 窗口层无需加锁**；会话累计是从 `Usage` 事件求和的派生值、**TPD 归票 18**；`AGENTS.md`+skills 注入为**第一条 user message**（`System` 身份 → `User` 规则 → 历史）、**逐轮不变以保前缀缓存、永不被裁剪、且记成流上一条事件**（否则 `project` 不再是"流+规则"的函数）；v1 = 字符/4 估 token + 粗暴丢弃，**compaction 后置且不返工**（前提：`trim` 独立纯函数 + 摘要记成事件）。

- [skills：渐进披露的指令包](issues/09-skills-progressive-disclosure.md): skills 是 `context` 的子模块；**全文加载靠一个内建只读工具 `skill(name)`**（产物是流上的工具结果——单一收口，可记账/可计量/可被权限语言覆盖；追加在尾部所以不破坏前缀缓存），`disable-model-invocation` 的既不进清单也拒绝加载；发现路径 **项目 + 用户两级、各三处**（`.fs-agent` > `.agents` > `.claude`，项目 > 用户），因为本机这两套库已真实存在且 38 个同名、不跟随就全部作废；边界规则 = **>80% 每轮都要 → `AGENTS.md`；按任务/长 → skill；必须无条件执行 → hook**；预算 **单 skill 5k / 已加载总 25k / 描述清单独立封顶 3k**，并**细化票 06 的丢弃顺序插入"老 skill body"一类**（比普通工具结果更黏）；"已调用集合"可重算 → 将来重注入不返工；**v1 的 skills 只装指令，把工具打包进 skill 留票 11**。

- [research：tree-sitter 的 Rust 绑定与符号抽取 / 图排序的实现代价](issues/23-research-tree-sitter-rust.md): 详情在 `research/04-tree-sitter-and-symbol-extraction.md`（593 行）。三条直接喂票 12：**代价低**（`tree-sitter` + `tree-sitter-rust` 合并依赖闭包 **9 包**；本机实测 237 文件 / 3.97MB / 120k 行 → 解析 395–466ms、tags 226–228ms、峰值内存 ≈133MiB；官方单文件 6.48ms、增量 <1ms）；**`tags.scm` 只有名字**（无签名、无作用域捕获）；**图排序没有现成构件**（petgraph 的 `page_rank` 无 edge weight、无 personalization，而 aider 是带权 + 个性化的整图 PageRank）。另：`tree-sitter 0.27.0` 的 MSRV 跳到 1.90 / edition 2024；aider `--map-tokens` 文档说 1k、源码 `clamp(输入/8, 1024, 4096)`、`map_mul_no_files=8`。第 6 节列了 14 条 ⚪ 缺口（含"官方无整仓解析时间/内存数字"）。
- [repo map / 符号检索如何进入上下文预算](issues/12-repo-map-symbol-search.md): **范围 = 只做符号检索，向量检索判为 out of scope**；**做成内建只读工具 `repo_map(focus?)`**（与票 09 的 `skill(name)` 同形：产物是工具结果、`effect()` = `ReadOnly`、落进票 06 的截断/丢弃），**不注入**——注入版"脏了就刷新"会让 map 之后的整段历史反复掉出缓存前缀，与票 06 的硬约束相抵；**固定预算 1k、可配上限 4k、不接受模型传 `tokens`**（aider 动态伸缩的前提"没有焦点只能全给"在带上下文的调用下不成立）；抽取 = tree-sitter + 官方 `tags.scm`（**只有名字，v1 不做签名渲染**；增量解析后置）；排序 = **朴素可解释的纯函数**（会话相关度为主 + 结构信号为辅），**不做整图 PageRank**（petgraph 缺边权与 personalization），`rank()` 留作可替换的纯函数接缝。

- [research：事件 schema 的三个一手先例（OpenHands / Cline / Claude Code）](issues/24-research-event-schema-precedents.md): 详情在 `research/05-event-schema-precedents.md`（401 行）。三条直接喂票 15：**OpenHands 的信封**（`id` + `timestamp` + `source: agent|user|environment|hook` + `parent_id` 对话树父，`kind` = 类名，`frozen=True`，`append` 拒绝重复 id 与不存在的父）与它的 **21 个事件类**；**增量不进日志有明文**（`StreamingDeltaEvent` docstring："Not persisted to the conversation event log … deltas are a UX affordance, not part of the durable conversation record"）；**Cline 的 `AgentFinishReason` 五值**逐字（挂在 `AgentDoneEvent.reason`，且只有 `mistake_limit` 有文档化触发条件：连续可恢复错误达 `maxConsecutiveMistakes`，默认 6）。另：Claude Code 的 JSONL 官方声明格式 **internal、随版本变**，`--continue`/`--resume` 追加同一 session 而 `--fork-session` 复制到新 id。⚪ 缺口含 Claude Code 的完整 schema、Cline 增量是否落盘、`/rewind` 的记录层实现。
- [事件流骨架与事件 schema](issues/15-event-stream-backbone.md): **粒度 = 完成单元**（一条消息 / 一次工具调用 / 一条结果 / 一次裁决 / 一个轮次边界），**增量文本不进日志**（走传输层旁路直达渲染；OpenHands 明文同此）；**信封** = `{ seq, at, speaker_id, payload }`——`seq` 是**唯一身份**（= JSONL 行号，不引入 ULID），**不带 `parent`**（线性日志；分支 = 新会话复制前缀，归票 07），**不带任何可见性字段**（可见性是投影规则的输出；写进事件就等于让事件携带协议状态、破坏 `project` 的纯函数性）；`speaker_id` **必填**，域 = `Debater(id) | Executor(id) | User | System`（`ContextInjected` 挂 `User`；归属绝不从 provider 的 `role` 反推）；**一条总线 + 一个 payload 枚举（20 个变体，按「谁在跑」分层）**，消费者靠**过滤**取用，其中 **hook 只拿到一个封闭的 7 变体公开子集**（工具 + 权限 + 会话边界——因为 hook 是"只能收紧"的**策略**机制，观察面归票 19）；**只追加、永不截断**，重新生成 / 撤销 / compaction 一律是 `HistorySuperseded { targets, reason, summary? }`（票 06 交接的两件事因此同一形状）；**持久化** = 一会话一 JSONL + **单一写入者**（绕开 `std` 的 append 不保证不交错）+ 每条 flush 不 fsync + 丢末行不完整记录 + 版本在 `SessionStarted` 且不承诺兼容；**"哪些事件进模型上下文" = `project()` 旁一张穷尽 match 表**（不放 payload 上，DAG 才不斜）；**`StopReason` 一个枚举被三层循环共用**（Cline 五类 + 讨论三类，挂在各自的结束事件上）。

- [事件接缝与权限门的控制流次序](issues/05-event-seam-permission-ordering-prototype.md): **`agent` 层的循环拥有控制流**，hook 与权限门都只是它按次序调用的**纯值变换**（次序 `hook.pre → 权限门 → [询问] → dispatch → hook.post`，最后一步由循环追加事件）。**"hook 只能收紧、不能放松"是代数性质而非运行时检查**：hook.pre 的输出是**约束**（`Continue | Rewrite(args) | Tighten(Ask|Deny) | Skip | Stop`），门产出裁决，生效裁决 = 两者在决策格 `Allow < Ask < Deny` 上**取上确界**——类型里根本没有"放松"这个变体。**裁决封闭三态**（`Allow|Ask|Deny` + `why`），**打开的是门的输入侧**（模式 / 黑名单 / 路径限制 / 规则表达式 / 委派链继承 → 全归票 20）；"总是允许"是用户回答**改了策略**，不是第四态。**PostToolUse 回灌 = 一条追加的 `HookExecuted` 事件**，由**投影合并**进那一条 tool 消息（provider 只允许一个 `tool_call` 对一条 tool 消息，所以合并是被迫的）；可见性**跟着它所评注的那条工具结果走**。**失败语义不对称**：`hook.pre` 失败/超时 → **fail-closed 阻止动作** + 诊断 + 合成错误结果；`hook.post` 失败 → **只能丢反馈**（世界已改变，fail-closed 在 post 侧买不到安全）。**hook.pre 在询问之前**：能**阻止**询问发生（收紧到 deny），永远不能**绕过**询问；v1 不另设 `PermissionRequest` 挂载点。**权限拒绝的产物由循环合成**（门是纯函数、工具没被调用），四条异常路径（权限拒绝/用户拒绝/hook 跳过/pre 失败）各合成一条结果，"每个 tool_call 恰好一条结果"因此恒成立。原型：`prototype/05-hook-permission-ordering.html`（7 个剧本，headless 验证通过；落点偏离分支约定，见票内说明）。

- [会话存储形状、多 agent 拓扑与 `--continue`](issues/07-session-store-topology.md): 数据落 **`~/.local/share/fs-agent/sessions/<cwd-slug>/<session-id>/`**，内含 `log.jsonl`（票 15 的流）+ `outputs/`（票 06 的落盘）——**会话是一个目录、一个可搬运单元**（票面两处机械修正：会话是目录不是单文件；落盘名用 `tool_call_id` 而不是 `seq`，因为 `seq` 在追加时才分配而落盘在入流前）。**会话与 cwd 绑定**（slug 只用于分桶，权威 `cwd` 在 `SessionStarted` 里），`--continue` 只扫本目录桶、取 **mtime 最新**，**不维护全局索引**；**会话 id = `<UTC 时间戳>-<短随机后缀>`，`--continue` 后绝不改变**（票 02 的 `prompt_cache_key` 靠它保住前缀缓存）。**拓扑 = 执行者的事件在同一个流里**（`ExecutorSpawned{executor_id,parent}` + `SpeakerId::Executor`；`CONTEXT.md` 的"嵌套 Session"是内存里的值结构，不是文件布局），存储层**不做可见性裁剪**。**恢复撞上悬空 `tool_call` → 补一条合成的失败结果**（`ok:false, error:"会话中断于此，结果未知"`；不丢弃、不重执行），**不追加第二个 `SessionStarted`**。**不变量精确措辞 = `流 + 落盘文件 + 投影规则 → messages`**，且**投影不读落盘文件**（保持纯函数）；指针失效**降级为预览、绝不失败**，模型要全文自己 `read_file`（不需要新工具）。**`/undo` 不碰用户的 git**：编辑工具把**实际被替换区段的旧内容**（不是 `old_string`——票 04 的降级梯会命中不同文本）落到 `outputs/<tool_call_id>.before`，`/undo` 按命名约定找回那次 `edit_file` 调用并写回；**不做"每次编辑自动 git commit"**（会裹走用户未提交的改动、且在非 git 目录失效）。明确不做：fork/rewind 手势、shadow git、快照系统、完整 event-sourcing、SQLite、全局索引、自动清理。

- [硬 plan 模式：用权限档位实现，不新增状态机](issues/10-hard-plan-mode-via-permissions.md): **确认"不需要新状态机"，并给出精确判据**——**plan 模式 = 策略里的一条预设：「`effect()` 不是 `ReadOnly` 的调用一律 `Deny`」**，直接落在票 03 的 `SideEffect` 上（零新机制）；是 `Deny` 不是 `Ask`（本票标题就是"硬"），连 `bash` 也拒（`Exclusive`，shell 里能写文件）。否掉 goose 的"工具全关"（正是"模式 ≠ 工具"要避免的形状，且让模型连读都不能读）与"用 hook 实现"（hook 是用户扩展点，且失败是 fail-closed）。**进出只用手势**（`/plan` / `/endplan` / Shift+Tab，**不进工具面**——给模型一个 `exit_plan_mode` 等于把"能不能写"交回给模型）。**计划落项目根 `PLAN.md`**，**对它的写入是模式里唯一的豁免**（形状必须是"`WritePaths` 的**全部**路径都是它"，否则借道；`Exclusive` 不可豁免）；否掉 goose 的隐藏 nudge 与 OpenHands 的"预设 agent"（后者是第三个身份，本图只有 `Debater`/`Executor`）。**compaction 后不重新注入**——票 06 的钉子已经保证那条指向文件的短指令不被裁剪，而注入全文会占每轮预算且**每次改计划都废掉前缀缓存**。**`/plan` 撞上已存在的 `PLAN.md` → 先问用户（覆盖 / 追加 / 保留）**（覆盖时由 CLI 清空，因为票 03 的 read-before-write 会拒写已存在文件；工具绝不擅自删用户的文件）。**模式不进事件流**，`--continue` 回到配置值（模式是 Session 的策略值，塞进流会与 `config.toml` 两个真相源；审计靠 `PermissionDecided.why`）。**一条硬推论：plan 模式的拒绝必须沿委派链继承**——否则"派执行者去写"就是绕过它的最短路径。

- [动态工具注册与「只读」标记的可信度策略](issues/11-dynamic-tool-registration-trust.md): **票面的"能不能被信任"被取消**——动态工具的声明语法里**没有副作用类别这个字段**（只声明"一条命令 + 参数 schema"），所以**"谎报只读"没有地方可说**；`effect()` **恒为 `Exclusive`**（走既有的最保守路径，并行判定**不需要为它写特例**）。代价写明：真正只读的动态工具也全局串行，且 **`read-before-edit` 覆盖不到它**（`WritePaths` 未知）——所以约束落在**权限门**上。**声明复用同一套 `ToolSpec`**（JSON Schema 原样就是 provider 的线级形状，零翻译），住在 `~/.config/fs-agent/config.toml`；**执行形态 = argv 数组、不经 shell**，参数按**整个 argv 元素**替换（缺席则省略元素；数组/对象 JSON 序列化成一个元素，**不展开**）——这直接消灭 shell 注入，并**消掉了票面第 3 条那个问题**（tree-sitter 解析命令是防"模型拼出危险命令"，而命令是用户声明的、固定的；它只对内建 `bash` 适用，归票 20）。仍然过权限门，按**声明名**匹配；**必须有超时与进程树终止**。**命名空间现在就定：`custom__<ns>__<tool>`**——内建名永不含 `__`，于是"有 `__` ⟺ 自定义工具"是**词法可判定的谓词**（规则/渲染/审计都能一眼分源）。**可见性 = 启动即全局可见、组装期固定**（否决"挂在 skill 上"：`tools` 数组属于**前缀**，中途增删会废掉前缀缓存；skill body 追加在尾部才安全）。明确不做：MCP client 本身。

- [research：TUI 库与语法高亮库的选型事实](issues/25-research-tui-and-highlighting-crates.md): 详情在 `research/06-tui-and-highlighting-crates.md`（712 行）。四条直接喂票 13：**`ratatui 0.30.2`**（2026-06-19，MSRV 1.88，默认特性含 `crossterm`；后端特性 `crossterm`/`termion`/`termwiz`/`termina`），实测闭包 **默认 70 / 关默认+crossterm 62 / +termion 45 / +termwiz 118**；**`tui-rs` 已废弃**（README "no longer maintained" + 仓库 Public archive，硬证据），`cursive` 仓库活着但无维护声明（⚪）；**官方异步形状 = 同一个 async 任务里 `select!(tick, EventStream)` 并 `draw`**（crossterm 的 `EventStream` 内部自建线程）；**inline viewport 具备**（`Viewport::Inline` + `insert_before`，且 `init_with_options` 只开 raw mode、不进 alt screen）。高亮：**`tree-sitter-highlight 0.27.0` 要求 `tree-sitter ^0.27`（与票 12 同版本）、闭包仅 15，但只有 `HtmlRenderer`**；`syntect` 闭包 42/44 且默认路径要 `cc` 编译 Oniguruma（官方自承构建困难）。终端判定用 `std::io::IsTerminal`（crossterm 的 `IsTty` 在未发布 master 上已删）；**alt screen 探测 API 不存在**（⚪）。第 6 节列了 14 条 ⚪。
- [渲染接缝：结构化事件 + plain / TUI / headless 三消费者](issues/13-render-seam-consumers.md): **同进程，渲染器是启动时选定的一个**（三个模式互斥 ⇒ 一个 trait 三个实现，不是并发订阅者）；否决两进程（自用单前端；IPC 整条流会逼票 15 的"单一写入者 + `seq` 唯一身份"重做；TUI 崩了不丢会话由票 07 的 `--continue` 满足）。**消费者接口**：每种模式一个 `tokio::spawn` 的任务消费**同一条广播通道**，通道由 `cli` 组装期注入（`events` 与 `render` **互不依赖，无新增边**），**增量与日志事件必须同通道**（分两条则相对顺序无定义）——这就是票 15 那个岔口的答案；TUI 额外 `select!`(广播/tick/键盘)，**输入归渲染器**（终端独占），答案经注入的 channel 回循环。**呈现**：说话人前缀**逐行**（`[kimi]`/`[deepseek]`/`[executor:<id>]`/`[user]`，与票 17 的**模型看前缀是两套、不共用生成器**）、轮次分节线、分歧缩进块、工具调用一行摘要、hook 反馈与 tool 结果**归成一处**（同 `tool_call_id`）；**headless 纯净性靠结构**——渲染器只写两个显式 sink（`stdout_result`/`stderr_diagnostic`），stdout 只放最终产物（`TurnEnded{Completed}` / 讨论 `Consensus` 的 assistant 文本），其余全 stderr。**终止原因**沿用 `StopReason`（`Completed` 与 `Aborted`/`Error` **不能同色**）。**TUI 栈 = `ratatui` + `crossterm` 默认特性 + inline viewport**（定稿内容 `insert_before` 推进 scrollback、live 留 `Viewport::Inline`；否决 alt screen——转录要能滚动/复制）；**高亮 = `tree-sitter-highlight`**（树已在，闭包 15，不引 C 构建依赖；代价是终端渲染器自己写），diff 着色与语法高亮是两层。**headless 的 `Ask` 自动降级为 `Deny`**（理由进 stderr 与 `PermissionDecided.why`，由循环在门外做）。明确不做：内置编辑器、两进程、alt screen、syntect 的 C 路径。

- [执行者：独立预算、权限继承与回传到讨论](issues/14-executor-budget-permission-inheritance.md): **v1 = 一个通用 `task` 工具**（不做专职角色/第三种身份；"角色"是配置不是代码结构，挂点是 args schema 加 `role` + 组装期构造）；**模型默认继承派出它的那个讨论者**，可配覆盖；执行者的 system prompt 私有、`AGENTS.md` + skill 清单照常注入、不带讨论协议注入。**独立预算：轮数默认 25**（goose 值），但**"独立"只指轮数不指钱**——它的 token 计入会话累计（票 18 的闸门看得见）。**权限：继承拒绝、不继承允许**（拒绝是约束，沿链只可能变严——与"hook 只能收紧"、plan 模式同一代数，票 10 那条硬要求因此自动满足）；**递归深度 1，且第一道防线是执行者的工具集里没有 `task`**（不是"有工具但拒绝"）；**read set 不继承且任一方向都不流动**（护栏防的是"凭想象改文件"，而"想象"是每个 agent 各自的）。**回传 = 摘要 + 元数据**（token、改动的文件从流上推导）；**过程默认不进任何讨论者的窗**（机制是票 15 的 `speaker_id` 过滤），**结论经派发者自己的发言进入讨论**（讨论传的是发言不是共享工具历史），对派发者与其他人**一视同仁**（否则制造信息不对称）；按需 zoom 归票 19。**`task` 是普通工具调用、同 Turn 内阻塞到执行者结束**（因果紧；"每个 tool_call 恰好一条结果"自动覆盖，不需要异步投递），**同一批里多个 `task` 可并发**——因为 **`task` 自己不碰工作区**（`SideEffect` 判的是"**工作区副作用**"这个语义澄清交接票 03），真正的写互斥在**执行者内部**的调用上、靠**共享 `PathLocks`**；**并发上限默认 5**（goose 值）是**成本/速率闸门**不是安全闸门。**失败**（`ExecutorFinished{reason: Error|MaxIterations|Aborted|MistakeLimit}`）= 一条错误内容的工具结果，**讨论不因此中断**；顺带纠正综述那个坑：Claude Code 的"subagent 编辑不恢复"在我们这里不成立——执行者的事件与 `outputs/` 都在同一会话目录，`/undo` 一样有效。

- [research：消息级 `name` 字段的约束](issues/26-research-message-name-field.md): 详情在 `research/07-message-name-field.md`。**三家（Kimi / DeepSeek / OpenAI）一律只写 `type: string` + 一句"optional name for the participant"，没有 pattern、没有 maxLength** → 字符集与长度是 ⚪。**一条对 `research/02` §4.2 的纠正**：**DeepSeek 的 tool 消息没有 `name` 字段**（只有 `role`/`content`/`tool_call_id`），OpenAI 的 `ChatCompletionToolMessageParam` 同样没有；Kimi 只是四个 role 共用 schema 才"语法上允许"。唯一被文档化的重语义是 Kimi 的 Partial Mode（我们不使用）；普通 chat 下三家都没有行为副作用；没有一家标 deprecated。
- [发言归属与投影接缝](issues/17-speaker-attribution-projection.md): **一份实现、住在 provider 适配器侧、按 `caps` 分叉**（逐字段差异是数据不是代码；纯函数、不带裁剪状态；"哪些事件进上下文"是一张穷尽 match 表）。**他人的回合投三档**：文本保留、**工具调用只留一行摘要**、**结果正文与 `reasoning_content` 都不投**（先决约束：他人一律投成 `user`，所以他的 `tool_calls` 不能以 `assistant` 出现，配对的 `tool` 结果也必须一起改写或丢弃，否则线级配对校验会炸）；对照 **我自己的 `reasoning_content` 必须回放**（DeepSeek 带 tools 不回传直接 400）。**我自己的工具往返 + PostToolUse 反馈按 `seq` 合并成一条 `tool` 消息**（票 05 的通则，provider 只允许一对一）。**合并按 role 序列、轮次是硬边界、无条数阈值、严格按 `seq`**；**两条钉住的注入（第一条 `user`、`ContextInjected`）不参与合并**。**前缀 `[轮 N · 名字] …` 只加在他人发言上**（给自己加会污染回放）。**`name` 发但只发在 `user`/`assistant` 上、值消毒到 `[A-Za-z0-9_-]` 且短、绝不依赖它**（`tool` 消息不发，因为 DeepSeek schema 里根本没这个字段）。**执行者事件不进讨论者投影（`ExecutorFinished` 也不进）**，而**执行者自己的投影是全量的**——同一批事件双向不同。**plan 模式的注入与钉住的第一条 `user` 同档：永不参与裁剪**（幂等注入消重复），**钉住的注入永不参与裁剪、不在这个序列里**；丢弃顺序因此变成 **老普通工具结果 → 老 skill body → 老整轮 → 硬失败**。

- [讨论协议与轮次（含发散 vs 合成的张力）](issues/16-discussion-protocol-rounds.md): **"结论冲突"用机械判定**——每个讨论者在正文后另起一行给**一行自由结论**，归一化后**精确相等或子串包含**即视为一致（**零额外调用、无假阴性、假阳性有界于 2 轮上限**；不用 `response_format` 因为两家能力不一致；**不用受控极性标签**——开放问题没有自然标签，硬压就是失真且失真无上界；**不用中立裁判**——MT-Bench 位置一致性 GPT-4 65.0% / Claude-v1 23.8%（75% 偏向第一个），且 Anthropic 自己实验发现多裁判更差）。它的价值是**把分歧显式化**（那两行结论就是 `DivergenceRecorded{positions}`），不是省调用。**N=2 不裁决**：冲突只被判定与记录，裁决发生在合成。**合成器（Synthesizer，已登记 `CONTEXT.md`）职责 = 画出选项空间**：一次独立的单发调用（不是 agent 身份），产出**共识 / 分歧（含各自成立的前提）/ 未决**三档——不收敛（与"探索/发散"的目的相反）、也不放弃聚合（"标出分叉点"本身就是聚合）。**揭示发言全文、不揭示私有推理**（`reasoning_content` 根本不进投影，票 17 已钉死；摘要则等于替他改写论点）。**上限 2 轮**（1 独立 + 1 定向，可配），终止三值落在票 15 的枚举上（`NoDivergence` / `Consensus` / `RoundsExhausted`）；第三轮不是新机制（`RoundStarted{mode}` 已有槽位）。**单侧失败 → 该方本轮缺席、讨论继续**（**缺席必须记成事件**，否则合成会把"只有一份回答"读成"共识"）；**双侧失败 → `RoundEnded{Error}`**；**不重跑**（票 02 的 adapter 已在传输层做过有界重试）。**协议指令常驻私有身份、不进流**（否则"第二轮 messages 能从流重算"当场不成立；投影里的 `[轮 N · 名字]` 已给足轮次信息）；**同一轮的两个讨论者并发发**（异构 ⇒ 并发预算天然分到两家 provider）。**一次讨论 = 3 次或 5 次调用**（无分歧 / 有分歧，判定不花钱）。

- [成本与预算上限](issues/18-cost-and-budget.md): **先纠正票面前提**：**TPD 只存在于 Tier0**（累计充值 ≤ \$1；Tier1 \$10 以上是 `Unlimited`，`research/02` §6.1 逐行），所以日常风险其实是 RPM/TPM/并发。**预算的单位有"三个量各管一段"**：窗口按 agent 模型（票 06）、轮数讨论 2 / 执行者 25（票 16/14）、**token 累计会话级且讨论者与执行者共享**（票 06/14，是 `UsageRecorded` 求和的派生值）。**硬停盯累计，行为 = 降级并收尾**（讨论不再开第二轮、**直接进合成**——合成是唯一不能省的调用；执行者不再派新的、**已在跑的跑完**；降级复用票 16 的"缺席"形状）；**闸门用 token，费用只作显示**（可配价目表，且 **`cached` 与 `miss` 分开计价**）。**前置估算做**，但阈值用**剩余额度的比例**（票 06 的字符/4 估不准，"估算 > 剩余"会频繁误拒）。**弱模型分流只有两个落点：合成器与执行者（可配覆盖）；讨论者绝不能换**（异构是多样性最强杠杆），**v1 默认全用讨论者模型**——机制留好、取值等数据。**TPD/限额：无任何限额响应头被文档化**（只有 429 `rate_limit_reached_error`），所以**被动 429 分类 + 本地日账本**（按 UTC 日聚合，**从今天的会话文件派生**，不是新状态文件——与"预算不是状态是函数"同一纪律）。**缓存计量归一成 `cached` / `miss` 进 `UsageRecorded` 一等字段**（Kimi `cached_tokens`；DeepSeek `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens`；**不照抄 Anthropic 的 `cache_creation`/`cache_read`**，那两家没有）；两条破坏缓存的操作写死：**Kimi 的 `reasoning_effort` 切档**（必须会话开始前定死）+ 投影保序（票 17 已保证）；Kimi 有 **>256 tokens 才缓存**的门槛。**`StopReason` 加第 9 个值 `BudgetExhausted`**（`RoundEnded`/`SessionEnded` 用；加值不改既有语义）。

- [可观测性：多 agent transcript 的结构化](issues/19-observability.md): 票 15 已答"结构化日志免费"，所以本票收敛成**查询与呈现**。**查询接口 = CLI 子命令族四个动词**（`sessions ls` / `show <id> [--round|--speaker|--kind|--tool|--only-error]` / `replay <id> --speaker X --round N`（**投影重算**，调试投影 bug 的唯一手段）/ `stats <id>`），**不建索引**（`seq` 就是行号、顺序读 + 内存过滤；索引会是第二个真相源）；输出表格或 `--json`，**stdout 只放结果、诊断走 stderr**；**这是给人用的，不是给 agent 的新工具**。**两个事后视图**：默认按轮次分组的时间线（工具调用与结果归成一条，前缀与 live 同形），加一个 **`--files` 文件改动史**（唯一按"工作区对象"而非"时间"索引的视图，直接回答票面那句"谁在哪一轮改了什么"，且是**推导**不是新字段）；**不做交互式浏览器**。**诊断指标 = 流上的 group-by，不是新埋点**；内置一组固定指标（per-agent token/花费/命中率、每轮调用数、**单侧缺席率** ⚠️、执行者失败原因分布、**编辑匹配梯降级分布** ⚠️、read-before-edit 拒绝与 read set 失效、权限决策分布、hook 三类结果、讨论分歧率与停法分布）——标 ⚠️ 的两类是**静默**的，固定指标的价值就是**替你问了该问的问题**；但**保留过滤原语 + `--json` 作逃生口**。**编辑匹配那类诊断量不加新变体**：以**约定文本**落在既有 payload（票 04 已定级名进工具结果、票 03 已定拒绝是错误结果——信息本来就在流上），**渲染与解析共用一个格式常量**（漂移是文本方案唯一的真风险，而它会让统计悄悄变成 0）；若某诊断量将来升级为**权威**再给它结构化位置。

- [权限表达与沿委派链的继承](issues/20-permission-expression-delegation-inheritance.md): **准备时发现一个冲突并解决了**：Claude Code 的 `deny > ask > allow` **且忽略具体程度**会让票 10 的 `PLAN.md` 豁免失效（宽 deny 压过窄 allow）——解决方式是**把规则的作用域做成"对这次调用的谓词"**，于是豁免成为 **deny 条件里的一个与项**（"非 `ReadOnly` 且写入集 ≠ `{PLAN.md}`"）。**规则 = `subject` + `Scope`（`Tool(glob)` / `CommandPrefix(argv)` / `Path(pattern)` / **`PathSet(exact)`** / `All`）+ `action` + `propagate`**；**优先级 = `deny > ask > allow` 且忽略具体程度**（让规则优先级、hook 收紧、执行者继承**共用同一个代数**，只有一套合并语义；"具体程度"无法定义，Claude Code 也明确忽略它）。**评估次序：断路器（短路 `Deny`，早于一切）→ 规则取上确界 → hook 收紧 → 无交互时 `Ask→Deny`**。**"继承拒绝、不继承允许"是默认值不是特例**：`propagate` 按动作定（`Deny`/`Ask` 真、`Allow` 假）⇒ 执行者策略 = 父级传播下来的 ∪ 自己的，票 10 的硬要求自动满足。**断路器进 v1 且是"短路"不是"规则"**（敏感路径写入永不自动批准；`rm` 打到根 / 家目录 `Deny`，任何 allow 与 hook 都不能翻转）；**诚实定位 = 减少误伤、不是抵抗攻击、别当主要防线**。**`.env` 家族默认 `deny`**（`*.example`/`*.sample`/`*.template` 除外）。**两处落点**：headless 的 `Ask→Deny` **由循环在门外做**（"有无交互能力"是运行期事实，门是纯函数不读环境；门的裁决如实保持 `Ask`，理由进 `why`）；**"总是允许"只改会话内策略、不写用户 `config.toml`、不进流**（策略是 Session 的值，与票 10 对模式的立场一致）。**动态工具这一侧没有额外信任策略要落**（票 11 已把"谎报"取消），它就是普通目标、`Tool("custom__*")` 一条规则兜底。**deny 不从上下文移除工具**（移除会废掉前缀缓存）——要"不存在"就在组装期不注册。

- [OS 沙箱与凭据暴露面](issues/21-sandbox-and-credential-exposure.md): **v1 不上进程级隔离**（本机 Linux x86_64，所以问题本可简化成"要不要上 bubblewrap"）——沙箱与"自用"有真实张力（要放行工作区 / 两家 API 的网络 / 读自己的配置与 skill，那份白名单**本身就是策略文件**），而并发的危险靠写串行化（票 03）与权限门 + 断路器（票 20）解决；升级路径写明是"**只做 Linux 的 bubblewrap**"，**不做抽象预留**。**cwd 路径限制 = 文件工具的"模型供路径"限定在会话 cwd 及其子树**（限的是模型给的路径，不是 harness 自己读的 `skill()`/会话目录），**例外走权限规则**——它保护的是一件具体东西：**agent 自己的密钥就在 `~/.config/fs-agent/config.toml`**。**秘密打码做**：值级、best-effort、**在入流前**（与票 06 的截断/落盘合成一条流水线 **打码 → 截断 → 落盘**），所以**流上的文本 == 模型看到的文本**（一致）而**工具执行时用真值**；范围**含消息正文**（跨 agent 那条路径的唯一剩余形态就是"他在发言里复述了密钥"）。**凭据路径清单**：(a) 文件工具读 → `.env` deny + cwd 限制**两条一起**；(b) 命令回显 → 入流前打码 + 建议的读断路器；(c) 跨 agent → **票 17 已把最强的通道关掉**（他人工具结果正文不投），剩的靠打码消息正文；**(d) 外传（票面漏了）→ 坦白写"我们基本无能为力"**，不加网络断路器的理由（难判定 / 误伤合法 `curl` / 原则是减少误伤），真正的边界是"密钥不给 agent"。**prompt injection 定位为"降低上限"而非"解决问题"**：门是 `(policy, tool, args)` 的纯函数、**从不读对话文本** ⇒ 注入不可能说服门放行；**破坏半径 = 你的策略允许的半径**；缓解三条（身份指令 / 他人工具结果不投 / 门不看文本），**不把发言降权、不让门读对话**。**root / sudo 时拒绝启动、不给 bypass flag**（任何一次误判都变成系统级，而护栏都假设"最坏只到工作区"）。**会话目录创建即 `0700`/`0600`**（`umask 022` 下默认会是 0644 = 同机可读）、**维持不自动清理 + 加一个手动 `prune`**。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

<!-- 当前**没有**未指定的雾：原有的两条（发散 vs 合成、讨论某轮失败）都已由票 16 答掉——第 2 节「画出选项空间」、第 5 节「单侧缺席 / 双侧 Error」。剩下的全部已经是票。 -->

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **证据型砍掉项**（判据变更不翻它们）：AST / tree-sitter 编辑、unified diff 编辑格式、原生多 provider 协议、MCP client、SWE-bench 跑分与多模型 dashboard。
- **向量检索 / RAG**：证据型排除——代表性实现里没有以它为核心卖点的，自用场景 grep + 符号检索够用；且它会引入第三个 API 面（embeddings）与索引存储，反向压到"只做一个 OpenAI-compatible client"。范围判定见 [repo map / 符号检索如何进入上下文预算](issues/12-repo-map-symbol-search.md)。
- **形态扩展**：IDE 集成、Slack / Web / 移动端接入、会话分享。
- **验收契约**（fake provider + 临时 git 仓库 e2e）：本图只产架构决策，该项由 `/to-spec` 承接。
- **本图的执行**：`/to-spec` / `/to-tickets` / `/implement`。
