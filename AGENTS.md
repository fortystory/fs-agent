## 语言

散文用中文：接口、流程、注释、ADR、`docs/**`、票与 spec，**你写给人看的那些也在内** —— 工具结果、诊断、以及**思考**（[ADR 0005](docs/adr/0005-model-visible-text-in-chinese.md)）。英文只留给不是散文的东西：标识符、schema 值与协议标记、路径与命令原文。

**思考也用中文写**。它和回答一样进事件流（`MessageCompleted.reasoning`）、也一样在转录的详情里被人读到（`── 思考 ──` 那一节），而它比回答更容易漂回英文 —— 推理里夹着大量标识符、路径与代码。这一句同时钉在你的身份提示里（`src/agent.rs` 的 `THINKING_IN_CHINESE`，四段身份共用同一处），所以换一个工作目录跑的会话读不到这份文件，也读得到它。

## Agent skills

### Issue tracker

issue 与 spec 都是这个仓库里 `.scratch/` 下的 markdown 文件：**一个 feature 一个目录，一张票一个文件**。见 `docs/agents/issue-tracker.md`。

### Triage labels

五个规范的分诊角色，label 字符串与角色名同名。见 `docs/agents/triage-labels.md`。

### Domain docs

单 context：仓库根目录的 `CONTEXT.md` 与 `docs/adr/`。见 `docs/agents/domain.md`。

### Docs

文档索引：[`README.md`](README.md) 的「文档」一节；feature 索引是 [`.scratch/README.md`](.scratch/README.md)。新增文档前先读这两处、照邻居写。

> 这五个小标题（`## Agent skills` 与它的四个 `###`）是技能工具链的锚点，**保留英文**；正文照 [ADR 0004](docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md) 用中文。
