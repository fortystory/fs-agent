## Agent skills

### Issue tracker

issue 与 spec 都是这个仓库里 `.scratch/` 下的 markdown 文件：**一个 feature 一个目录，一张票一个文件**。见 `docs/agents/issue-tracker.md`。

### Triage labels

五个规范的分诊角色，label 字符串与角色名同名。见 `docs/agents/triage-labels.md`。

### Domain docs

单 context：仓库根目录的 `CONTEXT.md` 与 `docs/adr/`。见 `docs/agents/domain.md`。

### Docs

每一份文档都只有一处索引：[`README.md`](README.md) 的**「文档」一节** —— 那张表，加上它写的语言约定（**散文用中文**：接口、流程、注释、ADR、`docs/**`、票与 spec，**模型可见与进流的散文也在内** —— [ADR 0005](docs/adr/0005-model-visible-text-in-chinese.md) 起；**英文只留给不是散文的东西**：标识符、schema 值与协议标记、路径与命令；`docs/highlight.md` 是 ADR 0001 的例外，`docs/research/` 是一手引文、一个字不改）。`.scratch/` 的 feature 索引是 [`.scratch/README.md`](.scratch/README.md)。新增文档之前先把这两处读掉，并**照你旁边那份邻居写**。

> 上面那五个小标题（`## Agent skills` 与它的四个 `###`）**是技能工具链的锚点，保留英文** —— 它们是这份文件的 schema，不是散文。正文照 [ADR 0004](docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md) 与 [ADR 0005](docs/adr/0005-model-visible-text-in-chinese.md) 用中文。
