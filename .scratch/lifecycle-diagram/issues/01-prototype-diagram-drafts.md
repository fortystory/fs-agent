# prototype：五张图的草稿 mermaid

Type: prototype
Status: resolved
Part of: ../map.md
Blocked by: —

## Question

在拍板「`docs/lifecycle.md` 到底长什么样」之前，先把五张图**粗糙地画出来** —— 不追求准确、不
追求好看，只要具体到能指着说「这张不对 / 这个节点不该在这儿」。prototype 的产物是 discussion
fidelity，不是最终文档。

要画的五张（冻结项 5、8、10）：

1. **鸟瞰总图**（`flowchart`）：敲下命令 → 启动与组装 → 主循环 → 收尾；`probe` / `prune` /
   `sessions` 各占一个节点；`--discuss` 作为第二条生命周期从主循环旁边岔出去。
2. **进程启动与收尾**（`flowchart`）：`main` → root 拒绝 → runtime → 子命令分派 → 配置与凭据 →
   provider 装配 → 工具表组装 → 沙箱探测 → `assemble` → 会话骨架或恢复 → 横幅与菜单；右端是三条
   退出路径（正常 / `Ctrl-C` 双击 / panic 钩子）与收尾回执。
3. **一次 turn**（`sequenceDiagram`）：参与者是用户 / harness 循环 / provider / 权限门 / 工具 →
   上下文组装（`project` + `trim`）→ 请求 → 流式增量（不进流）→ 工具调用逐条分发（固定顺序
   `hook.pre → 门 → [询问] → dispatch → hook.post → 追加`）→ 回边 → `TurnEnded`。
4. **委派与嵌套**（`flowchart`）：`task` → 推迟批 → 执行者（深度为一、独立回合预算、`Allow`
   不传播）→ `ExecutorSpawned` / `ExecutorFinished` → 那一条唯一结果；旁边是 `--discuss` 的
   讨论者与合成器那条线。
5. **基础设施回路**（`flowchart`）：事件流（唯一写路径 `append_event`）↔ 投影 ↔ 裁剪 ↔ 持久化 ↔
   `replay` / `--continue`；goal 循环；两类问询发起者（模型 / harness）；渲染三后端与广播通道。

事实基线是 [`research/01-runtime-lifecycle-facts.md`](../research/01-runtime-lifecycle-facts.md)：
先读它，别重新考古。

## 交付

- 五段草稿 mermaid，落成 `prototype/01-drafts.md`（一个文件、五节），从本票链接出去。
- 每张图下面写**三条**「我知道这里可能不对」的自评 —— 这是这张票最值钱的部分。
- 标出每一处**虚线**（冻结项 7）。

## 不做什么

- 不写 `docs/lifecycle.md`（那是 `/to-spec` 之后的事）。
- 不求 mermaid 语法完美，但**要能贴进 GitHub 渲染出来**，别用编辑器私有的写法。
- 不逐条标注 `文件:行号`（那是票 04 的事）。

## 验收

- 五张图齐全，能在 GitHub 上渲染。
- 节点用 `CONTEXT.md` 的正式用词（`/domain-modeling`）。
- 自评三条写全，且至少有一条是「这张图可能是错的 / 多余的」。

## Answer

（2026-10-03，与维护者的 live exchange）五张草稿画完并自检通过：
[`prototype/01-drafts.md`](../prototype/01-drafts.md) —— 五张图、每张三条自评、末尾四处待拍板的冲突。

**定形的五张**

| 图 | 类型 | 节点 | 边 |
| --- | --- | --- | --- |
| 鸟瞰总图 | `flowchart TD` | 14 | 20 |
| 进程启动与收尾 | `flowchart TD` | 24 | 24 |
| 一次 turn | `sequenceDiagram` | — | — |
| 委派与嵌套 | `flowchart TD` | 18 | 17 |
| 基础设施回路 | `flowchart TD` | 19 | 20 |

**四处的裁决**（采用草稿里的推荐）

1. **虚线语法**：票 03 的方言白名单**扩一条** —— 认 `-.->`，把它当「虚线边」分类。
2. **`sequenceDiagram` 不进 C4/C5 对账**：只有四张 `flowchart` 参与「表与图的节点集合相等」那条
   检查；「一次 turn」靠人读。把解析器扩到 sequence 子集（参与者 / 消息 / 两种箭头）留作将来可选。
3. **并发不做画法约定**：图只表达「存在」，不表达并发；并发事实用边标签或文档里的一句话交代。
4. **虚线语义写死为「可注入但 CLI 不注入」+「尚未实现」**：异常路径（两下 `Ctrl-C`、panic 钩子）
   改用**边标签**。第一稿混用过这个语义，已改正。

**自检**：四张 `flowchart` 全部通过「id 唯一」与「先定义后引用」——用票 03 定的那两条规则**实测**
（不是目测）。**没有实测渲染**：仓库里没有 Node / `mmdc`，但语法是 mermaid 标准的。

**对票 03 的两处修订**（已在该票追加带日期的补记）：白名单加 `-.->`；sequence 图不进 C4/C5。

**留给后续票的**：五张图的节点清单已经定形 —— [逐节点证据表](04-task-evidence-table.md) 按它取证，
[文档骨架与 ADR 0011](05-grilling-doc-skeleton-and-adr.md) 按它排章。
