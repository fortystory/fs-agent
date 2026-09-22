# grilling：保真度、面板与 header 的历史重建

Type: grilling
Status: resolved
Blocked by: 02
Part of: ../map.md

## Question

定下「重播出来的转录长什么样」与「重播顺带重建哪些新界面状态」。

**冻结输入**（charting）：**原样重播全部块**（消息 / 工具 / 思考提示 / 权限询问与裁决 / hook / usage / 轮次分节），不做「只给人看的子集」过滤；**重建信息面板**与 **header 的模式**；banner 在重播完成后追加。

## 需要定

1. **保真度的边界**：`Transcript` 吃 `RenderEvent::Logged` 就会产出与 live 相同的 `Block`——确认这是「原样」的实现口径；有没有**必须例外**的块（例如合成器的 reasoning、执行者的事件、`SessionStarted` 骨架）？live 时它们长什么样，历史里就必须长什么样。
2. **信息面板**：重播的 `UsageRecorded` 累加是否等于历史总用量（与 live 的累加器同一口径：`cached`/`miss` 是 `input` 的拆分，不重复计）；`last_input`（上下文分子）取历史里最后一条 usage；`turns` 数历史里的 `TurnEnded`。**与 live 的衔接**：重播的用量只累一次，重播完成后继续累加新事件。
3. **header 的模式**：从历史里**最后一条**模式事件推（`ContextInjected{PlanMode}` → 计划；`HistorySuperseded{ModeChange}` → 询问）；没有模式事件时保持注入初值「询问」。给出精确规则与「历史里进过计划模式又出来」的例子。
4. **`SessionFacts` 的一致性**：`session_id` / `cwd`（实为会话目录）/ `model` / `context_window` / `budget_limit` 在重开时本来就来自组装期——确认它们与历史内容不冲突（例如历史里的讨论会话 model 是两人串）。
5. **重开后的滚动位置**：重播完成后是否强制吸底（冻结项与 `Not yet specified` 都指这里）；重播期间的上滚是否打断（结合票 01 的分帧）。
6. **空 / 极短 / 全是讨论的会话**：空日志、只有 `SessionStarted`、只有讨论轮次——各自重播后屏幕上是什么。
7. **与 live 的视觉连续性**：重播结束后，live 的流式尾部 `live` 缓冲从空开始；有没有需要复位的状态（`follow` / `seen` / `indicator`）。

## 先读

- `map.md` 的 Notes（冻结项）
- `src/render/tui.rs` 的 `apply` / `panel.observe` / `mode` 更新分支 / `draw_transcript`
- `src/render/transcript.rs` 的 `push` / `push_logged`（块的唯一决定者）
- `src/render/panel.rs` 的 `observe` / `lines`
- `src/render/wording.rs` 的 `context_*` / `token_pair` / `mode_field`
- `research：分帧重播的接缝事实与成本实测`（票 02）的答案
- `.scratch/tui-ux/issues/02-grilling-collapse-and-detail-contract.md` 的 Answer（块与提示行的契约）

## 答案落点

契约级：重建清单（哪些状态、从哪条事件、什么口径）+ 衔接规则 + 边界表。不要写实现代码。

## 进度

**100%** —— 完成。一轮 grilling、4 条决定，票面 7 个「需要定」全部覆盖；契约见 `## Answer`。

**下一步**：无（已 resolved）。票 04（历史详情）与票 05（验证）引用本票的重建口径与分隔行规则。

## Answer

**契约（2026-09-23，4 条决定）。**

### §1 保真度：块的来源与边界

- 「原样」的实现口径 = 逐条 `apply(RenderEvent::Logged(event))`，块由 `Transcript` **唯一**决定（票 02 事实 5/21）——不另写过滤器。
- **只有已记录事件能重播**：`Notice` / `Diagnostic` 是**渲染层事件、不落日志**（`src/render/mod.rs:74-90` 的注释），所以 banner、技能加载、渲染器丢弃、恢复诊断这些**历史里没有**、也不重建。
- 例外与说明：
  - `SessionStarted`：不产块（骨架事件）。
  - 合成器（`System`）的 reasoning：`MessageCompleted.reasoning: None`（tui-ux research 事实 6/23），所以历史里**没有**「思考完成」行——与 live 一致（live 的增量也不留）。
  - 执行者的 `ExecutorSpawned` / `ExecutorFinished`、权限询问与裁决、hook、usage、轮次分节：照 live 原样产出。
  - 悬空 `tool_call`：恢复时**已写进日志**的合成失败结果会照常重播（票 02 事实 30-32）。
- **历史分隔行（用户的决定）**：重播结束、flush live 缓冲**之前**，插一条 `Notice`——`wording::history_divider()` = **`── 以上为历史 ──`**（`DarkGray`）。
  - 规则：**仅当这次重播至少产出一个块**才插；空日志 / 只有骨架时不插。
  - 它是渲染层合成的行，**不落日志**，所以下一次 `--continue` 不会重复出现旧的（会新插一条）。
  - 顺序固定：`[历史块] → [分隔行] → [缓冲的 live 事件：banner / 恢复诊断] → …`。

### §2 信息面板

- 重播逐条 `apply` → `Panel::observe` **自然累加一次**（票 02 事实 34/35）；`last_input` = 最后一条 `UsageRecorded.input_tokens`；`turns` = `TurnEnded` 数。
- 口径与 live 完全一致：`cached` / `miss` 是 `input` 的拆分，**不重复计**。
- 与 live 的衔接：**不重置**；重播完成后继续累加新事件。
- `budget_limit` / `context_window` 来自组装期（**当前**配置）：若配置变过，历史 token 数与当前预算可能不自洽——**如实显示、不做调和**（记为已知代价）。

### §3 header 的模式

- 逐条 `apply` 顺序更新（票 02 事实 36）：`ContextInjected{PlanMode}` → 计划；`History{ModeChange}` → 询问；其余块不动模式。
- 「进过又出来」的例子：`PlanMode` 注入后跟一条 `ModeChange` → 重开后是**「询问」**。
- **恢复补写的 `ModeChange` 也算**：被杀在 plan 模式的会话，`OpenedSession::start` 会补一条（`src/lib.rs:313-324`），所以重开后显示**「询问」**——与 harness 实际策略（重开从配置模式开始）一致。
- 没有任何模式事件 → 保持注入初值「询问」（`TuiState::new`，`src/render/tui.rs:769` 一带）。

### §4 `SessionFacts` 的一致性

- 五个字段都来自组装期、取**当前**值；与历史内容不冲突（讨论会话的 `model` 本来就是两个模型名的串，`src/cli.rs:596-599`）。
- 唯一要记住：`cwd` 装的是**会话目录**、不是 workspace（票 02 事实 16）——本图与票 04 都不把它当工作目录。

### §5 落点与滚动

- 重播期间吸底（票 01 §3）；**完成后仍吸底**：落在最新一行；`follow = true`、`seen = total`、`indicator = None`（不显示「到最下」）。
- `live` 缓冲在结束时为空；`top_source` 指向末尾。

### §6 边界表

| 会话 | 重播后 |
| --- | --- |
| 空日志 / 只有 `SessionStarted` | 0 块 → 不进重播态（票 01 §2）、**不插分隔行**、转录为空 |
| 只有 1 条消息 | 1 块 → 插分隔行，吸底 |
| 只有讨论轮次 | 轮次分节线 + 发言块照常；面板 / 模式同规则 |
| 历史末条是悬空调用的合成结果 | 显示 `ok: false` + `INTERRUPTED` 文本（票 02 事实 30-32） |
| 历史里有 `outputs/` 缺失的工具调用 | 转录不受影响（只有详情层用到，见票 04） |

### §7 给下游

- 票 04：分隔行是转录里的一个 `Block::Notice`，**不可点**（没有详情）。
- 票 05：要断言「历史块 → 分隔行 → banner」的**顺序**，以及面板 / 模式的最终值。
- `wording::history_divider()` 落 `src/render/wording.rs`。
