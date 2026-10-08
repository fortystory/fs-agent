# 03 — 循环侧：候选清单、忙闲判定与两条新命令

Type: implement
Status: done
Part of: ../spec.md
Blocked by: 02

循环拿到「模型候选」与「档位候选」，判忙闲，然后推一条 `Picker` 给前端、或者拒绝并给回执。
`/model` 与 `/effort` 两条命令也在这一票里落地。

规格见 [`spec.md` §6、§8、§9](../spec.md)。

## 要做什么

### `ConsoleRequest` 加两格

`src/render/input.rs`：

```rust
/// 一次「从这份清单里挑一个」的问话，答 `None` = 取消。
Picker(PickerRequest),

/// 换完模型/档位之后循环推回来的新事实（§3）。
SessionUpdate { model, effort, context_window, speakers },
```

`PickerRequest { title: String, options: Vec<PickerOption>, reply: oneshot::Sender<Option<usize>> }`，
`PickerOption { label, detail, current: bool, enabled: bool }` —— 与 `Ask` 同形（那个也是
`reply: oneshot::Sender<Answer>`）。

`SessionUpdate` 是**发完就完**的通知，与 `Muted` / `RunState` 同一格，不等答案。

`spawn_plain_console_with` 里两格都要有分支：`Picker` 回 `None`（那条路径上没有选择器，
见 spec「明确不做」），`SessionUpdate` 忽略（plain 没有状态行）。

### 候选清单

模型候选（spec §8）：`config.models` 的全部键，每个带一条 `detail`：

- 缺 key 的：`enabled: false`，`detail` 点名那个环境变量（`profile.key_env` 就是修法，
  `ProviderProfile` 的文档注释明写这一点）。
- 缺 key 但那是 `KeySource::Missing` 之外的形状（`config.toml` 里直接写了 key）→ `enabled: true`。
- 当前那个：`current: true`。

档位候选：`caps_for(当前模型).reasoning_efforts` 按强度排序，**前面加一档 `默认`**
（`effort: None`，spec §5）。表是空的（`MiniMax-M3`、`kimi-for-coding-highspeed`）时给一个空清单
—— 前端那一段显示 `固定`，点它给一句说明。

### 忙闲判定

判据在循环侧，`interactive_loop` 的 `tokio::select!` 里 `events.recv()` 那一支（`cli.rs:1232`
旁边）。已经有一个 `loop_running` 与 `console.set_running()`，所以判据现成：

- 运行中 → `harness.notice(wording::switch_busy())`，不发 `Picker`。
- 空闲 → 发 `Picker`，等答案。

### 两条命令

`wording::BUILT_IN_COMMANDS` 加两条（数组长度 7 → 9）：

```
model   换这个会话用的模型：`/model <model id>`
effort  换思考强度：`/effort <low|medium|high|xhigh|max|默认>`
```

`Submission` 的解析：`/model <id>` 与 `/effort <档>` 都**必须带参数**，不带是用法错误
（与 `parse_goal_new_line` 缺参数是 `Err(())` 然后给一句用法同形）。理由见 spec「明确不做」：
选��器是 TUI 独占的交互，plain 拿不到键盘，所以命令这一路必须能一步到位。

- `/model <未知 id>` → 用现有的 `caps_for` 报错（`UnknownModel` 的 Display 已经把已知 id 列全了），
  外面套一句 `heng: `。
- `/effort <当前模型不认的档>` → 一句说清「`<model>` 没有这一档」，并列出它有的那几档。
- `/effort 默认` → `None`。

## 测试

- `tests/render_console.rs`：plain 前端收到 `Picker` 时回 `None`；`SessionUpdate` 被忽略；
  三个既有的通知型分支不受影响。
- `src/cli.rs` 的 `mod tests`：`the_menu_covers_exactly_the_commands_the_loop_parses` 与
  `the_menu_lists_the_built_in_commands_before_the_skills` 两条断言自动覆盖新命令（它们是拿
  `BUILT_IN_COMMANDS` 与 `submission` 对照的）。补一条：一条 `/model` 开头的普通消息里夹着
  `/model` 记号仍被解析成命令（`input-tokens` 票 04 那条「命令可以在任何位置」的纪律）。
- 候选清单：模型候选按 `config.models` 的键排序（`BTreeMap` 的序，不是插入序），缺 key 的那个
  `enabled` 为假且 `detail` 点名了环境变量；档位候选前面是 `默认`。
- 忙闲：`RunState.running` 为真时收到切换请求 → 没有 `Picker` 发出去，且有一句回执。

## 评论

2026-10-08 实现完毕，`Status: done`。

- 忙闲判据就是**各自分派点的处境**：`interactive_loop` 那一支（回合之间）空闲，开清单；
  `run_one_turn` 与 `discuss_in_session` 那一支运行中，给「这一回合跑完再切」。于是不需要跨函数
  共享一个「在不在跑」的状态，而判据仍然在循环侧。
- 讨论 CLI 那条路径（`run_discussion`）给的**不是**忙闲那句，而是「讨论里的两位是配置事实，
  不在会话中途换」—— 那里换的判据是 `SessionFacts::switchable`（组装期定），运行中那一格另有分支。
- 三处运行中的回执都要穿过被 run future 借走的 harness，所以给 `Harness` /
  `DiscussionHarness` 各加了一个 `render_handle()`（在借用之前克隆，与 `ModeCycle` 同一个理由）。
- 档位候选：表为空时给**空清单**（前端那一格显示 `固定`）—— 一个只有「默认」可选的清单会读成
  「有东西可切」。
- 模型候选：`config.models` 的键序（`BTreeMap`）；缺 key 的 `enabled: false` 且 detail 点名
  `profile.key_env`。
- `/model` 与 `/effort` 的参数形状照 `/goal-new` / `/loop`：**记号旁边那一整段**。所以
  `先看看这个 /model kimi-k3` 被解析成 `Model("先看看这个 kimi-k3")` —— 与其余内建命令同形，
  而它会组不出 provider 并把那句话说清楚。

### 维护者的一处收窄（2026-10-08，实现之后）

候选从「`config.models` 的全部键、缺 key 的标灰并点名环境变量」收窄成「**只列已经有密钥的
那些**」，同日再收窄成「**`config.toml` 里显式配置过、且有密钥**，外加当前模型兜底」—— 理由与
来由见票 05 那一节（内建 profile 是代码里的常量，所以「注释掉一段就当它不存在」只能在选择器这一层
按配置事实判）。`enabled` 与 `model_detail_missing_key` 那条文案随之退了场；
`Config::configured_providers` 是为这次收窄新记的。

测试：`tests/render_console.rs` 两条（plain 回 `None`、忽略 `SessionUpdate`）与 `cli.rs` 的
`mod tests` 四条（解析、模型候选、档位候选、忙闲——最后两条用 `open_picker` 直接驱动，配一个
headless harness 抓提示行）。
