# 15: CLI 接线：斜杠命令只看第一行

**What to build:** 让斜杠命令的判定只看**第一行**，以便多行输入下 `/ask-matt` 后面能跟多行任务，而粘贴一段以 `/` 开头的多行代码不会被误判成 unknown command。单行行为逐字不变。

Blocked by: None

Status: done

**参考:** spec §12（CLI 接线与斜杠命令）、§6（`Ctrl-J` 换行带来的多行输入）

- [x] 现在 `src/cli.rs:383` 是 `let command = line.trim(); match command { … other if other.starts_with('/') }`；改为取**第一行** trim
- [x] 第一行是 `/name` 或 `/name task…` → 走命令分支，**其余行按 `\n` 拼进 task**（skill 名仍从第一个空白切开）。**内建命令（`/quit`/`/exit`/`/undo`/`/plan`/`/endplan`）不带 task**，只有独占整条输入时才算命令 —— 评审发现原实现会把 `/plan` 后面的行**静默丢掉**，已修
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

**评审收口**（`/code-review` 双轴，2026-09-21）：

- **我自己引入了一个真 bug**：把命令判定改成「只看第一行」之后，`/plan` 后面跟的行会被**静默丢掉**（旧代码会把它当成未知命令提示出来）。修法：**内建命令只有独占整条输入时才算命令**，否则落回「以 `/` 开头」的那套规则 —— `/plan` 不是技能名，于是整条输入当 prompt，用户至少能在转录里看见自己发出去的东西。原来 Comments 里「循环那段是机械改写、行为等价」的说法不实，已改。
- **技能 + 只有空白的后续行**（`/review
   
`）原本会跑一次空回合（违背「裸 `/<skill>` 只加载」）—— 判空改成看 `rest.trim()`；任务内容本身仍然**原样保留**（只丢尾部空行）。
- 票面清单第 2 条、spec §12 的「四个内建」都与新规则不符（spec 那句还列了五个命令）—— 都改了。`wording::unknown_command` 的内建清单本来就不列 `/exit`（它是 `/quit` 的别名），保持原样。
- **一处有意的偏离**：`submission()` 是私有纯函数，单元测试直接打它 —— spec 的 Testing Decisions 说「不断言私有函数」，那条是针对**渲染面**的（断言看得见的东西）；这里「一次提交是什么意思」本身就是这条接缝的全部契约，而驱动交互循环需要一整套 harness（现有测试不驱动它）。`#[cfg(test)]` 单元测试在 `src/tools/edit.rs`、`src/render/tui.rs` 已有先例。

**本轮变异检验**：内建命令改成不要求独占整条输入、把「只有空白的后续行」当成任务 —— 两处都被抓到。
