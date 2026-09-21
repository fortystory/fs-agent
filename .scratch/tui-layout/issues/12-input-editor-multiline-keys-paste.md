# 12: 输入编辑器：`Input` 类型、多行、键位、粘贴与两种确认

**What to build:** 把单行输入升成**多行编辑器**：编辑器拆成独立的 `Input` 类型（`src/render/editor.rs`），支持软换行、跨行移动、按行作用的 readline 键位、`Ctrl-J` 换行、粘贴多行不提交，以及两个确认（超长粘贴、Esc 清空多行草稿）。光标的唯一真相源是「逻辑行 + 行内字符偏移」，显示行列渲染时算。

Blocked by: 10

Status: ready-for-agent

**参考:** spec §5（`Input`）、§6（键位表）、§7（粘贴与两种确认）、§2（输入区行数与续行缩进 2 格）

- [ ] 新建 `src/render/editor.rs`，`pub struct Input`；搬进 `input` / `cursor` / `history` / `history_at` / `draft` + 全部行编辑方法 + `byte_at`。列宽算术**不搬**：票 11 已经把它收到 `src/render/width.rs`（`text_columns` / `char_columns` / `truncate_columns`），编辑器直接用那一份；换行口径与 `pane::wrap_line` 一致（逐字符、CJK 2 列）。`TuiState` 持有一个 `Input`，只保留「提交后把文本送出去」那一半
- [ ] `Input` 新增：软换行（逻辑行 → 显示行）、光标行列计算、`insert_str`、`rows(width)`
- [ ] 软换行**逐字符、CJK 2 列、不按词边界**（与转录同一口径）；最小单位是字符不是 grapheme 簇
- [ ] **光标的唯一真相源 = `(逻辑行, 行内字符偏移)`**；显示行列每次渲染算出来，不累加维护；`set_cursor_position` 直接写绝对坐标（全屏下无视口偏移）
- [ ] `↑`/`↓` 缓冲内移动并**保持视觉列（goal column）**；横向移动或插入字符时清掉；首行再 `↑`、末行再 `↓` 不动
- [ ] `Key` 枚举新增 `CtrlJ`；**`map_key` 的 CONTROL 分支加 `'j'`**（现在它落到 `_ => None`，Ctrl-J 被丢弃）
- [ ] 全键位表：`Enter` 提交；`Ctrl-J` 插入 `\n`；`Shift+Enter` **不做**（等同 Enter）；`Backspace`/`Delete` 在行首行尾**跨行合并**；`←`/`→` **跨行**；`Ctrl-P`/`Ctrl-N` 历史（唯一入口）；`Home`/`Ctrl-A`、`End`/`Ctrl-E`、`Ctrl-U`/`Ctrl-K`/`Ctrl-W` **全部作用于当前逻辑行**
- [ ] 渲染循环同时接受 `CtEvent::Key(kind == Press)` 与 `CtEvent::Paste(_)`（`Event::Paste` 不是 `Key`，别被 `Press` 过滤丢掉）
- [ ] 粘贴：`\r\n` 与裸 `\r` **归一成 `\n`**；**过滤除 `\n`/`\t` 外的控制字符**；按光标整体插入；**永不触发提交**
- [ ] **超过 100 000 字符**（`chars().count()`，不是字节）→ 先确认 `粘贴 {n} 字符？`，默认**否**
- [ ] **Esc 清空多行草稿**（输入含换行）→ 先确认 `清空输入？`，默认**否**；单行仍然直接清空；忙 → 取消回合、有模态 → 默认答案的行为不变
- [ ] 两个确认都走既有 `TuiState.pending` / `Question` / `AnswerChoice` 机制（新增两个变体）
- [ ] 历史保持**进程内**；多行条目原样保存（含内部 `\n`）、召回时整体替换、光标置末尾；`draft` 语义不变
- [ ] 提交只 trim **首尾**空白（含首尾换行），**中间换行保留**；空提交不入历史、不发消息；与上一条相同不入历史；提交后输入区回 1 行、光标回 0
- [ ] 输入区超出 10 行时内部滚动**跟随光标**
- [ ] 用例：软换行行列映射（含宽字符）、goal column、全部键位、跨行退格、`Ctrl-J`、`↑/↓` 不动历史而 `Ctrl-P/N` 动、粘贴归一且不提交、两个确认的键位与默认答案、单行 Esc 直接清空

## Comments
