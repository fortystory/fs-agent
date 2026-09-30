# 模型可见与进流的文本也走中文：语言按「是不是标识符」分，不按「谁读它」分

**决定**：把语言线**再画一次**，换成一条按**词性**判的线 —— **英文只留给不是散文的东西**：① 标识符（类型、函数、字段、文件名、CLI 旗标、环境变量、事件名与它的字段名、`tool_call_id` 这类值）；② 协议标记与 schema 值（`CONCLUSION:`、`Ask` / `Allow`、`cwd` / `token` / `assistant`）；③ 路径、命令原文与代码片段；④ `docs/research/` 里的一手引文（一个字不改）。

**除此之外的散文一律中文，不问谁读它**：工具声明与描述（`ToolSpec.description`、各参数的 `description`）、工具结果（fs-agent 自己生成的**错误文本**、写入回执、截断说明、shell 的 `exit code:` / `--- stdout ---` 那一批）、`AgentError.message`、`SessionError.detail`、`PermissionDecided.reason`、`HistorySuperseded.summary`、skills 目录与 `repo_map` 的省略说明、上下文丢弃与预算说明、执行者的身份提示与收尾汇总。

这条**取代** [ADR 0001](0001-chinese-ui-frozen-model-text.md) 里「模型可见文本冻结成英文」的那一半与 [ADR 0004](0004-prose-in-chinese-identifiers-and-model-text-in-english.md) 里「英文只留给三类东西」那张清单（ADR 0004 的「被否决的替代方案」里明写「不做」，本文就是把它重新打开）。**ADR 0001 的另一半原样保留、且仍然最硬**：**一个会话的前缀在会话内只增不改** —— 中途切 `reasoning_effort`、中途改 `tools`、逐轮重写 system 都还是禁止的。本文改的是**字面量本身**，也就是新会话前缀头里那几段文字，不是「会话进行中动前缀」。

## 为什么重新打开

1. **这一侧的读者不止模型 —— 而人读的是同一份文本。** 工具结果与错误文本既进 `messages`（`project()` 投成 `tool` 消息），又进转录与详情弹窗：`src/render/tui.rs` 的 `DetailKind::Tool` 把错误原文 `pane::wrap_text(error, width)` 直接画出来（成功的调用则读 `session_dir` 里落盘的全文）。按「谁读它」划线，代价落在人这一侧 —— 详情弹窗里那段 `read before write: … has not been read in this session; read it first`（`src/tools/registry.rs:40` / `:257`）就是这么来的。
2. **那条线已经在漏，而且是我们自己承认过的。** `AGENTS.md` 在 2026-09-30 翻成中文（`76c1d7f`），理由是「它确实是注入进 `messages` 的 `ContextInjected`，但那一类指的是 provider 按 schema 读的、格式敏感的串，不是这份给人写、给人读的约定」；`src/agent.rs` 的四段身份提示、`src/discussion.rs` 的讨论者身份、`PERMISSION_ASK` 那几条、投影的 `[轮 N · 名字]` 前缀**早就是中文**，模型一直在读中文。ADR 0001 真正要保的是「provider 按 schema 读的、格式敏感的那一半」—— 那是**标识符那一类**，本文一条都没动（见下「代价」第 3 条）。
3. **代价的性质是「每个版本一次」，不是「每个会话一次」。** 前缀缓存按 `prompt_cache_key` = 会话 id 命中（`.scratch/fs-agent-v1/spec.md` §4）：翻字面量只让**升级之后的新会话**用上新前缀，老会话照旧命中自己那份。翻 `AGENTS.md` 时已经付过这笔钱，当时判定值得；本文只是把它推广到同一侧的其余文本。
4. **不翻的替代方案更脆。** 想让人眼里的英文消失，就得在渲染层再造一张「英文约定文本 → 中文说明」的映射表：同一个事实两处维护，而且那张表依赖被冻死的英文**不许漂** —— 而 frozen 这一侧的英文一直在漂（第 2 条那些就是），映射表会跟着烂。让文本本身就是中文，只有一处真相。

## 代价（如实写）

1. **老流永久中英混排，且不可逆。** 升级前写下的工具结果、错误、`reason` / `detail` 都是英文，`--continue` 重放出来与新流混着；升级后写下的才是中文。与 ADR 0001 记的那一笔同性质，只是方向反过来。**这不是缺陷，是选择的代价。**
2. **新会话的前缀缓存各未命中一次**（每个 provider、每个新会话一次），升级那一刻之后老会话继续跑时，模型看到的语言会换一次。spec §17 禁止的是「会话内改」，不是「版本间改」。
3. **标识符那一侧一个字都不许跟着翻**：`tool_call_id`、事件名（`ToolCallCompleted`）、`Type:` / `Status:`、`Ask` / `Allow` / `Deny`、`cwd` / `token` / `assistant`、CLI 旗标（`--continue`）、协议标记（`CONCLUSION:`）—— 它们参与匹配、落进 schema、与上游契约对齐。翻它们只会让代码与流里的名字对不上。**语言按「是不是标识符」分**，这条不松。
4. **约定的可观测文本要整条翻**：`READ_BEFORE_WRITE_PREFIX`（`read before write: `）、`edit match level: `、`WROTE_PATH_PREFIX`、`TIMEOUT_PREFIX` 这些不是普通散文，而是**产生它的地方与统计它的查询共用**的约定文本（spec §18：前缀一漂，计数悄悄变成零）。要么整条翻、两边一起改，要么整条留，不许只改一半。
5. **测试与护栏的连带**：`tests/` 里大量断言按英文文本比对（`error.contains("read before write")`、`tests/observe.rs` 里逐字的工具结果、provider 适配层断言工具声明），要改成断语义或改到新的中文约定文本 —— 断言里**被断言的字面量（测试数据）与 fs-agent 生成的文本要分清**，后者才是要翻的。`scripts/check-language.py` 的第一条检查（冻结面无 CJK + 白名单）随之**反过来**：那一侧的中文串数变成**只许上升的棘轮**，英文散文串数变成**只许下降的棘轮**（见「执行」）。
6. **模型那一侧的效果没有先例可抄**：工具声明与描述是模型用来选工具、拼参数的指令，中文更短，但**没有证据**说它更好或更差。身份提示与讨论前缀已经是中文，说明这条路走得通；翻完要跑一遍 `cargo test` 与一次真实会话，效果记在这条 ADR 的「执行」里。

## 被否决的替代方案

- **只在渲染层加中文说明**（弹窗与转录对已知的英文文本先出一行中文、原文照旧透传；形状照 `SessionError` 的「按 `code` 映射、`detail` 透传」）：**这是这次本来要选的保守方案**，被否的理由见「为什么」第 4 条 —— 同一个事实两处维护，且映射表押在「冻结面的英文不漂」上，而那一侧一直在漂。
- **只翻工具结果与错误，不翻工具声明与描述**：它们在同一条线上（都是模型可见的散文）。只翻一半，`messages` 里就会中英相邻，以后没人分得清「哪些中文是刻意的」。
- **全量翻译、连标识符一起**：见「代价」第 3 条。名字是要参与匹配与契约的，语言线必须在「词性」上停下。

## 执行（2026-09-30 起，分批落地）

分五批，每批一次提交、`cargo test` 全绿、`check-language.py` 的两条棘轮同步收紧（只许往上 / 只许往下）：

1. **护栏重立**：`check-language.py` 的第 ① 条由「冻结面无 CJK + 13 条白名单」改成两条棘轮 —— **中文串数只许上升**（起点 = 今天实测的 13）、**英文散文串数只许下降**（起点 = 今天实测的 163，判据是「≥3 个英文词、含空格、不含中文」）。白名单与「混住文件里必须留英文的 12 条」两条旧检查退休：前者因为那一侧不再冻在英文，后者因为它的作用（那几条串必须仍在、且仍是英文）已经由 `cargo test` 的断言接管。
2. **`src/tools/*`**：工具声明与描述、工具错误与回执（用户最先看到的那一批）。
3. **`src/agent*`**：身份提示里剩下的英文段、`AgentError.message` 那一批、执行者的身份与收尾汇总、hooks 的阻塞说明。
4. **`src/provider*` / `src/context*`**：provider 的告警与错误、投影的截断说明、skills 目录、`repo_map` 与上下文预算的说明。
5. **剩余与收口**：`src/permissions.rs`、`src/questions.rs`、`src/discussion*`、`src/events.rs`、`src/hooks.rs`；收尾把两条棘轮提到实测值，并把 `docs/`、README《约定》、`docs/agents/` 里描述这条线的段落一起对上。

## 进度与收口（2026-09-30，同日晚）

**②–⑤ 批已落地**，这一侧的中文串 **26 → 219**、英文散文 **199 → 27**（`src/tools/*` 95 → 0，其余 104 → 27）。分三次提交：护栏重立（`2c382ca`）、`src/tools/*`（`a38fef7`）、其余（本提交）。验收每次都是 `cargo test` 757 passed / 0 failed、`clippy --all-targets` 干净、`cargo fmt --check` 零漂移、`check-language.py` OK。

**剩下的 27 条没有一条是散文**，全是棘轮判据的假阳性，所以上限就收在 27：

- `fs-agent: {message}` / `fs-agent: {error}` 这类**程序名前缀** 25 条（`src/cli.rs`）—— 判据按英文词数算，`fs-agent` 被劈成 `fs` + `agent`，再加一个占位符名就够三个词。它是标识符那一类（票 01 当年就把它记成「前缀与字段名」，刻意不动）。要把它从计数里清掉得动 CLI 输出的标点（25 处 ASCII 冒号换全角 `：`，与 `wording::startup_banner` 的 `fs-agent：会话 …` 对齐）—— **那是另一件事，没做**；
- 两条纯 `format!` 骨架：`→ {tool_name}({rendered})`（`src/provider/projection.rs`）、`{text}{separator}{display}: {}`（`src/context/repo_map.rs`）。

**刻意留在英文的三个字段值 / 标记**（ADR 0005 的「决定」把它们归进「schema 值」与「协议标记」）：

- `hook_format::FAILED_PREFIX = "failed: "` 与 `FEEDBACK_PREFIX = "feedback: "`（`src/events.rs`）：它们是 `HookExecuted.outcome` 这个**字段值**的头，而且 `failed: ` 被 `src/session/observe.rs` 当成**计数依据**读（正是「代价」第 4 条说的那种约定文本）；
- 投影里给模型看的 `[hook feedback]` 标记（`hook_format::FEEDBACK_MARKER`）：与 `CONCLUSION:` 同族。

于是结果里会出现 `failed: 钩子超时` 这种**英文头 + 中文身**的形状 —— 这是刻意的：**头是机器读的，身是人读的**。要改成中文就得把这几处一次改齐（常量、`observe` 的计数、投影、以及 `tests/hook_mount_points.rs` 里那几条断言，其中 `!contains("[hook feedback]")` 一起改了才不会变成永远为真）。

**实测到的一个副作用（记账）**：`context::estimate_tokens` 是「字符数 ÷ 4」，对中文**低估**，所以同一份预算现在能装进更多内容（技能清单表头 26 → 13 估 token 等）。`tests/context_budget.rs` 有一条夹具的预算按新的生成文本重调过（190 → 185），**断言本身没放宽**（仍然断言「整个旧回合走掉、不留替身」），理由写在那条夹具的注释里。其余预算类测试不需要动。

**顺手统一的两处拼接**（同一个可观测串里翻一半会成中英拼盘）：`src/permissions.rs` 的 `reasons.join("; ")` → `join("；")`；`src/agent.rs` 的 `annotate_reason` 由 `"{reason} {note}"` 改成 `"{reason}{note}"`（现在是 `模式 ask：写要问用户（钩子收紧了这个裁决）`）。

**收口时改到的活文档**：`docs/executor.md`（围栏块与那张图里的执行者报告格式）、`docs/adr/0003` 的 reason 引文（加日期注，不假装当年就是中文）、`docs/bash.md` 与 `docs/observability.md`（第 ② 批）、`src/render/wording.rs` 与 `src/render/plain.rs` 里那几处按 ADR 0001 写的注释。README《约定》、`AGENTS.md`、ADR 0001 / 0004 的「后加」一节在护栏那一批就对上。

**没做、也不打算做的**：`.scratch/` 里老票与老 spec 正文中逐字引用旧文案的地方（它们是**当时的记录**，按仓库惯例不重写 —— 谁读到时对照本 ADR 即可）。**留给真机的一件**：模型侧的实际效果（工具声明与描述现在更短）没有实测记录，ADR 0005 的「代价」第 6 条说了要跑一次真实会话再回来记。

## 后加（2026-09-30，同日）：模型自己产出的思考也在内

上面那张清单讲的是 **fs-agent 生成的**文本，漏了一处：**模型自己产出的思考**。它没有独立事件，但随 `MessageCompleted.reasoning` 落进流，也随 `StreamEvent::ReasoningDelta` 到达渲染器 —— 在 TUI 里它是一条 `[名字] ▸ ✓ 思考完成` 的行，点开是一个写着 `── 思考 ──` 的详情弹窗（`DetailKind::Thinking`）。**读者还是人**：那个弹窗按 `pane::wrap_text` 把原文画出来。所以按本文同一条线（英文只留给不是散文的东西），它也该是中文。

**决定**：一句 `agent::THINKING_IN_CHINESE` 定义在 `src/agent.rs`，四段身份各自拼上它 —— 本程序（`agent_identity()`）、讨论者（`discussion::debater_identity()`）、合成器（`discussion::synthesizer_identity()`）、执行者（`agent::executor::executor_identity()`）。一处定义、四处引用：在四段身份里各写一遍，就是同一个事实四处维护 —— 正是本文「为什么」第 4 条否掉映射表的同一条理由。它拼进 `system` 提示本身，不另发一条消息：`messages` 的第一条就是身份（`build_messages`），多一条消息就多一处要与 provider 和 `replay` 对齐的格式。

**为什么它比别处更容易漂**：推理里夹着大量标识符、路径与代码片段，模型在这些东西中间默认就滑回英文。这是实测到的现象 —— 本文落地当天，人在详情弹窗里读到的就是英文思考。

**代价**：四段身份都在缓存前缀的头部，所以加这一行让**升级之后的新会话**各未命中一次（老会话照旧命中自己那份 ——「加行」正是本文保留的那条 ADR 0001 规矩所允许的改动）。为了让四段共用一处真相，`agent_identity()` 与 `executor_identity()` 的返回类型从 `&'static str` 变成 `String`，与讨论者 / 合成器那两句一致：`concat!` 只吃字面量，拼不上一个 `const`。预算类测试不用改 —— `tests/context_budget.rs` 的 `caps_with_usable_input` 本来就按当前身份的长度算。

**别处的连带**：`AGENTS.md` 顶部新增《语言》一节（项目级约定，也是人读得到的那一份）；README《约定》第一条列进了它；新增 `tests/thinking_language.rs` 断言公开的三段身份都拼上了这一句，第四段（执行者）在 `tests/executor.rs` 里从一次真实派发发出去的请求里断言 —— 它的身份不在公开 API 上。

**效果留待真机**：这一句能不能让思考真的走中文，没有实测记录 —— 它与本文「代价」第 6 条那条待办（跑一次真实会话再回来记）是同一笔账。
