# 种子材料：主输入区里的 `@` 选文件

> **这不是 spec，也不是票。** 它是 2026-10-04 记下的一条意向——**还没被访谈、也没有票**：
> 在主输入区打字时输入 `@` 触发工作区文件选择，参考 codex 的 `@` 功能。想推进时走
> `/grill-with-docs` 把它折成 `spec.md`，再 `/to-tickets` 拆票。
>
> 与本目录同批的另一条意向（问卷的回车语义）**不在这里**，它落在
> [`../questionnaire-keys/`](../questionnaire-keys/spec.md)——那是一条只动既有 spec 的增量。

## 它要什么

用户原话（2026-10-04）：

> 输入时添加使用@选择文件功能，参考 codex 的 @ 功能

## 现状（2026-10-04 核实）

- **输入区今天没有任何文件补全**：只有 `/` 命令菜单那一套
  （[`src/render/editor.rs:317`](../../src/render/editor.rs) 的 `complete_slash`），
  既没有文件索引，也没有模糊匹配。
- `CONTEXT.md` 里没有对应的词条；`.scratch/README.md` 的需求池里此前没有这条。
- 与之相邻的另一条意向是 [`../image-input/seed.md`](../image-input/seed.md)（粘贴 / 路径 /
  拖拽进来的图进请求）——两者都落在「把工作区里的东西带进请求」这条缝上，**是否算同一条路
  还没谈**。

## 材料：codex 实际怎么做

[`research/01-codex-at-file-mention.md`](research/01-codex-at-file-mention.md)（2026-10-04）
已经把事实查清：触发条件、候选来源与忽略规则、模糊匹配、浮层与键位、插入形态、目录、
别的触发符、边界处理与分层，逐条带 `文件:行号` 出处。一手来源是 `openai/codex`
commit `afb436df8b70bb5bc57b86d9a3e829968988cd21` 与两页官方文档。

**这份材料只回答「codex 怎么做」，不替我们做决定。** 里面有四处明确写着我们的提问假设
不成立（不是任意位置触发、不是文件专用、不是内联下拉、MCP resource 不在候选里）。

## 待谈的分叉

尚未访谈，所以**故意留空**——frontier 要等 `/grill-with-docs` 一轮一轮推出来，不在这里预支。
