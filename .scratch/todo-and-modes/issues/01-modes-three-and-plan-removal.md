# 模式回到三档：删掉 plan，补齐 `auto` / `readonly` 入口

Type: implement
Status: ready-for-agent

> 规格：`.scratch/todo-and-modes/spec.md` §1、§5（删除清单）、`Testing Decisions`。
> 这是本轮**唯一**需要 ADR 的票：它推翻 `.scratch/fs-agent-v1/spec.md` §13 那条「进出 plan 只由用户手势」的安全论证。

## 目标

权限回到 `readonly` / `ask` / `auto` 三档，三档都**真的能选到**；`plan` 整套退场（票 02 的 `todo` 工具接手「计划」这件事，不在本票）。

## 落点

`src/permissions.rs`、`src/lib.rs`、`src/cli.rs`、`src/config.rs`、`src/context.rs`、`src/agent.rs`、`src/render/{input,tui,wording}.rs`、`.scratch/fs-agent-v1/spec.md`（§12/§13 回改）、`docs/adr/0003-*.md`（新增）、`docs/plan-mode.md`（删除）、`tests/`。

## 具体行为

1. **`Mode` 三档**：删 `Mode::Plan` 与它的三个 stance 分支、`is_plan_write`、`PLAN_FILE_NAME`；`mode_label` 去掉「计划」；`as_str` 只剩三个词。
2. **入口**：
   - `config.toml` 新增 `[permissions] mode`（默认 `ask`；未知值启动即报错，语气照 `Error::UnknownModel`）；
   - `--mode readonly|ask|auto` 覆盖配置（与 `--model` / `--cwd` 同一风格，写进 `--help`）；
   - `Shift+Tab` 在会话内**循环** `readonly → ask → auto → readonly`：`FrontEndEvent::TogglePlan` 改名 `CycleMode`，TUI 的 `BackTab` 走它，状态行照旧显示 `模式 X`。
3. **循环不注入任何东西**（spec §1 的选择）：不新增 `ContextInjected`，不碰 `messages` 头部；模型第一次被拒时从 `PermissionDecided.reason` 读到档位。
4. **三个组装点读配置**（`src/cli.rs:369/663/1557`）：headless 与 `probe` 也跟配置，**「无 answerer 时 `Ask → Deny`」那条照旧在门外兜底**。
5. **删除**（spec §5 的清单）：`Asker::ask_plan_conflict` / `PlanConflict` 及其在 `render/input.rs` 的问答、`wording` 里的冲突标题/正文/候选键/plain 行、`Harness::enter_plan_mode` / `exit_plan_mode` / `plan_conflict`、`Submission::{Plan,EndPlan}`、`context::plan_mode_instruction` 与它的钉住/撤回/`--continue` 修补（`agent::retire_plan_instructions`、`OpenedSession::start` 里那一段）、`docs/plan-mode.md`、`tests/plan_mode.rs`。
6. **保留两个枚举变体**（向后兼容，硬约束）：`events::ContextSource::PlanMode` 与 `events::HistoryReason::ModeChange` 留着，只是不再发射 —— 老流里有它们，删了会让 `--continue` 在旧会话上反序列化失败。加测试钉住。
7. **ADR 0003**（`docs/adr/0003-plan-leaves-the-permission-modes.md`）：写清①原来为什么把 plan 做成模式（`deny > ask > allow` 忽略 specificity ⇒ 那条 `PLAN.md` 豁免必须是 deny 里的与项）；②现在为什么搬出来（权限与计划是两件事；要给模型计划的能力就得把「能不能写」还回用户）；③后果（模型不再被强制只读；`Shift+Tab` 从「切一档」变成「循环三档」；老的 `PlanMode` / `ModeChange` 事件仍在流里被重放但不再产生）。
8. **回改两处正文**：`.scratch/fs-agent-v1/spec.md` §12（模式表从四行变三行、四条不变量里提到 plan 的那句）与 §13（整节标为「已被 `.scratch/todo-and-modes/spec.md` 取代」并保留原文 + 一条带日期的说明，而不是删掉 —— 这个仓库的规矩是历史不重写）。
9. **`PLAN.md` 文件不动**：只是不再有豁免。

## 测试

- `tests/permission_gate.rs`：三档 stance；plan 的用例删除；「模式是 `Session` 的值、不进流」那条照旧。
- `tests/config_profiles.rs` / `tests/render_console.rs`：`[permissions] mode` 解析（三档合法 / 未知值报错 / 缺省 `ask`）；`--mode` 覆盖配置。
- `tests/render_tui.rs` + `tests/render_layout.rs`：`Shift+Tab` 三次循环回到原点，状态行依次 `模式 只读` / `模式 询问` / `模式 自动`；plan 的弹窗与提示字样从测试里消失。
- 向后兼容：一段含 `ContextInjected{PlanMode}` 与 `HistorySuperseded{ModeChange}` 的老流仍能反序列化并被重放（照 `tests/history_replay.rs` 的写法）。
- `cargo test` 全绿、clippy 干净、fmt 只留既有漂移。

## 不做什么

`todo` 工具与侧栏标签（票 02/03）；任何权限门语义改动（规则代数、断路器、`.env`、cwd 限制、传播）；沙箱。
