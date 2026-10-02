# ADR 也走中文：小标题、行话夹注与 docs/adr 的护栏

Type: implement
Status: done

> 规格：`.scratch/language-migration/spec.md`；决定：[ADR 0004](../../../docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md)（本轮补的「后加」一节说的就是这一批）。
> 这一批**只动文档，不动代码**：不改 `src/` / `tests/` 一个字符，所以 `cargo test` 的条数必须一条不变。

## 目标

ADR 自己是不是「散文」——这个老问题在这一批里答掉，并且答成**可检查**的：ADR 的标题与小标题用中文，行话按「中文名（English）」写，`docs/adr/*.md` 进护栏。

先量再改（2026-09-30 量的）：

- `src/` + `tests/` 的注释、`docs/**`（除 `research/` 与 `highlight.md`）里**已经没有英文散文** —— 剩下的英文行逐条看下来全是文件路径引用（`/// （.scratch/tui-history-replay/spec.md §1）。`）、代码块、图表、CLI 用法，属 ADR 0004 允许留英文的那一侧。
- `docs/adr/` 里可以用中文却写着英文的地方：`## Consequences` × 3（0001 / 0002 / 0003），以及 0002 的行话（`inline viewport` / `scrollback` / `alt screen` / `raw mode` / `bracketed paste` / `panic hook` / `no-op` / `dump` / `pty` / `preview`），另 0001 的 `locale` / `i18n` / `glossary`、0003 的 `stance`、0004 的 `durable` / `bug` / `diff`。

## 具体行为

1. **小标题中文**：三处 `## Consequences` → `## 后果`（与 0004 的「## 代价（如实写）」「## 被否决的替代方案」一致）。ADR 的**文件名**是标识符，留英文。
2. **行话中文夹注英文**（首现夹注一次）：行内视口（inline viewport）、备用屏幕（alt screen）、回滚缓冲（scrollback）、原始模式（raw mode）、括号粘贴（bracketed paste）、panic 钩子（panic hook）、系统提示词（system prompt）、区域设置（locale）、国际化（i18n）、伪终端（pty）、空操作（no-op）、转储（dump）、预览（preview）；`cwd` / `token` / `assistant` / `args` 这类**字段名与 schema 值不夹注**、保持英文（`CONTEXT.md` 里「token 不给中文名」同一条）。
3. **护栏扩到 ADR**（`scripts/check-language.py`）：`docs/adr/*.md` 进 `DOCS_MIN_RATIO`（逐份「实测 −2 点」），并新增第四条「ADR 的标题 / 小标题必须含中文」；文档串从「三条检查」改成五条。
4. **ADR 0004 补「后加」一节**记下这三条，并把正文里那句「三条检查」改对。
5. **README**：`docs/adr/` 索引行补上**漏掉的 ADR 0004**、《约定》补一条「术语写中文名（English）」；「alt screen」「tab 条」在 README / `docs/render.md` / `docs/tui-manual-checklist.md` 里一并改中文（它们是同一批散文里的同一个词）。

## 验收

- `python3 scripts/check-language.py` OK；**并且这条新检查要真的会红** —— 拿一个含 `## Consequences` 的探针 ADR 跑一遍，确认它报「标题里一个中文字都没有」再删掉。
- `cargo test` **757/0**（条数不变）、`clippy` 干净、`cargo fmt --check` 零漂移。
- `docs/adr/*.md` 的中文占比实测 42.6–46.4%，下限取「实测 −2 点」。

6. **`AGENTS.md` 也翻了**（2026-09-30 追加的一个决定，见下）：它正文改成中文，那五个小标题（`## Agent skills` 与四个 `###`）留英文 —— 它们是技能工具链的锚点。`AGENTS.md` 进 `DOCS_MIN_RATIO`（实测 25.8% → 下限 23）。

## 不做什么

- **不动 `.scratch/` 里的英文小标题**（`## Question` / `## Answer` / `## Comments` / `## Problem Statement` / `## Destination`……）：它们是 tracker 的字段名还是散文，当时没定；留成[票 04](04-tracker-headings-in-chinese.md)。
- 不动 `docs/research/`（一手引文）、不动任何模型可见 / 进流的字符串。

## Comments

**2026-09-30 收尾**：落地在 `7d4d1d4`（`docs(language): ADR 也走中文 —— 小标题、行话夹注与 docs/adr 的护栏`）。

- 改动面：四份 ADR + `scripts/check-language.py` + `README.md` + `docs/render.md` + `docs/tui-manual-checklist.md`；**`src/` 与 `tests/` 一行未动**（所以 757 条测试只是复跑确认）。
- 新检查验红过：`.scratch/adr` 的探针文件（`# temporary probe` + `## Consequences`）被两条都报出来，删掉后回到 OK。
- `DOCS_MIN_RATIO` 新增的四条：0001 → 44、0002 → 40、0003 → 41、0004 → 43（实测 46.4 / 42.6 / 43.8 / 45.5）。
- **`AGENTS.md`（当天追加）**：它是仓库里最后一份英文文档（中文占比 0.2%），而且它自己那句语言约定还写着「English for the `docs/*.md` design docs」—— 早被 ADR 0004 取代了。定下来翻：它虽然是 `ContextInjected { source: AgentsMd }`，但不属于「模型可见的文本」那一类（那一类指 provider 按 schema 读的格式敏感串），与 `src/agent.rs` 里那几段中文身份提示同属「harness 对模型说话」。代价与新会话的缓存前缀换一次，记在 ADR 0004 的「后加」一节；`--continue` 的老会话重放出来仍是英文。五个小标题留英文（工具链锚点），正文中文。
- 顺手对齐的词：README 与 `docs/render.md`、`docs/tui-manual-checklist.md` 里原先混用的 `alt screen`、`tab 条`（`CONTEXT.md` 的词是**页签**）。
