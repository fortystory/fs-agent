# 会话回执：终端交还之后往 stderr 打一行

Type: implement
Status: done
Blocked by: 03

> 规格：`.scratch/exit-gesture/spec.md` §5（退出回执）、§7、「补充说明」。
> 交互式会话退出时给一行能直接粘的复盘命令；它必须在 `TerminalModes::drop` 与 `ratatui::restore()` 之后打，所以依赖 [票 03](03-ordered-exit-130.md) 把 130 那条也变成有序收尾。`--plain` 与 TUI 共用 `interactive()`，同一个收尾函数两处都覆盖。

## 目标

- `wording::discussion_replay` 抽成通用的会话回执生成器，`discuss` 与交互式退出共用。
- 交互式会话（TUI 与 `--plain`）正常结束时，**在终端交还之后**往 **stderr** 打一行 `会话 {id}；复盘：fs-agent sessions show {id}`；正常退出（0）与忙碌双击（130）都打。
- 打印点落在**可测的收尾函数**上 —— `interactive_loop` 里直接 `eprintln!` 的话没人能断言它（spec「测试决定」）。

## 现状（2026-10-01 量的，改前先复核）

- `wording::discussion_replay`（`src/render/wording.rs:74-76`）已经在生成这个模板；它的文档注释本来就写着"给 alt screen 恢复之后打的那一行"。
- 目前唯一的调用点是 `src/cli.rs:695`：`discuss` 在 `harness.shutdown()` **之后**刻意 `eprintln!`（同行 `:685-696`），但用的是 `stored.id`，不是新函数。
- 交互式会话没有任何回执：`interactive()`（`src/cli.rs:201`）的尾部（`:420-435`）是 `let code = interactive_loop(...).await; harness.shutdown().await; code`。
- 通道规矩：stdout 只承载最终产物（`src/render/plain.rs:337-350`、`docs/render.md`），所以回执走 stderr。
- `tests/wording.rs:115` 有 `discussion_replay` 的现有断言，改名时要一起动。
- 启动横幅已经有 id（`src/cli.rs:404-410`），所以启动**不打**。

## 落点

- `src/render/wording.rs`：改名/抽通用生成器。
- `src/cli.rs`：新增收尾函数；`interactive()` 的收尾；`discuss` 的调用点 `:695`。
- 测试：`tests/wording.rs`，以及一条覆盖收尾函数的断言（放进 `tests/render_console.rs` 同形状的新文件，或 `src/cli.rs` 的 `mod tests`）。

## 具体行为

1. `wording::discussion_replay` → `session_receipt`（通用名），内容一字不变；`src/cli.rs:695` 与 `tests/wording.rs:115` 同步改名。`discuss` 与交互式退出用**同一个**生成器，理由就是那条"给 alt screen 恢复之后打的那一行"的注释。
2. 新增一个可测的收尾函数（名字实现定，例如）：

   ```rust
   pub fn finish_session<W: std::io::Write>(
       code: ExitCode,
       session_id: &str,
       out: &mut W,
   ) -> ExitCode
   ```

   它往 `out` 写一行 `fs-agent: {session_receipt(id)}`，然后**原样返回 `code`**。写失败（管道断了）吞掉就好，绝不因此改退出码。
3. 生产调用点：`interactive()` 里 `harness.shutdown().await` **之后**、返回 `code` 之前，用 `harness.session_id().as_str()` 调 `finish_session`，writer 传 `&mut std::io::stderr()`。id 取 `harness.session_id()`，**不**碰 `SessionFacts.session_id` 这个死字段（spec「补充说明」）。
4. 打几次、给谁：
   - 交互式会话（TUI 或 `--plain`）正常结束：**一行**；
   - 启动：不打（横幅已有 id）；
   - `discuss`：继续打它自己那行（改成走 `session_receipt`）；
   - `probe` / `sessions` 一族：不打；
   - **不**打 `-c` 的提示 —— `-c` 是"继续本工作区最新"，它不需要 id。
5. 两条路径都打：正常退出 0 与忙碌双击 130（后者由票 03 带回 `interactive()` 的返回值）。

## 测试

- `tests/wording.rs`：`session_receipt("01J8ZQ4K7M") == "会话 01J8ZQ4K7M；复盘：fs-agent sessions show 01J8ZQ4K7M"`；断言内容含会话 id 与 `fs-agent sessions show`（原 `:115` 那条改名后继续用）。
- 至少一条覆盖收尾函数的断言：传 `Vec<u8>` / `CaptureBuf` 当 writer，调 `finish_session`：
  - 写出的那一行含会话 id 与 `fs-agent sessions show`（即"内容含 id 与 `sessions show`"）；
  - 返回的 `ExitCode` 就是传进去的那个 —— `ExitCode::SUCCESS` 与 `ExitCode::from(130)` **各一条**；
  - writer 是测试自己传的，所以"打在哪"由生产调用点保证（传的 stderr）；`--plain` 与 TUI 都走 `interactive()`，同一个收尾函数把两处都覆盖到。
- `discuss` 那行仍然打同一句（换成 `session_receipt`），`tests/discussion.rs` / `tests/wording.rs` 里相关断言保持绿。
- `cargo test` 全绿。

## 不做什么

- 不改启动横幅、不给 `probe` / `sessions` 一族加行、不打 `-c` 的提示、不在 `--plain` 里加手势。
- 不删 `SessionFacts.session_id`（spec「补充说明」：是否顺手删掉留给别的收尾）。
- 回执里不放别的东西（不打印日志路径、模型名、花费）。
- 不改 `discuss` 的打印时机与通道（它已经在 shutdown 之后打 stderr）。
- 不把这行写进 TUI 的活动区域或 stdout。

## 评论

- **落地**：`wording::discussion_replay` → `wording::session_receipt`（内容一字不变，注释改成「一场会话可以从哪里读回来」）；`claude` 侧新增 `finish_session<W: Write>(code, session_id, out) -> ExitCode`，它写一行 `fs-agent: {session_receipt(id)}` 并原样返回 `code`（写失败吞掉，绝不改退出码）。
- **生产调用点**：`interactive()` 的尾部 —— `interactive_loop` 返回、`harness.shutdown()` **之后**，writer 是 `std::io::stderr()`。会话 id 在 `shutdown` 之前抄下来（`shutdown` 把 harness 收走）。TUI 与 `--plain` 共用 `interactive()`，所以两处一次覆盖；0 与 130 两条路径都打（130 由票 03 带回返回值）。
- **`discuss`**：它自己那行改走 `session_receipt`，打印时机与通道（shutdown 之后、stderr）一个字没动。
- **不打的地方**：启动（横幅已经有 id）、`probe` / `sessions` 一族、以及 `-c` 的提示（`-c` 是「继续本工作区最新」，不需要 id）。
- **测试**：`src/cli.rs` 的 `mod tests` 新增 `the_receipt_names_the_session_and_never_changes_the_exit_code` —— 传 `Vec<u8>` 当 writer，`ExitCode::SUCCESS` 与 `ExitCode::from(130)` 各一条，断言整行内容（含会话 id 与 `fs-agent sessions show`）与返回码不变；`tests/wording.rs` 的旧断言改名后继续用同一句期望。
- `cargo test` 全绿（968 条）；`cargo clippy --all-targets` 无警告。
