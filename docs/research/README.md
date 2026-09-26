# `docs/research/`：一手调研的原始笔记

**这些是材料，不是结论。** 正文又长又未删节（合计约 796KB），里面混着上游文档的原话、源码片段
与评测数据；它们是当时为了回答「别的 coding agent 怎么做」而读的东西，**结论已经折进**
[`.scratch/`](../../.scratch/README.md) 的 spec 与 [`docs/`](../) 的逐面文档。要查某个设计
**为什么**这样定，看 spec 与 ADR；要看**当时读到了什么**，才来这里。

- `coding-agent-features.md`：横向对比（按能力维度整理，是这批笔记的入口）。
- `notes/`：五份上游笔记（aider/openhands、claude-code/amp、cline/continue、codex/gemini、
  opencode/goose），逐份摘录加引用。

两条读法上的提醒：

- 笔记里的**路径与链接是上游的**，不是本仓库的 —— 例如 `notes/cline-continue.md` 里的
  `/sdk/plugins` 指向它自己的仓库，不要当成坏链去修（全仓链接扫描会把它当死链报出来）。
- 笔记**不加维护**：上游改了、我们对齐了，笔记就停在写它的那一天。它们不是现状的描述。
