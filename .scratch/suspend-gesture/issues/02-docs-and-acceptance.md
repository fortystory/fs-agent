# 文档与验收收口：挂起写进逐面文档、手工清单与索引

Type: implement
Status: done
Blocked by: 01

> 规格：[`.scratch/suspend-gesture/spec.md`](../spec.md) §6（子进程与 deadline）与「测试决定」里的文档那几条。
> 本票**只写文档与索引**，不碰 `src/` 与 `scripts/`。[票 01](01-suspend-gesture-and-handover.md) 落地之前不开工 —— 文档要写的是它的实际行为。

## 目标

- `docs/render.md` 新开一节「挂起与恢复」：TUI 的交还/重进顺序（spec §3、§4）、plain 为什么不进 raw 模式所以天然有效、以及 §6 的「先 Ctrl-C 再 Ctrl-Z」。
- `docs/tui-manual-checklist.md` 加**两条**真机走查：① 挂起 → shell 里跑两条命令 → `fg` → 画面完整无残影、标题正确、转录滚动位置没丢；② 忙碌态挂起，回来看到超时/断流走正常错误呈现。
- `.scratch/README.md` 的 feature 索引加 `suspend-gesture` 一行，并把状态段的收尾补记写进去（同一天 spec、票与实现落地）。

## 现状（开工前先复核）

- `docs/render.md` 的节：`## 一个 trait，三个实现` / `## 一个呈现层，两个画家` / `## 严重度` / `## 高亮` / `## 外壳` / `## 键盘` / `## 重新打开会话：历史重播` / `## 交互式 CLI`。新节放在 `## 键盘` 与 `## 重新打开会话` 之间，或紧接 `## 键盘` 之后 —— 与「键盘手势」同族。
- 手工清单的编号：⑱ 是 sandbox、⑲ 是 `workspace` 档（`.scratch/README.md` 的表格里记着）；新条目接在最后。
- `.scratch/README.md`：表格一行一个 feature（`目录 | 形态 | 一句话 | 票`），第 11 行那一大段是状态补记，按日期往后接。

## 落点

- `docs/render.md`（新节）
- `docs/tui-manual-checklist.md`（两条）
- `.scratch/README.md`（索引一行 + 状态段一句）

## 具体行为

### 1. `docs/render.md`

一节写完四件事：挂起的手势（单下、任何视图都拦不住）、交还与恢复的顺序（先交还再发信号、恢复时用底层原语而非 `ratatui::init()`）、plain 的天然行为与它为什么天然、以及「想连子进程一起停就先 Ctrl-C 再 Ctrl-Z」。用词表里的**挂起**，别写「暂停」「休眠」。**现状文档只讲现在什么样**：不搬 spec 的理由与替代方案，也不写「我们曾考虑过」。

### 2. `docs/tui-manual-checklist.md`

两条按现有条目的写法（编号、一句判断标准、怎么做）。第二条要能在一眼内看出「回合以它自己的错误收尾」与「界面卡死」的区别。

### 3. `.scratch/README.md`

- 表格加一行：`suspend-gesture/`｜`spec`｜一句话（Ctrl-Z 真暂停到后台、`fg` 回来重绘；plain 的天然行为有回归守着）｜`2/2 done`（视落地情况写实数）。
- 第 11 行那段状态补记里接一句，格式照邻居：日期、这次走了什么流程、落了什么。

## 测试

- 没有代码测试。验收靠：`python3 scripts/check-language.py` 通过（新增散文是中文）、`docs/render.md` 的节标题与词表用词一致、手工清单两条能被人照着走一遍。
- 若本票在执行时发现 [票 01](01-suspend-gesture-and-handover.md) 的行为与 spec 有出入，**改文档去对齐代码**（逐面文档讲现状），并把出入记进 Comments 交回。

## 不做什么

- 不碰 `src/` 与 `scripts/`（票 01 的地盘）。
- 不碰 `CONTEXT.md`（**挂起**词条已在 spec 落盘时写好）。
- 不新增 ADR：spec 的「明确不做」已经记下取舍，它够不上本仓库 ADR 的三条门槛（难回头 / 没上下文会奇怪 / 真有替代方案被否 —— 只中两条半）。
- 不改 `docs/bash.md`：进程组与超时的现状没变，变的只是「挂起时它们照走」，那属于挂起这一节的话。

## 评论

- **逐面文档**：`docs/render.md` 新开「挂起与恢复」一节（放在「键盘」与「重新打开会话」之间）：手势、**先交还再停**的顺序、`--plain` 为什么不参与、以及「停着的那段时间世界照常走 / 想连子进程一起停就先 `Ctrl-C` 再 `Ctrl-Z`」。逐面文档只讲现在什么样，理由与替代方案留在 spec §3–§6。
- **手工清单**：`docs/tui-manual-checklist.md` 加 ㉔（空闲挂起、忙碌挂起、挂久了会超时、窗口尺寸变过、plain 也照旧、三种终端各走一遍），把这条需求剩下的「真终端上逐项看」那半收进去。所以这一轮**没有** `ready-for-walkthrough` 的票：能自动化的部分已经跑进 pty 脚本（TUI 与 plain 各一轮），剩下的观感本来就住在这份清单上。
- **索引**：`.scratch/README.md` 加 `suspend-gesture` 一行（`2/2 done`）并在状态补记里接了一段；根 `README.md` 的状态行同步（`src/` **36,423** 行、`tests/` **34,767** 行、**976** 条测试，并补记这次落地）。
- **`CONTEXT.md` 未动**：「挂起」词条已在 spec 落盘那一刻写好（票面写明本票不碰它）。
- **没有新 ADR**：spec 的「明确不做」已经记下那段取舍，够不上三条门槛（难回头 / 没上下文会奇怪 / 真有替代方案被否 —— 只中两条半）。
- **一处返工**：新写的手工清单那节把 `docs/tui-manual-checklist.md` 的中文占比压到下限以下（45.9% < 46%），`check-language.py` 当场变红；补了一段中文说明（顺便把「这一节真正要看的是交还与停止的先后」讲清）之后复跑 OK。
- **验收**：`python3 scripts/check-language.py` OK。
