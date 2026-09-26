# 孤儿文档与约定落到 `AGENTS.md`

Type: implement
Status: done
Blocked by: 01, 02

> 规格：`.scratch/docs-tidy/spec.md`（Problem Statement 1/4/5/6、Solution 4）。

## 目标

没有入口指向的文档不再是孤儿；语言与位置的约定在**agent 也会读**的地方有一句指向。

## 落点

`AGENTS.md`、`docs/highlight.md`（只在必要时加一行）、可能还有 `docs/research/` 顶部的说明（同样只在必要时）。

## 具体行为

1. **`AGENTS.md`** 加一小节（或一行）：文档索引在 `README.md` 的《文档》一节，`.scratch/` 的 feature 索引在 `.scratch/README.md`，语言约定见同一节 —— agent 要写新文档时先照那两处。
2. **`docs/highlight.md`**：它讲的是「这个模块没有生产消费者、为什么留着」。在开头补一行**状态**（一句话：现在仍无消费者，最后复核日期），免得读者按文档里某句话以为它上线了。**不要**改写正文。
3. **`docs/research/`**：给它一个明确的读者说明 —— 在 `docs/research/` 里加一份两三行的 `README.md`，说明这些是一手调研的**原始笔记**（大、未删节、含上游路径与原话引用），结论已经折进 `.scratch/` 的 spec 与 `docs/` 的逐面文档；顺手记下 `notes/cline-continue.md` 里那个 `/sdk/plugins` 是**上游路径**而不是本仓库的死链（spec 的 Problem Statement 5）。
4. **扫一遍全仓 markdown 相对链接**（脚本进 Comments，供下次复用），确认除上面那条之外没有别的坏链。
5. 复核每条被指到的文档**确实存在**（`docs/highlight.md`、`docs/research/README.md`、`.scratch/README.md`）。

## 测试

- `AGENTS.md` 的指向都能打开。
- 链接扫描零坏链（研究笔记里的上游路径除外，且已在 `docs/research/README.md` 里写明）。
- 本轮不碰 Rust：`cargo test` / `clippy` / `fmt` 基线不变，仍跑一遍确认。

## 不做什么

删 `.scratch/call-rationale/`；改写 `docs/research/` 的笔记正文；给 `highlight.md` 补内容。

## Comments

**实现完成（2026-09-26）**。落点：`AGENTS.md`、`README.md`（一行）、`docs/highlight.md`（加状态块）、`docs/research/README.md`（新增）。

1. **`AGENTS.md`** 新增 `### Docs` 一节（英文，与它上面三节一致）：所有文档的索引进在 README 的 `文档` 一节（表 + 语言约定），`.scratch/` 的 feature 索引在 `.scratch/README.md`，写新文档前先读这两处、并跟邻居的风格走。README 的 `AGENTS.md` 那一行也补了「文档该往哪写、语言怎么选，也在这里指回本节」。
2. **`docs/highlight.md`** 开头加了一段**状态块**，写明复核日期与复核方式：`grep -rn 'highlight::' src/` 除模块自己一行都没有、`render/mod.rs` 只有 `pub mod highlight;`、`pane` / `tui` 都没调它 —— 免得读者把文档里「它是什么」当成「它已经接上了」。正文一字未改。
3. **`docs/research/README.md`（新增）**：说清这堆笔记的读者与用法 —— **材料不是结论**（结论已折进 spec 与逐面文档）、`coding-agent-features.md` 是入口、`notes/` 五份是上游正文、**不加维护**；并点明 `notes/cline-continue.md` 里的 `/sdk/plugins` 是**上游路径**，不是本仓库的坏链。
4. **链接扫描**（脚本如下，可复用）：

   ```python
   import os, re, glob
   files = [p for p in glob.glob('**/*.md', recursive=True) if not p.startswith('target/')]
   for f in files:
       for m in re.finditer(r'\[([^\]]*)\]\(([^)]+)\)', open(f, encoding='utf-8').read()):
           t = m.group(2).split('#')[0].strip()
           if not t or t.startswith(('http', 'mailto:')):
               continue
           if not os.path.exists(os.path.join(os.path.dirname(f), t)):
               print(f, '->', t)
   ```

   结果：**只剩那一条**上游路径（已在上面的 README 里写明），其余全绿。
5. **Rust 基线不变**（本轮没碰 Rust）：`cargo test` 736 passed / 0 failed，clippy 干净，fmt 只剩 `src/context/repo_map.rs` 的既有漂移。
