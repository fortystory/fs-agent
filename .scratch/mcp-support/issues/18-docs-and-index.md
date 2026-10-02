# 18 — 文档与索引

Type: implement
Status: ready-for-agent
Part of: ../map.md
Blocked by: 10, 11, 12, 13, 14, 15, 16, 17

> 规格：[`../spec.md`](../spec.md) §10。这一票不写代码，只把已经落地的四个元工具写进逐面文档与
> 三个索引。

## 目标

`docs/mcp.md` 落地并挂进 README 的文档表；架构的边界数字跟着改；词表收下这一层的名词。

## 现状（2026-10-03 核实，改前先复核）

- 逐面文档的先例是 `docs/repo-map.md` / `docs/web.md`：一个面一份，讲它怎么工作与为什么。
- README 的「文档」一节是唯一索引；架构一节的顶层边界数当前是 **14**（加 `src/mcp/` 之后是 15）。
- `.scratch/README.md` 的 feature 表里 `mcp-support` 那一行现在写着决策图的进度。
- `CONTEXT.md` 只收领域词汇与流程词汇（格式：中文名 + 英文标识符 + `_Avoid_`）。

## 落点

`docs/mcp.md`（新）、`README.md`、`.scratch/README.md`、`CONTEXT.md`。

## 具体行为

1. **`docs/mcp.md`**：三层结构（工具 / 服务 / 连接）与各自的职责；四个元工具的形状与参数面；
   `[mcp]` 与 `.mcp.json` 的字段（含优先级）；沙箱与环境白名单做了什么；**信任三个位**的含义；
   以及两条边界 —— **server 的工具不进表**、**模板由人发起**。
2. **`README.md`**：文档表加一行；架构一节的边界数 14 → 15。
3. **`.scratch/README.md`**：`mcp-support` 那行的形态与票数改成实际结果。
4. **`CONTEXT.md`**：加**元工具**、**原语**（tool / resource / prompt / elicitation）、**server**
   三个词条（措辞照邻居）。

## 验证

1. `python3 scripts/check-language.py` 通过（`docs/mcp.md` 与 `CONTEXT.md` 的新词条都在它的检查面里）。
2. README 的文档表链接可达；架构一节的边界数与 `src/lib.rs` 的清单一致。
3. **文档与实现逐条对一遍**：字段名、错误 code、上限、四个工具的参数面 —— 这四处最容易写成
   「应该如此」而不是「就是如此」。

## 不做什么

- 不重写 `docs/` 里其它任何一份文档。
- 不在这一票里改代码（发现不一致时改文档或另开一条 bug）。
- 不把 `.scratch/mcp-support/research/` 的两份调研搬进 `docs/`（那是材料，不是结论）。
