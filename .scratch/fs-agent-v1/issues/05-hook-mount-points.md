# 05: hook 挂载点

**What to build:** 用户能在工具调用前后挂 hook 加自己的策略与反馈，而「hook 放松权限」在类型上不可能——前置 hook 输出的是**约束**，与门的裁决在同一个决策格上取上确界。

Blocked by: 04

Status: ready-for-agent

**参考:** spec §3（控制流次序与失败语义）、§5（投影合并）、原型 `prototype/05-hook-permission-ordering.html`

- [ ] 前置输出是约束：`Continue | Rewrite(args) | Tighten(Ask|Deny) | Skip | Stop`；**类型里没有放松变体**
- [ ] 生效裁决 = 约束与门裁决在 `Allow < Ask < Deny` 上取**上确界**（只有一套合并语义）
- [ ] 前置发生在询问**之前**：能阻止询问发生，永远不能绕过询问；v1 **不设** `PermissionRequest` 挂载点
- [ ] 失败语义不对称：前置失败 / 超时 → **fail-closed**（阻止动作 + 诊断 + 合成错误结果）；后置失败 → **只丢反馈**
- [ ] PostToolUse 回灌 = 一条追加的 `HookExecuted`，由**投影**合并进那条工具消息（provider 只允许一个 `tool_call` 对一条 tool 消息）；可见性跟着它所评注的那条工具结果走
- [ ] hook 只拿到**封闭的公开子集**（工具 + 权限 + 会话边界，7 个变体）
- [ ] `hook.pre` 失败这条异常路径也合成结果 ⇒ 与正常路径、权限拒绝、用户拒绝、无交互降级合起来，每个 `tool_call` 仍恰好一条结果
- [ ] 原型 `05-hook-permission-ordering.html` 里那 7 个剧本**逐个**变成测试用例
