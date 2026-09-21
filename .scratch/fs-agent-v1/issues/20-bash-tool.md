# 20: `bash` 工具

**What to build:** 一个 `bash(command, timeout?)` 内建工具，让模型能在会话 cwd 里跑命令——argv 不经 shell 拼接、有超时、超时连进程树一起终止、结果照常进事件流并受单次结果截断。

Blocked by: 04

Status: done

**为什么会有这张票**：spec §7 把 `bash` 列进 **v1 全部内建工具集**（读 / 写 / 编辑 / `bash` / `skill` / `repo_map` / `task`），但 19 张实现票里没有一张拥有它——票 03 与票 04 的注释都写着「`bash`（后续票）是它的主人」，票 09 也写着「`bash` / `task` 未做」，`task` 由票 11 补上，`bash` 漏了。票 15（硬 plan 模式）要断言「`bash` 被拒」时发现了这个缺口，用一个注入了 `Exclusive` 的测试工具代替，并留下本票。

**参考:** spec §7（工具 trait 与 dispatch）、§12（`CommandPrefix` 作用域与 `rm` 断路器）、§10（单次结果截断）、§20（v1 不做进程级沙箱）

- [x] `spec()`：`bash(command: string, timeout_ms?: integer)`；`effect()` **恒为 `Exclusive`**（shell 能写任何东西，注册表按它取工作区独占锁）
- [x] `command(args)` 返回 `["bash", "-lc", command]`（或 `sh -lc`）——`CommandPrefix` 作用域与 `rm` 断路器靠它看到 argv；**`rm` 打到 `/` / `~` 及其父目录一律 `Deny`**（票 04 已在门里实现，本票把它接到真工具上）
- [x] **执行不经 shell 拼接**：argv 数组直接 `spawn`，命令文本作为一个元素交给 `bash -lc`；模型拼不出第二层注入
- [x] **超时 + 进程树终止**：超时不是 drop future 就算——按进程组（`setsid` + `killpg`）终止，子进程不留在后台；默认超时与上限进 `SessionConfig`
- [x] 结果约定：exit code、stdout、stderr 分节进 `ToolOutput`；非零 exit **不是** `ToolError`（模型要看到失败输出），只有 spawn 失败才是
- [x] 非交互：stdin 接 `/dev/null`；不申请 TTY；`TERM`/`NO_COLOR` 之类环境变量不猜（v1 不做环境净化，但要在 `docs/` 里写明）
- [x] 单次结果超限走既有落盘 + 预览指针（票 07 的路径），本票不新增截断机制
- [x] e2e：跑一条真命令断言 stdout 进结果；`rm -rf /` 被断路器拒；超时后子进程真的没了（`killpg` 可断言）；plan 模式下 `bash` 被拒（把票 15 的测试替身换成真工具）
- [x] 补 `docs/bash.md`（边界规则 + 超时/进程树 + 与权限门的关系），并回改票 15 的 e2e 注释与 `docs/plan-mode.md` 里「v1 没有 `bash`」那段

**明确不做**：进程级沙箱（spec §20：v1 升级路径是「只做 Linux 的 bubblewrap」，且不预做抽象）；PTY / 交互式程序；后台任务与作业控制。

## Comments

实现落点：`src/tools/bash.rs`（工具本体、argv、超时、进程组终止、结果格式）、`src/tools/tool.rs`（`BashLimits` + `ToolContext::bash`）、`src/tools/registry.rs`（`PendingCall::bash`）、`src/config.rs`（`bash_timeout_ms` / `max_bash_timeout_ms` 与两个构造器）、`src/agent.rs`（每次调用从 `SessionConfig` 造 `BashLimits`）、`src/permissions.rs`（`rm` 断路器看穿 shell 包装）、`src/tools/mod.rs`（注册与导出）。测试：`tests/bash_tool.rs`（8 例）、`tests/permission_gate.rs`（新增 1 例 × 11 组 argv）、`tests/plan_mode.rs`（替身换真工具）。文档：`docs/bash.md`（新增）、`docs/plan-mode.md`。

实现期把票面留白写实的几处：

1. **`CommandPrefix` 只匹配声明的 argv，不穿透 shell；穿透的只有 `rm` 断路器。** `command()` 返回 `["bash", "-lc", command]`，规则作者看到的正是这串。断路器为了不被一层包装绕过而做词法穿透（票面「把它接到真工具上」）：按 `;` / `&&` / `|` / 换行拆简单命令、去掉配对引号、对每条复用既有的 `rm` 目标判定。它是 **best-effort**：`sudo rm …`、`$HOME`、混淆拼写看不到——spec §12/§20 已经把断路器定位成「减少误伤，不是抵抗攻击」，这些边界写进了 `docs/bash.md`。
2. **超时是结果，不是 `ToolError`。** 票面只规定「非零 exit 不是 `ToolError`，只有 spawn 失败才是」，超时落在同一边：结果里写 `timed out after N ms; the process group was killed` 与信号，stdout/stderr 照常带上，回合照常 `Completed`。模型因此能看到超时前已经产生的输出。
3. **进程组 = `process_group(0)` + `libc::killpg`。** 新增 tokio 的 `process`/`io-util` 特性（连带新依赖 `signal-hook-registry`）与 `libc` 直接依赖。`ProcessGroup` 守卫在 `Drop` 时也 killpg，所以取消手势丢掉在飞 future 时同样不留下后台子进程（票 13 的既有机制，不新增接线）。stdout/stderr 用独立 reader task 收，避免管道写满与超时互相卡住。
4. **上限语义是「可以更小、不能更大」。** `timeout_ms` 缺省用 `SessionConfig::bash_timeout_ms`（默认 120s），给了就 clamp 到 `max_bash_timeout_ms`（默认 600s）；`0` 或非整数是参数错误而不是静默回退。两个值都不进 `config.toml`（与 `max_tool_result_tokens` 同规矩），只进 `SessionConfig` 的构造器，经 `BashLimits` 随每次调用传给工具。
5. **结果格式固定三节**：`exit code: <code | killed by signal N>`、`--- stdout ---`、`--- stderr ---`，常量导出供测试与文档共用。截断不在这里：`agent::emit_completed` 的既有「截断 → 落盘 + 指针」管线照旧生效，`tests/bash_tool.rs` 有一例断言超限结果确实落盘并带指针。
6. **非交互 = stdin `null` + 不申请 TTY + 环境原样继承**（不猜 `TERM` / `NO_COLOR`），`bash -lc` 会 source 登录文件。票面要求写进文档的边界都在 `docs/bash.md`。
7. **`bash` 保持 `delegable`**，执行者的表里也有它；`effect()` 恒 `Exclusive`，所以执行者的调用同样持工作区独占锁，plan/readonly 对父子一视同仁。
8. **票 15 的测试替身已替换**：`tests/plan_mode.rs` 删掉 `ShellStandin`，改用真 `bash` 断言拒绝；`docs/plan-mode.md` 的「v1 没有 `bash`」段改写并指向 `docs/bash.md`。

**两轴 review 抓到的真缺陷（已修）**：第一版超时只包住 `child.wait()`，超时后即 `disarm` 再去 await 两个 reader task。但「shell 退出」与「输出管道关闭」不是同一时刻——后台子进程继承管道，`bash -lc "sleep 300 &"` 的 shell 立刻退出而管道还开着，于是 `wait` 立刻返回、超时永不触发、调用挂在 reader 上直到那个子进程结束（子进程反而留在后台）。修法：用一个 `select!` 循环同时盯 shell 退出、stdout EOF、stderr EOF，`timeout_at` 的 deadline 约束**整个调用**；一到点就 `killpg`，再给 1s grace 收读者。新增 e2e `a_backgrounded_child_cannot_outlive_the_timeout` 钉住它（并在 `<10s` 内返回）。同时据 review 补了断路器的词法覆盖（`-o` 参数后的 `-c`、括号、`then`/`if`/`!` 等语法词），并把 `docs/bash.md` 的「看不到什么」列全（变量、`sudo`/`env`/`eval`/`xargs`/别名、命令替换、here-doc、落盘脚本）。另据 review 把 `spec()` 里写死的 120s/600s 改成「默认配置下」的措辞。review 提到的两点保留原样并说明理由：非法参数与 wait 失败仍记 `ToolError`（前者与其它工具一致，后者是无结果可报的基础设施失败，票面那句讲的是命令自身的 exit）；`process_group(0)` 语义上等价 `setsid` 的进程组部分（不发新 session），文档已按此措辞。
