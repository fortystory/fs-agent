# 14 — 收口：全绿 + 索引核对 + 人工验收

Type: implement
Status: done
Part of: ../map.md
Blocked by: 07, 08, 13

> 规格：[`../spec.md`](../spec.md) 的「补记」（落地清单第 7 步与验收一节）。这一票不写新内容，
> 只把整件事验完并把四处索引对齐。

## 目标

`docs/lifecycle.md` 完整可读、护栏全绿、三处索引一致，且**五张图在 GitHub 上真的渲染出来**（这一条
只能人看）。做完这一票，这个 effort 从 tracker 的角度就算走完了。

## 现状（2026-10-03 核实，改前先复核）

- 票 06–13 落地后，`docs/lifecycle.md` 应当含：开头一段 + §1 约定 + 五张图 + §7 + 附录证据表。
- **三处索引**：`README.md` 的「架构」一节、`README.md` 的 `## 文档` 表、`.scratch/README.md` 的
  feature 行。前三处在本轮都动过，要逐处核对。
- **两条命令**：`python3 scripts/lifecycle-check.py`（票 08）与 `python3 -m unittest`（票 08）；
  另外 `python3 scripts/check-language.py` 与 `cargo test` 也不能因为这一轮变红。
- **没有 CI、没有 Makefile**：验收是人手工跑的，命令写在 `README.md` 的「开发」一节。

## 落点

`README.md`、`.scratch/README.md`、`docs/lifecycle.md`（只在核对时修错别字级的偏差）。

## 具体行为

1. **渲染验收（人做）**：五张图在 GitHub 上渲染出来；长图没有被截断得读不了；`sequenceDiagram` 的
   中文标签没有挤在一起。这一条**没有自动化**，做完在票底记一句结果。
2. **护栏全绿**：`python3 scripts/lifecycle-check.py` 退出 0；`python3 -m unittest` 全绿；
   `python3 scripts/check-language.py` 全绿；`cargo test` 仍绿（这一轮不该碰 `src/`）。
3. **手工破坏两次**：改一个节点 id、删一行证据 → 脚本必须退出 1 并指出是那一项；改回来。
4. **索引核对**：`README.md` 两处都指向 `docs/lifecycle.md`；`.scratch/README.md` 的 feature 行
   写成本轮的实际结果（形态、票数、完成度）。
5. **§1 的判据与脚本的行为对得上**：脚本认的方言与文档 §1 写的是同一套（这是最容易漂的一处 ——
   文档说「不许 subgraph」而脚本却不认，或者反过来）。
6. 顺手把 `.scratch/lifecycle-diagram/map.md` 的进度段记一句「已交棒、实现票落地情况」。

## 验证

1. 上面六条逐条有结论（渲染那条由人写在票底）。
2. `git status` 只看得到本轮该动的文件；没有误改 `src/`。
3. 用 `sessions replay` 那类**不适用**——本 feature 不改运行时行为，所以**没有事件流层面的验收**；
   这一点要在票底写明，免得下一个人去找。

## 不做什么

- 不在收口票里改运行时行为。
- 不重新论证任何决定（有异议就改对应的票或另开一条 bug）。
- 不把 `research/` 里的材料搬进 `docs/`（那是材料，不是结论）。
- 不为了让脚本通过而放松脚本（C8 那条尤其：宁可改文档）。

## 结果（2026-10-03）

本票因此标 **`ready-for-walkthrough`**（不是 `done`）：能自动化的部分都跑过了，剩下的只有
「在 GitHub 上把五张图看一眼」。

## 验收（2026-10-07）

维护者在 GitHub 上看过五张图：都渲染出来了、长图没有被截断得读不了、`sequenceDiagram` 的中文
标签没有挤在一起。本票转 **`done`** —— 那是最后一项，本 effort 至此 14/14（5 resolved + 9 done）。

**六条逐条有结论：**

1. **渲染验收 —— 自动跑了，且抓到一个真缺陷。** 开发机上有 `npx` 与 `/usr/bin/google-chrome-stable`，
   于是用 `@mermaid-js/mermaid-cli@11`（puppeteer 指向系统 Chrome、`--no-sandbox`、npm cache 与
   Chrome 缓存都落在 `target/` 下）把五张图各渲染了一次，SVG 与 PNG 都生成成功、中文标签清晰、
   长图没有被截断。**图 3 第一次渲染失败**：`participant loop` 撞上 `sequenceDiagram` 的保留字
   `loop`（`Parse error on line 8 … Expecting '+', '-', '()', 'ACTOR', got 'loop'`）——
   GitHub 上本来会显示成一个报错框。改名成 `core`（标签仍是「循环」）后五张全部通过。
   这条坑连同「图 3 护栏管不到」写进了 `docs/lifecycle.md` §1.1。
   **本机渲染不等于 GitHub 渲染**，那一眼仍留给人（图的源文件没变，只是 id 改了）。
2. **护栏全绿**：`python3 scripts/lifecycle-check.py`（74 节点 / 78 边，0 行号漂移）、
   `python3 -m unittest`（18 条）、`python3 scripts/check-language.py`、`cargo test`（全绿，未碰 `src/`）。
3. **手工破坏两次**：图里把 `aborted` 改成 `abortedx` → 退出 1，报出「图里有、表里没有」「表里有、
   图里没有」与「边端点没有显式定义」三条；删掉 `aborted` 那一行证据 → 退出 1，报出反向那条。
   两次都改回来了。
4. **索引核对**：`README.md` 的「架构」一节与 `## 文档` 表各指向 `docs/lifecycle.md` 一次
   （`grep -c` = 2）；`.scratch/README.md` 的 feature 行改成「5 resolved + 9 done」；
   本票与 06–13 的 `Status` 全部 `done`，`spec.md` 抬头改成 `9/9 done`；`map.md` 的 `## 进度`
   补了实现落地一段。
5. **§1 与脚本行为对照**：核了一遍，补正两处文档（不是改脚本）——① §1.1 原写「`flowchart <方向>`
   一行」，脚本其实也认等价的 `graph`，方向集合是 `TD/TB/BT/LR/RL`；② §1.1 补上「`sequenceDiagram`
   有自己的关键字，participant id 要避开」这条（第 1 点那个缺陷的由来）。其余七条校验、符号列
   写法、`impl` 不充当证据符号、虚线语义都对得上。
6. `map.md` 的进度段已记（见第 4 点）。

**没有事件流层面的验收**：本 feature 不改运行时行为（`src/` 一个字节没动，`git status` 可证），
所以不需要 `sessions replay` 那类核对 —— 下一个人不必去找。

**随行的一处既有缺口（未动）**：`scripts/check-language.py` 的 `DOCS_MIN_RATIO` 里没有
`docs/adr/0008-*.md` 与 `docs/adr/0010-*.md`（实测 39.9% / 50.3%，都远高于邻居下限）。
本轮只按惯例加了 0011，没有顺手补这两条 —— 那属于另一条线，留在这里备查。
