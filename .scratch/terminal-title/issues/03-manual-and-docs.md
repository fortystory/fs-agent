# 验收面收口：手工清单新一节与随之更新的文档

Type: implement
Status: ready-for-human
Blocked by: 02

> 规格：`.scratch/terminal-title/spec.md` §4 末（不支持 push/pop 的终端如实记一句）、§Testing Decisions 的「手工」一条（xterm / tmux / 一个 VTE 系各看一次；tmux 的 `allow-passthrough` 开与关）。
> 票 01 与票 02 把能自动化的都钉住了（纯函数、状态机、pty 序列）。这一票做它**测不到的那一半**：标题在真终端里看起来对不对、退不退出得回来。

## 目标

- [`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md) 新增一节，覆盖三种终端与 tmux 的 `allow-passthrough` 开 / 关。
- 把随实现需要对齐的文档一起收口（那份清单的「来源」段、[`docs/render.md`](../../../docs/render.md) 的 TUI 一节）。

## 现状

- [`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md) 现有 ①–㉒；⑦（`:107-130`）是「退出后终端干净」，㉒（`:509-532`）是上一轮的目标循环。抬头的「来源」段（`:8-13`）逐轮列各 spec，这一轮的 `.scratch/terminal-title/spec.md` **还没进那一列**，也还没有一节。
- 规格明说这一条不自动化：「**不测**：终端是否真的把标题显示出来 —— 那是终端的行为，归手工清单」（spec §Testing Decisions 末条）。pty 脚本只证明**我们发出了哪些序列**（票 02），看不到终端把它们变成了什么。
- 「不支持 push/pop 的终端」的退化是**接受**的：两个序列是 no-op，标题会停在我们写的那条上；spec §4 末要求文档里如实记一句，不要加 fallback（补发「清空标题」在不支持 push/pop 的终端上会把用户原本的标题抹掉）。
- [`docs/render.md`](../../../docs/render.md) 的 TUI 那一节（`:15` 的表行、`:160-215`）讲的是渲染器边界、状态机与历史重播，没有一句说标题是 TUI 的副作用；`:214` 指向那份手工清单。

## 具体行为

1. **清单新增 `## ㉓ 终端标题：内容、更新与还原`**，放在文件末尾、㉒ 之后，照现有各节的形状写：一小段这一轮在验什么，然后是「前置 → 怎么做 → 该看见什么，看不见就是回归」的编号项。写清前置：`cargo build`，`cd` 进一个基名可辨识的目录，跑 `./target/debug/fs-agent`（TUI），要验状态变化的那几项得能触发一次权限询问 / 让一回合跑起来。
2. **三种终端各看一次**：`xterm`、`tmux`（窗格里的 TUI）、一个 VTE 系（`gnome-terminal` / `Tilix` / `xfce4-terminal` 任一）。每一种都看这四件事：
   - **内容**：路径段是 cwd 的形态（`$HOME` 之下显示成 `~/…`）、空闲时**没有**状态词、`--cwd` 指到别处时路径跟着走；
   - **更新**：跑一回合时变 `运行中`，出现权限询问 / 问卷时变 `等你`，重放（`--continue`）期间是 `重放中`；`/loop <名字>` 跑着时目标名跟在后面；
   - **还原**：退出（空闲 `Ctrl-C`、`/quit` 各一次）之后标题回到进入前那条；`--continue` 退出也一样；
   - **不闪**：一次长回合里标题不随增量文本翻动（它只在状态 / 目标变时写一次）。
3. **tmux 的 `allow-passthrough` 开与关各看一次**：`tmux set -g allow-passthrough on` 与 `off` 各走一遍第 2 条；两种结果都如实记（开 / 关各自是「更新 / 还原」还是没动、还是别的情况），不要只记能过的那一种。
4. **不支持的终端如实记一句**（spec §4 末）：某个 VTE 系终端上如果保存 / 还原是 no-op、退出后标题停在我们写的那条，就在本节里点名记下来，并写明这是**接受**的退化、不加 fallback、也不清空标题。
5. **对齐那两份文档**：
   - 把 `.scratch/terminal-title/spec.md` 加进 [`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md) 抬头「来源」段（`:8-13`）那一串里；
   - 在 [`docs/render.md`](../../../docs/render.md) 的 TUI 一节（`:160-215` 里合适的位置，挨着「`TuiState` 是可测的那一半」那段）补一句：标题是 TUI 渲染器的副作用 —— 进 alt screen 时发 `\x1b[22;0t` 保存、按状态写 OSC 0、退出（含 panic 路径）发 `\x1b[23;0t` 还原，它不进事件流、不落 `log.jsonl`。

## 验收

- 「标题内容」「状态变化时更新」「退出还原」三组，每组都在 xterm、tmux、一个 VTE 系里**各做过一次**，结果**逐项**写进这一票的 Comments（绿写成看见什么，没还原 / 没更新写成如实描述 + 终端名 + 版本）。
- tmux 的 `allow-passthrough` 两种设置各有一次记录。
- 不支持 push/pop 的终端如果出现，有一句点名 + 「接受这个退化」的注记（spec §4 末）。
- 抬头「来源」里出现 `.scratch/terminal-title/spec.md`；`docs/render.md` 里出现那一句标题副作用。
- 跑一遍 `cargo test` 与 `python3 scripts/tui-startup-check.py` 确认这一票没误伤（本票不该改代码）。

## 不做什么

- **不改代码、不改 spec、不改 seed**：这一票只动文档。
- 不加配置项、不做标题模板语法、不给不支持 push/pop 的终端加 fallback（spec §4 末、§明确不做）。
- 不包 tmux passthrough 序列（`\x1bPtmux;…`）—— tmux 对普通 OSC 有自己的那套，不猜用户配置（spec §明确不做）。
- 不把「终端是否真的把标题显示出来」写成自动检查，也不把 40 列封顶 / `~` 缩写搬到这里（那是票 01 的 `tests/wording.rs`）。
- 不改状态行 / 提示行里已有的任何文本（spec §明确不做）。

## Comments

- **已落地（文档）**：
  - `docs/tui-manual-checklist.md` 末尾新增「## ㉓ 终端标题：内容、更新与还原」，照 ㉒ 的形状写：一段这一轮在验什么 + 前置 + 七条（内容 / 更新 / 不闪 / 还原 / 三种终端各一遍 / tmux `allow-passthrough` 开与关 / 不支持的终端如实记一句）。
  - 抬头的「来源」段补上 `.scratch/terminal-title/spec.md`（§1–§6、§Testing Decisions → ㉓）。
  - `docs/render.md` 的「键盘」一节末尾（挨着「`TuiState` 是可测的那一半」那段）补了一段：标题是 TUI 渲染器的副作用 —— `CSI 22 t` 保存、`OSC 0` 写入、绘制路径上比对、`CSI 23 t` 还原，不进事件流、`--plain` 与 headless 不发序列，以及不支持 push/pop 的终端上那个被接受的退化。
- **已完成的两条自动腿**（本票不改代码，这里只记复核结果）：`cargo test` 全绿（959 条）；`python3 scripts/tui-startup-check.py` 12/12 GREEN —— 四类出口各三轮，每条都验到 `CSI 22 t`、含 cwd 基名的 `OSC 0` 与 `CSI 23 t`（本机跑时用 `FS_AGENT_MODEL=kimi-for-coding`，因为默认模型那个 provider 在这台机器上没有 key）。
- **未做（这一票剩下的全部）**：第 1–7 条的真机验收 —— 在有 xterm / tmux / VTE 终端的机器上逐项走过，并把结果写进这一票。执行这次落地的环境里**没有真终端**（只有 pty，`scripts/tui-startup-check.py` 能跑，但没有人眼能看的那一格窗口标题），所以「标题看起来对不对、退不退出得回来」一次都没有被观察过。这也是这一票标成 `ready-for-human` 而不是 `done` 的原因：剩下的活只能由人在真终端上做，逐项记录照「验收」那一节的要求写。
