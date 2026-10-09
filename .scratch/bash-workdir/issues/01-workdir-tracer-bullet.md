# 01 — `workdir` 的 tracer bullet

Type: implement
Status: done

> 规格：[`../spec.md`](../spec.md) §1（形状）、§2（四条工具错误）、§3（三条不变式）、
> §5（落点）、§6（验收）。这一票让 `bash(command, timeout_ms?, workdir?, escalation?)` 跑通：
> 参数进声明、解析、站到那个目录、而沙箱边界一个字不动。

## 目标

模型可以在 `bash` 的参数里指一个**工作区之内**的目录，让这条命令站在那里跑；指到区外、
目录不存在、空串、或与 `escalation` 同现，都是一条中文工具错误，命令不跑。

## 现状（2026-10-09 核实，动手前复核）

- `src/tools/bash.rs:72-106` 是 `spec()`：三个属性（`command` 必填、`timeout_ms`、`escalation`），
  描述由 `SANDBOX_NOTE` 与 `ESCALATION_NOTE` 两个常量拼成。
- `src/tools/bash.rs:129-142` 是 `call()`：算完 timeout 就 `process::run(ctx.cwd, &argv, limit, ctx.sandbox)`。
- `src/tools/process.rs:106-119` 是唯一的 spawn 点：`sandbox.wrap(argv, cwd)` 之后
  `Command::new(program).current_dir(cwd)`。
- `src/tools/sandbox.rs:280-331` 的 `wrap(argv, cwd, spec)` 里**同一个 `cwd`** 同时喂给
  `writable_roots(cwd)`（:297）与 `protected_paths(cwd)`（:324）——所以要拆两参。
- `src/tools/custom.rs:95` 也走 `process::run(ctx.cwd, ...)`，动态工具不加这个参数。
- `src/tools/tool.rs:110-143` 的 `ToolContext` 暴露 `read_paths`（`dyn ReadPathResolver`）与
  `cwd`。`SessionPaths` 有严格与 `relaxed_read` / `relaxed_write` 两态
  （`src/tools/paths.rs:52-100`），**必须用严格的那一份**解析 `workdir`：区外读被放行时
  `read_paths` 不收容，拿它解析会让「只能落在区内」漏成「区外可跑」。
- 收容判定的口径在 `src/permissions.rs:125`（`workspace` 档按 `call.cwd` 划界）与
  `src/tools/paths.rs` 的 `SessionPaths::resolve`（不存在时经最深已存在祖先解析）。
- 测试的落点先例：`tests/bash_tool.rs`（假 provider 驱动真实工具）、`tests/sandbox.rs`
  （`wrap()` 是纯函数，可以逐字断言 argv）。

## 做什么

1. **`src/tools/bash.rs`**：`parameters` 里加 `workdir`（string，描述按 spec §1 定稿的那句常量
   的一部分，**与实现同一票**——否则模型会先看见一个还不存在的参数）。解析函数照
   `requested_timeout_ms` 的形状：存在但不是非空字符串 → 工具错误（`""` 同样）。
2. **严格解析**：`ToolContext` 补一个入口走 `SessionPaths` 的严格解析（不复用可能 relaxed 的
   `read_paths`），拿回绝对、无符号链接的目录；解析结果落在工作区之外 → 工具错误。
3. **存在性**：解析结果必须是**已存在的目录**。不存在与「不是目录」（文件、`.git` 那个文件）
   都给工具错误，且**不得**替它 `mkdir`。
4. **同现检查**：`workdir` 与 `escalation` 同时存在 → 参数错误。
5. **`src/tools/process.rs` + `src/tools/sandbox.rs`**：`run()` 与 `wrap()` 拆成**边界与站位**
   两个参数——可写根与保护路径取边界（会话工作区），`current_dir` 取站位。
   `src/tools/custom.rs` 的调用点给同一个值。
6. **门与事件流不动**：`Call.cwd` 仍是工作区，不新增事件类型（spec §3 第三条）。

## 验收（`../spec.md` §6 的九条）

区内相对路径、区内绝对路径、区外的三种写法（`../`、区外绝对路径、`src/../../..`）、
`""` 与 `"."`、不存在的目录（并断言盘上没有多出东西）、与 `escalation` 同现、
`wrap()` 产出的 argv 里仍有工作区根的 `--bind` 与工作区 `.git/config` 的 `--ro-bind`
（`workdir` 取 `src`）、兄弟工具基准不变、`ToolCallStarted.args` 带着原文。

`tests/sandbox.rs` 里断言 argv 的那类测试要跟着拆参改签名；别为了让旧断言继续过而把
边界与站位合成一个参数。

## 不做

- 不加 `/tmp` 特例、不替模型建目录、不让 `workdir` 改沙箱边界、不给动态工具加这个参数、
  不动 `permissions.rs`、不记新事件、不在转录里单独画。
- 不碰 `--cwd`（工作区仍是 `--cwd` 或启动时的当前目录）。

## 完成判据

`cargo test` 全绿；`bash(command, timeout_ms?, workdir?, escalation?)` 的声明与
[`docs/bash.md`](../../../docs/bash.md) 里那张形状表逐字一致（票 02 收口时核对）。

## 评论

2026-10-09 落地。形状与本票一致，只有两处值得记下来：

- **同现检查住在 `BashTool::escalation()`，不在 `call()`**：`facts()` 会在权限门**之前**调它
  一次，于是同现的调用连一次审批都不会弹。放进 `call()` 的话，用户会先被问一次注定失败的动作
  —— 这一条第一次跑测试时就露出来了（`asked` 断言拿到 1）。
- **`wrap()` 只收边界一个参数**：站位不进挂载表，它由 `process::run()` 的 `current_dir`
  表达。本票那句「`run()` 与 `wrap()` 拆成边界与站位两个参数」按这一族函数读 —— `run()` 收
  两个，`wrap()` 只碰前者。

上面「现状」一节的行号随这次改动漂了：`bash.rs` 多了 `WORKDIR_NOTE`、
`requested_workdir()` 与 `resolve_workdir()`；`run()` 与 `wrap()` 的签名各改了参数。

验证：`cargo test` 1550 passed / 0 failed（`tests/bash_tool.rs` 19 条、`tests/sandbox.rs`
43 条）；`cargo clippy --all-targets` 只剩基线里那批 `collapsible_if`；`cargo fmt --check` 干净。
