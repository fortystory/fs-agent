# research：DSH 的 ask_user_question 与 composer takeover

## Question

模型需要一个「向人提问并把答案当工具结果拿回来」的能力，DSH 已经把它做成了 first-party 实现：一个模型面工具、一个 `ctx.userQuestions` seam、一个 Web 端的「底部输入框接管」问卷界面，外加 plan-review 这条 intent 分支。本note 只做事实清点：读完已安装的编译产物，把这四层各自的契约、边界、取消/失败路径摊开，并对照 fs-agent 现有的事件流、工具表、四种问题覆盖层与三个渲染器，把五个待谈分叉分别标注为「sources 已经 settle 了什么」与「sources 留开了什么」。

**路径约定**：下文所有 `<pkg>/...` 相对 `/usr/lib/node_modules/@deepseek-ai/dsh/node_modules/@deepseek-ai/`；在逐包小节里，裸 `lib/...` 指该小节所讲的那个包。代码是发布的编译 JS，结论以「文件 + 行号」给出；凡是从编译代码推导而文档没有明写的，正文里写明依据，不当作文档事实。

## Findings

### 调用链全貌（模型发出 → 人答 → 工具结果）

- 模型发 `ask_user_question` 工具调用 → `dsh-tool-ask-user` 的 `execute` 调 `ctx.userQuestions.ask(...)` → `dsh-user-questions` 的 `UserQuestionService` 校验并派发 Agent-scoped waterfall `user-questions/request` → `dsh-api-remotes` 把这个事件列入 forwarded allowlist（mode `waterfall`）→ 浏览器侧 `dsh-client-ui-user-questions` 注册的 listener 用 `PendingQuestion` 接管 `conversation.composer` 链槽 → 用户作答/取消 → waterfall 的 promise settle → `execute` 的返回值成为该 `tool_call` 的唯一结果。
- 每一环都必要：`dsh-tool-ask-user/lib/index.js:1`（唯一模型面工具）、`dsh-user-questions/lib/index.js:32`（唯一服务定义与验证点）、`dsh-api-remotes/lib/types/remote-events.d.ts:67`（唯一允许跨到浏览器的声明）、`dsh-client-ui-user-questions/lib/client.js:880`（唯一 shipped answerer）、`dsh-scope/lib/invariant.js:37`（该事件的 scope key 取 `args[0].agent`）。
- 运行期 DSH 的 seam 与工具是**两个包**：seam 不依赖工具，工具依赖 seam（`dsh-tool-ask-user/package.json` peerDeps 含 `dsh-user-questions`）。非模型调用者也可直接用 seam：`dsh-plan-mode` 的 `exit_plan_mode` 自己调 `interaction.ask(...)`（`dsh-plan-mode/lib/index.js:259`）。

### 1. 模型面工具 `ask_user_question`

- 工具名 `ask_user_question`，插件名 `tool-ask-user`，`inject = ["tools", "userQuestions"]`，只注册一个 `defineTool` — `dsh-tool-ask-user/lib/index.js:11-16`。
- 模型看到的 description 原文：`"Ask the user a concise question when you need confirmation, a choice, or missing information before proceeding. Send one or more questions, each with a stable id that will be echoed in the answer."` — `dsh-tool-ask-user/lib/index.js:13`。
- 参数 schema（`parameters.questions`，逐字段照抄编译前的 authoring 形状）— `dsh-tool-ask-user/lib/index.js:18-65`：
  - `questions`: array, **required**。
  - item: object, `additionalProperties: true`；`id`(string, required, "Stable id … echoed in the answer")、`question`(string, required)、`header`(string, optional)、`options`(array, optional)、`multi_select`(boolean, optional, "Defaults to false")。
  - option: object, `additionalProperties: true`；`label`(string, **required**)、`description`(string, optional)。
- 编译后：`defineTool` 把根编译成隐式 open object，`required: true` 的字段汇成 `required` 数组（`dsh-tools/lib/types/schema.js:68-82,238-247`）。**根对象没有 `additionalProperties`**，`questions` 的 item 与 option 的 item 显式为 `true` —— 所以模型多送的键（例如 `detail`、`intent`）会通过校验，但被 `execute` 的显式映射丢掉（见下）。序列化后的键序不在产物里（推断：形状可读，字节序不可读）。
- 输出 schema（canonical value）— `dsh-tool-ask-user/lib/index.js:66-90`：`{answers:[{id: string(required), selected: string[](required), custom?: string}]}`，根与 answer item 都是 `additionalProperties: false`。`render` 把该值 `JSON.stringify` 成**单个 text block**（`:91-94`），所以模型看到的是紧凑 JSON。
- `execute` 的字段映射 — `dsh-tool-ask-user/lib/index.js:96-112`：只转发 `id`/`question`/`header`/`options`/`multi_select→multiSelect`；**不转发 `detail`、不转发 `intent`**（`additionalProperties:true` 让它们能进 args，但在这里被丢）。同时转发 `exec.agent`（若存在）与 `signal: exec.signal`；返回时 `selected: [...answer.selected]`、`custom` 仅在定义时带上。
- 错误/拒绝：`execute` 不 catch；seam 抛出的 `UserQuestionError` 由工具管线规范成错误结果。README 原文列举模型会看到的失败文本 — `dsh-tool-ask-user/README.md:58-60`、`dsh-user-questions/README.md:55`。
- **没有**任何 permission / effect / read-only 分类：`DefineToolOptions` 只有 `name/description/parameters/output/timeoutMs/isConcurrencySafe/execute/presentCall/presentResult`（`dsh-tools/lib/types/schema.d.ts:178-208`），本工具只声明 `name/description/parameters/output/execute` — `dsh-tool-ask-user/lib/index.js:15-113`。`timeoutMs` 也没声明，README 明说「declares no `timeout-policy` budget; cancellation rides the turn's `exec.signal` only」— `dsh-tool-ask-user/README.md:141`。
- 挂载面：Web 插件的 **node half `apply()` 是空的**，并写明理由——在 tools registry 的 global layer 挂它会给每个 agent 的工具表加一项，而「能不能问」是 preset/组装层的事 — `dsh-client-ui-user-questions/lib/index.js:1-11`。本 install 里没有任何组装配置引用 `dsh-tool-ask-user`（全 scope grep 只命中它自己的文件，以及 UI 包 README 里的链接），所以「挂在哪些 preset」在这些产物里不可判。

### 2. seam `ctx.userQuestions`

- 请求/答复类型全文（client-safe）— `dsh-user-questions/lib/types/types.d.ts`：
  - `AskUserQuestionOption { label: string; description?: string }`（`:5-10`）
  - `AskUserQuestionIntent = { kind: 'plan-review'; approve: string }`，注释明确「intent changes presentation only, never the protocol」（`:18-27`）
  - `AskUserQuestionItem { id; question; detail?; header?; options?; multiSelect?; intent? }`（`:29-44`）
  - `AskUserQuestionAnswerItem { id; selected: string[]; custom? }`（`:46-53`）；`custom` 注释：「May accompany custom text for a multi-select question」
  - `AskUserQuestionAnswer { answers: AskUserQuestionAnswerItem[] }`（`:55-58`）
  - `AskUserQuestionRequestEvent { questions; agent?; signal?: AbortSignal }`（`:60-67`），`signal` = "Cancellation lifetime of the pending request"
  - 事件声明：`'user-questions/request'(this: Scoped<Agent>, request, next): Promise<AskUserQuestionAnswer>`，`@mode waterfall`，scope-filtered（`:68-79`）
  - 服务侧 `AskUserQuestionRequest extends AskUserQuestionRequestEvent`（`dsh-user-questions/lib/types/index.d.ts:19`）
- 验证规则与错误分类（`UserQuestionError extends HarnessError`）— `dsh-user-questions/lib/index.js`：
  - `ASK_ABORTED`："ask_user_question was aborted before the user answered"，进入 `ask()` 时 `request.signal?.aborted` 已真就立刻抛（`:21,53`），以及 waterfall 抛出后若 signal 已 abort 则改写为它（`:76`）。
  - `EMPTY_QUESTIONS`："ask_user_question requires at least one question"，条件 `request.questions.length === 0`（`:54`）。
  - `CALLER_NOT_LIVE`：给了 `agent` 时，`agents.get(agent.id) !== agent`（不是注册表里那个 live 实例）（`:58`）。
  - `DELEGATED_CALLER`：给了 agent 且该 agent 不在 `agents.roots()`（被另一个 live agent 拥有）（`:59`）。文档强调判据是**运行期所有权**，不是持久 lineage（`:39-43`、`dsh-user-questions/README.md:41`）。
  - `BAD_INTENT`（两条，逐字）：`!options.some(o => o.label === intent.approve)` → "…whose approve label … names none of its options"；`question.detail === void 0` → "…without the detail it reviews"（`:64-65`）。无 intents 时 `continue`，所以普通问题两个都不查。
  - `NO_PROVIDER`："no user-questions answerer accepted the request"，是 waterfall 的 fallback（`:67`）。
- answerer 选择（waterfall）：agentless → `ctx.waterfall("user-questions/request", request, noAnswerer)`；有 agent → `ctx.waterfall(scopeTarget(agent, agent), ..., {...request, agent}, noAnswerer)`（`:69-72`）。第一个返回值的 listener 认领；都调 `next()` 则走 fallback。无人认领 = `NO_PROVIDER`。
- 跨端错误还原：`restoreUserQuestionError` 只认 `name === "UserQuestionError"` 且 `message`/`code` 都是 string 的 wire 错误，重建成 `UserQuestionError`；否则原样抛（`:26-30,73-77`）。这意味着 UI 侧可以造出**不在服务文档taxonomy 里**的 code。
- 取消只由 `signal` 表达，没有第二个取消入口；README 另记两条限制：Web answerer 只接 Agent-scoped 请求（agentless 落到 NO_PROVIDER），且「vocabulary 只有选项 + 自定义文本」— `dsh-user-questions/README.md:41,65-66`。
- 持久化/审计：**没有**独立请求/答复事件流——"the seam publishes no independent request/answer audit stream" — `dsh-user-questions/README.md:79`。

### 3. Web 端 composer takeover

- **机制**：`conversation.composer` 是 `kind: 'chain'`、`scope: 'session'` 的槽，声明为「Selector-routed replacements for the current Session's resident composer」— `dsh-client-ui-conversation/lib/types/client/contract/slots.d.ts:162-167`。宿主用 `renderSlotChain("conversation.composer", { sessionId, session, pendingInteraction }, { fallback: composerBar, fallbackOnly: sessionId === undefined, overlay: true })` 渲染 — `dsh-client-ui-conversation/lib/client.js:14932`。
- chain 选择语义（`dsh-client-ui-renderer/lib/client.js:825-849`）：按注册序遍历，第一个 `select(ownerProps) !== null` 的 entry 当选，selector 抛异常按「放弃」处理；`overlay: true` 时 fallback **不卸载**，只是 `display:none`（`renderChainResult`，`:871-877`），elected 节点追加在其后；`fallbackOnly` 在无 session 时根本不选举（`:782`）。`fallback` 就是常驻 composer 栈（hero + `conversation.input.dock` + `conversation.composer.bar`）— `dsh-client-ui-conversation/lib/client.js:14905-14930`。
- **接管方**：`ui-user-questions` 往该槽注册一个 entry，`select: ({pendingInteraction}) => pendingInteraction instanceof PendingQuestion ? pendingInteraction : null` — `dsh-client-ui-user-questions/lib/client.js:874-879`。所以接管发生在「当前 Session 有一个 pending question 且前面没有别的 chain entry 先认领」时。
- **交接回**：`answer()` / `cancel()` / `delegate()` / `abort()` 都走 `finish`，settle 后 `answerQuestion` 的 `finally` 调 `remove()` 把它从 pending 表里撤掉（`dsh-client-ui-user-questions/lib/client.js:113-158,856-859`）；entry 的 selector 随之返回 null，链回落到常驻 composer（fallback 恢复可见）。
- **什么被替换/什么没动**：被替换的是 `data-composer-seat` 容器里的常驻 composer 内容（输入卡片）——chain 结果就渲染在那个容器内 — `dsh-client-ui-conversation/lib/client.js:14935-14941`。fallback 只被内联 `display:none` 隐藏、**仍挂载**（`dsh-client-ui-renderer/lib/client.js:871-877`），所以常驻 composer 的草稿文本不因接管而丢。会话根另有一套 `:has([data-conversation-composer-overlay])` 布局钩子（改宽度手柄与 composer seat 定位，`dsh-client-ui-conversation/lib/client.js:14652`），但本 install 里设置该属性的只有另一个 overlay 消费者 `dsh-client-ui-trajectory/lib/client.js:8112`——question takeover 会不会触发这套布局位移，产物读不出。
- **一次只有一个**：pending-interaction 表按 Session 只留一个（并列时按 `precedence` 取大，相等时后注册的胜）— `dsh-client-ui-session/lib/client.js:213-220`；README 记「One request owns the composer at a time — later pending requests remain in the session snapshot and become visible after the earlier request resolves」— `dsh-client-ui-user-questions/README.md:93`。本包给两种 presentation 注册**同一个 domain**，`precedence = plan-review ? 2 : 1` — `dsh-client-ui-user-questions/lib/client.js:873`。
- **「one takeover, two presentations」**：路由在**同一个** chain entry 内部（`QuestionComposer`），而不是两个 entry——文档给的理由是「A separate chain entry per shape would race the same carrier」— `dsh-client-ui-user-questions/lib/client.js:438-465`、`lib/types/client/QuestionComposer.d.ts:11-25`。`planReviewOf()` 的收窄条件（`lib/client.js:38-56`）：恰好 1 个问题、`intent.kind === 'plan-review'`、有 `detail`、非 `multiSelect`、options ≤ 2、且 approve label 在 options 里；否则交回 generic flow。注释点明判据是「card 必须能发出该请求允许的每一个答案」。
- **草稿存储与键**：`QuestionDraftState { requestKey?; progress: { index; drafts: {selected; custom; skipped}[] } }`，动作 `replace(draft, requestKey, progress)` / `clear(draft, requestKey)`（key 不匹配就 no-op）— `lib/types/client/draft-store.d.ts:8-35`、`lib/client.js:167-190`。它是 **Session-scoped Slot store，且 non-persisted**（注释："The store never writes them to the Host, `localStorage`, or disk"；工厂在模块里导出是为了「a plugin reload cannot reuse a module-global handle」）— `dsh-client-ui-user-questions/README.md:40,92`、`lib/types/client/draft-store.d.ts:1-5,31-35`。键是 `PendingQuestion.key = "question:" + <客户端自增序号>` — `lib/client.js:57,89-90`，文档称其为 "Opaque render identity and request key"（`lib/types/client/contract/slots.d.ts:62`）。stated reason（`QuestionComposer.d.ts:11-14`）：按 pending carrier 键控，**strict Session entry remount 能恢复同一个请求，又不会把它暴露给另一个 Session**；README 补充：切到 B 再回 A 会复用 A 的 store，而新 request identity 读到空草稿并在首次编辑时替换旧值（`:40`）。
- **推荐标签约定**：`parseRecommendedLabel(label)` 用 `/\s*(?:\((?:recommended|推荐)\)|（(?:recommended|推荐)）)\s*$/i` 匹配并剥掉后缀，返回 `{label, recommended}`；只用于**显示**（badge + `aria-label`）— `lib/client.js:388-397,649-694`。**答案值不变**：点击调 `choose(option.label)` 用的是原始 label（`:660`），选中判定 `draft.selected.includes(option.label)` 也是原始 label（`:650`），工具参数描述与 README 例子都显示 `selected` 里带着 "(Recommended)"（`dsh-tool-ask-user/lib/index.js:42`、`README.md:42,55`）。
- **交互模型**（可从代码逐条读出）：
  - 多问题分页：`index` + `drafts`；footer 有 prev/next 与 `index+1 / questions.length`（`:729-762`）；单选的 `choose` 会自动前进到下一题（`:526`）。
  - 选项是 `<button>`，单选 `role="radio"`、多选 `role="checkbox"`（多选显示勾选框，单选显示序号）；无显式方向键导航，选项上的 `onKeyDown` 只在 `Enter` **且所有题都 completed** 时提交（`:649-694`）。
  - 自由文本：`AnswerField` 是 `<textarea rows=1>` + `aria-hidden` mirror 决定高度；`Shift+Enter` 换行，`Enter`（非 IME composing）→ `continueFlow`（`:398-437,580-584`）。单选下打字会清空已选选项（`draftCustom`），多选下保留（`:571-579`）。
  - `header` 渲染成标题上方的 eyebrow；`detail` 在选项上方用同一个 `MarkdownText` 渲染（`:606-608,640-645`）。
  - 焦点：**只有无 options 的题**的 block 输入框带 `autoFocus`，且用 `focusedQuestions` ref Set 保证每题只自动聚焦一次；有 options 时 inline 自定义行不自动聚焦（`:713-724`）。卡片是 `aria-labelledby` 指向当前题标题，minimize 按钮带 `aria-expanded`（`:599-626`）。
  - 部分作答：每题可 `Skip` → `{selected: [], custom: "", skipped: true}`（`:585-595`）；`submitDrafts` 要求每题 `completed`（answered 或 skipped），否则跳回第一道缺题并报 `error.incomplete`（`:530-536`）；提交时 skipped 编码为 `{id, selected: []}`，单选带自定义文本时 `selected: []` 且 `custom` 是 trim 后的文本（`:537-549`）。primary 按钮在「本题未答」时禁用，`error.unanswered` 只会从 Enter 路径出现（`:559-563,776-781`）。
  - 取消：卡片右上 close → `pending.cancel()`，reject `"the user cancelled ask_user_question"` / code `ASK_CANCELLED`，并 `clear` 草稿（`:136-142,497-506`）。
- **i18n**：只有 `zh` 与 `en` 两本字典，命名空间 `question`；zh 是 key-set 的 source of truth（`lib/types/client/locales.d.ts:1-39`、`lib/client.js:789-826`）。14 个 key：`error.incomplete/unanswered`、`nav.prev/next/minimize/maximize/cancel`、`option.recommended`、`custom.placeholder`、`action.skip/next`、`plan.header/approve/decline/discuss`。卡片上另外用到的 `t("submit")`/`t("submitting")`/`t("copy")`/`t("markdown.footnotes")`（`:247-253,780`）**不在**本包字典里，走共享的 "common namespace" 兜底（"…in the entry namespace, then repeats it in the shared common namespace before showing the key itself"）— `dsh-client-locale/lib/client.js:1062-1063`，词条在 `:816-853`（zh）与 `:860-878`（en）。
- 失效模式（README 明写，`:40,92-93`）：草稿只有 page + Session 寿命，整页刷新 / Session 被 prune / 新 request identity 都从空草稿开始；「host remains authoritative for whether the request is pending」。

### 4. 工具调用在转录里的呈现（`dsh-client-ui-tool`）

- 注册为 `tool.call.toolview` 的 keyed entry，`key: "ask_user_question"` — `dsh-client-ui-tool/lib/client.js:1684-1694`。
- 它**从原始 wire JSON 重建**：questions 解析自 tool-call 的 args JSON（`questionEntries`，`:1570-1584`，重复 id / 缺 id / 空数组 → null），answers 解析自 result 的 text JSON（`answerEntries`，`:1553-1568`），再按 echoed id 严格配对（`pairAnswers`，数量不等或 id 缺失 → null）。配对失败时只保留计数摘要 `ask.answered {answered}/{total}`（`:1619-1629`）。
- 状态分档（`:1631-1682`）：
  - pending（结果还没到，state `running`）→ summary `ask.waiting`（zh「等待回答」/ en "waiting"）。
  - 成功 → `ask.answered`（zh「{n}/{m} 已回答」），并列出每题答案；`selected: []` 的项显示 `ask.skipped`（zh「未回答」）。
  - `code === "ASK_CANCELLED"` → summary「已取消」、state 反而标成 `ok`，正文是「本轮已取消，未提交回答」。
  - `code === "ASK_ABORTED"` → summary「已中断」、state `stopped`，正文「本轮已中断，未提交回答」。
- 这两组文案住在 conversation 命名空间，不在 `question` 命名空间 — `dsh-client-ui-conversation/lib/client.js:13713-13720`（zh）/ `:13875-13882`（en）。

### 5. plan-review 端到端（`dsh-plan-mode`）

- `exit_plan_mode` 工具**在 plan 模式外也保持注册**，进出只改 prompt section，不改工具目录 — `dsh-plan-mode/lib/index.js:21-23,229`。
- 调用 seam 的请求是硬编码的（`:259-282`）：`id: "plan-review"`、`header: "Plan review"`、`question: "Approve this plan and leave plan mode?"`、`detail: args.plan`、options = `Approve`（"Leave plan mode; the plan is carried out from the next step."）与 `Keep planning`（"Stay in plan mode; feedback goes back to the model."）、`intent: { kind: 'plan-review', approve: 'Approve' }`，并带 `agent` 与 `signal: exec.signal`。
- **approve 的验证与消费**（`:284-294`）：先按 id 过滤，必须恰好一条；判定 `item.selected.length === 1 && item.selected[0] === APPROVE_LABEL && item.custom === void 0`。任何其他形状都按「keep planning」处理，`item.custom ?? ""` 作为 feedback 进错误文本；通过后才 `pendingIntents.set(session, { active: false, narrate: false })` 并返回 canonical `{ approved: true }`（output schema `const: true`，`:237-251`）。
- **与 plan 模式自身审批流的关系**：model 那侧不能切模式——进出只由 `/plan` / `/plan off`（`set()` / `onBoundary()`，`:360-391`）；exit tool 只是「审完再排一次模式切换」，真正落流的 `plan/mode` 事件由下一次 accepted in-turn pre-step 追加。README 明说「Without a user-questions channel … the call fails closed and `/plan off` remains the manual escape」— `dsh-plan-mode/README.md:93`。
- `ASK_CANCELLED` 在这里被就地翻译：catch 到它 → 抛「The user dismissed the plan review to speak instead; stay in plan mode, stop here, and wait for their message.」— `:279-282`。此外服务被 reload（`disposed`）也会让审批失败并让模型重新呈现计划（`:283`）。
- 已知限制：只有 Web 有 `plan-review` renderer，别的 provider 走 generic option list；live child 不能开这个 review — `dsh-plan-mode/README.md:186-187`。

### 6. 其他参与同一流程的文件（及参与理由）

- `dsh-api-remotes/lib/types/remote-events.d.ts:67-68` + `lib/index.js:109-124`：`user-questions/request` 是 forwarded allowlist 里唯一的 question 事件，mode `waterfall`；转发前要求 `request.agent === carrierKeyOf(this)`，否则 TypeError。理由：这是 host 事件跨到浏览器的唯一入口。
- `dsh-scope/lib/invariant.js:37`：该事件的 scope key 取 `args[0].agent`。理由：决定「哪个 agent 的 UI 收到请求」。
- `dsh-client-ui-session/lib/client.js:160-177,213-220` 与 `lib/types/client/index.d.ts:110-117`：pending-interaction domain 注册表（`precedence` 越大越优先）；teardown 时先撤可见值，再 delegate 并 await 每个仍未决的请求。理由：takeover 的选举与撤销机制。
- `dsh-client-ui-renderer/lib/client.js:782,825-849,871-877`：chain slot 的选举与 `overlay` fallback 保留语义。理由：回答「到底替换了什么」只在这一处。
- `dsh-client-ui-conversation/lib/client.js:14905-14941` + `lib/types/client/contract/slots.d.ts:162-167`：composer 链的宿主与 fallback 组成。理由：同上。
- `dsh-client-ui-tool/lib/client.js:1553-1694`：转录面的 pending/answered/cancelled/interrupted 形态。理由：模型之外的人看到的记录。
- `dsh-tools/lib/types/schema.d.ts:178-208`、`lib/types/schema.js:68-82,238-247`：`defineTool` 能声明什么、schema 怎么编译。理由：证明该工具没有 effect/permission 分类，且根 schema 的开放性。
- `dsh-client-connection/lib/client.js:5599-5622`：测试 fixture 里 `user-questions/request` 的 waterfall 形状（`{questions: fixtureQuestions}`，无 signal）。理由：唯一一处能看到该事件在线上的样例形状。

## What the sources leave open

- **`signal` 是否真的跨 Remote 线、绑定到谁的取消**：类型里它是请求的一部分（`dsh-user-questions/lib/types/types.d.ts:66`），客户端 `PendingQuestion` 也监听它（`lib/client.js:98-107`），但 `dsh-api-remotes` 只把 `request` 原样转发，shipped fixture 的请求不含 `signal`。产物里读不出浏览器端拿到的是哪个 signal。
- **哪些 preset 挂了 `ask_user_question`**：本 install 中没有任何组装配置引用 `dsh-tool-ask-user`（全 scope grep 只命中它自己的文件与 UI 包 README 的链接）；Web 插件 node half 的 `apply()` 为空。只能确定设计意图（「belongs to the presets that include it and to the TUI composition」），读不出实际名单。
- **工具在 `exec.agent === undefined` 时的真实路径**：代码上会省略 `agent`、seam 跳过身份检查、走 unscoped waterfall；但没有任何 shipped answerer 会认领 unscoped 请求（Web answerer 在 `scopeOf(owner) === undefined` 时 `next()`，`lib/client.js:840-842`），所以实际落到 `NO_PROVIDER`——这是**读代码得出的**，没有文档直说。
- **键盘模型没有规范级描述**：选项没有方向键导航 / roving tabindex，`Enter` 的行为分散在 `onKeyDown` 与 `continueFromCustom` 两处（`lib/client.js:580-584,662-666`）；产物里没有「完整的键位表」，也没有 IME 之外的输入法/无障碍说明。
- **常驻 composer 在接管期间是否仍可聚焦**：机制上 fallback 是 `display:none` 但仍挂载（`renderer/lib/client.js:871-877`），能推出草稿文本被保留；产物没有一句把它写成契约。
- **`key` 的稳定性**：`question:<n>` 是页面内自增计数器（`lib/client.js:57,89-90`），没有从请求内容派生的稳定身份；跨页面重放时如何重建同一 key 不在产物里。
- **`header`/`detail` 在 generic flow 里的历史与优先级**：代码里两者都被渲染（`:606-608,640-645`），但没有变更记录说明它们何时进入 vocabulary。
- **seam 服务由谁注册/默认是否挂载**：只看到 `UserQuestionService` 的 class 导出（`dsh-user-questions/lib/index.js:82`），看不到 node 侧的装配点。
- **`ASK_CANCELLED` 的归属**：服务文档的 taxonomy 不含它，它由浏览器侧 `questionError` 造出并被 host 的 `restoreUserQuestionError` 还原（`lib/client.js:59-64,136-142`）。产物没说这是有意留出的「UI 私有 code」还是文档未跟上。

## 这些事实对 fs-agent 的五个分叉各自 settle 了什么

> 只做映射：每条先写 sources 已经定住的事实，再写它们没有回答、留给本仓库的问题。不写建议。

### 分叉 1：谁发起（同一个类型的第三个变体，还是自己的类型）

- **Settle 了**：DSH 把「问的通道」和「问的模型面入口」拆成两层——seam `ctx.userQuestions` 是通用服务，`ask_user_question` 只是它的一个 consumer；同一个 seam 还有一个**非模型的 first-party 调用者**（`exit_plan_mode` 直接调 `ask()`）。所以「harness 发起」与「模型发起」可以共用**请求/答复形状**，而发起者与结果去向各自独立（`dsh-tool-ask-user/lib/index.js:96-112`、`dsh-plan-mode/lib/index.js:259-282`）。
- **Settle 了**：seam 的请求自带 `agent?` 与 `signal?`，答复统一是 `{answers:[{id, selected, custom?}]}`，与发起者无关（`dsh-user-questions/lib/types/types.d.ts:46-67`）。
- **留开**：DSH **没有**「渲染器发起」的问题走这条 seam——超长粘贴之类的渲染器自有确认不在其中（`dsh-client-ui-user-questions` 只接 host waterfall）。因此 sources 不支持「三类共用一个类型」，也没有展示任何一种三合一的类型组织方式。fs-agent 的 `Question`/`Pending` 该怎么容纳第三类，没有先例可抄。

### 分叉 2：流上的形状（`--continue` 撞上挂起的问题）

- **Settle 了**：请求与答复**不另立事件流**——seam 明说没有独立审计流（`dsh-user-questions/README.md:79`）；问题正文留在 assistant 的 tool-call arguments 里，等待期间 UI 的交互**不进模型上下文**，答复只作为该调用唯一的结果回来（`dsh-tool-ask-user/README.md:124`）。「每个 `tool_call` 恰好一条结果」在 DSH 侧同样成立，只是结果可能是 `ASK_ABORTED` / `ASK_CANCELLED` / `NO_PROVIDER` 等错误。
- **Settle 了**：cancellation 只有 `signal` 一条路，挂起期间没有任何持久记录；pending 表的寿命与页面/插件一致，draft 明确不落 host / localStorage / disk（`dsh-tool-ask-user/README.md:141`、`dsh-user-questions/lib/types/types.d.ts:66`、`dsh-client-ui-user-questions/README.md:40,92`）。
- **留开**：进程死掉时挂起问题怎么办，sources 完全没有对应物——DSH 的 pending 是 UI 生命周期的值，恢复会话时不存在「未决问题」这回事，也没有 `--continue` 语义。fs-agent 的「悬空 `tool_call` 合成 unknown 结果」（spec §11）与「问题被遗弃」是否同一语义、要不要新语义，sources 不置一词。
- **留开**：DSH 的工具**没有 timeout 预算**（`dsh-tool-ask-user/README.md:141`），所以也没有「等太久自动收尾」可参考。

### 分叉 3：呈现在哪里（底部接管 vs 中段覆盖层）

- **Settle 了**：takeover 是 `conversation.composer` 这个 session-scoped **chain 槽**，fallback 是常驻 composer 栈；`overlay: true` 下 fallback 只被 `display:none` 隐藏而不卸载，header / transcript / 其他槽位不动（`dsh-client-ui-conversation/lib/client.js:14932`、`dsh-client-ui-renderer/lib/client.js:782,871-877`）。接管何时成立、何时交回由「该 Session 有没有 pending interaction + 链上谁先 `select` 成功」决定（`lib/client.js:874-879`、`renderer/lib/client.js:825-849`）。
- **Settle 了**：同一时刻每个 Session 只有一个有效的 pending interaction，按 `precedence` 择一；后到的请求留在 snapshot 里、等前面的结束才可见（`dsh-client-ui-session/lib/client.js:213-220`、`dsh-client-ui-user-questions/README.md:93`）。
- **Settle 了**：两种 presentation（generic 与 plan-review）不走两个槽/两个 entry，而是在同一个 entry 内路由，理由写明了是「两个 entry 会 race 同一个 carrier」（`lib/client.js:438-465`）。
- **留开**：sources 里**不存在**中段覆盖层这种问法，因此没有「什么时候该用接管、什么时候该用覆盖层」的比较材料。fs-agent 现有的四种问题都在中段覆盖层，且票 30 正 open 地讲那个覆盖层的形态——把哪些问题下移、是否统一，sources 不回答，也不该被它替答。

### 分叉 4：非交互渲染器（`--plain` / headless）

- **Settle 了**：**没有 answerer 认领 = 工具调用以错误结果结束**，不是挂住：waterfall fallback 抛 `NO_PROVIDER`，README 说「Without one, the tool call fails with an error instead of degrading」，模型看到的是错误文本（`dsh-user-questions/lib/index.js:67`、`dsh-tool-ask-user/README.md:28`、`dsh-user-questions/README.md:55`）。
- **Settle 了**：谁**不能**答是有明确规则的——只有「注册表里那个 exact live 实例」且是 runtime root 的 agent 才能问（`CALLER_NOT_LIVE` / `DELEGATED_CALLER`，`dsh-user-questions/lib/index.js:56-60`）；Web answerer 还会主动放弃 agentless 请求（`dsh-client-ui-user-questions/lib/client.js:840-842`）。
- **Settle 了**：「根本不挂进工具表」在 DSH 自己是**被采用**的一条路：Web 插件的 node half 故意不全局挂工具，把「有没有这个工具」留给 preset/组装层（`dsh-client-ui-user-questions/lib/index.js:1-11`）。
- **留开**：sources 里没有 plain / headless 渲染器，也没有任何「自动作答」或「替用户选一个」的先例——失败（错误结果）与不挂载，是产物里仅有的两种降级形态。fs-agent 的「绝不挂住」传统要在「报错 / 自动答 / 不挂」之间怎么选，sources 不决定。

### 分叉 5：v1 收多少（逐特性：协议要求 vs 纯呈现）

- `questions`（≥1，成批）：**协议要求**——参数 required，seam 对空数组抛 `EMPTY_QUESTIONS`（`dsh-tool-ask-user/lib/index.js:18-21`、`dsh-user-questions/lib/index.js:54`）。单问题只是调用者惯例（plan-review 就是一批一问）。
- `id`：**协议要求**，答复按它 echo（工具参数 required、output schema required；`dsh-user-questions/lib/types/types.d.ts:29-33,46-53`）。**重复 id 的拒绝不在 seam**，而在两处消费者：client toolview 配对时判 null（`dsh-client-ui-tool/lib/client.js:1570-1584`）、plan-mode 按 id 过滤后要求恰好一条（`dsh-plan-mode/lib/index.js:284-286`）。
- `question`：**协议要求**（参数 required）。
- `selected`：**答复里协议要求**，且**可以为空**——skip 的编码就是 `{id, selected: []}`（`dsh-client-ui-user-questions/lib/client.js:537-549`、`dsh-user-questions/README.md:39`）。
- `custom`：**协议可选**；语义随 `multiSelect` 变——单选下「overrides the selected choice and `selected` is empty」，多选下「may supplement the labels in `selected`」（`dsh-user-questions/README.md:39`、`dsh-client-ui-user-questions/lib/client.js:543-548`）。工具 output schema 里 `custom` 不 required（`dsh-tool-ask-user/lib/index.js:86`）。
- `options` / `option.label` / `option.description`：**协议可选**（`label` 若给 option 就 required，`description` 纯呈现）。无 options 时 UI 退化成只有一个自由文本框（`dsh-client-ui-user-questions/lib/client.js:713-724`）。
- `multiSelect`：**协议可选，默认 false**（`dsh-tool-ask-user/lib/index.js:59-62`、`dsh-user-questions/lib/types/types.d.ts:40-41`）；它同时改变 UI 控件与答案编码（`dsh-client-ui-user-questions/lib/client.js:511-527,537-549`）。
- `header`：**纯呈现**，optional；UI 渲染成 eyebrow，不参与任何校验（`dsh-client-ui-user-questions/lib/client.js:606-608`）。
- `detail`：**协议可选、呈现为主**，但**不是纯呈现**：intent 问题缺 detail 会被 `ask()` 拒为 `BAD_INTENT`（`dsh-user-questions/lib/index.js:65`）。另外 `ask_user_question` 的参数 schema **根本没有 `detail`**，所以它只对 first-party 调用者（`exit_plan_mode`）可达（`dsh-tool-ask-user/lib/index.js:25-63` 对比 `dsh-plan-mode/lib/index.js:264`）。
- 草稿/分页/已跳过态：**纯呈现**，全在客户端非持久 store 里，协议上不可见（`dsh-client-ui-user-questions/lib/types/client/draft-store.d.ts`、`dsh-client-ui-user-questions/README.md:40,92`）。
- 推荐标签 `(Recommended)`：**纯约定**，协议零支持；`parseRecommendedLabel` 只剥显示标签，答案值保留原串（`dsh-client-ui-user-questions/lib/client.js:388-397,650,660`、`dsh-tool-ask-user/lib/index.js:42`）。
- `intent`：类型上 **optional 且声明为「presentation only, never the protocol」**（`dsh-user-questions/lib/types/types.d.ts:11-27`），但 `ask()` 对已声明 intent 施加两条结构校验（`approve` 命中自身 option、必须有 `detail`，`dsh-user-questions/lib/index.js:61-66`）。`ask_user_question` 同样不暴露 `intent`。
- **协议与具体 UI 不一致的几处**：
  - `intent` 协议上允许任意 option 数，但 plan-review 卡只在「≤2 options 且非 multiSelect」时认领，否则回落 generic（`dsh-client-ui-user-questions/lib/client.js:38-56`）。
  - output schema 是闭合的（`additionalProperties:false`），参数 schema 的问题/选项对象是开放的（`additionalProperties:true`）——模型多给的键会被静默丢弃（`dsh-tool-ask-user/lib/index.js:22-24,44-45,66-90,96-112`）。
  - seam 允许「一批里某题 `selected: []`」，而 plan-mode 的消费端要求 approve 时恰好一个 `selected` 且 `custom === undefined`（`dsh-plan-mode/lib/index.js:286`）。
  - `header`、`detail`、`description` 在 transcript 侧不参与渲染——`dsh-client-ui-tool` 只从 args 里取 `id`/`question`，其余一律不显示（`dsh-client-ui-tool/lib/client.js:1578-1582`）。
  - plan-review 的 approve label 是宿主常量 `"Approve"`（`dsh-plan-mode/lib/index.js:38`），UI 按钮文案走 `plan.approve` 字典（zh「确认执行」/ en "Approve"，`dsh-client-ui-user-questions/lib/client.js:805,823`）；答案值用的是**宿主常量**而不是界面文案——即 presentation 与协议值可以不同字面。
