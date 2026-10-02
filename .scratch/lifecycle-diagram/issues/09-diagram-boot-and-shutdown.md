# 09 — 图 2：进程启动与收尾

Type: implement
Status: done
Part of: ../map.md
Blocked by: 06

> 规格：[`../spec.md`](../spec.md) §2（图 2）。图源是
> [`../prototype/01-drafts.md`](../prototype/01-drafts.md) 的第二张，证据在
> [`../research/04-node-evidence.md`](../research/04-node-evidence.md) §2。

## 目标

`docs/lifecycle.md` 的「进程启动与收尾」一节有图 2（`flowchart TD`，24 节点 / 23 边）与它的节点表、
边表。读者能回答：从进程被敲下到会话骨架建好，中间**依次**发生了什么；以及三条退出路径各从哪里
出去。

## 现状（2026-10-03 核实，改前先复核）

- **图已定形且改过两处**：① `tools → open → probe`（原稿把探测画在 `open` 之前是**错的** —— 组装期
  只有 `workspace` 档那一条检查，真正的探测在 `open` 之后的 `session()` 里惰性做）；② `panic` 删掉了
  入边，改成**无入边的 TUI 专属节点**（钩子只在 `TerminalModes::enter` 装一次，plain / headless 不装）。
- **取证时发现图源自评存疑的一条其实不成立**：`--discuss` 那条独立组装路径走的是另一个入口，不在
  本图里（它在图 1 与图 4 里）。
- **已有的类似 ASCII 图**：没有一张覆盖这条线；
  [`docs/credentials.md:22-29`](../../../docs/credentials.md) 只画入流那一段，可在「细则在哪」里引用。

## 落点

`docs/lifecycle.md` 的「进程启动与收尾」一节与附录里对应的一段。

## 具体行为

1. **图 2** 照图源落地。`refuse` 是**有意的断头路**（root 直接拒、无旗标可绕），不要给它补边。
2. **证据表**：24 个节点的节点表 + 边表，内容取 `research/04` §2；每条带 `路径:行号` 与符号。
3. **「细则在哪」**指向：`docs/permissions.md`（模式）、`docs/sandbox.md`（探测与拒绝）、
   `docs/credentials.md`（打码器）、`docs/observability.md`（会话与事件流）。
4. 正文里写明这条读者最容易问的事：**沙箱探测的时机**（`workspace` 档在组装期拒；其余惰性探一次）
   与**panic 节点的特殊性**（TUI 专属、无固定入边）。

## 验证

1. 图 2 在 GitHub 上渲染出来；节点数与图源一致（24）。
2. 每个节点在证据表里有且只有一行；每条边的两端都能在节点表里找到。
3. `python3 scripts/lifecycle-check.py`（票 08 之后）对这一节仍退出 0。
4. 正文没有把 `probe` 写回 `open` 之前。

## 不做什么

- 不动图 1 与其余三张图。
- 不写 §7（归票 13）。
- 不在这一节里展开 TUI 的绘制细节（那是 `docs/render.md` 的事）。
