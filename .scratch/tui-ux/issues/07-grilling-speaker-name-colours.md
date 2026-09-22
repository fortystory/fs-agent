# grilling：角色配色的落点与绘制边界

Type: grilling
Status: resolved
Blocked by: —
Part of: ../map.md

## Question

「不同角色名字用不同的文字颜色」的色板已在 charting 定下，本票定**落点与边界**：染哪些行的哪一段、由谁产生、与现有语义色的冲突怎么处理、以及转录之外的界面要不要跟着染。

## 冻结的输入（不得重开）

- **只染名字前缀**（`speaker_label` 的文本本身），不染整条正文。
- 色板：讨论者 1 = `LightCyan`、讨论者 2 = `LightMagenta`、执行者 = `LightYellow`、用户 = `LightGreen`、系统 = `Gray`。
- 用 ratatui 命名色，**不加 truecolor、不加主题配置项**。
- 只动 TUI；plain / headless 不动。

## 需要定

1. **哪些行带角色色**：目前 `src/render/tui.rs` 里带 speaker 的行散在 `attribute()`（消息）、`tool_lines()`（工具调用）、`TurnStarted` / `TurnEnded` / `PermissionAsked` / `PermissionDecided` / `Hook` / `ExecutorSpawned` / `Usage` / `AgentError` 的 narration。是「凡出现 `speaker_label` 的地方都染」，还是只染消息与工具这两类？给出清单，并说明为什么。
2. **讨论者的「第几个」怎么确定**：`SpeakerId::Debater(id)` 里的 `id` 是配置里的人物名、顺序来自 roster（`discussion::pick_pair`）。按出现顺序、按 roster 顺序、还是按名字稳定哈希取色；同一会话里两次 `/discuss` 抽到不同配对时颜色会不会跳；`SpeakerId::Executor(id)` 与讨论者重名时怎么办。
3. **与现有语义色的关系**：现有用法是 `Cyan`=轮次分节、`Magenta`=分歧/logo、`Yellow`=权限与警示、`Red`=错误、`Green`=成功、`DarkGray`=narration/边框。新角色色与它们同屏时的优先级：错误/成功行是否仍按 severity 压过角色色；`BOLD` 的用法是否保持。
4. **实现归属**：颜色是**画家的事**（`src/render/tui.rs`），`wording::speaker_label` 保持纯文本（确认）；如果多行消息的续行缩进、工具输出预览、折叠提示行都要区分角色，是否需要一个公共的 `speaker_color(speaker) -> Color` 纯函数。
5. **转录之外**：信息面板（`panel.rs`）、header、问卷 / 覆盖层里出现名字前缀的地方要不要染；不做的话明确列出来。
6. **可访问性**：在浅色背景终端上 `LightYellow` / `LightGreen` 的可读性；是否需要 `BOLD` 兜底；不做主题配置的前提下说明这是已知取舍。
7. **验收**：如何在 `TestBackend` 里断言颜色（buffer 的 `fg`），以及既有测试断言的是文本还是样式。

## 先读

- `src/render/tui.rs` 的 `attribute` / `tool_lines` / `render_block` / `severity_line` / `narration` / `mark_lines` / `questionnaire_parts`
- `src/render/wording.rs` 的 `speaker_label`（`[用户]` / `[系统]` / `[执行者 id]` / `[id]`）
- `src/events.rs` 的 `SpeakerId`
- `src/discussion.rs` 的 `pick_pair` / roster 顺序
- `.scratch/tui-layout/spec.md` §10（「配色沿用现有六种 + BOLD」要被本图打破）
- `.scratch/tui-layout/issues/06-grilling-existing-messages-in-new-layout.md`（配色的既有立场）
- `tests/render_tui.rs` / `tests/render_layout.rs`（现有样式断言口径）

## 进度

**100%** —— 完成。一轮 grilling、5 条决定，票面 7 个「需要定」全部覆盖；契约见 `## Answer`。

**下一步**：无（已 resolved）。`/to-spec` 按 §7 回改 tui-layout spec §10；`grilling：测试与验证迁移` 按 §6 加 `fg` 断言。

## Answer

**契约（2026-09-23，5 条决定）。**

### §1 色板与分配规则

- 色板（冻结）：讨论者 1 = `LightCyan`、讨论者 2 = `LightMagenta`、执行者 = `LightYellow`、用户 = `LightGreen`、系统 = `Gray`。即 `DEBATER_PALETTE = [LightCyan, LightMagenta]`。
- **`SessionFacts` 新增一个字段**（暂记 `speaker_order: Vec<String>`）：组装期能确定的、按讨论顺序排列的讨论者名。普通单 agent 会话 = `[该 profile 名]`（`src/cli.rs:366` 的 `SpeakerId::Debater(profile.name)`）；讨论会话 = `pick_pair` 抽中的两人、按 roster 顺序。
- 分配：
  - `Debater(id)` 在 `speaker_order` 里 → `DEBATER_PALETTE[下标]`。
  - `Debater(id)` **不在**里面（**会话中途 `/discuss` 抽到名册里的其他人物**，注入时还不知道）→ 按**首次出现顺序**分配 `DEBATER_PALETTE` 中**未被 `speaker_order` 占用**的槽；槽用尽后回退到 **`Gray`**。
  - `Executor(_)` → `LightYellow`；`User` → `LightGreen`；`System` → `Gray`。
- 同一会话内稳定；跨会话可变（可接受）。`Executor(id)` 与讨论者重名不冲突（走执行者色）。

### §2 染哪些行

**凡出现 `speaker_label` 的行都染名字**：`attribute()`（消息）、`tool_lines()`（工具行），以及 `TurnStarted` / `TurnEnded` / `PermissionAsked` / `PermissionDecided` / `Hook` / `ExecutorSpawned` / `Usage` / `AgentError` 这些 narration。没有 speaker 的行（`RoundStarted`、`RoundEnded` 的分节线、`Divergence`、`SessionError`、`SessionEnded`、`ContextInjected`、`History`、`Notice`、`Diagnostic`）**不动**。

- 票 02/03/06 的折叠提示行也带 `speaker_label`（`{label} … 正在思考` / `{label} ✓ 思考完成` / `{label} 调用 …`），因此**它们的名字也染**；`▸` 标记保持 `DarkGray`，`…`/`✓` 保持行色（`DarkGray` / 失败 `Red`）。
- **与 severity 共存**：名字用角色色，**其余部分仍按原语义色**（成功 `Green`、错误 `Red`、权限 `Yellow`、narration `DarkGray`）。实现上把现在拼成单串的 `format!("{} {}", speaker_label(speaker), …)` 拆成**两个 span**：名字 = 角色色，正文 = 原样式。`severity_line` 因此要接收「名字 + 正文」两段，而不是一整串。

### §3 实现归属

- 颜色是**画家的事**：新增纯函数（暂记 `speaker_color(speaker, &facts, &mut extra) -> Color` 或 `TuiState` 上的方法），住在 `src/render/tui.rs`；`wording::speaker_label` **保持纯文本**，返回 `String` 不变。
- `extra`（首见兜底）是 `TuiState` 的一小块状态：`HashMap<String, Color>` 或「已分配的名字 → 槽」；注入名册在每次渲染时作为种子。
- 不改 `events` schema、不改 `wording` 的文案、不动 plain / headless。

### §4 转录之外

**只染转录**：信息面板（`panel.rs`）、header（含 Mark）、问卷（`draw_questionnaire`）、详情覆盖层标题**保持现状**。问卷/覆盖层不靠名字辨识发言；面板本来就没有 speaker 标签。

### §5 BOLD 与可访问性

- **只加颜色，不加 `BOLD`**；该行原本就 `BOLD` 的（工具调用行的 `调用 …` 段）保持原样。
- 已知取舍：`LightYellow` / `LightGreen` / `LightCyan` 在**浅色背景**终端上可读性弱；本图不做主题配置，也不加 `BOLD` 兜底——写进 spec，作为接受了的代价。

### §6 验收

- `TestBackend` 的 buffer cell 带 `fg`，所以可以对**名字所在的 cell** 断言颜色（现有测试的 `find_cell` / `row_text` 已经能定位到格）。既有测试大多断言文本；本票只新增 `fg` 断言，不改既有文本断言的语义。
- 必须覆盖：四种 speaker 各一条；讨论者 1/2 按注入顺序；不在名册里的第三人（首见兜底 / 用尽回退 `Gray`）；错误行 = 名字角色色 + 正文 `Red`；无 speaker 的行不受影响。

### §7 给 `/to-spec` 的回改

- tui-layout spec **§10** 的「配色沿用现有六种 + BOLD」被本票打破：新增 `speaker_color` 与 `DEBATER_PALETTE`、`SessionFacts` 的 `speaker_order`、以及「名字用角色色、正文用语义色」的拆分规则。
- 注意一处近撞色：Mark header 的 logo 用 `LightMagenta`（`mark_lines`），与讨论者 2 同色系；两者不在同一区域（header vs 转录），记为已知观感。
