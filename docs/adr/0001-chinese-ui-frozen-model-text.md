# 人类可见文本用中文，模型可见文本冻结

前端文本（转录三个画家、CLI help 与错误、`sessions *` 的输出）一律硬编码中文，不引入运行期 locale——自用 CLI 没有第二种语言需求，省掉 i18n 依赖与随之而来的断言复杂度。而**会投影进模型 `messages` 的文本保持英文**：包括 `AgentError.message` 与 fs-agent 自己生成的工具结果文本（`project()` 把它们投成 `user` / `tool` 消息），以及 `SessionError.detail`。

这条边界不是风格偏好，而是一条代码里看不见的约束：**模型可见文本是 provider 缓存前缀的头**，中途改它就会废掉前缀缓存（spec §4 / §17 已把这类操作列为明令禁止），恢复旧会话时还会出现中英混排的历史。事件流只追加、永久回放，所以 durable 的错误文本按语言冻结；好在 `SessionError` 根本不进投影（`src/provider/projection.rs:239`），它的中文只在渲染时按 `code` 映射，`detail` 原样透传。

## Consequences

- 人类可见文案有唯一处（措辞层，先例是 `render::transcript::decision_source_label`），`sessions show` 不再自己重拼第四份。
- 新增字符串前要先判断它属于哪一侧：进 `messages` 的一侧只增不改。
- UI 术语一律用 `CONTEXT.md` 的中文名；glossary 里没有的概念，先补 glossary 再写 UI。
