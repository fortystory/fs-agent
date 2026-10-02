# MCP 接入 fs-agent（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-03，一轮广度 grilling 二十问 + 一份一手调研）。本图只做**规划**，不产实现代码。
**✅ 本图已完成（2026-10-03）**：9 张子票全部 resolved、`Not yet specified` 为空 ⇒ 路线 clear；下一步是 `/to-spec`（见 `## 进度`）。**不要再往这张图加票。**

> **交棒已发生（2026-10-03）**：二十条冻结项与九张票的 `## Answer` 已经折进
> [`spec.md`](spec.md) —— 四个元工具（第一阶段两个：`mcp_list` / `mcp_call`）、`rmcp` 3.5.0
> （`Discover` 生命周期、`reqwest-native-tls`）、按四阶段落地（工具 → 资源 → 提示词 → 边角）。
> 本图至此**只作决策存档**，不再是待办；「做」由实现票与 `/implement` 接手。
> 同日由 `/to-tickets` 拆出 **9 张实现票**（`issues/10`–`18`，见下面任务清单的第二段；第一张是
> [服务层骨架 + `mcp_list`](issues/10-mcp-service-and-list.md)）。

## Destination

一份 **spec-ready 的 MCP 接入决策集** —— 交给 `/to-spec` 折成构建计划。它要能回答「怎么把外部
MCP server 接进 fs-agent」的每一个还需要拍板的问题，并且**回改 v1 的 `Out of Scope`**（MCP client
那一行加一条带日期的补记，原文不改写）。

**范围 = 现行 MCP 规范（2026-07-28）的全集**：tool / resource / prompt / elicitation + MRTR，
走 `server/discover` 的无状态 `_meta` 形态。**只谈最新版**，不做旧版 `initialize` 回退。

落地分阶段，图里按依赖排：**工具 → 资源 → 提示词模板 → 边角（MRTR / 其余 capability）**。

## Notes

- **领域**：`fs-agent` —— 自用 coding agent CLI（Rust）。两条与之相关的既有事实：**事件流是唯一
  真相源**、**工具表在组装期建好之后不再变化**（它是缓存前缀的一部分）。
- **一手调研**：[`research/01-mcp-client-implementation.md`](research/01-mcp-client-implementation.md)
  —— 规范要点（六原语分档、无状态化、MRTR、`listChanged`、`ToolAnnotations` 只是 hints）、DSH 的
  做法、上游八个实现、fs-agent 侧的撞点、Rust 侧可用的件、八条待拍板点（本文的冻结项已经答掉其中
  五条，剩下的在票里）。
- **参照物**：DSH 的 [`dsh-mcp-client`](/usr/lib/node_modules/@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-mcp-client/README.zh.md)
  与 `dsh-mcp-resources`（一台 server 一个插件实例、`mcp__<server>__<tool>`、首轮前原子换代、
  `scrubbedParentEnv()` 擦掉 `/KEY|PASSWORD|SECRET|TOKEN/i` 与 `DSH_*`）。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 对话。
- **要咨询的 skills**：`/grilling`（HITL 票默认）、`/domain-modeling`（术语：「原语」「元工具」
  「不可信外部数据」都会进词汇表）、`/research`（research 票）。

### Tracker 事实与降级（本图适用）

- map = `.scratch/mcp-support/map.md`，child = `.scratch/mcp-support/issues/NN-*.md`；阻塞 = 票面
  `Blocked by: NN`；claim = `Status: claimed`；resolve = 票底 `## Answer` + `Status: resolved` +
  追加一行到本文 `Decisions so far`。
- **没有 native sub-issue / 依赖边**，所以回退到正文约定：本文的 `## 任务清单` 逐条引用子票
  （条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/mcp-support/map.md`。宣布图走完之前必须 PASS。

### 冻结项（charting 的 grilling 定下，票里不得重开）

1. **destination 是一份 spec**，不是「只做一个决定」，也不含实现落地。
2. **v1 的 `Out of Scope` 另起 effort + 补记**：`fs-agent-v1/spec.md` 的 `:449` / `:636` / `:667`
   与 [`README.md`](../../README.md) 的「这一版不做」清单**原文不改写**，各加一条带日期的补记
   （照 `sandbox` 的先例）。
3. **scope = 现行规范全集**（tool / resource / prompt / elicitation + MRTR）。
4. **已 deprecated 的 sampling / roots / logging 不做**（SEP-2577，最早 2027-07-28 后移除）——
   归本文 `Out of scope`。
5. **不扩工具表**：MCP 的工具**不进** `builtin()` / `with_dynamic()` 那张表。这条直接绕开
   「工具表冻结 vs `notifications/tools/list_changed`」那个撞点 —— 表里只有两个固定的元工具，
   server 侧工具的增删只影响 `mcp_list` 的**返回**，前缀一个字不动。
6. **两个元工具**：`mcp_list` 与 `mcp_call`（不是一条 `mcp(action, …)`，也不是「描述里带快照」）。
7. **参数面**（模型可见的 schema，一次定死）：`mcp_list(server?)` —— `server` 不传就列全部；
   `mcp_call(server, tool, arguments)` —— `arguments` 是一个自由 JSON 对象，fs-agent 不校验它
   （schema 在 server 那侧），只透传。
8. **外来工具的 `Effect` 默认最严**：一律按 `Exclusive` 看待（像今天的 `custom__*`），人在配置里
   逐条放宽。**不信 server 的 `ToolAnnotations`** —— 规范原文写着它们全是 hints，而且
   「clients should never make tool use decisions based on ToolAnnotations received from untrusted
   servers」。
9. **server 进程随会话起、随会话停**（不是常驻守护进程），**过 bubblewrap 沙箱**（与 `bash` 同一层）。
10. **配置两个来源**：`.mcp.json`（项目级）+ `config.toml` 的 `[mcp.servers.<名字>]`（用户级），
    **项目级盖住用户级**。
11. **结果算不可信外部数据**：带中文不可信标记（与 [`web_search` / `web_fetch`](../web-search-tool/spec.md) 同一条规矩）。
12. **凭据写在 server 配置里**（环境变量名或 header），值**接进打码器**（与 provider 密钥同一条路）。
13. **谁能用**：主会话 / 讨论者 / 执行者都能用（`delegable()` 保持 `true`）。
14. **失败即报错**：server 起不来 / 崩了 / 超时都回一条结构化错误；**不自动重连**，同会话内不重试，
    重开会话才重连。
15. **server 的 `instructions` 不进系统提示词**：系统提示词是身份层（可信 + 缓存前缀），而
    server 指令是**外部文本**。放进 `mcp_list` 的结果里，带不可信标记。
16. **MRTR 复用已有的问询通道**：server 回 `resultType: "input_required"` 时，走
    `ask_user_question` 那条第三类发起者的路（主会话接管底部输入区），**不新开第四类发起者**。
17. **分期**：工具 → 资源 → 提示词模板 → 边角（MRTR 与其余 capability）。
18. **传输**：stdio（本地子进程）+ Streamable HTTP（远端）。不做 legacy HTTP+SSE。
19. **协议版本**：只谈 2026-07-28（无状态 `_meta` + `server/discover`），不做旧版 `initialize` 回退。
20. **Rust SDK 是候选不是结论**：`rmcp` 3.5.0（Apache-2.0、tokio 原生、stdio / Streamable HTTP、
    自带 discover + legacy 双生命周期与 MRTR 驱动、不做 legacy SSE；只做 client 要
    `default-features = false`）—— 用不用它归 [票 02](issues/02-grilling-sdk-or-own-client.md)。

### 会撞的既有决定（`/to-spec` 时回改，不在本图改）

- **`fs-agent-v1/spec.md` 的 `Out of Scope`**：`:449`（「明确不做：MCP client 本身」）、`:636`
  （「证据型砍掉项」）、`:667`（「实现者请勿顺手改进」），与 [`README.md`](../../README.md) 的
  「这一版不做」清单。
- **「工具表组装后不变」那条不变量**（[`src/tools/mod.rs`](../../src/tools/mod.rs) 顶部注释与
  `with_dynamic` 的调用点）：元工具方案**不动**它 —— 这是本图选元工具的首要理由。
- **`Effect` 三类的语义**（[`src/tools/tool.rs`](../../src/tools/tool.rs)）：`ReadOnly` 说的是
  「只读**工作区**」，表达不了「查远端 Jira」这类外部副作用 —— 冻结项 8 用「默认最严」绕开它，
  不新增第四类。
- **`CONTEXT.md`**：要收「原语」「元工具」这类词（措辞归 `/to-spec`）。

### 基线与纪律

- 验收基线：`cargo test` 当前全绿（2026-10-03 实测，web 那轮修完是 0 failed）；`cargo clippy
  --all-targets`、`cargo fmt --check`、`python3 scripts/check-language.py` 都干净。
- **提交用限定路径**：本工作区出现过并行 session 与全量暂存互相卷进无关改动的情况。
- 改了 spec 的决定就**回改 spec 正文**，不要只写在票的评论区。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**决策票**（wayfinder 的九张，全部 resolved）：

- [x] [research：`rmcp` 3.5.0 的契合度](issues/01-research-rmcp-fit.md)
- [x] [grilling：自己写 client 还是用 `rmcp`](issues/02-grilling-sdk-or-own-client.md)
- [x] [grilling：resources 在元工具方案下的形状](issues/03-grilling-resources-shape.md)
- [x] [grilling：prompts 模板的人机入口](issues/04-grilling-prompts-entry.md)
- [x] [grilling：`mcp_list` 的新鲜度与 `listChanged`](issues/05-grilling-list-freshness.md)
- [x] [grilling：server 进程的环境与可写根](issues/06-grilling-server-process.md)
- [x] [task：v1 的范围补记](issues/07-task-v1-scope-note.md)
- [x] [grilling：多台 server 的启动与命名](issues/08-grilling-multi-server.md)
- [x] [grilling：第三方 server 的信任分级](issues/09-grilling-trust-tiers.md)

**实现票**（交棒后拆出的构建切片，`Type: implement`；wayfinder 会话不认领它们）：

- [ ] [服务层骨架 + `mcp_list`（tracer bullet）](issues/10-mcp-service-and-list.md)
- [ ] [`mcp_call`：转发一次外部调用](issues/11-mcp-call.md)
- [ ] [连接层：`rmcp` + 真 stdio / Streamable HTTP](issues/12-rmcp-connection.md)
- [ ] [server 进程：环境白名单与可写根](issues/13-server-process-env.md)
- [ ] [`Effect` 与信任三个位](issues/14-effect-and-trust.md)
- [ ] [MRTR：把 elicitation 接到问询端口](issues/15-mrtr-elicitation.md)
- [ ] [资源：`mcp_resources` 与 `mcp_read`](issues/16-resources.md)
- [ ] [提示词模板接进 `/` 菜单](issues/17-prompts-menu.md)
- [ ] [文档与索引](issues/18-docs-and-index.md)

共 **18** 张票（**9 决策 + 9 实现**），当前 **9 resolved / 9 ready-for-agent**。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [research：`rmcp` 3.5.0 的契合度](issues/01-research-rmcp-fit.md): 推荐 `rmcp` 3.5.0（`default-features = false`）—— `ClientLifecycleMode::Discover` 与「只谈 2026-07-28」精确对齐且不回退（源码注释：Discover mode does not fall back; a legacy server is an error.），`call_tool` 自带 MRTR 驱动、elicitation 落在可 override 的 `create_elicitation` 上（冻结项 16 的挂点，但要求 `'static`，与借用的 `questions` 端口不兼容）；依赖几乎全在现有 lock 里，净新增约 4 包（rmcp、process-wrap、sse-stream、tokio-stream），stdio server 仍可过 `Sandbox::wrap`。代价：process-wrap 清理弱于 `killpg`、`'static` 端口、`close` 的 `&mut self`。完整事实见 research 文件。
- [grilling：自己写 client 还是用 `rmcp`](issues/02-grilling-sdk-or-own-client.md): **用 `rmcp` 3.5.0**（`default-features = false`、TLS 取 `reqwest-native-tls` —— 否则会开 rustls、推翻 `Cargo.toml` 的既有决定）—— `Discover` 生命周期与「只谈 2026-07-28」逐字对齐，MRTR 挂点现成。三个取舍一并定下：**子进程保留「杀整组」纪律**（先验能否在 `CommandWrap` 上叠 process-wrap 的进程组包装，不行就自己 spawn）、**问询端口改成 `Arc<dyn UserQuestions + Send + Sync>`**（一处改动，不给 MCP 开平行通道）、**关闭走 Drop 兜底**（`close` 要 `&mut self` 而 `Tool` 只给 `&self`）。它给票 06 添了一条硬输入：`rmcp` 不做环境清洗，擦洗得 fs-agent 自己做。
- [grilling：resources 在元工具方案下的形状](issues/03-grilling-resources-shape.md): **再加两个元工具** —— `mcp_resources(server?)` 列、`mcp_read(server, uri)` 读（元工具从两个变四个；不与冻结项 6 冲突 —— 那条讲的是「MCP 的**工具**不进表」，资源是另一类原语）。两条由机制定死的边界：**资源不登记进 `ReadSet`**（它装的是工作区路径，URI 装不进去，「先读后写」对资源不适用），且资源与工作区路径是**两套坐标系**。上限走既有那条截断流水线，正文同样带不可信标记。
- [grilling：prompts 模板的人机入口](issues/04-grilling-prompts-entry.md): **接进 `/` 菜单** —— 这个 effort 因此把「`/` 菜单支持运行时条目 + 一个填参数的界面」纳入 scope（今天的菜单条目是常量，而模板清单是运行时从 server 拉的）。边界：模板仍由**人**挑、模型不自发调用；菜单的动态部分不进工具表、不进前缀缓存；server 不可用时它的模板条目不出现。
- [grilling：`mcp_list` 的新鲜度与 `listChanged`](issues/05-grilling-list-freshness.md): **每次现问**（每次都向 server 发一次 `tools/list`）—— 不订阅 `toolsListChanged`、不做会话内缓存。没有缓存就没有失效；与冻结项 14（不自动重连）不矛盾 —— 它天然会探到 server 的当前状态。
- [grilling：server 进程的环境与可写根](issues/06-grilling-server-process.md): **环境走白名单** —— `env_clear()` 后只注入配置里显式声明的 `env` 加最小必需的 `PATH`/`HOME`/`LANG`（黑名单是 fail-open，与沙箱 fail-closed 的调性不合；且 `rmcp` 不做清洗，清洗必须发生在交给它之前）。**可写根逐台声明**，不给默认临时目录。
- [task：v1 的范围补记](issues/07-task-v1-scope-note.md): 四处落点都改完 —— v1 spec 的 §14、`Out of Scope` 的证据型砍掉项、`Further Notes` 的「请勿顺手改进」各加一条带日期的补记；README 的「这一版不做」清单**移除** `MCP client` 并新加一段说明。**原文一个字没有改写**。
- [grilling：多台 server 的启动与命名](issues/08-grilling-multi-server.md): **并发起、失败的跳过**（每台各自一个超时；谁起不来谁经 `mcp_list` 报成结构化错误）；**不做数量上限**；**重名是启动错误**（不用「后盖前」——那会让人以为两台都活着）。
- [grilling：第三方 server 的信任分级](issues/09-grilling-trust-tiers.md): **允许逐台声明，且声明是一组能力** —— 三个各自默认关的位：`trust_results` / `trust_effects` / 不过沙箱。不捆成一个 `trusted = true`，否则会让人为了省一句标记把写权限一起放出去。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

<!-- 当前**没有**未指定的雾：九张票全部 resolved，原有的五条雾各自有了归宿 —— 「多台 server」与
「信任分级」毕业成票 08 / 09 并已 resolved；「`mcp_list` 的缓存失效」被票 05 的「每次现问」消掉；
「资源 URI 与 `ReadSet`」由票 03 定死（不登记、两套坐标系）；「`mcp_call` 的墙钟与错误码形状」由
票 02 选定的 `rmcp` 收窄 —— 它现在是 `/to-spec` 时的一条实现决定，不再是需要单独拍板的雾。 -->

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **已 deprecated 的原语**：sampling / roots / logging（SEP-2577，最早 2027-07-28 后移除）。
- **旧版协议回退**：只谈 2026-07-28 的无状态形态，不做 `initialize` 握手回退 —— 代价是连不上
  只支持旧版的 server，这是 2026-10-03 明确选的。
- **legacy HTTP+SSE 传输**：`rmcp` 也不提供。
- **MCP server 端实现**：fs-agent 只做 client。
- **把 MCP 工具铺进工具表**（每台 server 的每个工具一条表项）：已选元工具方案，永不。
- **MCP 提示词模板的模型自发调用**：模板的发起者是人，这条不会毕业成「让模型自己挑」。
- **server 指令进系统提示词**（DSH 的做法）：系统提示词是身份层，外部文本不进去。
- **本图的执行**：只产决策；「做」发生在 `/to-spec` → 实现票 → `/implement`。

## 进度

**100%** —— **本图完成（2026-10-03）**：9/9 张子票全部 resolved，`Not yet specified` 为空 ⇒ 通往
destination 的决策已 clear。二十条冻结项 + 九张票的答案合起来就是那份 spec-ready 的决策集。

**交棒已完成（2026-10-03）**：折成 [`spec.md`](spec.md) —— 三层的四个元工具、`rmcp` 3.5.0、
四阶段落地、九条测试用例、以及两份调研与 v1 补记的指向。下一步是 `/to-tickets` 与实现，不在本图里。

**待确认**：无。九张票的决定都已在 live exchange 里由维护者拍板。
