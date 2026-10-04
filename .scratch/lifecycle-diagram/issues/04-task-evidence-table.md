# task：逐节点证据表

Type: task
Status: resolved
Part of: ../map.md
Blocked by: 01

## 问题

冻结项 12 要的「节点/边 → `文件:行号`」证据表，是这份 spec 的核心素材：`/to-spec` 之后写文档的人
靠它保证每个节点都能落到代码，`lifecycle-check.py` 靠它做校验。

这是取证活、不是 decision —— 所以是 task。它被票 01 阻塞：要先有定形的节点清单，才知道给谁取证。

要做：

- 以票 01 定形的五张图为准，**逐节点、逐边**给出 `文件:行号`，优先复用
  [`research/01-runtime-lifecycle-facts.md`](../research/01-runtime-lifecycle-facts.md) 已有的证据，
  不重复考古。
- 对 `research/01` 标「存疑」而图上又画到了的地方，**回代码复核**并记下新证据（尤其
  `src/render/tui.rs`、`tools/edit.rs`、`tools/file.rs` 那几处没逐行读的）。
- `research/01` §5 的 12 条不变量各要有一个「在图上对应哪条边」的映射 —— 这张表是 Q3(b)
  「变更影响分析」那半个用途的落点。

## 交付

`research/04-node-evidence.md`：一张大表（节点/边 → `路径:行号` → 一句话） + 一节「图上画了但
代码里没找到」的诚实清单。

## 作答

（2026-10-03）交付物：[`research/04-node-evidence.md`](../research/04-node-evidence.md)，343 行 ——
五张图**逐节点**表（图 1 的 14 个 · 图 2 的 24 个 · 图 3 的 5 个参与者 · 图 4 的 18 个 · 图 5 的 19 个，
每个带 `路径:行号` + 符号）、每张图的**边**表、`research/01` §5 那 **12 条不变量的边映射**、
`research/01` 五条存疑的复核、以及一节 **§8 诚实清单**。

**推翻侦察报告的一处结论**（这是本票最值钱的产出）：`research/01` 说「`cli.rs` 从不构造 headless」
**不成立** —— `probe` 子命令经 `probe_model` 在 `src/cli.rs:2238` 真的构造 `Renderer::headless`，
而那段在 `#[cfg(test)]`（`:3312` 起）**之外**。所以图 5 的 `headless` 不再是虚线。

**其余四条存疑的复核**：hook 在交互式路径恒为 `None` **成立**（三个生产组装点 `cli.rs:396` / `:715` /
`:2232`，测试在 `tests/hook_mount_points.rs:297-298` 注入 `Some`）；`docs/web.md` 与 `dsh web`
**无关联**（grep 无结果）；`context/` + `config/` 是子目录、边界清单在 `src/lib.rs:28-42` **成立**；
`tui.rs` 等仍未逐行读，但五张图都没画到那些路径，**不阻塞**。

**图已按诚实清单修正**（[`prototype/01-drafts.md`](../prototype/01-drafts.md) 现在是 v2）：九处改动 ——
图 2 的探测顺序（`tools → open → probe`）、图 2 的 panic 边（删，改成无入边的 TUI 专属节点）、
图 1 的渲染器二选一、图 1 的 `maint` 各自退出、图 5 的 `roll` 节点（删，改成边标签）、图 3 的
`gate` 标注、图 3 的消息方向（`loop->>gate` + `gate-->>loop`）、图 5 的 headless 实线、图 4 的三条
并列派生。改完重跑自检：四张 `flowchart` 仍通过「id 唯一」与「先定义后引用」，虚线只剩图 4 的
`pre -.-> hooks`。

**不是图的错、是检查器的错的那一条**（§8·8）：五张图的节点 id 跨图撞名（`cmd` / `loop` / `ok` /
`gate` / `red` / `goal`），会让票 03 设计的 C4/C5「有且仅有一行」判不了。裁决是**不让图去迁就检查器**
—— **C4/C5 按图分别比对**（每张图自己的节点集合 == 它自己那一节证据表的键集合），逼全局唯一只会长出
`g1_boot` 这种噪音前缀。这条已作为**第三处修订**追加到[票 03](03-research-lifecycle-check.md)。

**留给票 05 的**：证据表可以直接当 `docs/lifecycle.md` 的图旁表；`research/01` §5 那 12 条不变量的
映射则是「变更影响分析」那半个用途的落点。
