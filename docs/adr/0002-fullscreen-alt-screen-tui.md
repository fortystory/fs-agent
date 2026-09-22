# TUI 用 alt screen 全屏四分区布局，取代 inline viewport

TUI 渲染器从「inline 视口 + 把转录插进 scrollback」改为 **alt screen 全屏四分区**：上为基础信息（名称+版本 / cwd / 模式 / 时钟），中左为对话面板，中右为信息面板（模型 / 上下文 / token / 回合），下为输入区与快捷键提示。对话面板因此**自己持有滚动缓冲**（上限 20 000 源行、超出丢最旧、吸底但上滚后不抢），转录不再依赖终端 scrollback。这**推翻 spec §19 的 TUI 栈那一行**（`.scratch/fs-agent-v1/spec.md:526`）与用户故事 129 的「**否决 alt screen** —— 转录要能滚动 / 复制」。

顶部那块后来长成**两个变体**（`Regions::header_kind`，`src/render/layout.rs`）：终端 ≥ 42 列 × 18 行时画 **fs 标记**（5 行字符画，`wording::logo_lines`，亮品红→品红渐变）、空一行、再加一行「cwd ←→ 模式 · 时钟」；不够大时退回原来的文字顶栏（两行 / 一行）。阈值最初是 41 列 × 19 行，块与块之间那两行空行被回收给转录时一并挪到现在的位置（`.scratch/tui-ux/issues/05-prototype-geometry-without-airy.md` 的「新」列是数字来源）。**名称与版本是标记本身**，所以高顶栏不再单列那一格字面量——这是这块唯一被让出的字段。

理由不是审美，而是一个修不好的缺陷：inline 视口下光标不跟随 `>`，`>` 还会在后续回合消失。`e25097e`（把视口锚到最后一行）之后**问题仍然存在**，而根因是 inline 视口的位置本身会漂移 —— `Frame::area().y` 是视口在屏幕上的位置，`insert_before` 每插入一行就把它往下推，光标与输入行一起被拖着走。改成全屏之后 `area()` 恒为 `(0, 0, w, h)`、`set_cursor_position` 就是终端绝对坐标，**光标的坐标系里不再有会漂移的量**；对话面板里更是根本没有光标。

被否决的替代方案：继续修 inline 视口的光标（已修过一轮，没有稳定结论，且它挡在"能用"前面）；两进程拆分 TUI 与 agent server（spec §19 已否决，理由 —— IPC 会逼「单一写入者 + `seq` 唯一身份」重做 —— 仍然成立，本 ADR 重申）；把多行输入延后（用户选择现在就做，代价是重做刚完成的单行编辑器：编辑器因此拆成独立的 `Input` 类型，`src/render/editor.rs`）。

## Consequences

- **代价（用户已知并接受）**：终端原生选择只覆盖可见区；**复制要按住 Shift 拖拽**（为了可点击的「到最下」按钮与滚轮滚动，鼠标捕获已开启）；转录只在进程内存里，退出 alt screen 即清，**不做 dump** —— 完整记录仍在会话日志里，`--continue` 可回看。
- **鼠标与 bracketed paste 是我们自己开的**：`ratatui::init()` 只负责 raw mode + alt screen + panic hook，`TerminalOptions` 连鼠标开关都没有。`EnableMouseCapture` / `EnableBracketedPaste` 必须在 `/quit`、空闲 `Ctrl-C`、panic 三条路径上**成对撤销**，否则退出后终端残留鼠标报告模式（panic hook 只 restore 终端面，不管这两个）。
- **inline 路径删除**：`insert_before` 循环、`paint_scrollback`（含 `src/render/mod.rs:52` 的再导出）、`Viewport::Inline(LIVE_HEIGHT)`、底部锚定的 `MoveTo(0, rows - 1)` 全部消失。注意 `Terminal::insert_before` 这个 API **本身仍然存在**（对 Fullscreen 是 no-op），删的是我们的用法，不是它。
- **保真度唯一变更**：非 assistant 的消息（主要是用户自己的输入）不再压成单行 + `truncate(text, 500)`，改为保留换行的多行渲染 —— 多行输入上线后，粘进来的 20 行正是最需要看全的内容。assistant 的回答本来就是全文 Markdown（`src/render/tui.rs:802-827`），不动；工具输出继续用 4 000 字符 preview。
- **可测性反而变好**：`TestBackend` 在默认特性下可用（无特性门），布局、降级阶梯、滚动与吸底都能进 `cargo test`，不必依赖 pty；`scripts/tui-startup-check.py` 只需改判定（底部块现在带边框，`退出` 后面多了 `│`）而不是重写。
- **与 ADR 0001 的关系**：本 ADR **不动模型可见文本**，只动呈现。ADR 0001 的冻结清单（debater/synthesizer system prompt、投影的 `[轮 N · 名字]` 前缀、`AgentError.message`、fs-agent 工具结果）逐条不受影响；本 ADR 新增的字符串全部落在措辞层 `src/render/wording.rs`。
- **标记顶栏的代价（后加，2026-09）**：高顶栏多占 5 行，120×24 的中块因此从 12 行缩到 7 行——**转录、面板、滚动翻页的可见行数都是它的下游**，`tests/render_layout.rs` 的若干断言（以及 `scripts/tui-startup-check.py` 的身份判定）随之从"记住第几行"改成"从帧里推出中块位置"。低于 42×18 一律回退文字顶栏，所以窄终端（含 40×10 的地板）行为不变。**块与块之间不留空行**（`layout::CHROME` 里那两项被删掉）：省下的两行归转录。
