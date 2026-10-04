# `.scratch/` 里的英文小标题中文化

Type: implement
Status: done

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

## 评论

- 2026-10-05 落地。**译名表**（全仓一个词一个译法，术语跟 `CONTEXT.md` 走）：
  `## Comments` → `## 评论`、`## Question` → `## 问题`、`## Answer` → `## 作答`、
  `## Problem Statement` → `## 问题陈述`、`## Solution` → `## 方案`、
  `## User Stories` → `## 用户故事`、`## Implementation Decisions` → `## 实现决定`、
  `## Testing Decisions` → `## 测试决定`、`## Out of Scope` / `## Out of scope` →
  `## 明确不做`、`## Further Notes` → `## 补记`、`## Destination` → `## 目的地`、
  `## Notes` → `## 笔记`、`## Decisions so far` → `## 已定的决定`、
  `## Not yet specified` → `## 尚未明确`、`## Frontier` → `## 前沿`、
  `## What to build` → `## 要做什么`、`## Deliverable` → `## 交付物`、
  `## Agent Brief` → `## 给 agent 的简报`、`## Findings` → `## 发现`、
  `## What the sources leave open` → `## 来源留下的口子`、
  `## Landing Notes (2026-10-01)` → `## 落地记录（2026-10-01）`。
  零散的 6 处：`# 08: skills` → `# 08 — 技能（skills）`、`# 09: repo map` →
  `# 09 — 仓库符号地图（repo map）`、`### §7 ADR 0011` → `### §7 决定记录：ADR 0011`、
  `### §1 fixture` → `### §1 夹具`、`### §4 pty` → `### §4 伪终端（pty）`、
  `### §8 handoff` → `### §8 交棒（handoff）`。
- 规模：**230 个文件、433 个标题**只动标题行；**194 处引用**跟着对上 ——
  [`docs/agents/issue-tracker.md`](../../../docs/agents/issue-tracker.md) 的「约定」与
  wayfinder 两节（`## Comments` / `## Answer` / `Notes` / `Decisions so far` /
  `Not yet specified`）、`CONTEXT.md` 的「构建计划」「决策图」「票」三条与**分叉合成**的
  `_Avoid_`、`README.md` 的三处 `Out of Scope`，以及 `docs/highlight.md`、`docs/render.md`、
  `src/render/highlight.rs`、`src/render/wording.rs`、`src/discussion.rs`、`src/lib.rs`、
  `tests/discussion.rs` 里「spec 的 `Out of Scope`」那类指路句。
- **刻意跳过的三份**：`language-migration/spec.md`、`issues/03`、`issues/04`。它们描述的
  就是这批标题本身（票面那张「`## Comments` 82、`## Question` 49 …」是改动前的实测统计，
  spec §59 是在派这一票的活），替换会让叙述自相矛盾 —— 它们是这次改动的**记录**，不是要
  对上的引用。
- **护栏**：[`scripts/check-language.py`](../../../scripts/check-language.py) 加了第 ⑤ 条 ——
  `.scratch` 里 tracker 的标题也要含中文。判据与 ③ 的 ADR 检查**共用一份**：剥掉行内代码与
  链接后，**只有带字母的才算散文**，所以 `### §12 \`/clear\``、`### 1. \`docs/render.md\``
  这类剥完只剩编号与路径的**指路标题**放过；围栏改按**行**配对挖除（原来的正则会被一个
  不成对的围栏吃掉下半篇，那些标题就漏检了）。**验过会红**：把
  `input-tokens/issues/01-file-index.md` 的 `## 目标` 临时改成 `## Goals`，脚本报红并以 1
  退出，还原后重新 OK。
- **`research/` 整档不查**：那是一手引文与专名笔记，留英文的是产品名与技术小节（护栏口径下
  66 处，如 `### DeepSeek`、`### petgraph`、`### firejail`、`### nsjail（Google）`、
  `### ② bubblewrap`）—— 票的验收把「引文小节」与「专名」列为允许的残留。`docs/research/`
  与 `.scratch/*/research/` 的**文件身份**照旧。按验收 grep 的残留因此分两档：
  - 非 research：**0 处**「带字母且无中文」的标题。剩下的英文开头标题要么含中文
    （`# research：…`、`### A. 布局与基础信息`），要么剥掉行内代码后不含字母
    （`### §12 \`/clear\``、`### 6. \`ToolOutput\` / \`ToolError\``、
    `### 3. \`.scratch/README.md\``）。
  - research：上面那 66 处专名小节。
- 复核**技能侧**有没有按这些标题找内容：本仓库里只有
  [`scripts/wayfinder-check.py`](../../../scripts/wayfinder-check.py) 读 tracker 的 markdown，
  它只认 `## 任务清单` 与 `Type:` / `Status:` / `Part of:` / `Blocked by:` 四行字段（改前改后
  一致）；仓库外的 skills 不在这棵树上，无法在本票里核对，如实记在这里。
- 验收：`python3 scripts/check-language.py` 退出 0（新纳进来的 `.scratch` 检查验过会红）；
  `python3 scripts/check-doc-size.py` 退出 0；`cargo test` 全绿（exit 0）。
- **`/code-review` 之后补的一轮**：上一轮的引用对齐只认带 `## ` 前缀的那几种形态，漏掉了
  **指路句**里的另一些写法 —— 现在把 `` `Notes` ``（7）、`` `Testing Decisions` ``（19）、
  `Further Notes`（26）、`Decisions so far`（9）、`Not yet specified`（8）共 **55 处**一起对上
  （`docs/agents/issue-tracker.md`、`CONTEXT.md`、`docs/tui-manual-checklist.md`、
  `src/agent/history.rs` 与各 spec 正文；`.scratch/language-migration/` 那三份描述这批标题自身
  的文件照旧跳过）。`language-migration/spec.md` 里两处过时的状态（第 3 行的「未做」与第 59 行
  的 `Status: ready-for-agent`）也改成已落地。
- **ADR 0012 顺手对了两处**：决定里原来写「`/` 与 `@` 共用一条**边界规则**……任意位置」，
  与 spec §2 那张两条边界的表冲突（ADR 自己的「被否决」一节倒是对的）—— 改成「共用一套浮层、
  键位与位置判据，而**边界各按前缀**」；「为什么」第 4 条引的 `src/cli.rs:1473-1474` 在这次
  改动后已经漂到别的行，换成不带行号的 `submission()`（行号在这种散文里本来就会烂）。
- **护栏当场抓了我一次**：给本票写 Comments 时标题顺手写成 `## Comments`，`check-language.py`
  第 ⑤ 条立刻报红 —— 改成 `## 评论` 才过。这是它该做的事，记在这里当一次现场验证。

（`/` 解析那一侧还有一处收紧，记在同一轮的 [input-tokens 票 04](../../input-tokens/issues/04-slash-anywhere.md)
的评论里。）
- **`wayfinder-check.py` 一处如实记下**：三张图 PASS，
  [`mcp-support/map.md`](../../../.scratch/mcp-support/map.md) **FAIL，而且改动前就 FAIL** ——
  `issues/19-mcp-catalog-in-context.md` 不在图的 `## 任务清单` 里，HEAD 版本的清单里也没有它，
  那是票 19 落地时留下的存量欠账（不是这次改名造成的）。本票不碰图的正文，所以原样留着。
