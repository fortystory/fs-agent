# 种子材料：模型发起的「选择工具」与底部问卷接管

> **这不是 spec，也不是票。** 它是进 `/grill-with-docs` 之前的路由判断与待谈分叉，
> 写下来是因为下一步要 `/clear`（那些推理只活在上一轮对话里）。规格以 grilling 的产物为准。
>
> **✅ 这张种子已经做完（2026-09-26 补记）**：功能实现于 **`.scratch/fs-agent-v1/issues/32-ask-user-question-tool.md`（`Status: done`，`cd6bf3f`）**，
> 本目录**不会**再长出 spec —— grilling 的产物直接折进了既有 v1 spec（`dece59a` 回改 §7 与 §19）与 `CONTEXT.md` 的两个词条。
> 下面的五个分叉各自的落点：
>
> | 分叉 | 落在哪 |
> | --- | --- |
> | 1. 谁发起 | `CONTEXT.md` **用户提问（User Question）** / **问卷（Questionnaire）** 词条；`Asker` 那条接缝**不扩展**（第三类发起者） |
> | 2. 流上的形状 | **零 schema 改动**：问题在 `tool_call` 的 args、答案在它的唯一结果（spec §7）；`--continue` 撞上悬空 `tool_call` 走 §11 既有的合成 unknown 结果，**不为「被遗弃的问题」发明新语义** |
> | 3. 呈现位置 | 底部输入区接管**只给这一类**；权限 / 计划冲突 / 粘贴 / 清草稿四类仍走中段覆盖层，**票 30 未受影响**（spec §19） |
> | 4. 非交互渲染器 | `plain` 逐行问答；`headless` 不把它挂进工具表；真漏了 answerer 时返回**模型可读的错误结果**，绝不挂住（spec §19） |
> | 5. v1 收多少 | 收了：多问题 / 多选 / 自由文本 / `header` / 每题草稿 / 推荐标记。**不收 `detail` / `intent`**；plan-review 式专属面板将来由**循环直接调接缝**，与模型面工具无关（spec §7） |
>
> 实现与验收：`src/questions.rs`、`src/tools/ask_user.rs`、`src/render/tui.rs` 的 `Pending::Questionnaire`；测试 `tests/ask_user_question.rs`（16 条）+ `tests/ask_user_question_tui.rs`（13 条）；手工清单 ①.5 与 ③.6、⑬ 各有一条。**已知没做的**：真终端里问卷的键盘手感（分页页脚、窄终端折行、自由文本行光标、提交禁用态的视觉）只有 `TestBackend` 帧断言，手工清单里没有对应条目。

## 原始请求

> 添加一个选择工具：当 fs-agent 在干活过程中需要向用户提问时，接管底部的输入框，把它变成一个
> 问卷界面，就像 dsh web 中的 `dsh-client-ui-user-questions`。

## 路线判断（`/ask-matt`）

- **main flow 第 1 步 = `/grill-with-docs`**：idea 段、在 working directory 里、且下面五条分叉
  必须谈掉才能开工。
- **multi-session**：动 schema（spec §2）+ 工具表 + 三个渲染器 + 文档 ⇒ 后面是 `/to-spec` →
  `/to-tickets` → 每票一次 `/implement`。
- **中间大概率绕一次 `/prototype`**：底部接管的「手感」（键位、多问之间怎么走、Enter 与现有
  `enter 发送` 的关系、多选、自由文本）属于「必须亲眼看」的问题。本仓库有先例：
  `.scratch/tui-layout/prototype`（还有 ADR-0002）。原型留在 `prototype/<name>` 分支上，
  由实现票指向它。

## 参照物（一手来源，已装在 checkout 里）

`/usr/lib/node_modules/@deepseek-ai/dsh/node_modules/@deepseek-ai/` 下的
`dsh-tool-ask-user`、`dsh-user-questions`、`dsh-client-ui-user-questions`、`dsh-plan-mode`、
`dsh-client-ui-tool`。

**事实部分已委托给 background research agent**，产物在
[`research/01-dsh-ask-user-question-and-composer-takeover.md`](research/01-dsh-ask-user-question-and-composer-takeover.md)：
工具名与 JSON schema、seam 的请求/答复类型与错误分类、composer takeover 的机制与边界、
`intent`/plan-review、无 answerer 与取消的路径。**先读它，再开始谈。**

## 要谈掉的五个分叉

1. **谁发起。** 现有两类：循环发起的（权限、计划冲突，`Question`）、渲染器发起的（超长粘贴、
   `Esc` 清草稿，`Pending`）。这一条是**模型发起**、答案是**工具结果**。它是同一个类型的第三个
   变体，还是自己的类型？`CONTEXT.md` 现在把「询问/问题」定义给前两类——这个词即将过载，
   属 `/domain-modeling` 的触发条件（`/grill-with-docs` 会内部拉起它）。
2. **流上的形状。** 仓库的不变量：`messages` 从只追加的流重算；每个 `tool_call` 恰好一条结果；
   有未出结果的 `tool_call` 时绝不调 provider。**会阻塞等人的工具调用**是新东西：进程死在问题
   挂着时，`--continue` 怎么办？（票 13 已为悬空 `tool_call` 合成 unknown 结果——复用还是新增
   「问题被遗弃」的语义？）
3. **呈现位置。** 请求要的是**底部输入框接管**；（**写这份 seed 时**，2026-09-22）四种问题全走
   **中段覆盖层**，而**票 30 正是 open 的、讲那个覆盖层形态的**。**后续**：票 32 落地后是**五类**
   问题，模型发起的这一类走**底部接管**，其余四类仍走覆盖层。所以：底部只给这一个新的问题类，还是所有问题都下移？**这个
   决定不能悄悄替票 30 作答。**
4. **非交互渲染器。** `--plain` 只有 stdin，headless 没有键盘。报一个模型能读的错误结果 /
   自动作答 / 干脆不把工具挂进工具表？否则 agent 会永久挂住——本仓库有很强的「绝不挂住」传统。
5. **v1 收多少。** 多问题、多选、自由文本、`header`、`detail`、草稿、推荐标签约定、`intent`；
   协议要求哪些、纯呈现哪些。顺带：`plan-review` intent 与本仓库**硬 plan 模式**的 `PlanConflict`
   询问是重叠的，值得一起看。

## 本仓库已经具备的接缝（别重造）

- 工具表与副作用分类：`src/tools/mod.rs`、`src/tools/tool.rs`（`Effect`）、`src/tools/registry.rs`、
  `src/tools/task.rs`（派子 agent 的现成先例）、`src/tools/custom.rs`（配置声明的动态工具）。
- 问与答的通道：`src/render/input.rs` 的 `ConsoleRequest` / `AskRequest` / `Question`，
  以及 `TuiState` 的 `Pending`（`src/render/tui.rs`）。
- 刚刚落地的两条相关机制：`ConsoleRequest::RunState`（循环推送「在不在跑」，票 29）与
  「运行结束就撤回它未答的问题」（票 31，`RunState { running: false }` 撤 `Pending::Loop`）。
  新工具的问题与这两条**必然交互**，设计时要正面处理。

## 工程约定（本轮踩过的）

- **提交用限定路径**（`git commit -F - -- <路径>`）。这个工作区里出现过并行 session 用全量暂存
  提交，把未提交的在制品卷进无关主题的提交里（票 31 的修复就落在 `939161d` 里）。
- 改了 spec 的决定就**回改 spec 正文**，不要只写在票的评论区（Further Notes 的明文规则）。
