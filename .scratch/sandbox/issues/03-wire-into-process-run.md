# 接进 `process::run`，并区分「沙箱坏了」与「命令失败」

Type: implement
Status: done
Blocked by: 01, 02

> 规格：`.scratch/sandbox/spec.md` §2（接缝）、§6（拒绝信号）。

## 目标

让 `bash` 与动态工具真的跑在沙箱里，同时保证两件事：超时与进程组 kill 一点没变；「bubblewrap 没起来」被认成工具错误，而「命令被只读挡回」原样交给模型。

## 落点

`src/tools/process.rs`（`run()`）、`src/tools/bash.rs`、`src/tools/custom.rs`、`src/tools/tool.rs`（`ToolContext`）、`tests/` 下的工具测试。

## 具体行为

1. `process::run` 现在直接 spawn `argv`；改成 spawn `wrap(...)` 的结果。**超时、`process_group(0)`、`killpg`、输出捕获一行不动**——bubblewrap 会 exec 目标命令并透传退出码，进程组语义不变（`bwrap` 是组的 leader，它的子进程与它同组，所以 `killpg` 照样收得干净）。
2. `ToolContext` 加 `pub sandbox: &'a Sandbox`，形状与现有的 `bash: &'a BashLimits` 一致。
3. 两个调用点把沙箱传下去：`tools/bash.rs` 与 `tools/custom.rs`。**执行者与讨论者共用同一张工具表，所以自动同等生效**——写进测试，别让它成为偶然。
4. **拒绝信号的判据只有一条**：stderr 以 `bwrap: ` 开头 → 沙箱没起来 → 返回 `ToolError`（文案说明命令没有跑）。其余任何非零退出（包括内核给的 `EROFS`）**原样返回给模型**，不做识别、不加解释。
   - **不要**拿「只读文件系统」这类文本做判据：它随 locale 变（中文环境下 bwrap 的 EROFS 显示成「只读文件系统」，`LC_ALL=C` 下是 `Permission denied`），那会是一处会在中文环境里静默失效的逻辑。
5. 非零退出仍然是**结果**而不是错误（仓库既有约定），这一票不改它。

## 测试

- 用假 `bwrap`（`PATH` 上的脚本）断言：`run()` spawn 的是 `bwrap`，原命令出现在 `--` 之后；
- 假 bwrap 输出 `bwrap: …` 且非零退出 → `ToolError`；
- 假 bwrap 透传一个非零退出码与 stderr → 是命令结果，`ToolError` 不出现；
- **超时与 kill 的既有测试不改一行，并且仍然过**——这是「超时逻辑没被碰」的验收；
- `ToolContext` 在三个前端与执行者下都带着沙箱。

## 评论

- **2026-10-08 补一条实测与两处修正**：第 3 条那句「执行者与讨论者共用同一张工具表，所以自动
  同等生效 —— 写进测试」当时只写了讨论者那一条
  （`a_discussion_forked_from_a_live_session_keeps_the_sandbox`）。今天实测到的真凶在**第三条路**
  上：`Session::retarget`（`model-switching` 那一轮加的）整份替换会话配置，而 `retarget_to` 造的那份
  里 `sandbox.availability` 还是解析期的 `Untested` —— 会话中途换一次模型，`bash` 就从那一刻起永久
  拒绝，理由还是一句与这台机器无关的「沙箱状态还没有定下来」（`WAYS_OUT` 那句「装一个 bubblewrap」
  当时把它指向了一个已经装好的东西）。修法是把这条规则收进 `Session::inherit_sandbox`（`fork` 与
  `retarget` 共用，第三个调用者不会再漏），并补三条测试：换模型之后沙箱照旧可用、执行者的 `bash`
  与主会话跑在同一个沙箱里、`Untested` 的错误里不许出现 `WAYS_OUT`。见 `3fe909e`。
