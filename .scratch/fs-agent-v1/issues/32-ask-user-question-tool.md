# 32: `ask_user_question` —— 模型发起的用户提问，接管底部输入区

**What to build:** 让**模型**能在干活途中向用户提问：一个模型面工具 `ask_user_question`，答案是那条
`tool_call` 的唯一结果；TUI 上**接管底部输入区**把它变成问卷界面（一屏一问、分页、可跳过）。这是
**第三类发起者**——现有两类是 harness 发起的（权限询问、计划冲突，走中段覆盖层）与渲染器发起的
（超长粘贴、`Esc` 清草稿）。对照物是 DSH 的 `ask_user_question` + `dsh-client-ui-user-questions`，
事实清点见 `.scratch/ask-user-question/research/01-dsh-ask-user-question-and-composer-takeover.md`
（路线与分叉见同目录 `seed.md`）。

Blocked by: None（它依赖的 §7 工具表 / §19 渲染与 CLI 组装 / 渲染接缝在票 07、18、19 里都已 done）

Status: ready-for-agent

**参考:** spec §7（工具 trait 与副作用）、§19（渲染与 CLI 组装：输入归渲染器、渲染器只管两个 sink）、
§6（取消传播）、§2（事件 schema，**本次不改**）、§12 与 User Story 8（无交互能力时降级而不是挂住）、
`docs/render.md`、`dsh-tool-ask-user` / `dsh-client-ui-user-questions` 的已安装产物

## 已定的决定（grilling 第 1–2 轮，12 条）

**词汇**
- [ ] `CONTEXT.md` 新增**用户提问（User Question）**词条：模型发起、结果是工具结果；并把现有的权限 /
  计划冲突明确为**询问（Ask）**——一个是 harness 的闸门（答案决定去留），一个是模型的问题（答案是
  上下文）。工具名 `ask_user_question`。

**工具契约（wire，snake_case，与现有工具一致）**
- [ ] `questions`（required，≥1 条），每条 `{ id, question, header?, options?: [{label, description?}], multi_select? }`；
  `multi_select` 默认 `false`
- [ ] 结果 = `{"answers":[{"id","selected","custom"?}]}` 序列化成 JSON 文本，作为那条 `tool_call` 的
  唯一结果
- [ ] **三条编码约定写进工具描述**：`selected: []` 无 `custom` = 跳过；单选下自定义文本**覆盖**已选
  （`selected: []` + `custom`）、多选下**补充**；推荐项放第一位且 label 末尾追加 `(Recommended)`，
  而**答案值保留原串**
- [ ] **不收** `detail` / `intent`（DSH 里它们是 harness 专属，模型面工具根本不暴露，`execute` 还会丢弃）

**呈现**
- [ ] TUI：接管底部输入区；一屏一问、分页（footer `2 / 3`）；**每题必须显式作答或跳过才能提交**，
  本题未处理时提交键禁用；单选选中即前进；单选下打字清空已选、多选下保留；`(Recommended)` 只做
  **显示**标记
- [ ] `Esc` / `Ctrl-C` 保持现有含义：取消**这次运行**（§6）。**不引入**「只放弃这次提问」的第三个手势，
  也**不新增错误类型**——取消后那条调用走 §6 的第 5 条路径拿到它唯一的结果
- [ ] **现有四类问题不动**（继续走中段覆盖层）：**票 30 不受影响**

**渲染器与降级**
- [ ] plain：**逐行问答**（它本来就读 stdin / 写 stderr）
- [ ] headless：**不把这个工具挂进工具表**（给模型一个必然失败的工具只是浪费一次调用）
- [ ] 兜底：真到了没有 answerer 的组装里，返回**模型可读的错误结果**，绝不挂住

**谁能问 / 流**
- [ ] 只有主会话能问：**执行者的工具表里不挂它**（与「执行者的表里没有 `task`」同一模式）
- [ ] **零 schema 改动**：问题在 `tool_call` args、答案在 result。`--continue` 撞上「进程死在问题挂着
  时」⇒ 复用既有的**悬空 `tool_call` 合成 unknown 结果**

## 测试

- [ ] 工具层：合法调用返回 JSON 答案文本；`questions` 为空 / 缺 `id` / `id` 重复被拒；没有 answerer
  时返回错误结果而不是挂住；描述里含那三条约定
- [ ] 接缝层：`ToolContext` 的端口被注入到工具；执行者的表里没有它；headless 组装的表里没有它
- [ ] TUI 层（`TestBackend` 帧断言）：接管出现／提问与选项在底部／分页 `/ 3`／未处理时不能提交／
  跳过与作答的编码／推荐标记不改答案值／作答完成后交回常驻输入区
- [ ] plain 层：逐行问答读到答案；EOF 时返回错误结果
- [ ] 端到端：假 provider 起一个真会话，模型调 `ask_user_question`，断言流上那条 `tool_call` 有且
  只有一条结果，且结果就是答案 JSON

## Comments

**2026-09-22（grilling）** 12 条决定全部由访谈定下，逐条在上。三条**不是分叉而是架构强制**的，
记在这里免得被当成漏项：问卷状态活在 `TuiState` 里（本仓库没有「重挂载」，所以 DSH 那套草稿 store
与键控不适用）；**不设等待超时**（DSH 也没有——人在键盘前，超时就是错的；headless 又不挂这个工具，
不存在挂死）；答案以 JSON 文本回来。

**代价（已知并接受）**：plain 逐行 + TUI 接管 + headless 不挂 = **三条路径都要建、都要测**，这是
这个设计里最贵的一块。它的手感（键位、分页、提交）属「必须亲眼看到」，所以 TUI 侧用 `TestBackend`
帧断言，必要时再拿 `/prototype` 兜。
