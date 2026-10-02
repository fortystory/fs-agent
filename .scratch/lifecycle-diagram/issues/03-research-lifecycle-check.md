# research：`lifecycle-check.py` 的可行设计

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## Question

冻结项 12 选了「图 + 证据表 + 一个弱校验脚本」。在 `/to-spec` 把这个脚本写进构建计划之前，要先把
**它能校验什么、会怎么误报**查清。

本仓库已有三个护栏脚本（[`scripts/check-language.py`](../../../scripts/check-language.py)、
[`scripts/wayfinder-check.py`](../../../scripts/wayfinder-check.py)、
[`scripts/tui-startup-check.py`](../../../scripts/tui-startup-check.py)），它们的写法 —— 报告格式、
退出码、对历史遗留的宽容处理 —— 是参照物。

要回答的：

1. 可校验项清单（每条给：校验什么 / 怎么实现 / 误报风险）—— 至少覆盖「证据表里的 `路径:行号` 仍
   存在」「点名的符号仍在」「`docs/lifecycle.md` 仍被 README 索引」「证据表键集合 == 图里的节点
   集合」「mermaid 块自检」。
2. 从 mermaid 文本里稳健提取节点 id 与边的方案，以及它**为什么做不到 100%**。
3. 宁松勿严还是宁严勿松，给理由。
4. 推荐的 v1 清单（进哪些、明确不做哪些）。

## 交付

设计报告由 subagent 写到 `research/03-lifecycle-check-design.md`；票底 `## Answer` 写结论 + 指向它。

## Answer

（2026-10-03）设计报告：[`research/02-lifecycle-check-design.md`](../research/02-lifecycle-check-design.md)
（282 行，逐条带 `文件:行号`）。结论如下。

**取舍一句话：对结构宁严勿松、对位置宁松勿严** —— C1–C8 全部计失败，只有 C9「行号精确」降级为
告警 + `--strict`。

**进 v1 的七条（全部计失败）**

| 项 | 校验什么 |
| --- | --- |
| C1 | 每条 `路径:行号` 的文件存在、行数够（区间形态 `路径:a-b` 一并认；全仓已有 837 处这种写法） |
| C2 | 点名的符号仍在该文件里 —— 证据表把符号**单独成列**，只支持 `fn` / `struct` / `enum` / `trait` / `const` / `static` / `mod` / `type` / `union`；`impl` 与 `macro_rules!` v1 不许当证据符号 |
| C3 | `docs/lifecycle.md` 仍被 `README.md` 引用（「文档」表或「架构」节任一命中即可 —— `AGENTS.md:23` 的「一处索引」是硬约定） |
| C4 / C5 | 证据表的节点键集合 **==** 图的节点集合（同一次比对的**两个方向**，分开打印） |
| C6 | 节点 id 不重复定义（复制粘贴事故探测器） |
| C7 | 每条边两端都已**显式**定义 |
| C8 | 图只用约定方言；**不认识的构造报红，不静默跳过** |

**C9 降级为告警 + `--strict`**：行号是结构性易漂量 —— 在 `src/events.rs` 顶部插一个 `use`，下面所有
证据行号全部 +1，而图与代码其实没脱节。把它计成失败，脚本第一天就变噪音机器，人会习惯性忽略它
（那比不检查更糟）；理由与 [`scripts/check-language.py:148-150`](../../../scripts/check-language.py)
留 2 个百分点的写法同源。漏报可接受：C2 已经守住真正重要的东西 —— **证据指向一个真实存在的符号**。

**三条硬前提**（写文档时必须照办，否则检查形同虚设）

1. **受约束的 mermaid 方言**：只允许 `flowchart <方向>`、节点定义 `id[文本]` / `id{文本}`、
   边 `id --> id` 与 `id -->|文本| id`、空行、`%%` 注释。**不许** subgraph、`&`、链式
   （`A --> B --> C`）、`classDef`/`class`/`style`/`linkStyle`/`click`、`[]`/`{}` 之外的形状。
   禁止项不是洁癖：每一条都是解析器的失效点（报告 §2 列了 15 种能骗过它的写法）。
2. **「先显式定义、后引用」写进文档**：mermaid 的 `A --> B` 会**隐式定义** B，所以 C7 只有在
   这条约定下才成立。节点 id 限 `[a-z][a-z0-9_]*`，避开 `end` / `graph` 这类关键字。
3. **节标题与表形状固定**：解析按固定二级标题取节；证据表分「节点表 / 边表」，键就是 mermaid 的 id。

**v1 明确不做**：语义正确性（图是否如实描述运行时 —— 工具做不到，docstring 里要写明「**通过 ≠
图是对的**」）、mermaid 渲染验证 / 接 `mmdc`（要 Node 生态，且渲染成功与图对不对无关）、接 CI、
扫描其它文档里的图。

**两条顺带查实的事实**

- **仓库没有 CI、也没有 Makefile**（`.github/workflows/`、`Makefile` 均不存在）：护栏脚本靠
  [`README.md`](../../../README.md) 的「开发」一节手工调用，**输出文本就是接口**，退出码是唯一机器
  信号 —— 所以 v1 用 `0` 通过 / `1` 有失败 / `2` 用法错，照
  [`scripts/wayfinder-check.py`](../../../scripts/wayfinder-check.py)。
- **`check-language.py` 有一处既有缺口**：`check_docs()` 只遍历 `DOCS_MIN_RATIO` 的键、**不扫
  `docs/*.md`**（`scripts/check-language.py:345-355`），新文档不登记就完全不受中文占比护栏。
  所以冻结项 13 的「加一行」是**必须**动作、不是可选项；两个脚本不联动（C10 明确不做，避免互相
  知道对方内部结构）。

报告另含：CLI 三个开关（缺省盯 `docs/lifecycle.md` / `--list` 打印解析结果 / `--strict`）、函数
划分、`PASS` / `FAIL` 输出样例（照 `scripts/wayfinder-check.py:146-158`）。

---

**2026-10-03 由[票 01](01-prototype-diagram-drafts.md) 修订两处**（原文不改写，照本仓库「原文不动、
加带日期补记」的先例）：

1. **方言白名单扩一条**：认 `-.->`，把它当「虚线边」分类。草图需要它表达冻结项 7 的那两类路径
   （「可注入但 CLI 不注入」与「尚未实现」），而它在 mermaid 里是稳定语法。
2. **`sequenceDiagram` 不进 C4/C5**：v1 的解析器仍只认 `flowchart`；「一次 turn」那张图靠人读，不参与
   「表与图的节点集合相等」那条检查。把解析器扩到 sequence 子集留作将来可选。
3. **C4/C5 按图分别比对，不要求跨图唯一**（同日由[票 04](04-task-evidence-table.md) §8·8 提出）：
   每张图自己的节点集合 == **它自己那一节**证据表的键集合。五张图之间撞名的 id 有
   `cmd` / `loop` / `ok` / `gate` / `red` / `goal` —— 原文那个「整个文档一个 `set[str]`、有且仅有一行」
   的比法会让它们必报假差异。让图的命名去迁就检查器（加 `g1_` 前缀）是本末倒置；**改检查器**。
