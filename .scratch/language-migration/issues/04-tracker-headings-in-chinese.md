# `.scratch/` 里的英文小标题中文化

Type: implement
Status: ready-for-agent

> 规格：`.scratch/language-migration/spec.md`；决定：[ADR 0004](../../../docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md)。
> 这张票是 2026-09-30 单独记下来的：ADR 那一批（[票 03](03-adr-headings-and-jargon.md)）当时只做 `docs/adr/`，tracker 的标题**故意留着没动**，因为「它们是字段名还是散文」没有定。

## 目标

把 `.scratch/` 里那批英文小标题改成中文 —— 它们和 ADR 的 `## Consequences` 是同一种东西（标题，散文），只因为它们写在 tracker 的约定里，上一轮才被当成字段名放过。

## 现状（2026-09-30 量的，改前先复核）

- **票**（`issues/NN-*.md`，131 个文件里有这批标题）：`## Comments` 82、`## Question` 49、`## Answer` 48、`## Solution` 8、`## Problem Statement` 8，另有 `## What to build`、`## What the sources leave open`、`## Frontier`、`## Findings`、`## Deliverable`、`## Agent Brief`、`#### OpenAI`、`### DeepSeek`。
- **spec**：`## Problem Statement` / `## User Stories` / `## Solution` / `## Implementation Decisions` / `## Testing Decisions` / `## Further Notes` / `## Out of Scope`（另有一处写成 `## Out of scope`）各 8 处左右。
- **决策图**（`map.md`）：`## Destination` / `## Notes` / `## Decisions so far` / `## Not yet specified` / `## Out of scope` / `## Frontier`。
- **约定文档正文明写这批标题**：[`docs/agents/issue-tracker.md`](../../../docs/agents/issue-tracker.md) 的「约定」一节（`## Comments`）、wayfinder 一节（`## Answer`、`Notes` / `Decisions so far` / `Not yet specified`）；`CONTEXT.md` 的**分叉合成**那条 `_Avoid_` 与 README 也各引了一次 `Out of Scope`。
- 图里那个 `## 任务清单` **已经是中文**，`scripts/wayfinder-check.py` 正是按它核对的；它查的字段行是 `Type:` / `Status:` / `Part of:` / `Blocked by:`，**不查这批标题**。

## 领票后先定的三件事（别跳过）

1. **译名表先定死再动手**，全仓一个词一个译法。建议：`## Question` → 「## 问题」、`## Answer` → 「## 作答」、`## Comments` → 「## 评论」、`## Problem Statement` / `## Solution` → 「## 问题陈述」/「## 方案」、`## User Stories` → 「## 用户故事」、`## Implementation Decisions` / `## Testing Decisions` → 「## 实现决定」/「## 测试决定」（`fs-agent-v1/spec.md` 的正文就是这么叫它们的）、`## Out of Scope` → 「## 明确不做」（README 那一节的现成中文）、`## Destination` → 「## 目的地」、`## Decisions so far` → 「## 已定的决定」、`## Not yet specified` → 「## 尚未明确」、`## Notes` → 「## 笔记」。
   译名跟 `CONTEXT.md` 走（比如「票」「决策图」「blocker 边」是它给的词），别自造同义词。
2. **老票改不改**：建议**一并改**（它们是要长期读的材料，半中半英更糟），但那就要求一次改完、`git log` 上一个可核对的提交，而不是「改到哪算哪」。
3. **`Out of Scope` 这类既是小节名、又被正文当概念引用的词**：改完要把引用处（README 的「明确不做」一节、`CONTEXT.md` 的 `_Avoid_`、各 spec 正文里那句「见 `Out of Scope`」）一起对上，别留下指向不存在小节的引用。

## 具体行为

1. 按定好的译名表改 `.scratch/**/*.md` 的标题（**只动标题行**，正文一个字不改）。
2. 改 [`docs/agents/issue-tracker.md`](../../../docs/agents/issue-tracker.md)：它正文里写死的 `## Comments` / `## Answer` / `Notes` / `Decisions so far` / `Not yet specified` 换成新译名。
3. 改 `CONTEXT.md`、`README.md` 里引到这些标题的地方（`Out of Scope`）。
4. 复核**技能侧**有没有按这些标题找内容：本仓库只有 `scripts/wayfinder-check.py`（已确认不查它们），但 skills 是仓库外的，若它对 `## Answer` 有依赖，要在本票的 Comments 里如实记下来。
5. **护栏**：`check-language.py` 现在只查 `docs/adr/*.md` 的标题。做完这一批，把 `.scratch/` 的标题也纳进同一条检查（哪些文件、放不放 `map.md`，在这一步定），否则下一次开票又会写回英文标题。
6. 一次 `git commit`，中文信息（例如 `docs(tracker): `.scratch` 的小标题中文化`）。

## 验收

- `grep -rn "^#\{1,4\} [A-Za-z]" .scratch --include="*.md"` 只剩**该留英文**的标题（引文小节、`### DeepSeek` 这类专名、代码块里的 `#` 注释）—— 逐条在 Comments 里交代，别用「差不多」收尾。
- `python3 scripts/wayfinder-check.py` 对四张图仍然 PASS；`python3 scripts/check-language.py` OK（且新纳进来的 `.scratch` 标题检查**验过会红**）。
- `cargo test` 757/0（这一批不该动代码，跑一遍是确认没误伤）。

## 不做什么

- 不改票与图里的**正文**（那是上一轮已经做完的散文）。
- 不改 `Type:` / `Status:` / `Part of:` / `Blocked by:` 这类**字段行** —— 它们是 schema，与 `spec.md` / `map.md` 一样按 `CONTEXT.md` 的规矩留原样。
- 不动 `docs/research/`（一手引文）。
