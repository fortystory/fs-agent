# 15: CLI 接线：斜杠命令只看第一行

**What to build:** 让斜杠命令的判定只看**第一行**，以便多行输入下 `/ask-matt` 后面能跟多行任务，而粘贴一段以 `/` 开头的多行代码不会被误判成 unknown command。单行行为逐字不变。

Blocked by: None

Status: ready-for-agent

**参考:** spec §12（CLI 接线与斜杠命令）、§6（`Ctrl-J` 换行带来的多行输入）

- [ ] 现在 `src/cli.rs:383` 是 `let command = line.trim(); match command { … other if other.starts_with('/') }`；改为取**第一行** trim
- [ ] 第一行是 `/name` 或 `/name task…` → 走命令分支，**其余行按 `\n` 拼进 task**（skill 名仍从第一个空白切开）
- [ ] 否则整体当 prompt —— 多行文本**不**因为以 `/` 开头就当命令
- [ ] 单行输入行为**逐字不变**（第一行 = 整行）：`/quit` / `/exit` / `/undo` / `/plan` / `/endplan` / `/<skill> [task]` 全部照旧
- [ ] `/ask-matt` + 多行 brief 能作为**一次** skill 任务跑起来（task 含换行）
- [ ] 用例：多行 `/skill` + task 拼装正确；多行以 `/` 开头的非命令文本走 prompt；单行各内建命令不回归

## Comments
