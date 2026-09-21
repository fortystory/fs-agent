# 20: `bash` 工具

**What to build:** 一个 `bash(command, timeout?)` 内建工具，让模型能在会话 cwd 里跑命令——argv 不经 shell 拼接、有超时、超时连进程树一起终止、结果照常进事件流并受单次结果截断。

Blocked by: 04

Status: ready-for-agent

**为什么会有这张票**：spec §7 把 `bash` 列进 **v1 全部内建工具集**（读 / 写 / 编辑 / `bash` / `skill` / `repo_map` / `task`），但 19 张实现票里没有一张拥有它——票 03 与票 04 的注释都写着「`bash`（后续票）是它的主人」，票 09 也写着「`bash` / `task` 未做」，`task` 由票 11 补上，`bash` 漏了。票 15（硬 plan 模式）要断言「`bash` 被拒」时发现了这个缺口，用一个注入了 `Exclusive` 的测试工具代替，并留下本票。

**参考:** spec §7（工具 trait 与 dispatch）、§12（`CommandPrefix` 作用域与 `rm` 断路器）、§10（单次结果截断）、§20（v1 不做进程级沙箱）

- [ ] `spec()`：`bash(command: string, timeout_ms?: integer)`；`effect()` **恒为 `Exclusive`**（shell 能写任何东西，注册表按它取工作区独占锁）
- [ ] `command(args)` 返回 `["bash", "-lc", command]`（或 `sh -lc`）——`CommandPrefix` 作用域与 `rm` 断路器靠它看到 argv；**`rm` 打到 `/` / `~` 及其父目录一律 `Deny`**（票 04 已在门里实现，本票把它接到真工具上）
- [ ] **执行不经 shell 拼接**：argv 数组直接 `spawn`，命令文本作为一个元素交给 `bash -lc`；模型拼不出第二层注入
- [ ] **超时 + 进程树终止**：超时不是 drop future 就算——按进程组（`setsid` + `killpg`）终止，子进程不留在后台；默认超时与上限进 `SessionConfig`
- [ ] 结果约定：exit code、stdout、stderr 分节进 `ToolOutput`；非零 exit **不是** `ToolError`（模型要看到失败输出），只有 spawn 失败才是
- [ ] 非交互：stdin 接 `/dev/null`；不申请 TTY；`TERM`/`NO_COLOR` 之类环境变量不猜（v1 不做环境净化，但要在 `docs/` 里写明）
- [ ] 单次结果超限走既有落盘 + 预览指针（票 07 的路径），本票不新增截断机制
- [ ] e2e：跑一条真命令断言 stdout 进结果；`rm -rf /` 被断路器拒；超时后子进程真的没了（`killpg` 可断言）；plan 模式下 `bash` 被拒（把票 15 的测试替身换成真工具）
- [ ] 补 `docs/bash.md`（边界规则 + 超时/进程树 + 与权限门的关系），并回改票 15 的 e2e 注释与 `docs/plan-mode.md` 里「v1 没有 `bash`」那段

**明确不做**：进程级沙箱（spec §20：v1 升级路径是「只做 Linux 的 bubblewrap」，且不预做抽象）；PTY / 交互式程序；后台任务与作业控制。
