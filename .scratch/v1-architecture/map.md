# fs-agent v1 架构（wayfinder map）—— ⚠️ 已封存

> **此图已封存，不再是活 tracker。** 目的地「极简 v1」已被用户推翻（去掉「极简」+ 加入多 agent 讨论），替代图在 **`.scratch/multi-agent-architecture/`**。
> 保留本图是因为它的 **ratify 记录**仍是主要证据源：哪些砍掉项是被「极简」砍的、哪些是被证据砍的。替代图正是靠这个分类决定重开哪些。
> **不要在此开票或解票。** 未被替代图承继的票（01、02、05、06、07、13、14）已在替代图中改写；08 已解并退休。

Label: `wayfinder:map`
Tracker: local markdown —— 见 `docs/agents/issue-tracker.md`

## Destination

一份 **spec-ready 的边界级 Rust 架构决策集**，交给 `/to-spec` 折叠成可建计划。

范围**由本图确定，ratify 已完成**。`docs/research/coding-agent-features.md` 的「结论二（17 条砍掉清单）」与「结论三（10 个最小切片）」已**逐条 ratify**，结果：

- **砍掉 12 条 + 第 9 条半边** —— 清单在 `## Out of scope`。
- **翻案 4 条 + 第 9 条半边** —— 子 agent（票 14）、skills（票 09）、自定义工具注册（票 11）、repo map / 符号检索（票 12）、全屏 TUI（票 13）、硬 plan 模式（票 10）。

因此本图产出**两组接缝**：综述那 10 个切片赖以落地的核心接缝（票 01–07），**加上**翻案六项各自的接缝（票 09–14）。内容仍是模块边界、关键 trait 签名、控制流归属。

## Notes

- **领域**：`fs-agent` —— 自用极简 coding agent CLI，Rust，从零实现。已确认使用 Rust。
- **设计输入（唯一）**：`docs/research/coding-agent-features.md`（940 行综述，含标签总表、跨实现对照表、砍掉清单、10 切片）。5 份实现笔记在 `docs/research/notes/`（10 个实现两两配对，共 8,396 行），**按票 zoom 时再读，不要一次全读**。
- **范围逐条 ratify：已完成**（17 条逐条过完；10 个切片随 Destination 一并接受）。结果写进了 Destination 与 `## Out of scope`。本条原文写「进行中」，是封存前遗留的措辞，已更正。
- **综述内部不一致：已解决（ratify 第 9 条）。** 解法是"**模式 ≠ 工具**"——**保留**「硬 plan 模式」（票 10，靠把写入类权限临时切成 `ask` 实现，不需新状态机），**砍掉** todo 工具。
- **配置（ratify 期间定下）**：agent 级配置固定在 **`~/.config/fs-agent/config.toml`**（TOML）。**不**自动加载项目 `.env`。优先级：**`config.toml` > 已导出的 env > 内置默认** —— 即配置文件覆盖一切，env 只作回退来源。综述切片 1 要求 `base_url` / `api_key` / `model` 三者都从这里读。
- **Provider 事实（2026-09-12）**：手上有 **KIMI_API_KEY**（Moonshot），DeepSeek 可再加 —— **两家 API 都是 OpenAI-compatible**，`base_url` 可配即够，正好印证切片 1 的「只做一个 OpenAI-compatible client」。旁注：子代理扫环境变量时没找到任何 `*_API_KEY`，因为 key 通常在登录 shell 里而非非交互进程的 env 中；改走 `config.toml` 后这一点不再是问题。
- **跨票约束**：票 01（模块布局）已被要求在模块列表里预留渲染接缝（票 13）与可重入的循环（票 14）；票 03 被要求把"动态工具的副作用标记如何被信任"显式交接给票 11。改这两张票时别只改内容、忘了它们下游的挂口。
- **架构深度 = 边界级**：crate/模块划分 + 模块边界 + 关键 trait 签名。**不要**定到逐个公开函数签名（那是接口级），更不要定文件名与函数职责。
- **扩展机制 = 档位 (iii) + 动态工具注册（原 (iii) 已 superseded）**：工具调用前后各留一个挂载点（`PreToolUse` / `PostToolUse`），v1 挂内建消费者；**并且**（ratify 第 3 条翻案）要做自定义工具注册 —— 外部声明「额外工具 = shell 命令 + 参数 schema」、由 agent 动态加载，即原档位 (iv) 的那一半。
  - **翻案的直接后果，必须由票 03 正面回答**：工具面不再编译期封闭，于是「只读」这一副作用标记变成**不可信输入**。而并行判定规则是「按有无副作用打标，只读并发、有副作用串行」（综述从五个数据点收敛出的共识）——一个动态注册的工具**谎报只读**，就会让两个写操作并发。所以动态注册必须配一条**可信度策略**：默认串行？要求显式声明 + 人工确认？这是票 03 的必答题，不是可选优化。
  - 综述有一条反向警告与本条相抵：「工具一多模型越容易选错」；Anthropic 因此建议工具做命名空间与合并。
  - 综述对 hook 的定位不变：「**如果只保留一个扩展机制，保留它，而不是 MCP。**」——但本条翻案**超出了综述的建议范围**。
- **已被用户明确排除**：验收契约（fake provider + 临时 git 仓库 e2e）**不进本图**，留给 `/to-spec`。
- **每张票的答案必须自足**：`/implement` 会在 `/clear` 之后的新会话里读它，不会看到本图的对话。
- 已知综述内部不一致（spec 阶段会踩到）：第 4 节「最小可用」写「拼到 system prompt 末尾」，但同节说明与结论表都写「放**第一条 user message**」。以结论表为准。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里，本文件不复述。按名字引用，不写裸编号。 -->

- [research：Rust 生态的 crate 选择与当前惯用法](issues/08-research-rust-ecosystem.md): 5 个问题的带来源答案在 `research/08-rust-ecosystem.md`。三条直接改架构票：`async fn` in trait **不** dyn-compatible（`Provider`/`Tool` 不能用 `dyn`）；hook 的惯用表达是「trait + typed enum」而非 channel，`rig-agent` 的 `AgentHook` 是最近先例；JSONL 无官方 API 且 `OpenOptions::append` 不保证跨线程追加不交错。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

- **错误处理与失败语义的横切约定**：工具执行失败、provider 429/超时、循环级失败各自如何表示与呈现。综述第 12 节把「重试 / 限流 / 降级」标为 🔷。这跨越 provider、工具、上下文三条接缝，等它们定形后再看是否需要独立成票。
- **「一个子 agent」与「多 agent 编排」的边界**：ratify 第 1 条翻案的原文涵盖"子 agent 与多 agent 编排"整句，但这是两件事。综述把编排（多子 agent 的调度、错误传播、结果聚合）单列为 ⬜，理由是"需要一整套调度、错误传播、结果聚合的工程，远超极简可用"。**票 14 默认只做前者**（一个通用 `task` 工具）；编排按兵不动，等票 14 把边界定清后再看要不要毕业。
- **repo map 含不含向量检索**：ratify 第 4 条的原文是「repo map / 符号检索 / **向量检索**」三项连写，但综述把它们分开标注，并指出"代表性的 agent 里**没有以 RAG 为核心卖点**的；grep + 模型自己找更可解释"。**票 12 默认只做符号检索**，向量检索是否也要待定。

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **验收契约**（fake provider + 临时 git 仓库端到端测试）：地图只产架构决策，该项由 `/to-spec` 承接。这不是雾，是范围边界。
- **综述结论二的砍掉项（已 ratify 确认砍掉的）**：MCP client（第 2 条）；AST / tree-sitter 编辑（第 5 条）；unified diff 编辑格式（第 6 条）；OS 级沙箱（第 7 条）；权限分类器模型 / 完整 allow-deny DSL / 声明式策略文件（第 8 条）；todo **工具**（第 9 条的半边，见 Notes）；会话 fork / 交互式 rewind 菜单 / shadow git / 自建快照（第 11 条）；SWE-bench 跑分与多模型 dashboard（第 12 条）。
  第 13–17 条已 ratify 确认砍掉：OpenTelemetry / trajectory 浏览器 / 结构化日志（13）；原生多 provider 协议（14）；显式 cache 断点调优 / 硬预算上限 / 弱模型分流（15）；图片 / PDF / notebook 读取、web 搜索、LSP、浏览器工具、**秘密打码**（16）；IDE 集成 / Slack-Web-移动端接入 / 会话分享（17）。**17 条 ratify 完毕。**
  - 第 16 条砍掉「秘密打码」与配置加载存在一个组合风险，spec 阶段需正视：若 fs-agent 主动加载项目 `.env`，同时会读文件、又把 transcript 落盘，密钥可能进到磁盘（opencode 的默认是 `.env` 系列 deny 读取）。
  注意几条**不在被砍范围**的相邻项：第 9 条保留的「硬 plan 模式」、第 11 条之外的「每次编辑自动 git commit + /undo」（切片 7）、第 13 条之外的「完整 transcript 落盘」（第 16 节 ✅ 项）、第 15 条之外的「system prompt 前缀稳定」。
- **本图的执行**：本图只产决策。「做」发生在 `/to-spec` → `/to-tickets` → `/implement`。那种「干脆直接写起来」的冲动，是走到地图边缘的信号，不是本图的活。
