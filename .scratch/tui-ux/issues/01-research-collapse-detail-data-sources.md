# research：折叠与详情层的数据来源与命中接缝

Type: research
Status: resolved
Blocked by: —
Part of: ../map.md

## Question

为 `grilling：折叠与详情覆盖层的交互契约`、`prototype：折叠提示行、详情覆盖层与工具行的形态`、`grilling：鼠标点击作答的命中与焦点契约` 提供**本仓库源码级事实**。

判据：**只报告事实与来源，不推荐方案、不选赢家。** 每条事实给 `文件:行号`；一手来源未写明者标 ⚪ 未证实，不做推断。

**已冻结、不必重查**：reasoning 沿用 `MessageCompleted.reasoning`，不新增事件；工具全文在 `outputs/<tool_call_id>.txt`；只动 TUI。本票只回答「这些数据现在怎么流动、命中要用哪些结构」。

## 需要查明

1. **`MessageCompleted.reasoning` 的全链路**：`src/events.rs` 的定义与序列化字段 → `src/agent.rs` 的写入点与**写入条件**（哪些回合会写、只有工具调用没有正文的 iteration 会不会写、空 reasoning 怎么处理）→ `src/render/mod.rs` 的 `RenderEvent` / `Delta` 路径（完成态的 reasoning 有没有单独的渲染事件，还是只挂在 `Logged(MessageCompleted)` 上）→ `src/render/transcript.rs` 构造 `Block::Message` 时是否丢掉 `reasoning`（现在是 `..`）→ `src/render/tui.rs` 的 `apply`。
2. **谁消费 `Block`**：`src/render/plain.rs`、`src/render/headless.rs`、`src/session/observe.rs`、`src/agent/replay.rs` 分别用不用 `Block`；若给 `Block::Message` 加一个字段，哪些消费者必须动、哪些会静默忽略。回放（`--continue` / `sessions replay`）走的是哪条路径，reasoning 能否据此重建。
3. **增量到达顺序**：reasoning delta 与 text delta 的相对顺序与 speaker 归属；同一 iteration 内会不会交错；一个回合里多个 iteration 各自是否都发 `MessageCompleted`。以 `src/agent.rs` 的流循环为准。
4. **工具输出的取数与指针**：`src/context.rs` 的 `truncate_result` / `preview` / `ResultText`（`preview`、`truncated`、落盘路径）；指针字符串的**确切格式**（能否从中解析出 `outputs/` 下的文件名）；`outputs/` 目录在组装期哪里已知（`src/cli.rs` 的会话目录构造、`src/session/store.rs`）；`/undo` 用 `.before` 的命名约定（`src/tools/edit.rs`）；指针失效时现有代码怎么降级。
5. **转录的可命中结构**：`src/render/pane.rs` 的 `lines` / `starts` / `wrapped` / `wrapped_sources` / `width` / `top` / `top_source` / `follow` / `total` / `seen` 各自语义，以及 `view()` / `window()` 怎样把「显示行」映射回「源行」；`src/render/tui.rs` 的 `draw_transcript` 取窗口、滚动条矩形、指示块矩形的计算路径。**现有 `indicator` 的「绘制时记录矩形、`mouse()` 里命中」是最接近的先例**——把它的完整调用链写清楚。
6. **两种问答绘制的行坐标**：中段覆盖层 `draw_modal`（`src/render/tui.rs`）与底部问卷 `draw_questionnaire` / `questionnaire_parts` / `option_window_start`：每一行、每个选项在绘制时的 y 坐标能否拿到；`Questionnaire` 的 `drafts` / `highlight` / `index` 与选项窗口的关系；自定义文本行与翻页行的位置。
7. **注入面与状态**：`TuiOptions` / `SessionFacts`（`src/render/tui.rs`）现在注入什么、组装点在哪；`TuiState` 持有哪些可变状态；若要给详情覆盖层一个只读的「取工具全文」能力（读 `outputs/` 或经端口），现有哪些接缝可复用（对照 `ConsolePort` / `Asker` / doc 里的注入纪律）。
8. **DSH 的点击作答先例**（**不必重查 DSH 产物**）：直接引用 `.scratch/ask-user-question/research/01-dsh-ask-user-question-and-composer-takeover.md` §3 里已核实的事实（单选点击后自动前进、多选切换、自定义文本、`Esc` 取消、答案取原始 label），标明出处行号。

## 产出

`.scratch/tui-ux/research/01-collapse-detail-data-sources.md`：

- 来源清单（本机路径 + 行号；外部来源给 URL + 抓取日期）
- 逐条事实，每条一句话说明它**约束了哪张票的哪个决定**
- 结尾一节「⚪ 未证实 / 查不到」如实列出

**不写建议、不选赢家。** 答案必须自足（`/implement` 在 `/clear` 后读它）。

## Comments

- 2026-09-23 charting：本票由 charting 会话的后台 research subagent 认领（`Status: claimed` 由认领者落笔）。

## Answer

已按票面八问逐条给出源码级事实，完整答案见 [research/01-collapse-detail-data-sources.md](../research/01-collapse-detail-data-sources.md)（68 条编号事实 + 8 条「⚪ 未证实 / 查不到」）。要点：

- **reasoning 全链路**：`MessageCompleted.reasoning: Option<String>`（`src/events.rs:316-320`，入流前打码 `src/events.rs:442-449`）只在 assistant iteration 写入，空串被转成 `None`（`src/agent.rs:563-572`）；**只有工具调用没有正文的 iteration 也会写**（`tool_calls` 非空即触发），但 `render_block` 对空正文的 assistant 消息返回 0 行（`src/render/tui.rs:2110-2112`）。完成态 reasoning 没有独立渲染事件，只能挂在 `RenderEvent::Logged` 上；断点唯一：`Transcript::push_logged` 构造 `Block::Message` 时用 `..` 丢掉了它（`src/render/transcript.rs:277-283`），而 `Block::Message` 当前只有 `speaker/role/text`（`:36-40`）。
- **加字段谁必须动**：`plain.rs` 的 `Block::Message { speaker, role, text }`（`src/render/plain.rs:81-85`）与 `tui.rs` 的 assistant 分支（`src/render/tui.rs:2105-2109`）都是逐字段无 `..`，会编译失败；`tui.rs` 的非 assistant 分支（`:2121`）、`panel.rs`（只 match Usage/TurnEnded）、**headless**（不走 `Block`，直接 match `EventPayload`，`src/render/headless.rs:96-149`）、`observe.rs`（自建 `Entry`，`src/session/observe.rs:500-507`）都不受影响。`agent/replay.rs` 与 `--continue` 都不重播转录到 TUI：全仓唯一 `render.logged` 在 `append_event`（`src/agent.rs:2148`），所以恢复会话后 `pane` 从空开始。
- **工具全文取数**：事件里只有 preview（`src/agent.rs:1981-1987`），指针从未入事件；`outputs/<tool_call_id>.txt` 由 `tool_call_id` 现算（`src/context.rs:423`），preview 文本里也带 `full output at <path>`（`:462-466`）。`outputs/` 目录在库侧可得（`src/lib.rs:231-234`、`Session::outputs_dir`）且 `SessionFacts.cwd` 实际就是**会话目录**（`src/cli.rs:320-327`），`facts.cwd.join("outputs")` 即可拼出——但渲染层现在完全不碰文件系统。`.before` 命名在 `src/tools/file.rs:47-49`，`/undo` 缺快照是硬错误（`src/agent/history.rs:172-182`），而「死指针降级为 preview」目前只是设计声明、无运行时实现。
- **命中接缝**：`Pane` 只有行级结构（`lines`/`starts`/`wrapped`/`top`/`top_source`/`follow`/`total`/`seen`，`src/render/pane.rs:33-57`），显示行 → 源行经 `starts`（`window()` `:254-271`，`sync_top_source()` `:245-250`），**没有块级索引**。indicator 的「绘制时记 `Option<Rect>`、`mouse()` 里包含判断」是唯一先例（字段 `src/render/tui.rs:377-379`、绘制 `:1835-1865`、命中 `:871-898`、清空 `:1498-1502`/`:1532-1535`、测试 `tests/render_layout.rs:664-673`），但它只面对单一矩形。
- **两种问答的行坐标**：覆盖层的候选键行永远是最末一行（`src/render/tui.rs:1566`），几何 `panes.modal(rows)` + `layout::inner` 在绘制期可得（`src/render/layout.rs:163-175,357-364`），但**没有任何字段记录选项位置**，且 `choices_row` 是一整条 `Line`、每个选项的列区间未存（`src/render/tui.rs:1604-1618`）。问卷的可见行号 = `prefix.len() + (i - option_window_start)`（`questionnaire_window` `:1372-1395`、`option_window_start` `:1400-1410`），基准 `panes.input`；`Questionnaire.drafts/index/highlight` 的关系与 `press`/`confirm_highlight`/`type_custom` 语义均已列明。
- **DSH 先例**：直接引用 `.scratch/ask-user-question/research/01-dsh-ask-user-question-and-composer-takeover.md` §3 的 `:65`（原始 label）、`:67`（单选自动前进）、`:68`（多选切换、Enter 才提交）、`:69`（自定义文本与单选清空）、`:73`（Esc 取消）、`:62`（一次一个 request 拥有输入区），未重查 DSH 产物。
