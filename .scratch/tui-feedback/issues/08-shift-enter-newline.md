# 08 — `Shift+Enter` 换行（与 `Ctrl-J` 同一个动作）

Type: implement
Status: done
Part of: ../spec.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §10。

## 目标

真机反馈：「添加一个快捷键 `shift-enter`，作用是换行，和 `ctrl-j` 相同」。

## 现状（改前先复核）

- `map_key`（`src/render/tui.rs`）把 `Ctrl-J` 映射到 `Key::CtrlJ`，`TuiState::key` 里它插入一个
  换行；裸 `Enter` 是提交。
- `enable_terminal_modes` / `disable_terminal_modes` 管着鼠标、括号粘贴与终端标题这三样，**没有**
  键盘增强协议 —— 于是 `Shift+Enter` 在协议层与裸 `Enter` 不可区分（多数终端根本不报它）。
- 既有约定：提示行**从不写** `shift+enter`（`tests/wording.rs` 的 `no_hint_ever_names_shift_enter`
  逐宽度扫过），理由写在 `docs/tui-manual-checklist.md` ⑥。

## 落点

`src/render/tui.rs`（`Key` 枚举、`map_key`、`enable_terminal_modes` / `disable_terminal_modes`、
lib 单测）、`docs/render.md`、`docs/tui-manual-checklist.md`。

## 具体行为

1. `Key::CtrlJ` 改名 **`Key::Newline`**：换行现在有两个键，内部名字不该只说其中一个。
2. `map_key` 在按 `KeyCode` 分派**之前**加一条：`Enter` + `SHIFT` → `Key::Newline`。裸 `Enter`
   照旧是 `Key::Enter`（提交）。
3. `enable_terminal_modes` 推 `PushKeyboardEnhancementFlags(DISAMBIGUATE_ESCAPE_CODES)`，
   `disable_terminal_modes` 弹 `PopKeyboardEnhancementFlags` —— 两支都成对：挂起恢复会走
   `disable` 再走 `enable`，因此不会叠层。不支持的终端把这一串当没看见。
4. **提示行不改**：在没有键盘增强协议的终端上 `Shift+Enter` 到不了我们手里，写它就是一句假话
   （`no_hint_ever_names_shift_enter` 照旧绿）。

## 验收

- lib 单测：`Enter` + `SHIFT` → `Key::Newline`；`Char('j')` + `CONTROL` → `Key::Newline`；
  裸 `Enter` → `Key::Enter`。
- `Key::Newline` 与从前的 `Ctrl-J` 一样插入换行（既有编辑器断言照旧绿）。
- `no_hint_ever_names_shift_enter` 与「任何宽度下都没有幽灵换行键」两条断言不变地通过。
- `cargo test` 全绿。
