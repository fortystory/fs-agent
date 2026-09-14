# 13: 取消传播

**What to build:** 一次取消手势干净地停下当前回合，并把取消**向下**传到正在跑的执行者——不留悬空状态、不把讨论误判成失败。

Blocked by: 11

Status: ready-for-agent

**参考:** spec §6（取消传播，spec 期新增的唯一决定）、§3（第 5 条异常路径）

- [ ] 手势本身**不进事件流**（流只记录 agent 的动作，与 `/undo` 写回、`/plan` 覆盖同规矩）
- [ ] Esc 中断在飞的 provider 流与在跑的工具；当前 Turn 以**既有的** `TurnEnded { reason: Aborted }` 收尾
- [ ] 在飞的 `tool_call` 由循环合成一条错误结果——这是 **第 5 条**异常路径 ⇒「每个 `tool_call` 恰好一条结果」仍成立
- [ ] 取消**只向下、不向父**传播：在跑的执行者各自以 `ExecutorFinished { reason: Aborted }` 结束并回一条错误内容的工具结果；讨论**不**因此以错误收尾
- [ ] 取消过程中再按一次 = 强退进程；随后 `--continue` 能正常恢复（走悬空 `tool_call` 那条合成路径）
- [ ] e2e：假 provider 卡住流 → 触发取消 → 断言事件流形状（`Aborted` 收尾 + 合成结果）与执行者收尾
