# grilling：多行输入编辑器

Type: grilling
Status: resolved
Blocked by: 01

## Question

输入区要从单行升成多行。这是本轮新增的 scope（见 map Notes 第 4 条），它会把刚做完的单行编辑器（`TuiState` + 26 个测试）重做一遍 —— 本票要定下新的语义与重写范围。

## 冻结的输入（不得重开）

- **Enter 提交**
- **Ctrl-J 与 Shift+Enter 换行**；Shift+Enter **尽力而为** —— 终端不支持键盘增强协议时它退化成 Enter（即提交），这个退化必须写进票里，不能假装它一定可用。
- **随内容长高，上限 10 行**（超过后内部滚动）
- **启用 bracketed paste**：粘贴的多行文本**不得触发提交**

## 需要定

1. **重写范围**。现有 `TuiState` 的单行编辑器（`input` / `cursor` / `input_view` / `cursor_column(width)` / `truncate_columns` / `text_columns` / `char_columns`）怎么升成多行缓冲：同一个结构体加"软换行 + 行内光标"，还是拆出独立的 `Input` 类型？`tests/render_tui.rs` 的 26 个用例哪些保留、哪些改写、哪些删。
2. **软换行**。逻辑行 → 显示行的映射（宽字符、组合字符怎么算列）；上下移动光标跨软换行时是否保持"视觉列"（goal column），以及行尾进出的细节。
3. **键位表**（逐键写清，并与票 03 对齐 Home/End 与 ↑/↓ 的归属）：
   - Ctrl-A / Ctrl-E：行首行尾，还是缓冲首尾？
   - Ctrl-U / Ctrl-K / Ctrl-W：作用于当前行还是整个缓冲？
   - Esc：清空？中断？什么都不做？
   - Backspace / Delete：跨行时的行为；跨软换行（同一逻辑行）时的行为。
   - Home / End / ↑ / ↓：归输入还是转录。
4. **输入历史**。↑/↓ 取历史与"在多行缓冲内上下移动光标"的冲突怎么解（常见解法：光标已在首行/末行时才取历史 —— 但显式定下来）；多行输入进历史后如何再取出来（保留换行还是不保留）；`draft`（未提交草稿）语义在多行下怎么变。
5. **粘贴**。bracketed paste 的文本**直接插入并保留换行**，不触发提交；`\r\n` 归一成 `\n`；粘贴内容里的控制字符怎么办；超长粘贴（例如百万字符）要不要截断或确认。
6. **光标**。全屏下依然要 `set_cursor_position` —— 原始 bug 是"光标不跟在 `>` 后面 / `>` 在后续回合消失"。本票要给出：**光标位置的唯一真相源**（渲染时由显示行/列算出来，而不是累加维护），以及一条**可验证的手工检查**（在真终端里，多行输入时按上下左右、跨软换行、行尾插入，光标应当在哪）。**注意**：本票不要求修 inline bug（已 out of scope），只要求新布局下光标行为**可判定**。
7. **提交**。Enter 提交后输入区回到 1 行；多行文本作为**单条 user message** 进入会话（确认这是既有约定，并指出代码位置）；提交后是否进入历史、是否清空草稿。

## 先读

- `src/render/tui.rs`（`TuiState` 与 `Key` 枚举、`map_key`、`draw_live`）
- `tests/render_tui.rs`
- `.scratch/tui-layout/issues/01-*.md` 的答案（bracketed paste 与 Shift+Enter 的 API 事实）

答案必须自足（`/implement` 在 `/clear` 后读它）。

**票 01 交接来的事实（2026-09-21）**：

- **没有键盘增强协议时 Shift+Enter 与 Enter 完全不可区分**：`\r` → `Enter` + 空 modifier；raw mode 下裸 `\n` 是 `KeyCode::Char('j')` + `CONTROL`（research §7.2）。所以**可靠的换行键只有 Ctrl-J**；Shift+Enter 只在发过 `PushKeyboardEnhancementFlags(DISAMBIGUATE_ESCAPE_CODES)` 后才以 `Enter + SHIFT` 到达（§7.3）。这坐实了票面的「尽力而为」，也意味着实现必须**先用 Ctrl-J 把功能做完整**，增强协议只作增量。
- `supports_keyboard_enhancement()` **会阻塞、2 秒超时**（§7.1）→ **绝不能放在首帧或绘制路径上**；要探测就在起渲染器之前一次性做。
- bracketed paste：`EnableBracketedPaste` / `DisableBracketedPaste` 在 `crossterm::event`（默认特性已开），到达 `Event::Paste(String)`（§6.1-6.2）。**crossterm 不剥 `\r`、不过滤控制字符、无长度上限，只做 UTF-8 lossy**（§6.3、§12.6）→ `\r\n` 归一、控制字符处理、超长粘贴的保护**全部自己写**。
- 现有键盘分支只处理 `KeyEventKind::Press`（`src/render/tui.rs`）—— `Event::Paste` 不是 `Key`，别被那个过滤顺手丢掉。
- `Frame::set_cursor_position` 经 Backend **原样** `MoveTo(x, y)`，即终端绝对坐标（§5）。全屏下 `area()` 是 `(0, 0, w, h)`，所以光标坐标**可以从显示行/列直接算出来**，不再是 inline 时代「视口偏移 + 累加维护」的形状 —— 这正是本票第 6 问要的「唯一真相源」的落点。

## Answer

**已定（2026-09-21，grilling 两轮 + 用户逐条确认）。本票只产决策，不含实现。**

### 0. 语义总表（覆盖此前一轮的说法）

- **Enter = 提交**
- **Ctrl-J = 换行** —— **唯一可靠的换行键**
- **Shift+Enter = 提交（等同 Enter）**。用户**明确选择不启用键盘增强协议**，因此 Shift+Enter 与 Enter 在协议层不可区分（research §7.2）。**它绝不能广告成「换行」** —— 快捷键提示行里**不许出现 `shift+enter 换行`**（交接给票 06 / 票 08）。
- 输入区**随内容长高、上限 10 行**，超过后内部滚动，**滚动跟随光标**。
- bracketed paste 开启：粘贴的多行**不提交**；**超过 100 000 字符先确认**。
- **Esc 清空前先确认**（多行草稿，见 §2）。
- 历史**保持进程内**，不持久化。
- 斜杠命令**只看第一行**（见 §7）。

### 1. 重写范围：把编辑器拆成独立的 `Input` 类型

新建 `src/render/editor.rs`（与 `src/render/input.rs` 平级 —— 后者是 console port 的家，编辑器是纯状态），`pub struct Input`，由 `TuiState` 持有一个。

**搬进 `Input`**：`input` / `cursor` / `history` / `history_at` / `draft`，以及现有的 `byte_at` / `insert_char` / `backspace` / `delete_forward` / `kill_to_start` / `kill_to_end` / `kill_word` / `set_input` / `clear_input` / `history_previous` / `history_next`（`src/render/tui.rs:411-522`）。
**留在 `TuiState`**：`transcript` / `ready` / `live` / `pending` / `events` / `busy` / `quit` / `prompt_reply`，以及「提交后把文本送出去」那一半（`prompt_reply` 的发送，`src/render/tui.rs:534-536`）。
**`Input` 新增**：软换行（逻辑行 → 显示行的映射）、光标行列（渲染时算）、`insert_str`（粘贴整体插入）、`rows(width)`，以及从 `tui.rs` 搬过去的 `wrap_take` / `text_columns` / `char_columns` / `truncate_columns` 自由函数。

**为什么拆**：多行之后「光标 ↔ 显示行列」成为核心不变量，混在 `TuiState` 里没法独立测；拆出来后编辑器用例可以只对着 `Input` 断言（测试归类见票 08）。

### 2. 键位表（最终版 —— **票 03 必须与此一致**）

| 键 | 作用 |
| --- | --- |
| `Enter` | 提交（trim 后为空则不发消息，行为同现状） |
| `Ctrl-J` | 插入 `\n` |
| `Shift+Enter` | 等同 `Enter`（**不实现**，因为不开增强协议） |
| `Char` / 粘贴 | 插入 |
| `Backspace` | 删光标前一字符；**在行首且不在缓冲开头 → 删掉换行符（两行合并）** |
| `Delete` | 删光标后一字符；**在行尾且不在缓冲末尾 → 删掉换行符** |
| `←` / `→` | 一行内移光标，**到行首/行尾时跨行**（textarea 语义） |
| `↑` / `↓` | 缓冲内移光标，**保持视觉列**；首行再 `↑`、末行再 `↓` **不动**（不取历史） |
| `Ctrl-P` / `Ctrl-N` | 历史 —— **唯一入口**（`↑/↓` 不再兼任） |
| `Home` / `Ctrl-A` | **当前逻辑行**行首 |
| `End` / `Ctrl-E` | **当前逻辑行**行尾 |
| `Ctrl-U` | 删到**当前行**行首 |
| `Ctrl-K` | 删到**当前行**行尾 |
| `Ctrl-W` | 删**当前行**内光标前的一个词 |
| `Esc` | 忙 → `FrontEndEvent::Cancel`；有 pending → 默认答案；闲且输入**多行** → **先确认**清空；闲且单行 → 直接清空（现状） |
| `Ctrl-C` / `Shift-Tab` | 不变 |

**实现要点**：`Key` 枚举要新增 `CtrlJ`，`map_key` 的 CONTROL 分支要加 `'j'` —— 现在 `'j'` 落到 `_ => None`，**Ctrl-J 是被丢弃的**（`src/render/tui.rs:97-107`）。

**跨票交接**：`Home` / `End` / `↑` / `↓` **全部归输入区** → **票 03 的转录「回到底部」不能依赖它们**，只能用 PgUp/PgDn（或另定）。

### 3. 软换行与 goal column

- 按**显示列**换行，口径复用既有 `wrap_take`：一个 CJK 字符 = 2 列，**逐字符**，**不做词边界换行**（与转录一致；词边界会让光标映射复杂化）。
- 光标的**唯一真相源** = `(逻辑行号, 行内字符偏移)`。显示行/列**每次渲染时算出来**，不累加维护。
- **goal column**：`↑/↓` 记住「想要的视觉列」，横向移动或插入字符时清掉；目标行更短时夹到行尾。
- 最小单位是**字符**不是 grapheme 簇 —— 与 `wrap_take` 的既有取舍一致（`src/render/tui.rs` 的注释已给出理由：`Span::styled_graphemes` 会丢控制字符）。

### 4. 历史与提交

- 进程内，**不持久化**（用户明确选择）。
- 多行条目**原样保存**（含内部 `\n`）；召回时**整体替换**缓冲，光标置末尾。
- `draft`（浏览前暂存的新输入）语义不变，多行一并暂存/恢复。
- 提交**只 trim 整个缓冲的首尾空白**（含首尾换行）—— 与现有 `submit()` 的 `trim()` 一致（`src/render/tui.rs:526`）；**中间的换行保留**。
- 空提交不入历史、不发消息（现状）；与上一条相同则不入历史（现状，`src/render/tui.rs:531`）。
- 提交后输入区回到 1 行、光标回 0。

### 5. 粘贴

- `Event::Paste(String)` **不是 `Key`**：现有键盘分支只处理 `CtEvent::Key(kind == Press)`，要改成同时接受 `CtEvent::Paste(_)`（否则粘贴被顺手丢掉）。
- **先归一**：`\r\n` 与裸 `\r` → `\n`；**过滤掉除 `\n` / `\t` 以外的控制字符** —— crossterm 不过滤、不剥 `\r`、无长度上限（research §6.3、§12.6），**保护全部自己做**。
- 按当前光标**整体插入**，**永不触发提交**。
- **超过 100 000 字符**：先弹确认「粘贴 N 字符？」。阈值按 **`chars().count()`**（字符数不是字节数，避免中文被误判）。
- 确认走**既有的 pending-question 机制**（`TuiState.pending` / `Question` / `AnswerChoice`，`src/render/tui.rs:273-276`、`answer_key` 在 `:539+`）：**新增两个 `Question` 变体**（超长粘贴、Esc 清空），`AnswerChoice` 加对应分支，**默认答案是「否」**（Esc 落在确认上 = 不删/不粘，安全侧）。
- **它们的呈现（浮层还是行内提示行）归票 06**；本票只定「走 pending 机制、语义是是/否、默认否」。

### 6. 光标：唯一真相源 + 可验证的手工检查

渲染时：`row = 光标所在显示行（相对输入区顶部）`，`col = 该显示行内的显示列`，
`frame.set_cursor_position((input_area.x + prompt_width + col, input_area.y + row))`。

全屏下 `area()` = `(0,0,w,h)` 且 `set_cursor_position` 原样 `MoveTo`（research §5），**没有视口偏移要加** —— inline 时代那个 bug 的根因（`area().y` 是视口位置且随插入漂移）在新布局里不复存在。

**手工检查清单（真终端，逐条照做）**：

1. 输入 3 行，光标在第 3 行文字末尾，**紧跟提示符后的内容**（不在屏幕底部、不在别的行）。
2. `↑` 两次 → 到第 1 行；`←` 若干次停在行首；再 `←` → 跳到上一行行尾。
3. 在第 1 行中间插入一个字 → 光标右移一列，**其余行不动**。
4. 一行长到自动软换行 → 光标在第二显示行的正确列；`Backspace` 到软换行点时回到上一显示行行尾。
5. 提交后：输入区回 1 行，光标回 `> ` 之后。
6. 粘贴 3 行 → 光标在插入块之后，且**没有提交**。
7. 多行草稿按 `Esc` → **出现确认**，选「否」后草稿还在。

### 7. 提交与斜杠命令

- Enter → `Input` 交出 trim 后的文本，`TuiState.submit()` 的现有行为不变（`prompt_reply` 送 `Option<String>`）。
- 多行文本作为**单条 user message**：现有链路就是「一个字符串进 `prompt_reply` → `run_one_turn(harness, events, input)` → `harness.run_turn(input)`」，**多行只是字符串含 `\n`**，不需要新协议。
- **斜杠命令只看第一行**（用户选）：`src/cli.rs:383` 现在是 `let command = line.trim(); match command { … other if other.starts_with('/') }`。改为：**取第一行 trim**，若它是 `/name` 或 `/name task…` → 走命令分支，**其余行按 `\n` 拼进 `task`**；否则整体当 prompt。
  - 这样 `/ask-matt` + 多行 brief **可用**（保住 spec §9 的「用户侧技能调用」），而粘贴一段以 `/` 开头的多行代码不再误判成 unknown command。
  - 单行行为**逐字不变**（第一行 = 整行）。

### 8. 交接出去的事

- **票 03**：`Home` / `End` / `↑` / `↓` 全归输入区 → 转录滚动只能用 PgUp/PgDn（或另定），不能依赖 Home/End。
- **票 06**：两个新 pending question（超长粘贴、Esc 清空）的**呈现**；快捷键提示行**不许写 `shift+enter 换行`**（只能写 `enter 发送` / `ctrl-j 换行` / `ctrl-c 退出` 一类）。
- **票 08**：编辑器用例改为对着 `Input` 断言；`tests/render_tui.rs` 的 26 个用例逐个归类（哪些留、哪些改、哪些删）。
