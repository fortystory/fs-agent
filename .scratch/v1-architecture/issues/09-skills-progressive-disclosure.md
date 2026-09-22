# skills：渐进披露的指令包

Type: grilling
Status: open
Blocked by: 01, 06

## Question

决定 skills 的接缝。ratify 第 3 条翻案把它从"砍掉"改成"要做"。

**机制**（综述第 9 节，来自 Claude Code 的实现）：一个目录 + `SKILL.md`。启动时**只加载一行描述**进上下文，模型真正调用时才加载全文 —— 本质是**渐进披露（progressive disclosure）**。相关细节：`disable-model-invocation: true` 的 skill 完全不进上下文，直到用户显式 `/name` 调用；compaction 后已调用的 skill body 会重新注入，每 skill 上限 5k tokens、总上限 25k，超了丢最老的。

要回答：

1. **注入点**。那"一行描述"进哪里 —— 与 `AGENTS.md` 同在**第一条 user message**，还是别处？全文加载时是追加一条 user message 还是别的？这直接接上票 06 的缓存前缀约束（综述把"system prompt 前缀稳定"标 ✅，称其为"免费的 5~10 倍成本差"）。
2. **发现与加载**。扫描哪些目录？综述记录 **Amp 默认会读 Claude Code 的 skills 目录**（`~/.claude/skills/`、`.claude/skills/`），并称 skill 格式正在事实标准化。是否跟随这个事实标准？
3. **与 `AGENTS.md` 的边界（本票必须自证价值）**。综述砍它的理由是"有了 AGENTS.md + hooks 就够了"。所以要说清 skills 提供了什么 `AGENTS.md` 给不了的东西 —— 答案是**按需加载**：`AGENTS.md` 每次全量注入，skills 只在调用时付费。请确认这个理由在你这里成立，并给出边界（哪些内容该进 `AGENTS.md`、哪些该做成 skill）。
4. **预算与重新注入**。单 skill 与总体的 token 上限取多少？压缩后是否重新注入？注意自动 compaction 本身是后置项（综述建议 ~90% 阈值、第一次真爆窗再做）。

**不要**做：自定义工具注册 —— 它是**票 11**，已从本条拆出（ratify 第 3 条内部是两件事）。
