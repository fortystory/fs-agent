# 12: 输入编辑器：`Input` 类型、多行、键位、粘贴与两种确认

**What to build:** 把单行输入升成**多行编辑器**：编辑器拆成独立的 `Input` 类型（`src/render/editor.rs`），支持软换行、跨行移动、按行作用的 readline 键位、`Ctrl-J` 换行、粘贴多行不提交，以及两个确认（超长粘贴、Esc 清空多行草稿）。光标的唯一真相源是「逻辑行 + 行内字符偏移」，显示行列渲染时算。

Blocked by: 10

Status: ready-for-agent

**参考:** spec §5（`Input`）、§6（键位表）、§7（粘贴与两种确认）、§2（输入区行数与续行缩进 2 格）

- [x] 新建 `src/render/editor.rs`，`pub struct Input`；搬进 `input` / `cursor` / `history` / `history_at` / `draft` + 全部行编辑方法 + `byte_at`。列宽算术**不搬**：票 11 已经把它收到 `src/render/width.rs`（`text_columns` / `char_columns` / `truncate_columns`），编辑器直接用那一份；换行口径与 `pane::wrap_line` 一致（逐字符、CJK 2 列）。`TuiState` 持有一个 `Input`，只保留「提交后把文本送出去」那一半
- [x] `Input` 新增：软换行（逻辑行 → 显示行）、光标行列计算、`insert_str`、`rows(width)`
- [x] 软换行**逐字符、CJK 2 列、不按词边界**（与转录同一口径）；最小单位是字符不是 grapheme 簇
- [x] **光标的唯一真相源 = `(逻辑行, 行内字符偏移)`**；显示行列每次渲染算出来，不累加维护；`set_cursor_position` 直接写绝对坐标（全屏下无视口偏移）
- [x] `↑`/`↓` 缓冲内移动并**保持视觉列（goal column）**；横向移动或插入字符时清掉；首行再 `↑`、末行再 `↓` 不动
- [x] `Key` 枚举新增 `CtrlJ`；**`map_key` 的 CONTROL 分支加 `'j'`**（现在它落到 `_ => None`，Ctrl-J 被丢弃）
- [x] 全键位表：`Enter` 提交；`Ctrl-J` 插入 `\n`；`Shift+Enter` **不做**（等同 Enter）；`Backspace`/`Delete` 在行首行尾**跨行合并**；`←`/`→` **跨行**；`Ctrl-P`/`Ctrl-N` 历史（唯一入口）；`Home`/`Ctrl-A`、`End`/`Ctrl-E`、`Ctrl-U`/`Ctrl-K`/`Ctrl-W` **全部作用于当前逻辑行**
- [x] 渲染循环同时接受 `CtEvent::Key(kind == Press)` 与 `CtEvent::Paste(_)`（`Event::Paste` 不是 `Key`，别被 `Press` 过滤丢掉）
- [x] 粘贴：`\r\n` 与裸 `\r` **归一成 `\n`**；**过滤除 `\n`/`\t` 外的控制字符**；按光标整体插入；**永不触发提交**
- [x] **超过 100 000 字符**（`chars().count()`，不是字节）→ 先确认 `粘贴 {n} 字符？`，默认**否**
- [x] **Esc 清空多行草稿**（输入含换行）→ 先确认 `清空输入？`，默认**否**；单行仍然直接清空；忙 → 取消回合、有模态 → 默认答案的行为不变
- [x] 两个确认都走既有 `TuiState.pending` / `Question` / `AnswerChoice` 机制（新增两个变体）
- [x] 历史保持**进程内**；多行条目原样保存（含内部 `\n`）、召回时整体替换、光标置末尾；`draft` 语义不变
- [x] 提交只 trim **首尾**空白（含首尾换行），**中间换行保留**；空提交不入历史、不发消息；与上一条相同不入历史；提交后输入区回 1 行、光标回 0
- [x] 输入区超出 10 行时内部滚动**跟随光标**
- [x] 用例：软换行行列映射（含宽字符）、goal column、全部键位、跨行退格、`Ctrl-J`、`↑/↓` 不动历史而 `Ctrl-P/N` 动、粘贴归一且不提交、两个确认的键位与默认答案、单行 Esc 直接清空

## Comments

## Comments

**实现完成（2026-09-21）**。落点：新增 `src/render/editor.rs`（`Input`：软换行、光标推导、键位、历史、粘贴归一）；`src/render/tui.rs` 换成持有一个 `Input` 并把 `Pending` 拆成三态；`src/render/layout.rs` 新增 `input_text_width`；`src/render/wording.rs` 新增两条确认文案；新增 `tests/render_editor.rs`（12 个用例）并改写/新增 `tests/render_tui.rs`、`tests/render_layout.rs` 的 9 个。

**四处与票面写法的偏差（都有意，理由写在代码里）**：

1. **光标的真相源是一个字符索引，不是「(逻辑行, 行内字符偏移)」二元组**。票面写的是二元组，字面实现等于把同一件事存两份，而插入/删除（热路径）要的正是扁平索引，`line_bounds()` 一趟就能把行边界求出来。两种存法的「唯一真相源」性质相同：都只有一个字段，显示行列一律在绘制时推导 —— 真正要避免的是**跨帧保存行列**，那才是 inline 时代漂移的根因。spec §5 已按此改写。
2. **正好写满一行时，光标另占一行**。`rows()` 对「恰好填满一行」的草稿返回 2：否则光标会落在它所在区域之外（终端也是这么做的）。这条是「光标必须落在画出来的区域内」的必然结果，不是选择。
3. **待答问题仍然画在输入行**（黄色），没有做成模态覆盖层 —— 那是**票 14** 的活。本票把 `Pending` 拆成 `Loop` / `Paste` / `ClearDraft` 三态并给它一个 `prompt()`，票 14 只需换绘制处。
4. **`trim` / 清空 / 入历史收在 `Input::submitted()` 里**，`TuiState::submit` 只负责「把文本送出去」与回到底部；历史是编辑器的状态，让它自己维护才只有一处会写它。

**删除与迁移**：`TuiState` 的 `cursor_column` / `input_view` / `input_line` 与全部行编辑方法删除；三个光标用例按新接缝安置 —— 宽字符与「光标跟随文本」两条并入 `tests/render_editor.rs`，「长行滚动」改写为「草稿高于输入区时窗口跟随光标」，`up_and_down_are_history_too_...` 改写为「↑/↓ 动光标、`Ctrl-P`/`Ctrl-N` 才是历史」（票 04 的决定在本票落地）。

**基线**：`cargo test` **516 passed / 0 failed**（502 → +12 编辑器 +4 状态机 +1 布局 −3 迁移）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移；pty 启动检查 3/3 GREEN。

**变异检验**（确认新测试不是摆设）：摘掉粘贴的 `\r` 归一、把超长阈值判断置假、把 Esc 的多行判断置假、让 `↑` 走历史、把输入区行数钉成 1、把 goal column 的列累加置零 —— 六处全部被抓到。

**评审收口**（`/code-review` 双轴，2026-09-21）：

Spec 轴抓到两个真 bug 与三处打折：

- **粘贴里的 `	` 会在 debug 构建下 panic**。`CellWidth for str` 对单字节控制字符有 `debug_assert`（"control character passed to cell_width without filtering"），而票面要求「保留 `\n` 与 `\t`」。现在 `\t` 在归一阶段展开成**四个空格**：缩进保住了，列宽算术也不必为 tab 猜一个宽度。同时把 `width::char_columns` 对控制字符改为返回 0 列（它们本来就不被绘制），让这个 helper 不会再踩那条断言。
- **「正好写满一行」的处理是错的**：原实现给光标**在数组末尾**补一行，于是 `"abcde\nf"`（宽 5）会把空行画在 `f` 的**下面**，光标跟着跑到下一行去。改成**终端式 pending wrap**：光标停在该行最后一格，不再另起一行（更简单，也少了一次输入区长高的抖动）。spec §5 与本票 Comments 里原先那句「给光标另起一行」已回改成这条。
- **`agrees` 把 `Enter` 当「是」**，与票 06 §1 的表「默认（Esc / 回车）= 否」冲突 —— 已改成只有 `y`/`Y` 是同意，`Esc` 与 `Enter` 都是安全答案。用例补齐了 `Enter` 这一支。
- **`paste()` 没看 pending**：模态期间粘贴会写进看不见的草稿；超长粘贴还会**覆盖** `Pending::Loop` 并丢掉 reply sender（等于把权限询问静默变成拒绝）。已加守卫（模态拥有键盘，spec §9），并加了一条更强的用例：问询期间再粘一次小的，`y` 之后提交出来的必须还是那一大段。
- `tests/wording.rs` 少了新措辞的断言、`Ctrl-J` 少了 `map_key` 那一支 —— 都补上了。

**第五处偏差（本轮补报）**：票面写「两个确认走既有 `Question`/`AnswerChoice` 机制（新增两个变体）」，实现改成了 `TuiState` 私有的 `Pending` 三态。理由：`input::Question`/`AnswerChoice` 是**控制台协议**的词汇（循环问、one-shot 回传），而这两个确认是**渲染器自问自答**，没有收件人；硬塞进去会让 `Question` 一名两义，`AnswerChoice` 也没有对应的「是/否」变体可说。票面真正要的「四问共用一套呈现」由 `Pending::prompt()` 提供，票 14 换绘制处即可。

Standards 轴：`PROMPT_COLUMNS` 常量与 `indent()` 里的 `"  "` 是同一个宽度的两处编码 —— 常量改成函数 `prompt_columns()`（由 `PROMPT` 推出），缩进由它生成；`layout::INPUT_INDENT` 删除（几何直接用 `prompt_columns()`）；输入区文本宽度只在 `layout::input_text_width` 算一次（`draw_bottom` 也调它，不再自己减）；`edited`/`moved` 改名 `text_edited`/`cursor_moved`（名字要能读出「历史浏览是否结束」的区别）；`Input::rows` 改名 `Input::height`。

**未改的两条 finding**：`clear_draft_confirm()` 用零参函数而非常量 —— 与本模块既有的零参文案（`nothing_to_undo` / `no_tool_result`）一致；`editor::PROMPT` 留在措辞层之外 —— 它是个界面符号而非文案，且旧实现就在渲染器里（先例一致）。

**变异检验（本轮）**：摘掉 tab 展开、把 `Enter` 加回「是」、摘掉粘贴的 pending 守卫 —— 三处全部被抓到。
