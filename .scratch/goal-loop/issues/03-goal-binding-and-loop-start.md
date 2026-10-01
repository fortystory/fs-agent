# 归属：`GoalSelected` 与 `/loop` 的启动

Type: implement
Status: ready-for-agent
Blocked by: 01

> 规格：`.scratch/goal-loop/spec.md` §4（归属与 `/loop` 的启动）。
> 这一票只让「会话属于哪个目标」成为流上的一条事实，并让 `/loop <名字>` 成为一个真命令 —— 连续工作的循环体是票 05/07/08 的事。

## 目标

- 归属落成一条只追加的事件；
- `/loop <名字>` 能选定目标并跑起来，三种边界各自拒绝。

## 落点

- `src/events.rs`：新 payload `GoalSelected`、`kind()`、打码穷尽匹配
- `src/cli.rs`：`/loop` 进 `Submission` 表
- 循环侧：`/loop` 的执行路径
- `tests/goal_loop.rs`（新增）

## 具体行为

1. **`EventPayload::GoalSelected { goal: String }`**：只追加。**当前目标 = 流上最后一条**（派生量，不存状态）—— 于是 `--continue` 后自然重建，与 `todo` 同一条纪律。
   - **不动 `SessionStarted`**：不加字段，也不涨 `SCHEMA_VERSION`。
   - `goal` 是标识符（名字），不是散文；`kind()` 加一行，`redact` 的穷尽匹配要覆盖它（它是键，**不打码**）。
2. **`/loop <名字>`** 进 `Submission`（`src/cli.rs` 那张表，当前有 `/quit` `/exit` `/undo` `/discuss`）；`/loop` 带一个参数，形状与 `/discuss` 的「剩下的话都是参数」一致。
3. **三种启动边界各自拒绝，各说各的人话**（在写任何事件之前判）：
   - 清单文件不存在 → 说找不到，提示先 `/goal new`；
   - 目标已全部完成 → 说没活可干；
   - 本会话已经有一个 loop 在跑 → 说正在跑。
4. **通过后**：写一条 `GoalSelected { goal }`，然后把循环跑起来。这一票的循环体只需要「跑起来、转录看得见、能被打断」，真正的完成判据、翻页、预算是后续票。
5. **运行期间输入区禁用**（状态机在这一票立起来，UI 是票 06）。
6. **重放**：`sessions replay` 与 `--continue` 都从流派生当前目标，不需要别的地方存它。

## 测试

- 三种边界各自的拒绝文本（各一条用例）；
- `/loop foo` 之后流上**恰好一条** `GoalSelected`，且重放算出的当前目标是 `foo`；
- 再 `/loop bar` → 两条，当前目标是 `bar`（切换，不是绑死）；
- `/loop` 在没有任何目标时拒绝；
- `kind()` 稳定短名；打码不碰 `goal`。

## 不做什么

- 不做完成判据（票 05）、不做阈值与翻页（票 07/08）、不做预算（票 10）。
- 不碰 `SessionStarted` 的形状。
- 不做 `/goal` 的其它子命令。
