## 语言

散文用中文：接口、流程、注释、ADR、`docs/**`、票与 spec，**你写给人看的那些也在内** —— 工具结果、诊断、以及**思考**（[ADR 0005](docs/adr/0005-model-visible-text-in-chinese.md)）。英文只留给不是散文的东西：标识符、schema 值与协议标记、路径与命令原文。

**思考也用中文写**：它进事件流（`MessageCompleted.reasoning`），也在转录详情里被读到（`── 思考 ──`），而推理里夹着标识符、路径与代码，比回答更容易漂回英文。同一句也钉在你的身份提示里（`src/agent.rs` 的 `THINKING_IN_CHINESE`），所以换一个工作目录的会话也读得到它。

## Agent skills

### Issue tracker

issue 与 spec 都是 `.scratch/` 下的 markdown：**一个 feature 一个目录，一张票一个文件**。见 `docs/agents/issue-tracker.md`。

### Triage labels

五个规范的分诊角色，label 字符串与角色名同名。见 `docs/agents/triage-labels.md`。

### Domain docs

单 context：根目录的 `CONTEXT.md` 与 `docs/adr/`。见 `docs/agents/domain.md`。

### Docs

文档索引：[`README.md`](README.md) 的「文档」一节；feature 索引是 [`.scratch/README.md`](.scratch/README.md)。新增文档前先读这两处；提交信息照 [`docs/agents/commits.md`](docs/agents/commits.md) 写。

> `## Agent skills` 与它的四个 `###` 是技能工具链的锚点，**保留英文**；正文照 [ADR 0004](docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md) 用中文。

## 工作区里的中间产物

可再生的东西——缓存、编译中间结果、日志——放进工作区的 `.cache/`。它已被 git 忽略，
删掉不影响仓库状态；源码、文档与票都在别处，所以往里写东西不必先想「会不会被提交」。
现在住在那里的：`.cache/pytest`（由根目录 `pytest.ini` 的 `cache_dir` 引导过去）与
`~/.cargo`（cargo 的索引与解包出来的依赖；它在沙箱的可写根里。早先工作区内的
`.cargo-home` 存在，是因为当时 `~/.cargo` 只读——那条已经不成立了，所以它被删掉而没有搬进来）。

两处搬不进来：`.dsh-mattskillsdeck-cache` 与 `.scratch/.dsh-write-probe`——落点由仓库外那个
skills-deck 自己决定，我们没有改它写哪里的入口。
