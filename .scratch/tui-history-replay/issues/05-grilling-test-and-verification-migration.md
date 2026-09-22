# grilling：测试与验证迁移

Type: grilling
Status: resolved
Blocked by: 01, 03, 04
Part of: ../map.md

## Question

把本图所有决定的**可验证性**定下来：哪些进 `cargo test`、哪些进 pty、哪些只能进真终端手工清单；以及要为「重开一个已存在的会话」造什么 fixture。沿用 `.scratch/tui-ux/issues/08-grilling-test-and-verification-migration.md` 的分层先例。

## 需要定

1. **fixture 会话目录**：怎么造一个可复现的会话（`log.jsonl` + `outputs/<tool_call_id>.txt`）——手工写 JSONL？用现有 `Harness` 跑一个 mock provider 落盘？还是把 `tests/support/` 里的既有 fixture 扩展？给出**一种**做法与它落哪。
2. **重播的自动断言**（`TestBackend`）：重播后的 pane 内容与顺序（历史在前、live 在后）；分帧中途的画面（**部分历史 + 进度行**）；完成后进度行清除、状态行恢复；面板的 token 累计 / 回合数；header 的模式（含「进过计划模式又出来」的历史）；空 / 单条 / 512 / 513 条的边界。
3. **顺序与输入**：重播期间按 `Enter` 不提交（草稿仍在、没有 user 消息入流）；重播期间到达的 live 事件在完成后**按 `seq` 追加**（用一个可控的 fixture 事件注入来断言顺序）。
4. **历史详情**：历史 `▸` 行可点、点开显示全文（`outputs/` 在时）；`outputs/` 不在时显示 `全文不可用`；`reasoning` 重建的「思考完成」行可点。
5. **pty**：`scripts/tui-startup-check.py` 要不要加一条 `--continue` 路径（先造一个会话目录再启动、等首帧/重播收敛、退出交还终端）？它的 docstring 分界是「只有 pty 看得见的东西」；重播的**正确性**归 `cargo test`，pty 只保「重开路径不崩、首帧正确、退出干净」。给出加或不加与理由。
6. **手工清单**：`docs/tui-manual-checklist.md` 新增哪些项（真会话开一次看进度观感、重开后的历史详情点击、分帧中途上滚、超大会话的启动手感）。
7. **既有测试与纪律**：本图会不会打破既有断言（提示行 / 几何 / 面板 / 模式）；开工前核实 `cargo test --all-targets` 的准确通过数（charting 时为 **633 passed**）；`cargo clippy --all-targets` 干净、`cargo fmt --check` 只留两处既有漂移。
8. **spec 回改**：本图要回改哪些既有 spec（`.scratch/fs-agent-v1/spec.md` §19/§7？`docs/render.md`？），列成清单交给 `/to-spec`。

## 先读

- `tests/render_layout.rs` / `tests/render_tui.rs`（`TestBackend` 与合成事件的既有口径）
- `tests/support/`（现有 fixture 形态）
- `scripts/tui-startup-check.py`（docstring 与 `GESTURES`）
- `docs/tui-manual-checklist.md`（既有 ①–⑪ 的编号与口径）
- `.scratch/tui-ux/issues/08-grilling-test-and-verification-migration.md` 的 Answer（分层先例）
- 票 01 / 03 / 04 的答案

## 答案落点

可执行的清单级：fixture 做法、新增 / 改写的用例名与断言对象、pty 增删、手工项编号与步骤、spec 回改清单。不要写实现代码。

## 进度

**100%** —— 完成。一轮 grilling、4 条决定，票面 8 个「需要定」全部覆盖；契约见 `## Answer`。

**下一步**：无（已 resolved）。本图 5/5，按 wayfinder 交接到 `/to-spec`。

## Answer

**验证契约（2026-09-23，4 条决定）。**

### §1 fixture

- 在 `tests/support/` 加一个 **session fixture**：`tempfile::tempdir()` + 照 `tests/session_store.rs:124` 既有的 `append(log_path, speaker, payload)` 写法直接落一条 `log.jsonl`；需要 `outputs/` 时再写 `outputs/<tool_call_id>.txt`。**不走 provider、不跑真 harness。**
- **大多数重播断言根本不需要文件**：直接构造 `Vec<Event>`，喂 `ConsoleRequest::Replay { events }`，再 `TestBackend` 出帧。临时会话目录只服务 `outputs/` 详情（§3）与 pty / CLI 路径。

### §2 `cargo test` 要新增的断言

- **内容与顺序**：`[历史块] → [分隔行 `── 以上为历史 ──`] → [缓冲的 banner / 诊断] → …`。
- **分帧中途**：喂 `Replay` 后**不跑完**，断言「部分历史 + 状态行 `恢复历史 n/m`」；跑完断言进度行清除、状态行恢复。
- **面板**：历史 `UsageRecorded` / `TurnEnded` 累加后的 `total` / `last_input` / `turns`（经右栏文本断言）。
- **模式**：历史含 `PlanMode` 后跟 `ModeChange` → 重开后 header 是**「询问」**；只有 `PlanMode` → **「计划」**。
- **边界**：空（`m == 0`：不进重播态、不插分隔行）/ 1 条 / 512 / 513（两批）。
- **顺序与输入**：重播期间 `Enter` 不提交（草稿仍在、没有 user 消息入流）；重播期间喂一条 live 事件 → 进缓冲 → 完成后按序追加。
- **历史详情**：`▸` 行可点；§3 的四种文件状态；**重播期间点击不响应**。
- **吸底**：重播后吸底、无「到最下」指示条。

### §3 历史详情的四种文件状态（照票 04 §3 的判据）

| 事件文本 | `<会话目录>/outputs/<id>.txt` | 断言显示 |
| --- | --- | --- |
| 含 `full output at …` | 存在且非空 | 文件全文 |
| 含注记 | 不存在（`prune` / 手删） | 预览 + `全文不可用` |
| 含注记 | 存在但为空 | 预览 + `全文不可用` |
| **不含注记** | （从不尝试读） | 事件文本即全文（不出现 `全文不可用`） |

### §4 pty

- 脚本加一条 **`--continue` 路径**：在同一临时 `XDG_DATA_HOME` 里**先造一个会话**（跑一次普通启动即可，或直接写最小 `log.jsonl`），再 `--continue` 启动。
- 断言：**不崩**、首帧 / 重播**收敛**（进度行消失）、退出后终端**交还干净**（alt screen / mouse / paste / termios）。
- 重播内容的**正确性不进 pty**——那是 `cargo test` 的活。

### §5 手工清单

- 新增 **⑫ `--continue` 重开**：真会话开一次看**进度观感**；**历史详情点击**；**分帧中途上滚**（预期无响应 / 吸底）；**超大会话的启动手感**。
- **⑦.1** 补一条「**重开后退出**」：`--continue` → `Ctrl-C` / `Ctrl-D` → 终端干净。

### §6 既有测试与纪律

- `ConsoleRequest` 加变体会**编译期强制**：TUI 的 `request()`（`src/render/tui.rs:1532-1609`）与 plain 侧的 match 各补一个 arm。
- 现有断言（提示行 / 几何 / 面板 / 模式）**预期不变**；开工前重新核实 `cargo test --all-targets` 的通过数（**2026-09-23 tui-ux 落地后是 664 passed**；charting 时是 633）。
- `cargo clippy --all-targets` 干净；`cargo fmt --check` 只留 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移（**不**顺手格式化）。

### §7 spec / 文档回改清单（交 `/to-spec`）

- `.scratch/fs-agent-v1/spec.md`：TUI 启动 / `--continue` 相关段落（§19）+ 若 §7 提到「转录从空开始」的表述。
- `docs/render.md`：渲染接缝（新增 `ConsoleRequest::Replay` 与历史重播的呈现）。
- `docs/tui-manual-checklist.md`：§5 的 ⑫ 与 ⑦ 补充。
- `scripts/tui-startup-check.py`：§4 的新路径（脚本自身）。
- `src/render/wording.rs` 新文案：`恢复历史 {n}/{m}` / `恢复中 {n}/{m}` / `恢复中` / `── 以上为历史 ──`。
- 注意：`tui-ux` 那张图的 spec 回改（§2/§3/§4/§6/§9/§10）是**另一张图**的事，本图不重复列。

### §8 实现顺序

- `tui-ux` **已实现并提交**（`7e437a0` / `940cd43` / `0641c57` / `b48871c` / `538c4aa` / `530a191`），本图实现**可直接开始**。接缝符号见 `map.md` 的「已落地的 tui-ux 接缝」一节；**历史行必须经 `TuiState::apply`**（它维护与 pane 平行的 `links`）才会带 `▸` 命中。
