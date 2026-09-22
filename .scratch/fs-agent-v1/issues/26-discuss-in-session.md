# 26: `/discuss` —— 在活会话上讨论

**What to build:** 票 24 只给了 `fs-agent discuss`：一次讨论 = 一次组装 = 一个**自己的**会话。用户在 TUI 里没法发起讨论，只能 `/quit` 再跑子命令，而且讨论拿不到当前会话的上下文。这一票把讨论接到**当前这条流**上：`/discuss [问题]`，讨论者是当前会话的**兄弟会话**。

Blocked by: 24（讨论的 CLI 入口）、25（roster 允许同厂商/同模型）

Status: done

**参考:** spec §5（发言归属与投影）、§15（讨论协议）、§19（渲染与 CLI 组装）、`docs/discussion.md`

- [x] **`Session::fork`**：同一会话流的兄弟会话——共享 log / 工具表 / 写锁 / 权限策略 / 回答口 / 钩子 / home / 技能 / 渲染器，只有模型与私有身份不同；**读集不继承**（读权限是每个 agent 自己的）
- [x] **`Harness::discuss(question, debaters, synthesizer, max_rounds)`**：在活会话上跑一场讨论；不写骨架、不恢复悬空调用（这条流的 `SessionStarted` 已经在头上）
- [x] **roster 校验只有一份**：`validate_roster` / `discussion_participants` 被 `assemble_discussion` 与 `Harness::discuss` 共用，不存在「一条路接受、另一条路拒绝」
- [x] **上下文继承**：投影把本会话的回合变成讨论者窗口里的 `user`（§5），所以讨论的是「我们正在聊的东西」；集成测试直接断言讨论者的**实发请求**里能看到会话先前的问题与回答
- [x] **轮次号在一次会话内唯一**：`discussion::last_round` 让新讨论接在流上已有轮次之后（第二次 `/discuss` 从第 4 轮起），`round_attendance` / `sessions show --round N` 因此能分辨是哪一场
- [x] **合成器的材料按阶段切**：`discussion::debate_phase_start`（纯函数）——否则第二次讨论的合成器会拿到第一次的作答；**`sessions replay` 调同一个函数**，实发与重算不会漂
- [x] **`/discuss [问题]`**：问句形式与 `/技能名 任务` 同形（首行余下 + 后续行，共用 `task_of`）；**不带问题**就用本会话最后一个用户问题（`Harness::last_question`），一个会话里还没问过任何东西时明说没有题目
- [x] **诊断走渲染器**：roster / 题目 / 结束原因都经 `harness.notice` 进转录（TUI 独占屏幕，不能写 stderr）；Esc 取消讨论、第二下退出（130）
- [x] **`/` 补全菜单列出 `discuss`**（内置命令表加一项），`--help` 与 README 说明用法
- [x] **测试**：活会话继承上下文（断言实发 messages）、同一会话两场讨论的轮次号与合成材料、replay == 实发（字节相等）、`/discuss` 解析、文案
- [x] **端到端**：假 provider 起一个真会话，跑「一轮对话 → `/discuss`（2 轮 + 合成）→ `/discuss 换角度`（2 轮 + 合成）→ `/quit`」，`sessions show` 读出第 1–6 轮，`sessions replay --speaker system --round 6` 与实发提示词逐字节相同

## Comments

**2026-09-22（实现）** 三条设计取舍：

1. **兄弟会话，而不是把讨论「塞进」当前 Session。** 讨论的每个参与者都需要自己的 `SessionConfig` 与私有身份，而 `Session` 恰好就是「会话级共享 + agent 私有」的那个分层；`fork` 只是把它显式化。渲染器也共用：一个进程一个渲染器（spec §19）不破。
2. **轮次号改成「会话内唯一」，不是「每次讨论从 1 起」。** 共享一条流之后，「同名轮次」会让 `round_attendance` 把两场讨论混在一起；而轮次号是流上的事实，让它在会话内唯一比让每个查询都带「哪一场」更省事。一次性讨论（自己的会话）偏移为 0，行为不变。
3. **合成器材料按「辩论阶段」切，而不是按「这一轮的序号范围」。** 取消掉的讨论没有合成轮，不能只靠序列号连续与否判断边界；`debate_phase_start` 的判据是「结束过一轮辩论、且后面还有辩论轮」，live 与 replay 共用。

**代价（已知）**：同一个会话的转录会混着单 agent 回合与讨论轮次（这是「继承上下文」的另一面，用户选的正是这个）；`sessions stats` 的逐发言人会把四种身份（本 agent、讨论者 A/B、system）并排列出。

**没做的事**：没有做「讨论完自动回到提问前的位置」之类的手势；没有让 `/discuss` 换渲染器（`--plain` 的会话里讨论也是 plain）。两者都不需要——讨论只是同一会话里的一件事。

**2026-09-22（后续）** 票 27 把 roster 换成人物池：`/discuss --debaters A,B` 可以指定抽哪两位，不写则随机抽两个（`discussion::pick_pair`）。
