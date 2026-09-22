# 29: `Ctrl-C` 退不出去（TUI 的 busy 不再从事件流猜）

**What to build:** 用户报告：触发讨论后 `Ctrl-C` 退不出去。根因不是讨论本身，而是 TUI 的 `busy` 是**从渲染事件流猜出来的**，而合成器的那一次调用不产生 `TurnEnded`。

Blocked by: 26（`/discuss` 在活会话上）、28（人物注入）

Status: done

**参考:** spec §6（取消）、§19（渲染与 CLI 组装）、§15（讨论协议）

- [x] **根因**：`busy` 只在 `Block::TurnEnded` / `SessionEnded` 上清零，而合成器（`run_single_shot`）只发 `Delta` / `Message` / `Usage`，**没有回合边界** ⇒ 讨论结束后 `busy` 永远是 true
- [x] **症状**：`Ctrl-C` 因此走「取消手势」分支，而空闲的 `interactive_loop` 对内层 select 里的 `Cancel` 是 `=> {}`（丢弃）⇒ 按键被吞，进程既不退出也不取消
- [x] **修法**：`busy` 改成**从循环自己的状态派生**——`prompt_reply.is_none()`。循环「准备好要一行时才要一行」（spec §6），所以「有一个未回复的 prompt 请求」就是空闲；提示行（viewer 模式）本来就用同一个判据，现在两者不可能再打架
- [x] **删掉**：`TuiState::busy` 字段，以及 `apply()` 里那两处置位/清零
- [x] **测试**：`an_idle_ctrl_c_quits_and_a_working_one_cancels` 改用「有 prompt 在飞」表示空闲；新增 `a_run_that_never_ended_a_turn_still_leaves_ctrl_c_quitting`（旧逻辑下必红）；`escape_answers_a_question_with_the_non_acting_choice` 与三个 idle-UI 布局测试改用新的 `idle()` 辅助函数
- [x] **端到端（pty）**：起真 TUI → 打 `/discuss --debaters 张三,李四 …` → 讨论画面上出现「第 1 轮 / 张三 / 李四」→ 第一个 `^C` 让讨论以「已取消」收尾 → 第二个 `^C` 让进程以 0 退出

## Comments

**2026-09-22（实现）** 这条 bug 是「用不完整的事件清单去猜一个状态」的必然结果：`busy` 要回答的是「循环现在在不在跑东西」，而那个事实只有循环自己知道；事件流能表达的只是「某次调用开始了」（`Delta`/`Tool`），并没有「一次调用结束了」的通用事件——回合有 `TurnEnded`，合成器的单发调用没有。改成派生之后，这类缺口不会再出现（新增非回合调用也不会再破坏 Ctrl-C）。

**顺带发现（未改，留给需要时）**：一次运行被取消时，如果当时有权限询问弹窗开着，弹窗不会被撤下——它会一直显示到用户按任意键（那个键会回答一个已经没有接收方的询问，静默失败，然后草稿恢复）。`Ctrl-C` 不受影响（它在弹窗之前处理，空闲时直接退出）。要修的话得让「这次运行的回合结束」和「问题属于哪次运行」对上，属于另一张票。

**2026-09-22（后续，`/code-review`）** 审查在**修法本身**上找到一处缺口，已改：

`busy = prompt_reply.is_none()` 仍然是一次**推断**，只是换了个信号。它的补集是「循环正在读一行」，而不是「循环正在跑东西」——两者在**循环问出第一行之前**不相等，而那正是整个组装期，此时 TUI 已经在 raw mode 里收键了。实测（pty 探针，启动后各发一次 `Ctrl-C`）：`0.020s` 时按 SIGINT 正常结束，**`0.022s` 时卡住不动**（raw mode 已开、prompt 未发 ⇒ 键被空闲循环丢弃），`0.039s` 起才正常退出。这个 repo 上窗口只有 ~30ms（组装快），`--continue` 的大日志或大仓库会拉长它。

改法是这一票自己那条原则的完整版——**让循环说，别让前端猜**：`ConsoleRequest::RunState { running }`，循环在「取到一行」时置真、「回到等一行」时置假，一次性 `discuss` 在前台起来时直接置真（它一生都不读行）。`TuiState` 里 `running` 初值 `false`，`busy()` 直接返回它。同一个信号也让状态词不再在组装期谎称「工作中」。

**测试账目的更正**：这一票正文里那条「端到端（pty）」的实际凭据不是 pty 脚本——`scripts/tui-startup-check.py` 的手势只有裸会话的 `ctrl-c` 与 `/quit`，从不输入 `/discuss`，所以它证不了讨论里的取消。真正的红绿凭据是接缝测试（`tests/render_tui.rs`：`ctrl_c_quits_before_the_loop_has_asked_for_its_first_line` 等三条，加 `tests/render_layout.rs` 的 `idle()`），加上本次那次一次性探针。三条旧测试原本是**因为「没有 prompt 在飞」才通过**的（它们预先 apply 一个 `Delta` 以为那就是「在飞」，而新谓词根本不看 Delta），现已改成显式调用 `RunState`。

**「顺带发现」的去处**：已补记成**票 31**。留在评论区与本仓库的既有先例不符——tui-layout 票 12 修掉了完全同类的问题（丢掉 reply sender ⇒ 询问静默变成拒绝）并加了守卫。
