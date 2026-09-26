# 孤儿文档与约定落到 `AGENTS.md`

Type: implement
Status: ready-for-agent
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
