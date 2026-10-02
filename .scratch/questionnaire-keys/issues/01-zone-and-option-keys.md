# 01 — 问卷的选项区与导航键位（`Zone`、`j`/`k`、`Ctrl-N`/`Ctrl-P`）

Type: implement
Status: done
Blocked by: —

> 来源：[`../spec.md`](../spec.md) §1、§2。这是这一组的**第一张**：它引入 `Zone`，
> 后面几张都建在它上面。设计理由在 [ADR 0010](../../../docs/adr/0010-questionnaire-keys-dispatch-by-zone.md)。

## 目标

- `Questionnaire.custom_focused: bool`（[`src/render/tui.rs:761`](../../../src/render/tui.rs)）换成
  **`Zone { Options, Input }`**：**问卷级**的状态（每题共用，翻页回到选项区），不是每题一份。
- 选项区里 `j`/`k`/`Ctrl-N`/`Ctrl-P`/`↑`/`↓` 走**同一条分派**移动高亮，两端**环绕**，
  越过边界就进输入区。
- 输入区里：`j`/`k` 是文本、`Ctrl-N`/`Ctrl-P` 静默、`↑`/`↓` 回选项区并移动高亮。
- 选项区里**可打印字符与 `Backspace` 一律吞掉**。
- 没有选项的题：`Zone` 恒为输入区，可打印字符直接进文本。

## 现状（2026-10-02 核实，改前先复核）

- 三个写入点与一个读点：`press` 的字符分支（[:838](../../../src/render/tui.rs)）、
  `move_highlight`（[:890](../../../src/render/tui.rs)）、`advance`（[:913](../../../src/render/tui.rs)）、
  `back`（[:920](../../../src/render/tui.rs)）、`select_option`（[:938](../../../src/render/tui.rs)）、
  `focus_custom`（[:948-950](../../../src/render/tui.rs)）、`unfocus_custom`（[:953-955](../../../src/render/tui.rs)）；
  唯一的绘制读点在光标那处（[:3824-3838](../../../src/render/tui.rs)）。
- `move_highlight` 今天是 **clamp**（[:893](../../../src/render/tui.rs)），不环绕，也进不了输入区。
- 裸 `j`/`k` 在全局**没有任何绑定**（只有 `Ctrl-j`/`Ctrl-k`，编辑器里是换行与 kill-to-line-end）。
- `Ctrl-N`/`Ctrl-P` 已经被 `map_key` 认识（[:133-134](../../../src/render/tui.rs)），
  编辑器里是历史上下条（[:1845-1846](../../../src/render/tui.rs)），**问卷里今天落 `_ => {}`**。
- 今天 `press` 里 `Key::Char(ch)` 是**无条件**的：`custom_focused = true` + 写进文本
  （[:837-840](../../../src/render/tui.rs)）——这正是本票要否掉的那条。

## 具体行为

- `Zone` 与 `highlight` **正交**：`highlight` 仍然只是选项区里的下标，进输入区**不写进**它。
- 环绕：选项区里向下越过最后一项、向上越过第一项（含只有一项时），落点都是输入区。
- 选项区的吞掉范围：`Key::Char(_)` 与 `Key::Backspace`；`Enter`/`Space`/`Tab`/`←`/`→` 照旧
  （`Enter` 与答案形状有关，见票 02）。
- 没有选项的题（`count == 0`）：`j`/`k`/`Ctrl-N`/`Ctrl-P`/`↑`/`↓` 一律静默，可打印字符进文本。

## 测试

- [`tests/ask_user_question_tui.rs`](../../../tests/ask_user_question_tui.rs) 新增：
  - 选项区 `j`/`k`/`Ctrl-N`/`Ctrl-P`/`↑`/`↓` 移动、两端环绕、越界后 `j`/`k` 变成文本；
  - 输入区里 `j`/`k` 落进文本、`Ctrl-N`/`Ctrl-P` 静默、`↑`/`↓` 回选项区并移动；
  - 选项区里可打印字符与 `Backspace` 吞掉（屏幕上不多出字符、`custom` 仍为空）；
  - 没有选项的题：可打印字符直接进文本。
- `cargo test` 全绿、`cargo clippy --all-targets` 干净。

## 不做什么

- 不动答案形状（票 02）、不动 `Esc`（票 03）、不动页脚（票 04）、不动折行（票 05）、
  不动焦点视觉（票 06）。
- 不给输入区加行内光标或历史（`←`/`→` 仍翻页，见 spec §10）。
- 不加 `Ctrl-U`/`Ctrl-W`/`Ctrl-J`/`Ctrl-K`。

## Comments

- **落地（2026-10-02）**：`custom_focused: bool` 换成 `zone: Zone`（`Options`/`Input`，问卷级、翻页按新题
  复位）；`move_highlight` 换成 `step` —— 选项区越过两端进输入区，输入区里回来并绕到另一端。
  `press` 里 `j`/`k`/`Ctrl-N`/`Ctrl-P`/`↑`/`↓` 走同一条 `step`，可打印字符与 `Backspace` 只在
  `Zone::Input` 才动文本。滚轮同一条路（输入区静默）。构造时 `reset_zone()` 定起始区域，所以
  没有选项的题一开始就在输入区。
- **先红后绿**：新加的六条（`j`/`k` 与 `Ctrl-N`/`Ctrl-P` 同路、越界进输入区、输入区里两键静默、
  选项区吞字符与 `Backspace`、无选项题只有输入区、票 34 那条按新路改写）改前全红。
  既有五条依赖「打开就能打字」的测试跟着改成先 `walk_into_the_input()`（新增的 helper）——
  那是这次有意改掉的行为。
- **验收**：`cargo test` 全绿（问卷那个文件 21 条）、`cargo clippy --all-targets` 干净。
