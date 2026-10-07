# 35 — `/undo` 成功时要开口

Type: implement
Status: done
Blocked by: —

> 来源：2026-10-07 维护者在真终端里连着试了两次 `/undo`（先由 agent 改 `rustfmt.toml`、再改
> `scripts/__init__.py`，两次都撤回了）：文件都**精确回滚**了、流里也留下了那条
> `HistorySuperseded { reason: Undo }`，**但对话里一个字都没有** —— 屏幕上看起来与「什么都没
> 发生」一模一样。这一票修的就是这处缺口，回滚本身一个字不动。

## 目标

`/undo` 的三种结果**各说一句话**，成功那次尤其：它刚把工作区里的一个文件改回原样，而转录里
没有别的东西替它说话。

## 现状（2026-10-07 核实，改前先复核）

- `src/cli.rs` 的 `Submission::Undo` 分支只处理了两种结果：`Ok(None)` 给「没有可撤销的修改」、
  `Err` 给错误报告，而 `Ok(Some(_)) => {}` 是**空的** —— `UndoOutcome` 被丢掉，尽管它的文档
  写的就是「供前端叙述」。
- 后果：撤销成功时前端一声不吭；人无法分辨「已经回滚」与「没东西可撤」。
- 回执通道是现成的：`Harness::notice` → `RenderEvent::Notice` → 三个渲染器都画成一行（TUI 里
  落进转录的 `Block::Notice`，plain 与 headless 同理）。缺的只是有人调它。
- 这条链路**没有测试**：`tests/session_store.rs` 与 `tests/credentials.rs` 里的 undo 测试都直接
  调 `harness.undo_last_edit()`，从不过前端（那也正是它能一直缺着的原因）。

## 具体行为

- `src/render/wording.rs` 加 `undone(path: &Path) -> String` → `已撤销对 <path> 的这次编辑`。
  路径就用 `UndoOutcome` 里那个已解析的路径，与流上 `HistorySuperseded.summary` 的
  `撤销对 <path> 的这次 edit_file` 同一口径。
- `src/cli.rs` 把三种结果收进一个纯函数 `undo_receipt(outcome) -> String`，循环里只剩
  `harness.notice(&receipt)`。三条都带 `heng: ` 前缀，与别的命令回执一致。
- 「没东西可撤」与失败两句的措辞一个字不改（失败仍是 `撤销失败：<detail>`）。

## 测试

- `src/cli.rs` 的 `tests`：`an_undo_always_answers_with_one_line` —— 三种结果各断言一次，成功
  那条断言的就是这一票的核心。
- `tests/wording.rs` 的 `interactive_feedback_reads_in_chinese`：加 `undone` 的精确文本。
- `cargo test` 全绿、`cargo fmt --check` 干净、`cargo clippy --all-targets` 不新增 warning。

## 不做什么

- 不让 `undo_last_edit` 自己发 notice：`src/lib.rs` 那句「手势只在前端存在」仍然成立，回执与
  「没有可撤销的修改」并排住在前端。
- 不给成功的回执加细节（改了几行、还剩几步可退）—— 那一行只回答「刚才那下有没有生效」。
- 不动事件流：`HistorySuperseded` 那条记录一个字不改。

## 评论

- **落地（2026-10-07）**：`undo_receipt` 落在 `src/cli.rs`（`interactive_loop` 之前），
  `Submission::Undo` 那一支从 9 行的 `match` 收成 4 行。`wording::undone` 紧挨着
  `nothing_to_undo` —— 这两句本来就是一对（有事发生 / 无事发生）。
- **写这一票时的实测**：`cargo test` **1388 passed / 0 failed**（本票新增 1 条集成测试 + 1 条
  断言）；`cargo fmt --check` 干净；`cargo clippy --all-targets` 的 43 条 `collapsible_if`
  warning 全是既有的、落在没有碰过的代码里（`src/agent/executor.rs`、`src/render/tui.rs`
  那一批），本票不新增。
- **验收没做完的部分**：真终端里那一下由维护者走查 —— 敲 `/undo` 之后转录里应当多出一行
  `heng: 已撤销对 <路径> 的这次编辑`。
