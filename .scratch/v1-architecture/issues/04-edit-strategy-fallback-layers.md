# 编辑策略的可插拔点与降级匹配分层

Type: grilling
Status: open
Blocked by: 03

## Question

决定 `edit_file` 内部策略的接缝。综述说编辑是"整条链路失败率最高的地方"——模型生成的 `old_string` 少一个空格就整块失败；而**真正的差异化在「编辑失败之后」**（opencode、cline、gemini-cli 三家各自独立收敛到同一类自愈层）。

要回答：

1. **降级匹配怎么分层。** 综述建议 v1 只做 **2 层**：**每行 trim 后匹配**、**忽略行尾空白**，理由是"就能吃掉大部分无谓失败"。这两层放在哪个接缝后面，才能将来加到更多层（opencode 有 10 层）而**不改工具接口**？
2. **护栏是否进 v1**：
   - 拒绝"省略占位符"的写入（Gemini 的 `write_file` 会因 `rest of methods ...` 直接报 `Provide complete file content.`）——综述标 🔷，「一行正则检查，直接消灭一整类事故」。
   - 防"匹配过大"的拒绝（opencode 的 `isDisproportionateMatch`：匹配到的片段远大于 `old_string` 时拒绝应用）。
3. **编辑格式是否留成可配置项。** 综述有一个尖锐观察：**编辑格式不是「哪个更好」，而是「哪个模型更能写对」**——opencode 按 model id 二选一（命中 `gpt-` 用 `apply_patch` 替换掉 `edit`/`write`），aider 有 `--edit-format`。v1 是固定为精确 search-replace，还是在第 1 条那个接缝上留出按模型选择的位置？
4. 明确**不做**：unified diff 解析、AST / tree-sitter 编辑（综述：10 个实现里零个把它当编辑路径）。

答案定到分层结构与每层的接口，不要实现。
