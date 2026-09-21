# 15: CLI 接线：斜杠命令只看第一行

**What to build:** 让斜杠命令的判定只看**第一行**，以便多行输入下 `/ask-matt` 后面能跟多行任务，而粘贴一段以 `/` 开头的多行代码不会被误判成 unknown command。单行行为逐字不变。

Blocked by: None

Status: ready-for-agent

**参考:** spec §12（CLI 接线与斜杠命令）、§6（`Ctrl-J` 换行带来的多行输入）

- [x] 现在 `src/cli.rs:383` 是 `let command = line.trim(); match command { … other if other.starts_with('/') }`；改为取**第一行** trim
- [x] 第一行是 `/name` 或 `/name task…` → 走命令分支，**其余行按 `\n` 拼进 task**（skill 名仍从第一个空白切开）
- [x] 否则整体当 prompt —— 多行文本**不**因为以 `/` 开头就当命令
- [x] 单行输入行为**逐字不变**（第一行 = 整行）：`/quit` / `/exit` / `/undo` / `/plan` / `/endplan` / `/<skill> [task]` 全部照旧
- [x] `/ask-matt` + 多行 brief 能作为**一次** skill 任务跑起来（task 含换行）
- [x] 用例：多行 `/skill` + task 拼装正确；多行以 `/` 开头的非命令文本走 prompt；单行各内建命令不回归

## Comments

## Comments

**实现完成（2026-09-21）**。落点：`src/cli.rs` —— 把原来那串 `match command { … other if other.starts_with('/') }` 换成**先分类、再执行**：新增私有的 `Submission` 枚举与纯函数 `submission(text, has_skill) -> Submission`，循环只负责按分类跑（`Quit` / `Undo` / `Plan` / `EndPlan` / `Unknown` / `Skill { name, task }` / `Prompt`）。文件末尾新增 `#[cfg(test)] mod tests`（4 个用例）。

**一处票面没写清、由本票定下来的规则**：票面说「取第一行，若它是 `/name` → 走命令分支，否则整体当 prompt」，但同时要求「单行行为逐字不变」—— 这两条在「单行 `/不认识的名字`」上冲突（老行为是给一句 unknown command，新规则会把它当 prompt）。定下来的是：

- **名字认识**（四个内建或已知技能）→ 命令分支，其余行拼进 task；
- **名字不认识且只有一行** → 还是 unknown command（单行逐字不变）；
- **名字不认识但后面还有行** → 整体当 prompt（粘一段以 `/` 开头的路径/代码不该被误判）。

这条把票面两个要求都满足了，并写进了 spec §12。原来的动机（`/ask-matt` + 多行 brief 可用；粘贴多行代码不误判）两条都在用例里钉住。

**测试面**：`submission()` 是纯函数，所以四条验收（单行各内建不回归、多行 + `/skill` 拼装、多行以 `/` 开头走 prompt、整体当 prompt）都打在它身上，不需要起 harness。**循环那一段只是机械改写**（四个内建臂逐个搬过去），行为等价；但**交互循环本身没有端到端测试**（现有 harness 不驱动它，pty 脚本也不敲任何字），这一点如实记着 —— 票 16 若要加，需要一个能敲字符的 pty 场景。

**基线**：`cargo test` **541 passed / 0 failed**（537 → +4 单元用例）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移；pty 启动检查 3/3 GREEN。

**变异检验**：改成按整段文本判定（而不是第一行）、不把它下面的行拼进 task、把「多行 + 不认识的 `/`」判成 unknown、第一行不 trim —— 四处全部被抓到。
