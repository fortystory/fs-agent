# 种子材料：退出手势与 session id 回执

> **已折成 [`spec.md`](spec.md)**（2026-10-01 一轮 `/grill-with-docs`）。本文件留作意向的来源；
> **构建计划以 spec 为准**，其中两处前提（空闲态 `Ctrl-C` 已是一下就退、`-c` 不需要 session id）
> 在折的过程中被纠正。
>
> **这不是 spec，也不是票。** 它是 2026-10-01 在一轮 `/ask-matt` 里记下的意向：连按两下
> `Ctrl-C` / `Ctrl-D` 就退出，退出时打印 session id 好让下次 `--continue` 直接用。
> 还没被访谈、也没有票；想推进时走 `/grill-with-docs` 把它折成 `spec.md`，再 `/to-tickets` 拆票。
> **写就于 2026-10-01**；下面《现状》一节核实于同一天。

## 它要什么

- 连按两下 `Ctrl-C`（或 `Ctrl-D`）就退出，不用再走一次确认。
- 退出时在终端留下 session id，下次直接 `fs-agent --continue`（`-c`）。

## 现状

- **运行中已经有双击语义**：第一次 `Ctrl-C` 只是把手势举起来（`signal.cancel()`），已举手时再按一次才 `std::process::exit(130)`（`src/cli.rs:1287`）。
- **空闲/重放态没有**：`Ctrl-C` 走的是 `Pending::Exit` 那个确认覆盖层，还要再按一次 Enter 才 `quit`（`src/render/tui.rs:2146`、`src/render/tui.rs:1832`）；`Ctrl-D` 被**刻意忽略**（`src/render/wording.rs:1061`）。
- **退出时不打印 session id**：id 只落在会话桶里（`src/session/`），`--continue` 已经是 `-c`（`src/cli.rs:156`）。

## 待谈的分叉

1. 「连按两下」有没有计时窗口，还是只要举手就一直算（运行中现在就是后者）？
2. `Ctrl-D` 与 `Ctrl-C` 完全等价，还是只在空闲态算数（运行态的 `Ctrl-D` 现在是特意忽略的）？
3. session id 打在哪、打几次 —— 退出后一行？启动时也打？`--plain` 与 headless 要不要？
4. 退出确认覆盖层是删掉，还是留给「有未提交的草稿 / 正在跑」这类真正需要确认的场景？
