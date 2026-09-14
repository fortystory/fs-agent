# 05: hook 挂载点

**What to build:** 用户能在工具调用前后挂 hook 加自己的策略与反馈，而「hook 放松权限」在类型上不可能——前置 hook 输出的是**约束**，与门的裁决在同一个决策格上取上确界。

Blocked by: 04

Status: done

**参考:** spec §3（控制流次序与失败语义）、§5（投影合并）、原型 `prototype/05-hook-permission-ordering.html`

- [x] 前置输出是约束：`Continue | Rewrite(args) | Tighten(Ask|Deny) | Skip | Stop`；**类型里没有放松变体**
- [x] 生效裁决 = 约束与门裁决在 `Allow < Ask < Deny` 上取**上确界**（只有一套合并语义）
- [x] 前置发生在询问**之前**：能阻止询问发生，永远不能绕过询问；v1 **不设** `PermissionRequest` 挂载点
- [x] 失败语义不对称：前置失败 / 超时 → **fail-closed**（阻止动作 + 诊断 + 合成错误结果）；后置失败 → **只丢反馈**
- [x] PostToolUse 回灌 = 一条追加的 `HookExecuted`，由**投影**合并进那条工具消息（provider 只允许一个 `tool_call` 对一条 tool 消息）；可见性跟着它所评注的那条工具结果走
- [x] hook 只拿到**封闭的公开子集**（工具 + 权限 + 会话边界，7 个变体）
- [x] `hook.pre` 失败这条异常路径也合成结果 ⇒ 与正常路径、权限拒绝、用户拒绝、无交互降级合起来，每个 `tool_call` 仍恰好一条结果
- [x] 原型 `05-hook-permission-ordering.html` 里那 7 个剧本**逐个**变成测试用例

## Comments

实现落点：`src/hooks.rs`（约束 / 收紧 / 公开子集 / `Hook` trait）、`src/events.rs`（`hook_format` 文本约定）、`src/agent.rs`（循环里的两个挂载点与合成结果）、`src/provider/projection.rs`（后置反馈合并）、`src/session.rs` / `src/lib.rs`（`hook` 注入）。测试在 `tests/hook_constraints.rs`（纯函数：上确界、无放松、7 变体闭合）与 `tests/hook_mount_points.rs`（组装接缝：7 个剧本 + 异常路径）。

实现期收口、把票面留白写实的几处：

1. **挂载点的形状 = trait + typed enum**（地图「Rust 生态事实」明确：不是 channel）。`Hook` 是一个注入端口，和 `Asker` 同规格：`Session` / `AssemblyParts` 各多一个 `Option<Arc<dyn Hook>>`，嵌套执行者共享同一个值（与 asker 同理，策略不因派子 agent 而丢失）。**给 hook 的命令行/`config.toml` 声明与子进程 runner 仍无票**（`HookExecuted.command` 先由 trait 的 `command()` 自报身份）；这是明确留给后续票的部分，不是本票偷偷落下的。
2. **公开子集取决策票的 7 个变体**：`SessionStarted` / `SessionEnded` / `ToolCallStarted` / `ToolCallCompleted` / `PermissionAsked` / `PermissionDecided` / `AgentError`。它**不含 `HookExecuted`**（hook 看不到别的 hook），实现成一个独立 `HookEvent` 枚举 + `from_payload` 的 7 个 `Some` 臂，`_ => None` 兜底——将来加事件不会默认泄进 hook 的观察面。
3. **`outcome` 的文本约定住在 `events::hook_format`，不放在 `hooks`。** 原因是一条真实依赖边：`tools` 依赖 `provider`（`ToolSpec`），而 `hooks` 需要 `tools::Effect`，所以 `provider::projection` 若 import `hooks` 就成环；`events` 是零内部依赖的底部，生产（`hooks` / `agent`）与解析（`projection` / 票 19）共用同一组常量与 `feedback_text()`（spec §18 的「渲染与解析共用一个格式常量」）。
4. **`Skip` 合成的是错误结果**（`ok: false`）。原型剧本 7 把它画成 `ok: true` 的「hook 跳过了执行」，但 spec §3 把「权限拒绝 / 用户拒绝 / hook 跳过 / `hook.pre` 失败」并列为各合成一条**错误结果**——spec 是折叠后的决定，按 spec 实现，测试也按 spec 断言。`Stop` 同理（见 6）。
5. **`hook.pre` 失败只落 `HookExecuted{failed}` + 渲染诊断 + 合成错误结果，不追加 `SessionError`。** spec §3 写的是「阻止动作 + 诊断 + 合成错误结果」，而 `SessionError` 在 §2 的定位是「会话级运行失败」；决策票原型里那条 `SessionError` 是原型期的写法。失败**在流上是可查的**（`HookExecuted.outcome = "failed: …"`，票 19 的 hook 指标就按它分组），渲染诊断只负责给人看。turn **继续**（决策票第 4 节：写坏一个 hook 不该把会话打死），模型看到那条错误结果后可以纠正。
6. **`Stop` 仍然守住不变量 1。** 它已经发了 `ToolCallStarted`，所以在 `TurnEnded{Aborted}` 之前先给这次调用合成一条错误结果；同批里尚未 `ToolCallStarted` 的调用不算欠结果（投影也只从 `ToolCallStarted` 重建工具调用，线级配对因此自洽）。
7. **后置 hook 在 `ToolCallCompleted` 入流之后才跑**，这是 spec §3 明确否掉「先跑 post 再把反馈并进结果」的原因：结果先对渲染器可见，挂住的 hook 藏不住它。
8. **合并规则是位置的，不是按 id 的**：`HookExecuted` 没有 `tool_call_id`，而 `hook.post` 恒紧跟在它所评注的那条结果之后，所以投影把 `point = post_tool_use` 且带 `feedback:` 前缀的 `outcome` 追加到**最近一条** own tool 消息；`failed:` 前缀的 outcome 被丢掉（这就是「后置失败只丢反馈」在模型侧也成立的原因）。
9. **`Rewrite` 在门之前重解析调用事实**（`Registry::facts`），所以门与工具看到的是改写后的参数，而 `ToolCallStarted.args` 记的仍是模型原始发出的调用——事件记录的是模型的动作，hook 的改写是 harness 的介入。
10. **`DecisionSource::Hook` 只在 hook 严格抬高了门的裁决时出现**（`tighten > verdict.decision`）；门已经是 `Deny`、hook 也收紧到 `Deny` 时来源仍是 `Policy`。hook 收紧到 `Ask` 而用户批准 → `source = User`；无交互降级 → `source = Policy`，理由里带上「hook 把它推到询问」这句。

**明确留给后续票的**：hook 的声明语法（`config.toml`）、子进程 runner 与超时阈值、stdin/stdout 的 JSON 编码/解析、以及 CLI 的挂载——本票只落「挂载点 + 约束代数 + 失败语义 + 投影合并」。**没有 `PermissionRequest` 挂载点**是有意的（决策票第 5 节：与 pre 点表达力重复）。

