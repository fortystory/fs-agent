# grilling：既有消息在新布局中的落位

Type: grilling
Status: resolved
Blocked by: 02, 04

## Question

现有 TUI 已经会显示一批东西 —— 权限询问、忙碌状态、斜杠命令反馈、技能加载、narration 行、丢弃警告、终止原因。四分区布局里它们各自去哪儿？**改的是"显示在哪"，不是"显示什么"**：`src/render/wording.rs` 是人面向中文措辞的唯一真相源，不得绕过它自己拼字符串（见 map 的领域约束）。

## 需要定

1. **权限询问**。模态覆盖层（浮在中段之上）还是"转录里一块 + 底部提示行"？是/否怎么按（现有是 y/n？查代码确认）？询问出现时输入区**还让不让编辑**（锁定？还是允许先打好字）？`wording::permission_asked` / `permission_prompt` 的文案是否保留原样（应当保留）。
2. **忙碌状态**。`wording::status_line(busy, width)` 在新的 header + 右栏里是**保留、被吸收、还是删除**？如果保留，放哪一行。**这是 map 的「Not yet specified」里点名可能毕业的一项** —— 在本票给出结论。
3. **诊断信息**。`wording::renderer_dropped`（渲染器丢弃事件）、`finish_reason` / `stop_reason_name`（终止原因）、`decision_name` 各显示在哪（header？转录末尾？footer 临时行？）。
4. **斜杠命令反馈**。`unknown_command`、`skill_loaded`、裸技能加载后的 `已加载技能 <name>；请输入你的任务。` —— 进转录，还是 footer 的临时提示（toast，几秒后消失）？**定下一个统一规则**，不要一类一个样。
5. **工具调用摘要与 narration**。现在每个工具调用一行摘要、narration 缩进 —— 这些缩进/前缀/颜色在新面板里变不变（面板更窄了，缩进预算更少）。
6. **焦点**。转录 / 右栏 / 输入三处，焦点是不是**永远在输入区**（没有切焦点的快捷键）？权限询问出现时焦点怎么办。**不许引入"点一下切面板"**（鼠标捕获已关闭）。
7. **离开时的补偿**。alt screen 下终端原生选择只覆盖可见区，用户已接受。是否需要在退出（`/quit`）时把转录 dump 到普通 scrollback 作为补偿？给出**要 / 不要**的结论与理由；若要，说明用什么机制（票 01 会给 `restore` 与相关 helper 的事实）。

## 先读

- `src/render/wording.rs`（现有全部条目）
- `src/render/tui.rs`（现有的询问态、busy、narration 处理）
- `src/render/transcript.rs`（`Block` 变体列表）
- `.scratch/tui-layout/issues/02-*.md` 与 `04-*.md` 的答案

答案必须自足（`/implement` 在 `/clear` 后读它）。

## Answer

**已定（2026-09-21，grilling + 用户确认）。本票只产决策，不含实现。**

### 0. 冻结输入

- 权限询问 = **模态覆盖层**（用户选）
- `wording::status_line` 的**状态词并进底部提示行最左**（用户选）—— **不删除**，这是 map 那条雾的结论
- 斜杠命令反馈、技能加载提示、诊断信息 **全部进转录**（用户选）
- 退出（`/quit`）**不 dump** 转录（用户选）

### 1. 四种 pending 问题共用一个模态覆盖层

`Question` 现在两个变体（`src/render/input.rs:27-32`：`Permission` / `PlanConflict`），本 effort 要加到**四个** —— 票 04 定下的**超长粘贴确认**与 **Esc 清空草稿确认**也走这套 `TuiState.pending` / `AnswerChoice` 机制（`AnswerChoice` 在 `src/render/input.rs:36-39`）。

- **外观**：画在中段（转录 + 右栏）**之上**、垂直居中；宽度取 `min(中段宽 − 4, 72)`，用 `Block` 边框 + `Clear` 打底，居中 `Paragraph`；问题文本走 `wording.rs`，同一行末尾附可接受键。
- **键位与默认答案**（沿用 `answer_key` / `default_answer`，`src/render/tui.rs:539-569`）：

| 问题 | 接受键 | 默认（Esc / 回车） |
| --- | --- | --- |
| `Permission` | 现状：`y` / `a` / 否认键 | 现状 = `Deny`（安全侧） |
| `PlanConflict` | 现状：`o` / `k` | 现状 = `Keep` |
| 超长粘贴（新） | `y` / `n` | **否** |
| Esc 清空草稿（新） | `y` / `n` | **否**（Esc 落在确认上 = 不删） |

- **模态期间**：转录与输入**不接收字符键**（现状 `if self.pending.is_some()` 分支已经如此，`src/render/tui.rs:385-392`）；`Ctrl-C` 与 `Esc` 仍然有效（Esc = 默认答案）。
- 覆盖层**不属于转录、不随滚动移动**；但 `PermissionAsked` 本来就在流里有一条块（`Block::PermissionAsked`），回看不受影响。

### 2. 底部提示行

- **保留 `wording::status_line(busy, width)` 的形态**（它现在就是「状态词 · 提示…」，`src/render/wording.rs:374-389`），只换 `KEY_HINTS`（`src/render/wording.rs:369`）为**六条**：`enter 发送` / `ctrl-j 换行` / `esc 取消` / `shift+tab 计划` / `PgUp/PgDn 滚动` / `ctrl-c 退出`。
- **绝不加 `shift+enter 换行`**（票 04：不开键盘增强协议，它等同提交）。
- **不加 `ctrl-g 到底`**：脱离吸底时转录区底部已经显示「点此到底」，提示行留给高频动作。
- **宽度** = 底部块的**内宽**（`w − 2`，票 02）。**降级算法要改**：现状是从右往左丢、状态词最后走（`wording.rs:374-389`）。**新规则（2026-09-21 实现期修正）**：提示从左边填、`ctrl-c 退出` 预留，**状态词只在提示后面还放得下时才加在最左**。原措辞「状态词与 `ctrl-c 退出` 先占位」与本节自己的实测数字矛盾 —— 40 列内宽 38，「就绪 + ctrl-c + 一条提示」要 31 列，再接 `ctrl-j 换行` 就 44 列，于是会把 `ctrl-j 换行` 挤掉；而票 02 那张已批准的 40×10 快照显示的是**三条提示、无状态词**。修正后的实测：`w=40` → 3 项（无状态词）、`60` → 5、`80` → 6、`120+` → 7（首项是状态词）；**`ctrl-c 退出` 与 `enter 发送` / `ctrl-j 换行` 在任何宽度都在**。

### 3. 通知与诊断：全部进转录，没有 toast

- `RenderEvent::Notice` → `Block::Notice`、`RenderEvent::Diagnostic` → `Block::Diagnostic`，**行为不变**（`src/render/mod.rs:68-83`、`src/render/transcript.rs`）。
- 覆盖清单：`unknown_command` / `skill_loaded` / `skill_loaded_waiting` / `plan_entered` / `plan_exited` / `nothing_to_undo` / `error_report` / `renderer_dropped` / `finish_reason` / `stop_reason_name` / `decision_name`。
- **不引入任何计时器/toast**（也就没有"几秒后消失"的测试面）。

### 4. 退出：不 dump

- `/quit`、`Ctrl-C`、stdin EOF 都**不**把转录打印回 scrollback。完整记录在会话日志里，`--continue` 可回看。
- 后果：**票 09 的 ADR 里不能写"退出时 dump 作补偿"** —— 代价清单照实写（可见区外无法选择、退出即清）。
- **必须显式撤销的终端状态**（票 07 复核）：raw mode 与 alt screen 由 `ratatui::restore()` / panic hook 负责（research §1.1、§1.5）；**鼠标捕获与 bracketed paste 是我们自己开的，必须自己关**。

### 5. 既有块的呈现保持原样

- 工具调用摘要、narration、divergence 缩进、speaker 前缀**全部不变**（`src/render/tui.rs:820-1017`）；变的只有可用宽度（票 02 给了每一档的转录内宽）。
- 缩进 2 格保留 —— 收益太小，不值得为窄栏单独调。
- 滚动条恒占转录区右缘 1 列（票 03）；「点此到底」也画在转录区底部右缘，两者不重叠（滚动条在右缘，指示块在底部右缘内侧）。

### 6. 焦点

- **永远在输入区**：没有切换焦点的键，**鼠标点击也不改变焦点**（票 03：除「点此到底」外，其它区域点击一律忽略）。
- 模态出现时输入被模态接管（现状），模态消失后焦点回到输入区。

### 7. 配色：沿用现有调色板（这条雾到此关掉）

只用现有那六种 + 加粗，**不新增配色、不加主题配置项**（`src/render/tui.rs` 现有用法）：

| 用途 | 样式 |
| --- | --- |
| 边框、提示行、speaker 前缀、narration | `DarkGray` |
| 权限询问、hook 反馈 | `Yellow` |
| 轮次分节线 | `Cyan` + BOLD |
| 错误 / 严重终止 | `Red` |
| 分歧块 | `Magenta` + BOLD |
| 成功 / 通过 | `Green` |
| 强调（工具调用、输入行） | BOLD |

**不需要 truecolor**：命名色本身就会在 16 色终端上降级，`w=40` 的极窄场景也够用。

### 8. 「点此到底」的措辞（本票拥有）

- 有新内容：`↓ {n} 行新内容 · 点此到底`
- 仅上滚、无新内容：`点此到底`
- 整块是一个鼠标命中矩形（票 03）；措辞归 `wording.rs`，`n` 按**显示行**算（票 03 §1）。

### 9. 交接出去的事

- **票 07**：模态覆盖层的绘制顺序（必须画在最后一层）；鼠标与 bracketed paste 的成对撤销。
- **票 08**：模态的 `TestBackend` 用例（画出覆盖层、背景被遮住）、提示行逐宽度的条目数断言（40/60/80/120 各一条）、「提示行与提示集里都不得出现 `shift+enter`」一条防止回归。
- **票 09（ADR）**：不得声称退出时 dump。
