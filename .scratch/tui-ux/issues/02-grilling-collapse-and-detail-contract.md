# grilling：折叠与详情覆盖层的交互契约

Type: grilling
Status: resolved
Blocked by: 01
Part of: ../map.md

## Question

定下「折叠思考与工具输出 + 覆盖层看详情」的完整交互契约。这是本图最大的一个决定，答案要能直接喂 `/prototype` 与实现票。

## 冻结的输入（不得重开）

- 折叠对象 = **reasoning（思考）** 与**工具输出正文**；**工具调用行保留**，且**保留参数摘要**（`kimi 调用 bash command=…`）。
- 提示进**转录**、可回看、可点击；**详情用覆盖层**（不就地展开）。
- reasoning 用已有 `MessageCompleted.reasoning`，**不改 schema**；工具全文读 `outputs/<tool_call_id>.txt`，指针失效降级为事件里的 head/tail 预览。
- 详情覆盖层**内部可滚动**，**不可叠在待答问题之上**。
- 只动 TUI；plain / headless 的**可见输出不动**。票 01 已查明：`Block::Message` 被 `plain.rs` 与 `tui.rs` 无 `..` 地穷尽解构，**加字段会编译期强制 `plain.rs` 出现一处不改变输出的解构改动**——允许这一点，但不得改变 plain 的任何输出字节。

## 需要定

1. **提示文案的精确形态**（全部落 `src/render/wording.rs`，中文）。原始诉求给了四种：「xxx 正在思考」「xxx 思考完成」「xxx 调用 xxx」「xxx 调用 xxx 失败」。要定：`xxx` 是 `speaker_label` 还是角色名；工具行失败时是替换那一行还是另起一行；同一回合连续多次工具调用/多次思考怎么呈现；讨论与执行者的 speaker 一并适用吗。
2. **流式期的状态机**：reasoning delta 到达时转录出现「正在思考」一行；正文 delta 到达时那一行怎么变；`MessageCompleted` 到达时是否原地变成「思考完成」；若一个 iteration 只有 reasoning 没有正文、或只有工具调用，提示怎么收尾。**这是本票最核心的一条。**
3. **无 reasoning 的回合**：不显示任何思考提示（确认），以及「正在思考」在 `reasoning` 为空但模型还在跑时的行为。
4. **`--continue` / 重放**：从 `MessageCompleted.reasoning` 重建「思考完成」行（确认）；重建出的提示是否一样可点开、内容是否一样。
5. **点击的语义**：点提示行的命中区域（整行 / 行首标记 / 文字）；点击后覆盖层的标题、正文分节（reasoning 全文 / 工具参数 / 工具全文 / 截断说明）、长内容滚动到顶还是底；关闭方式（`Esc` / 再点同一处 / 点空白）。
6. **与其它覆盖层的边界**：有待答问题（权限模态 / 问卷）时提示行不可点、点击被丢弃（确认），以及详情覆盖层自己算不算 `pending`（会不会触发现有 `pending.is_some()` 守卫而误伤输入/滚动）。
7. **折叠对滚动的影响**：折叠后的提示行是否计入 20 000 **源行**上限与显示行滚动；详情覆盖层打开时转录是否继续吸底/滚动。
8. **失败与边界**：工具指针失效、`outputs/` 文件读不到、reasoning 为空、超长（数十万字符）时的降级文案与上限。
9. **颜色**：思考提示与工具行失败用什么颜色（与 `07` 的角色配色、现有 severity 色的关系；`07` 只定名字前缀，本票定这些提示行）。

## 先读

- `src/render/tui.rs`（`apply` / `render_block` / `tool_lines` / `draw_modal` / `mouse`）
- `src/render/transcript.rs`（`Block` / `ToolBlock` / `summarize_args`）
- `src/render/pane.rs`（滚动与源行/显示行）
- `src/render/wording.rs`（措辞层的家）
- 票 01 的答案（`.scratch/tui-ux/research/01-*.md`）
- `.scratch/ask-user-question/research/01-*.md` §3（DSH 先例）
- `docs/tui-manual-checklist.md`（现有手工口径）

## 答案落点

答案要定到**可实现的契约级**：提示文案表、状态转移、命中与关闭规则、失败降级表。不要写实现代码。

## Answer

**契约定稿（2026-09-23，两轮 grilling，共 11 个决定）。** 状态机、文案表、点击/详情规则、降级与颜色如下；**几何与绘制**留给 `prototype：折叠提示行、详情覆盖层与工具行的形态`（票 03）定。

### 1. 提示行的生命周期（核心）

- 粒度是**一个思考段一行**，不是「一个回合一行」。
- reasoning delta 首次到达（该 speaker 当前没有开放思考行）→ 插入临时行 `{speaker_label} 正在思考`。
- **正文 delta 到达 → 把当前开放思考行「原地定格」为 `{speaker_label} 思考完成`**（即使该 iteration 后面还会补 reasoning）；后续 reasoning 再开新行。理由：票 01 事实 20 证实 reasoning 与 text delta **可以交错**，没有「先全部 reasoning」的保证。
- `MessageCompleted` 到达时：
  - `reasoning: Some(..)` 且有开放思考行 → 定格为「思考完成」，该行持有全文（详情用）。
  - `reasoning: None` 但有开放思考行（**讨论合成器路径**，`src/agent.rs:1616` 发增量而 `:1663` 写 `None`）→ 同样定格为「思考完成」，但详情显示事件预览 + 一行「本次未记录思考全文」。
  - 没有开放思考行（该回合无 reasoning）→ **不插入任何思考行**。
- 该行是**普通转录源行**：计入 20 000 源行上限、参与吸底、随滚动移动、`evict` 时一并丢最旧。
- 只有**完成态**（「思考完成」）可点击；流式期的「正在思考」**不可点**（全文还没到手）。

### 2. 文案表（全部落 `src/render/wording.rs`）

| 场景 | 文案 |
| --- | --- |
| 思考中 | `{speaker_label} 正在思考` |
| 思考完成 | `{speaker_label} 思考完成` |
| 工具调用 | `{speaker_label} 调用 {tool} {参数摘要}` |
| 工具失败 | 同一行**行尾**加 ` 失败`（不另起一行） |
| post-hook 反馈 | 现有 `wording::hook_feedback`，折叠态照常显示 |
| 详情 / 全文读不到 | `全文不可用` |
| 详情 / 思考未记录 | `本次未记录思考全文` |
| 详情 / 超过读取上限 | `已截断` |

- `xxx` = 现有 `wording::speaker_label`（`[kimi]` / `[执行者 id]` / `[用户]` / `[系统]`），不另造名字、也不去方括号。
- 工具行**去掉现在的 `→`**，改写成「调用」；参数摘要沿用 `transcript::summarize_args`。
- 适用范围：讨论者、执行者、合成器（`System`）一律适用；用户消息与系统行不受影响。

### 3. 折叠后工具块的内容归属

- **折叠态** = 调用行（失败时行尾 `失败`）+ **post-hook 反馈行**（若有；hook 是策略层反馈，不经点击也要看得见）。
- **进详情**：工具完整参数（折叠行是 `summarize_args` 的 160 字符摘要）、输出正文、错误详情、`wording::no_tool_result` 的无结果提示、截断说明。
- 颜色：调用行沿用现有 `BOLD`；**失败行 `Red`**；**思考提示 `DarkGray`**。

### 4. 详情覆盖层

- **打开**：点击「思考完成」或工具调用行；**整行可点**。标题 = 被点那一行的原文。
- **内容分节**：思考全文 / 工具参数 / 工具全文（含 `[truncated: …]` 说明）；打开时**滚到顶部**。
- **取数**：按 `tool_call_id` 拼 `{会话目录}/outputs/<tool_call_id>.txt`（`SessionFacts.cwd` 实际就是会话目录，`cwd.join("outputs")` 即此目录，票 01 事实 56）；读不到 → 显示事件里的 head/tail 预览 + `全文不可用`。整段读入，**上限 200 000 字符**，超过则读前 N 并显示 `已截断`。
- **它是一个「查看态」，不是 `pending` 问答**：独占键盘（`Esc` 关闭、`↑/↓` 与 `PgUp/PgDn` 给详情，其它键忽略），鼠标滚轮滚详情、点击命中关闭/滚动。**不要走 `pending.is_some()` 那条整体早退**——否则详情自己收不到滚轮与点击（`src/render/tui.rs:872-876`）。
  - **2026-09-23 由 `grilling：Ctrl-D 退出的确认与提示行` 修正**：`Ctrl-D` 是「其它键忽略」的**例外**——详情覆盖层打开时按 `Ctrl-D` 会**关闭详情覆盖层**（不是忽略，也不弹退出确认）。见票 06 的 Answer §5。
- **关闭**：`Esc` 或**再点同一行**。
- **不可叠在待答问题上**：有待答问题（权限 / 计划冲突 / 粘贴 / 清草稿 / 问卷）时，折叠提示行不可点、点击被丢弃。
- **打开时转录冻结在当前位置**（不吸底、不被新输出抽走），关闭后恢复。

### 5. `--continue` 与历史

- 票 01 事实 18：`--continue` **不把历史重播进 TUI**（全仓库唯一 `render.logged` 在 `src/agent.rs:2148` 的 `append_event`），pane 从空开始。因此提示行**只在本次进程内**存在。
- 「TUI 启动时把历史事件重播进 pane」被判为**本图 out of scope**，另立独立票（见 map 的 `## Out of scope`）。

### 6. 实现落点提示（不写代码；形态由票 03 定）

- **reasoning 到 TUI 的唯一断点**是 `src/render/transcript.rs:277-283` 那个 `..`；给 `Block::Message` 加字段会**编译期强制**改 `plain.rs:81-85`（只补字段、不改输出——map 冻结项 1 已允许），或另立一个块/状态承载思考行；实现票二选一。
- 「临时行 → 定格」需要 `Pane` 能改写最后一行，或 `TuiState` 另设一个 live 思考状态行；票 03 的形态决定用哪条。
- 命中需要「转录显示行 → 块」的映射；票 01 事实 35/41：`Pane` 只有行级结构，`indicator` 是**单矩形**先例（`Option<Rect>` + 绘制时记录 + `mouse()` 包含判断）。
