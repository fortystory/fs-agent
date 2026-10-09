# 轨迹页对齐 DSH：把轨迹升格成可寻址的账本（wayfinder 决策图）

Label: `wayfinder:map`
Status: 11 resolved + 12 done + 1 ready-for-walkthrough
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-09，两轮 grilling 共九问：终点形状 / 欠缺的判据 / 允许动到哪一层 /
DSH 里真用过的能力 / 范围铺多宽 / TTFT 的数据路 / 轨迹页的定位 / 交付批次 / 用量放哪）。
本图只做**规划**，不产代码改动。

## 目的地

一份**可执行的 spec**（交给 `/to-spec` 折成构建计划）：把 DSH 轨迹视图的能力**逐条对齐**到衡的
轨迹页上 —— 把它从「一条可扫的流 + 点开单条」升格成**可寻址的账本**。

「可寻址」是这次的中心：DSH 的每一行都是一个能选中、能折叠、能被搜索与时间轴区间命中的对象，
而衡今天是一串只能点开的行。围着它有四件事 —— **可寻址**（行选择、折叠、搜索过滤、区间聚焦）、
**结构**（回合 / 迭代的分组与组头摘要）、**数字**（每行耗时、检视器里的用量与计时拆分）、
**另一种读法**（时间轴总览）。逐条判定见 [`parity.md`](parity.md)。

**范围**：`src/render/`（`tui.rs` / `pane.rs` / `transcript.rs` / `layout.rs` / `wording.rs` /
`links.rs` / `palette.rs`）、`CONTEXT.md` 的词条、`docs/render.md`、`docs/tui-manual-checklist.md`、
`scripts/tui-startup-check.py` 的锚点。**模型可见文本零变化**；事件 schema 动不动以
[衡的数据面](issues/01-research-data-surface.md) 的结论为准。

## 笔记

- **领域**：衡（heng）的 TUI 外壳与渲染层。轨迹页是主列页签条上的第二页（`trace-in-main`），
  画**全量块**、每块行首 `HH:MM:SS`、单位之间一条分隔线、点一行开详情覆盖层、有自己的滚动与
  贴底跟随（`trace-tab`）。它与对话视图共享同一个源（`painted`），两个 `pane` 各持折行与滚动。
- **对齐矩阵**：[`parity.md`](parity.md) —— DSH 的每一条能力 × 衡今天的对应物 × 判定
  （已对齐 / 补齐 / 变形做 / 不做）。这次要做的面全在那里，本图只画路线。
- **DSH 侧的一手事实**（只读，随包发布产物）：
  `/usr/lib/node_modules/@deepseek-ai/dsh/node_modules/@deepseek-ai/dsh-client-ui-trajectory`
  —— `README.zh.md` 是入口，`lib/types/client/*.d.ts` 是契约，`lib/client.js` 是实现（8777 行，
  带 `//#region` 分区注释可定位）。要补事实时照这条路径读，不要凭印象。
- **既有决定必须先读**：[`trace-tab/spec.md`](../trace-tab/spec.md)（两个视图的分工与共享源）、
  [`trace-in-main/spec.md`](../trace-in-main/spec.md)（进主列、页签条、时间戳）、
  [`trace-thought-stamp/spec.md`](../trace-thought-stamp/spec.md)、
  [`trace-usage-tail/spec.md`](../trace-usage-tail/spec.md) 与 [ADR 0016](../../docs/adr/0016-usage-rides-the-row-of-its-call.md)、
  [`tui-visual-language/spec.md`](../tui-visual-language/spec.md)（语义色板与字形语法）、
  [`questionnaire-reading/spec.md`](../questionnaire-reading/spec.md)（浮层与键盘分层）。
- **要咨询的 skills**：`/research`（两张事实票）、`/prototype`（三张形态票）、`/grilling` +
  `/domain-modeling`（五张决策票；术语会进 `CONTEXT.md`）。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 对话。
- **文档纪律**：新增或改写的 `docs/**` 要过 [`scripts/check-doc-size.py`](../../scripts/check-doc-size.py)
  的压表达规则（`docs-slim`）。

### 冻结项（charting 的 grilling 定下，票里不得重开）

1. **destination 是一份 spec**，不是「只做一个决定」，本图不含实现落地。
2. **判据 = 以 DSH 为基线逐条对齐**：DSH 有而衡没有的，全部进 [`parity.md`](parity.md)，
   每条给「补齐 / 变形做 / 不做 + 理由」；**「不做」也要有结论**，不许悬着。
3. **范围五块全进**：账本结构、时间轴总览、检视器多面、搜索与折叠、长历史与行选择。
4. **轨迹页升格成可寻址的账本**：行成为一等对象，键盘可选中（`↑` / `↓` + `Enter`），
   并且是折叠、搜索命中与区间聚焦的**共同落点**。
5. **用量两处都要**：行尾保留现状（ADR 0016 不动），检视器里另加一个分拆面
   （缓存读 / 缓存写 / 推理 / 会话累计）。
6. **TTFT / 生成时长 / 吞吐**：先由[衡的数据面](issues/01-research-data-surface.md)查清；
   在结论落地之前，任何票的设计**不得押在 TTFT 上**（拿不到就画不出来，不许编数字）。
7. **模型可见文本零变化**。事件 schema 动不动，由[衡的数据面](issues/01-research-data-surface.md)
   给代价、由[时间轴与计时](issues/06-grilling-timeline.md)拍。
8. **交付一张图一次拆完**（维护者的选择）：票按依赖排序，不依赖新数据的那半先做。

### Tracker 事实与降级（本图适用）

- map = `.scratch/trace-ledger/map.md`，child = `.scratch/trace-ledger/issues/NN-*.md`；
  阻塞 = 票面 `Blocked by: NN`；claim = 把 `Status:` 从 `open` 改成 `claimed`；
  resolve = 票底写 `## 作答` + `Status: resolved` + 本文 `已定的决定` 追加一行。
- **没有 native sub-issue / 依赖边**，回退到正文约定：本文的 `## 任务清单` 逐条引用子票
  （条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/trace-ledger/map.md`。宣布图走完之前必须 PASS。
- 每个 session 至多 resolve 一张票（research 票除外）。用户可能并行跑 unblocked 的票。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**事实票**（AFK，charting 当天派出 subagent 并已收回，产物在 [`research/`](research/)）：

- [x] [衡的数据面：对齐 DSH 需要的数据有没有、缺的怎么拿](issues/01-research-data-surface.md)
- [x] [衡的绘制与输入面：这次改动能站在哪些件上](issues/02-research-draw-and-input-surface.md)

**形态与决策票**（HITL，prototype 先提 fidelity、grilling 收口）：

- [x] [账本骨架的几帧](issues/03-prototype-ledger-skeleton.md)
- [x] [账本的两级结构与每行信息](issues/04-grilling-ledger-structure.md)
- [x] [时间轴在字符格里的候选形态](issues/05-prototype-timeline.md)
- [x] [时间轴与计时](issues/06-grilling-timeline.md)
- [x] [检视器分面的形态](issues/07-prototype-inspector.md)
- [x] [检视器的面与内容](issues/08-grilling-inspector.md)
- [x] [搜索与折叠的形态](issues/09-grilling-search-and-folding.md)
- [x] [长历史与行选择](issues/10-grilling-history-and-selection.md)
- [x] [过滤这一档在界面上怎么被读出来：状态行与轴](issues/11-status-row-and-axis-readout.md)

**实现票**（`/to-tickets` 从 [`spec.md`](spec.md) 拆出的十三张，2026-10-09）：

- [x] [12 — 块身份与「按行键替换任意一行」](issues/12-block-identity-and-pane-replace-at.md)
- [x] [13 — 首 token 时刻落流，详情覆盖层分成面](issues/13-first-token-and-detail-faces.md)
- [x] [14 — 来源面与概述面](issues/14-source-and-summary-faces.md)
- [x] [15 — 用量面](issues/15-usage-face.md)
- [x] [16 — 账本的两级分组与并进虚线的组头](issues/16-two-level-groups-and-headers.md)
- [x] [17 — 行尾的数字与异常](issues/17-row-numbers-and-anomalies.md)
- [x] [18 — 键盘归属层与选中](issues/18-keyboard-layer-and-selection.md)
- [x] [19 — 层级跳转](issues/19-hierarchical-jump.md)
- [x] [20 — 搜索与过滤](issues/20-search-and-filtering.md)
- [x] [21 — 折叠三档与全折全展](issues/21-folding.md)
- [x] [22 — 时间轴的三行横带](issues/22-timeline-band.md)
- [x] [23 — 命中在轴上的底色](issues/23-hit-band-on-timeline.md)
- [ ] [24 — 文档、走查与收口](issues/24-docs-and-close-out.md)

共 **24** 张票（3 research + 4 prototype + 7 grilling + 13 implement），**24 张都走过了** ——
十一张决策票全部 `resolved`（**图已走完**）；十三张实现票里 **12 张 `done`**，最后一张
[24 — 文档、走查与收口](issues/24-docs-and-close-out.md) 收成 **`ready-for-walkthrough`**：
能自动化的那一半全部做完并跑绿，只剩 [`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md)
的 [㉙](../../docs/tui-manual-checklist.md) 那一条要人在真终端上逐项勾。实现票住在同一个目录里，
由 `/implement` 认领，不由 wayfinder 会话认领。

这一轮另有两笔**清单外**的账：**工具详情的第六面 `Schema`**（规格 §6 与用户故事 39 要它，
却没有落在任何一张实现票上；[票 24](issues/24-docs-and-close-out.md) 的「收口前的补记」记了
六条口径）与**宽度重放之后再开一条「开场」小标题的缺陷**（[票 18](issues/18-keyboard-layer-and-selection.md)
记下的旁支缺陷：`c1640cc` 修、`e092b44` 补记成已修）。

**决策票的 frontier 上没有票了 —— 这张图走完了。** 最后那张是[长历史与行选择](issues/10-grilling-history-and-selection.md)
动完选中之后才清晰的那两条 fog（状态行怎么报、命中在轴上怎么画）：等的就是「选中一动，这两处的答案
就会变」。charting 当天那张「先走 09 还是先走 10」的悬念也已解开：09 先走是对的，但不是因为编号 ——
它把折叠与过滤改成**重放时推什么**，于是「按行键替换任意一行」那一层只剩
[账本的两级结构与每行信息](issues/04-grilling-ledger-structure.md)的增量组头一个消费者，归属清楚地
落到 10 票身上。**没有 fog 剩下**：剩下的全是执行 —— 已经折成 [`spec.md`](spec.md)，
再由 `/to-tickets` 拆成十三张实现票（见上面的「实现票」）。

## 已定的决定

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [衡的数据面：对齐 DSH 需要的数据有没有、缺的怎么拿](issues/01-research-data-surface.md) — 时刻停在**块级**（事件信封的 `at` 随 `Painted` 记住），所以「每块耗时 / 回合跨度 / 组头摘要」全部不动流就能做；工具 `duration_ms` 已在手、只是没上屏与没进详情；用量五个桶在手（会话累计在 `Panel` 里、无 getter）。**拿不到的只有 TTFT / 生成时长 / 吞吐** —— 首 token 时刻在 provider / agent / 流三层都不存在；动流的最省做法是给 `MessageCompleted` 加一个可选「距调用开始的毫秒」（可选字段双向兼容，要一条 ADR；**新增事件变体则是单向不兼容**，老二进制 `read_events` 会拒读整条流）。另两条硬事实：时间轴的**「输入」泳道没有数据源**（别拿 `ContextInjected.at` 冒充）、失败行上的**错误 code 拿不到**（流上只有 `error: String`）。横切一条：**「块 → 它画出来的源行」今天没有记账** —— 折叠 / 行选择 / 区间聚焦三条线都要先在渲染层补这一层。产物 [`research/01-data-surface.md`](research/01-data-surface.md)。
- [衡的绘制与输入面：这次改动能站在哪些件上](issues/02-research-draw-and-input-surface.md) — 键盘归 `TuiState::key` 里一条自上而下的守卫阶梯，**轨迹页今天不是其中的一层**（只在「当前显示那一页」这一支拿到三键与滚轮）—— 行选择与搜索入口要先新增一层归属。行身份是**位置型下标**（`CAP` 裁剪让全体左移、宽度变化整批重放），要活过这两件事得自带稳定行键；反向寻址（源行 → 第几个显示行）今天缺失。覆盖层的面加在 `DetailView` 这层，标签条可复用既有的 `draw_label_bar`。反显通道被封（全屏唯一的反显是「键盘所在」），命中高亮用**底色**、行选中用 `ACCENT + BOLD`。轨迹页这一层还空着的键位：裸 `j` / `k`、`n` / `N`、`r`、`f`、`?`、`{` / `}`、数字键、未绑的 Ctrl 字母、右键与中键、双击（**没有识别机制**）。产物 [`research/02-draw-and-input-surface.md`](research/02-draw-and-input-surface.md)。
- [账本骨架的几帧](issues/03-prototype-ledger-skeleton.md) — 八组真帧（基线 / 四种分组画法含两级分组 / 四种行首排法 / 三种耗时挂法 / 三种失败形态 / 五种结果读法 / 四种折叠形态 / 三处无主段落），宽度照今天的实现（79 列轨迹内容宽、9 列时间戳、内容 70 列）。**帧里跳出来三个张力**：行尾挤不下耗时（一加 `· 0.2 s`，用量尾巴当场被截断）、`▸` 已被「可展开」占用（折叠要另找字形）、两级分组把一条工具行压到只剩 36 列。产物 [`prototype/03-ledger-skeleton.py`](prototype/03-ledger-skeleton.py) + [`03-ledger-skeleton.txt`](prototype/03-ledger-skeleton.txt)；维护者判定够用。
- [账本的两级结构与每行信息](issues/04-grilling-ledger-structure.md) — 拍下**两级分组 + 成员不缩进**（帧 1d 的两级缩进会把工具行压到 36 列；实测一个回合 1–557 次迭代、`TurnEnded` 与 `TurnStarted(iteration == 1)` 逐文件同数，边界可靠）。组头**并进那条虚线**（零额外行数），字段 = 起时刻 · 序号 · 跨度 · 工具直方图，超宽从右丢，摘要**随回合增长**（要 `pane` 能按行键替换任意一行）；`STAMP_COLUMNS` 仍是 9、行上不给号；工具耗时挂行尾（用量之前，溢出先丢耗时、保用量）、迭代耗时挂二级头、拿不到不画；失败 = 末尾红 `失败` + 错误首行多画一行（code 那一半不做，要动流）；结果摘要只在空输出 / 被截断时给一句；开场 / 压缩 / 新会话段三种无主段落各给小标题；消息行照旧。`parity.md` A 节九条已逐条收口；三个张力的结论都在票里（展开态一律不用 `▸`，它只留给折叠态）。
- [时间轴在字符格里的候选形态](issues/05-prototype-timeline.md) — 十二帧（脚本 + 产物，206 行）。三组候选的价位：**顶部横带** 3–4 行换全程 + 当前屏；**行内迷你条** 0 行但**必须有个分母**（按整趟 48 s 则 0.2 s 与 0.4 s 的调用塌成一格；按本回合最大值则分母会变、行与行不可比），且行尾已是最挤处（03 票张力 1）；**状态行附近** 1 行、分辨率最高（一格 0.65 s），但那 1 行是从账本 15 行里挪的（贴底跟随时盖住最新一行），并进状态行只剩 14 列。**画出来才看清三条**：分辨率是硬约束（保底画一格就是说谎，轴越宽越糟 —— 12 分钟里一格 9.7 s）、**「模型在想」切不开**（TTFT 拿不到，只能整段说「在等模型」）、**等审批是唯一切得开的内段**（`PermissionAsked.at` → `PermissionDecided.at`，但它已被算进 `duration_ms`，两个口径并排必须说清）。TTFT / 生成时长 / 吞吐 / 「输入」泳道**一帧都没有**。产物 [`prototype/05-timeline.py`](prototype/05-timeline.py) + [`05-timeline.txt`](prototype/05-timeline.txt)；维护者判定够用。
- [时间轴与计时](issues/06-grilling-timeline.md) — 五轮 grilling 十五问。**做一条固定在内容区顶部的 3 行横带**（刻度 + 模型 + 工具，输入泳道不做；行内迷你条与贴状态行两条候选都判不做），轴域 = **整趟会话**、视口用刻度行的**区间底色**标；模型泳道按**首 token 时刻**切「等首 token / 吐字」两段、**不足一格不画**；**TTFT 走动流** —— `MessageCompleted` 加可选 `first_token_ms`（首个增量，含 `ReasoningDelta`）并写 [ADR 0020](../../docs/adr/0020-first-token-time-rides-the-completion.md)，一个字段推出 TTFT / 生成时长 / 吞吐；**交互只做读法**（放弃拖选 / 缩放 / 平移 / hover，轴由行选中与搜索命中驱动、不可聚焦、不进键盘阶梯），降级阶梯 = 稀刻度 → 一行三态 → 不画；整趟跨度**增量累积**（`painted` 不裁剪）；横轴实测只有 **63 列**（帧里的 74 是 prototype 的口径）、一格 ≈ 0.76 s；`parity.md` B 节五行逐条收口。留给 spec 的形状：刻度排布、两段字形与色号、降级阈值。
- [检视器分面的形态](issues/07-prototype-inspector.md) — **24 帧、755 行**（脚本 + txt，脚本带每行显示列自检），尺寸全从布局常量算出来：100×30 → 覆盖层 **92×26**、文字区 **88 列**、标题 1 + 主体 20 + 页脚 1（**覆盖层尺寸一行都不许动**）。面加在 `DetailView` 那一层，标签条复用 `draw_label_bar`。三条候选的代价：顶部标签条最便宜（借走一行、画法现成、不需要拖拽调宽），真正的取舍是**面里的小节标题收不收**（收掉省一行，长正文滚到中段认不出在哪一面；留着换「滚到哪都认得」）；面在流里堆叠**就是今天的详情再加两节**、零新概念，代价是**没有「第几面」这个位置**（帧 2b：滚到第 601 行、页脚只说 `↕ 601/701`）；只有页脚提示主体零代价、代价全在发现性；窄屏降级后前两者只剩排版差别。画出来才看清四条：**「来源」面在字符格里唯一不可替代的是跳回去**（那行文字标题行已给，跳回去要稳定行键 = 10 票的活，帧 8b 给了「列出祖先链」这一档）；用量面与行尾那两个数**不冲突但必须分节**（会读重的是「这一趟」累计，且 `输入` 的条形 = 缓存读 + 未命中，画三条等于数两遍）；**Schema 只拿得到一半**（`ToolSpec.parameters` 在手上，`mcp_call` 转发出去的那个住在 server 那边，只能写「不可用」）；**分派表不是一张「所有面都能进」的表**（只有工具那一行用得上用量与计时两面，七种 `DetailKind` 穷举可测）。维护者过帧判定够用。产物 [`prototype/07-inspector.py`](prototype/07-inspector.py) + [`07-inspector.txt`](prototype/07-inspector.txt)。
- [检视器的面与内容](issues/08-grilling-inspector.md) — 两轮十四问。**做顶部标签条 + 一张七行的分派表**（`Tool` = 参数 / 输出 / Schema / 计时 / 来源 / 概述，`Message` = 正文 / 用量 / 概述，`Thinking` = 思考 / 用量 / 概述，`Context` = 注入 / 概述，`File` / `Diff` / `Todo` 单面、**面数 ≤ 1 就不画标签条**），**默认面永远是今天那一面**（工具默认落在参数），`Tab` / `Shift+Tab` 与点标签切（覆盖层立着时独占键盘、`Tab` 空着，不撞输入框），**各面各记自己的滚动位置**；面内小节标题**只在 ≥ 2 节时画**。四条修正 07 帧的前提：①**工具调用没有用量** —— `UsageRecorded` 带 `speaker` 不带 `tool_call_id`（`set_tail` 也只挂在消息与思考上），所以帧 5a / 5b 那个工具框是错的，用量面挂**消息与思考详情**，与行尾那两个数同源同口径（跨多次调用时「本次」就是行尾那个 `合计`）；②**原文面不做** —— 渲染层里没有 markdown 渲染函数，正文走 `folded_text` 原样折行，**今天那份就是原文面**，真缺的只有事件 JSON（要块 → 事件记账 + 读盘）；③**计时面要降级** —— 「这次模型调用」那一节依赖「工具行归属哪次模型调用」，今天没有这笔账，拿不到就只给「这次工具调用」一节；④**prompt 差异面不做**（历史 system prompt 不进流，代价是把整个请求组装重算一遍搬进渲染层）。「来源」面 = **只读的祖先链三环**（本行 → 迭代的回复 → 那一回合 → 上面那条用户消息），**不押在稳定行键上** —— 10 票若不给行键它退成只读链仍然有用；**层级跳转整体归 10 票**。`detail_opener` **不升级**（切面不改变谁被冻住，关掉照旧还原给打开方）。新立两条词条：**详情覆盖层**与**面**；不立新 ADR（改起来是局部的）。`parity.md` C 节已逐条回填。
- [搜索与折叠的形态](issues/09-grilling-search-and-folding.md) — 三轮十七问。**折叠与过滤都是「重放时推什么给 pane」**，不是改窗格（`Pane` 今天只能改最后一行，本来就没有删一段的入口）—— 于是 `Pane` 一个字节不用动，滚动与跟随由 `Pane::clear` 保留的 `top_source` 兜住。**状态**（过滤集 / 折叠集合 / 搜索串）全存 `TuiState` 侧按块身份，宽度重放后原样还在。**搜索**：`/` 进搜索（轨迹页新增一层键盘归属，排在 L5 记号菜单之前）、查询串打在**输入区**（草稿原样存着、退出还原）、每块一条索引（摘要 + 正文全文，块定稿时增量建，覆盖**整场会话**），查询 = 分词 / 大小写不敏感 / 各词 AND，未定稿的增量块不进索引。**命中落在块上** → 账本真过滤、未命中在**轴上**淡出（用 06 票那档区间底色）、组头保留不改写、跟随暂停；过滤集**只有一个**且只有搜索产生 —— **时间轴区间聚焦整体不做**（区间这个输入被取消，行选中也不产生过滤）。**折叠三档**：单位 / 迭代 / 连续工具调用（`▸` 只在折行出现，这是那字形第一次真正兑现），折行**就是那一行**、可选中可开详情，`{` / `}` 全折全展（全展还原到全折前那份折叠态），**不做双击**。键位：`↑`/`↓` 与 `j`/`k` 并存、`Enter` 开详情、`Space` 折展、`n` / `N` 跳命中并把选中移过去、`Esc` 一层层退；点一行开详情这条**明确保护**。**归属记账整层归 10 票**，本票只做到块级寻址；`parity.md` D 节已逐条回填，E 节「区间聚焦」那一行改判。
- [长历史与行选择](issues/10-grilling-history-and-selection.md) — 四轮十五问。**行的身份从事件信封的 `seq` 派生**（块身份 + 块内序号，实时与恢复两条路径同一个值；[ADR 0021](../../docs/adr/0021-line-identity-comes-from-the-event-envelope.md)），并给 `Pane` 开**按行键替换任意一行**那个今天没有的入口 —— 它是 04 票增量组头绕不开的唯一解，而 09 票的三档折叠走重放所以不需要它。**选中粒度是块不是行**，整行 `ACCENT + BOLD`（不新开通道、**不画反显**：全屏唯一的反显仍只给问卷与菜单，而选中行自己就是「键盘在这」的锚点）。**新增一层键盘归属**（判据 = 主列当前显示轨迹页且未交还，十六层里第一个读主列页签的），`Esc` 依次退「搜索 → 过滤 → 选中 → **交还键盘**」、**点账本任何位置拿回**（点行 = 拿回 + 开详情）—— 这一层**不画反显**，与左栏两页的**焦点行**同形；`j`/`k` 与 `↑`/`↓` 并存、`g`/`G` 跳头尾、**`[`/`]` 是 08 票交过来的层级跳转**（到所属组头 / 到组内第一行），翻页键与滚轮**刻意不动选中**。选中**认块不认位置**（宽度重放后原地不动）、滚出视口**不拉回来**（视口照跟随）、被过滤掉时**留着只是不画**。**`CAP = 20 000` 之后不画回来**（块全在 `painted` 里、整场会话从 `log.jsonl` 原样 replay，要救的东西没丢），DSH 的分页与「加载更早」判**不做**；**聚合条数判不做**（字符格里没有屏幕阅读器这个消费者）。`parity.md` E 节逐条回填；`CONTEXT.md` 新立**账本**、**选中**、**行身份**三条词条。事实产物 [`research/03-line-identity-and-loading.md`](research/03-line-identity-and-loading.md)（731 行，票面第 4 条那个担心被它推翻：**要救的东西没丢**）。
- [过滤这一档在界面上怎么被读出来：状态行与轴](issues/11-status-row-and-axis-readout.md) — 三轮十一问，把它走完这张图就没有 fog 了。**状态行一块都不动**（它两个视图共用，而过滤只属于轨迹页 —— 在对话页显示「n 个命中」是件假事），命中数进**内容区右下角那个浮字位**（与新内容指示器同一位、同一色、每视图各一个，现成的件）：`n 个命中`，自进入过滤以来有新命中时加 `· 新增 M`，**无匹配就写 `0 个命中`**（不为「出了事」单造一个词）；**过滤期间整句让给命中读法**，那个位置那一刻不再是可点手势位，回到最新交给 `G`（`G` 只恢复跟随、**不动过滤**，退出过滤仍归 `Esc` 的层序）。轴上：**连续命中合成一段底色**铺在模型/工具两条泳道行（间隔 < 一格即合成，阈值跟分辨率走而不是写死秒数），**刻度行的底色只给视口区间**、不与命中抢（两个具名常量、两个语义），**未命中段不调弱**（省一档色阶，而底色已经是零先例的第四个通道），**注入 / 问卷作答 / 命令那类命中不落轴** —— 搜的是整场会话、轴是两种时间的摘要，这条限制写进 `docs/render.md`；轴上的**选中标记是前景记号**、与命中底色**正交**，同格共存所以不用定谁压过谁。顺带修正 09 票一句（「状态行给 `n 个命中`」改成浮字位）。`parity.md` D 节回填两行；`CONTEXT.md` 新立**新内容指示器**一条词条；不立新 ADR。

## 尚未明确

<!-- 通往目的地、但还不够清晰到能成票的视野。解决一张票会把它前方的一片 fog 升级成新票。 -->

（当前没有。10 票走完时把剩下的 fog 全部处理掉了：两条升级成
[过滤这一档在界面上怎么被读出来：状态行与轴](issues/11-status-row-and-axis-readout.md)
（同日也走完，成了最后一张票），一条被判「不做」进了范围之下，一条本来就是 `/to-spec` 的活 ——
「这次会改到 `docs/render.md` 与 `docs/tui-manual-checklist.md` 的哪些小节」交给折 spec 那一步，
那不是一张 decision 票。**11/11，没有 fog 剩下。**）

## 范围之外

<!-- scope 边界，不是路线的一步：封闭的结论，永远不 graduate。 -->

- **图片与附件面**（缩略图、附件清单、灯箱）：衡还没有图像输入 —— 那是
  [`image-input`](../image-input/seed.md) 这条 seed 的事，这次不预支。
- **`run_code` / PTC 那一族的代码检视与子调用缩进**：衡没有工具内派发这种工具。
- **虚拟化引擎**：衡有自己的 `pane` 窗口与 `CAP`；「只挂载可见行」在字符格里不是问题。
- **像素级手势**（时间轴滚轮缩放 / 右键平移 / 边缘自动平移 / 500 ms hover；检视器拖拽调宽、
  双击复位标签条、右键切面）：
  字符格里没有这些词，取舍写进[时间轴与计时](issues/06-grilling-timeline.md)与
  [检视器的面与内容](issues/08-grilling-inspector.md)。
- **剪贴板按钮与灯箱**：衡已有拖选 + OSC 52 复制。
- **双击行折叠**：今天没有双击识别机制（`DRAG_THRESHOLD` 只分「点击 vs 拖选」），折叠走
  **点组头字形**与 `Space` 就够了。见[搜索与折叠的形态](issues/09-grilling-search-and-folding.md)。
- **分页 / 「加载更早的历史」/ 滚到顶部自动加载 / 行键在 prepend 时的稳定性**（DSH 的 E 节那一整
  行）：**要救的东西没丢** —— 块全在 `painted` 里、`CAP` 裁的只是折好的显示行，而整场会话从
  `log.jsonl` 原样 replay（`seq` 还是同一个 `seq`）。真要画回来只有一条路：`pane` 记总量与视口、
  要滚回去就重放到那一行，那是 `Pane` 的结构改造加 O(k) 重放。代价是超过 2 万条源行的会话翻到
  顶也看不到头（今天就有测试钉住）。见[长历史与行选择](issues/10-grilling-history-and-selection.md) §10。
- **聚合条数**（DSH 的 `aria-rowcount`，写成「共 N 行 / 已加载 M 行」）：字符格里**没有屏幕阅读器
  这个消费者**；而且上一条定了不加载更早之后，「已加载」那一半恒等于总数 —— 一句恒真的信息不如
  不写。见[长历史与行选择](issues/10-grilling-history-and-selection.md) §11。
- **`Enter` / `Space` 选中行**（DSH 用这两个键选中，衡把 `Enter` 留给「开详情」、`Space` 留给
  「折 / 展」）：与「点一行开详情」那条既有语义对齐，选中绑在方向键上。见
  [长历史与行选择](issues/10-grilling-history-and-selection.md) §13。
- **时间轴区间聚焦**（在轴上拖一段、把账本聚到那一刻）：轴**不产生过滤**，视图里唯一的过滤集
  来自搜索命中；「未命中淡出」由搜索命中在轴上兑现。见[搜索与折叠的形态](issues/09-grilling-search-and-folding.md)。
- **深链接与选择持久化**：DSH 自己也没有（记录选择与时间轴区间都是视图本地状态）。
- **thinking 的固定字号排版**：像素排版，字符格里无对应物。
- **system prompt 面 / 工具目录面 / prompt 差异面**：历史 system prompt 不进流，想给「前后差异」
  只能靠 `replay::replay` 从流重算每个请求的 `messages` —— 为一个窄读法把整个请求组装搬进
  渲染层，代价已知而不划算；注入详情已经给了正文的本来样子。见
  [检视器的面与内容](issues/08-grilling-inspector.md)。
- **事件的原始 JSON（DSH 的 Raw 那一档）**：消息 / 思考 / 注入的正文**本来就是**流上的原文字符串
  （渲染层里没有 markdown 渲染），工具输出也有落盘全文 —— 缺口只剩事件信封的 JSON，而它要
  「块 → 源事件」的记账加读盘。见[检视器的面与内容](issues/08-grilling-inspector.md)。
- **模型面**：一块都不动 —— 轨迹是视图，模型可见文本与投影零变化。

## 进度

**决策 11/11 全部 `resolved`，图走完了**（2026-10-09）。
charting 当天派出两张 research 票的 subagent、当天收回，结论已回填 [`parity.md`](parity.md)；
同一天走完 [账本骨架的几帧](issues/03-prototype-ledger-skeleton.md)（八组真帧，产物在
[`prototype/`](prototype/)）与 [账本的两级结构与每行信息](issues/04-grilling-ledger-structure.md)
（三轮 grilling 十问：帧里那三个张力都在那里收了口，`parity.md` 的 A 节同时逐条收口）；
[时间轴在字符格里的候选形态](issues/05-prototype-timeline.md) 也在同一天收（十二帧，维护者判定
够用，产物 `prototype/05-timeline.*`），[时间轴与计时](issues/06-grilling-timeline.md) 随后收口
（十五问 + [ADR 0020](../../docs/adr/0020-first-token-time-rides-the-completion.md)）。
[检视器分面的形态](issues/07-prototype-inspector.md) 是第四张形态票：**24 帧**（脚本带每行显示列
自检，覆盖层 92×26 / 文字区 88 列全从布局常量算出来），维护者过帧判定够用，产物
`prototype/07-inspector.*`；[检视器的面与内容](issues/08-grilling-inspector.md) 两轮十四问收口
（输入就是那 24 帧）：定下标签条 + 七行分派表 + 默认面 + 切面键 + 各面各记滚动，并**推翻了帧
5a 的一个前提** —— 工具调用没有用量，用量面改挂消息与思考详情（`parity.md` 的 C 节逐条回填，
`CONTEXT.md` 新立**详情覆盖层**与**面**两条词条）。
[搜索与折叠的形态](issues/09-grilling-search-and-folding.md) 三轮十七问收口：折叠与过滤都改成
重放，`Pane` 一个字节不用动。
[长历史与行选择](issues/10-grilling-history-and-selection.md) 四轮十五问收口（另派一张事实票，
产物 `research/03-line-identity-and-loading.md`）：**行身份从 `seq` 派生** + [ADR 0021](../../docs/adr/0021-line-identity-comes-from-the-event-envelope.md)，
选中粒度是块，新一层键盘归属与 `Esc` 交还 / 点账本拿回，`[`/`]` 层级跳转（08 票交过来的），
`CAP` 之后不画回来（**票面第 4 条那个担心被事实推翻**：要救的东西没丢），`CONTEXT.md` 新立
**账本**、**选中**、**行身份**三条词条。
那张票走完时把剩下的 fog 清干净了，只剩一张
[过滤这一档在界面上怎么被读出来：状态行与轴](issues/11-status-row-and-axis-readout.md)
—— 它是 09 票 §13 那两条 fog 加上 10 票动的选中之后才清晰成票的（状态行怎么报、命中底色在轴上
怎么排布）。它同日走完（三轮十一问）：**状态行不动**，命中数进内容区右下角那个浮字位，轴上用
**合成一段底色**、刻度行留给视口、未命中不调弱、不属模型/工具的命中不落轴。**11/11，没有 fog
剩下**：交给 `/to-spec` 折成构建计划，再用 `/to-tickets` 拆实现票。
