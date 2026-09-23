# 06 — 重开时把历史铺进转录（接缝、分帧与进度行）

Type: implement
Status: done
Blocked by: None
Part of: ../map.md

**What to build:** `--continue` 重新打开一个已有会话后，TUI 转录里出现上一段对话——不再是一块空屏。整条事件流经**与 live 完全相同的那条 apply 路径**逐条重放，所以块、折叠提示行、名字配色都自动继承；重放**分帧**推进（每批 ≤ 512 条事件**且** ≤ 2000 源行，先到先停），期间界面保持响应、底部提示行临时显示 `恢复历史 n/m`；重放未完成时不按 120ms tick 节流（否则近百帧会累成十几秒）。期间草稿照常能打但 `Enter` 不提交，翻页 / 跳转 / 滚轮一律忽略（吸底），live 事件先按到达顺序缓冲；完成后清态、按序 flush、落到底部。

**Blocked by:** None —— 可立即开工（`tui-ux` 已落地，接缝符号都已就位）。

**来源：** `.scratch/tui-history-replay/spec.md` 的 §1–§5、§9–§10 与 `Testing Decisions`（本票是那份 spec 的实现切片，不是新决定）。**历史行必须经同一条 `apply` 路径**，绕过它直接推面板不会有 `▸` 命中——那是票 08 要的东西，本票先把路径铺对。

**验收：**

- [x] `--continue` 启动后转录里出现历史块，顺序与事件流一致（消息 / 工具 / 思考提示 / 权限询问与裁决 / hook / usage / 轮次分节照 live 原样）。
- [x] 分帧：喂进历史后**未跑完**时，画面是「部分历史 + 进度行 `恢复历史 n/m`」；跑完断言进度行消失、原状态行（含 `Ctrl-C` / `Ctrl-D` 提示）恢复。
- [x] 双闸预算：每批 ≤ 512 条事件**且** ≤ 2000 源行、先到先停；重播未完成时不进 `select!`。
- [x] 进度行降级：`40×10` → `恢复中 n/m`；更窄 → `恢复中`。重播期间不显示 exit 提示。
- [x] 边界：空日志（**不进重播态**、不显示进度行）/ 1 条 / 恰好一批 / 超过一批（两批）。
- [x] 重播期间：`Enter` 不提交且草稿保留；可打印字符与编辑键照常；`Ctrl-C` 退出；`Ctrl-D` 与 `Esc` 忽略；`PgUp`/`PgDn`/`Ctrl-G`/滚轮忽略；`resize` 正常重排且分帧继续。
- [x] 重播期间到达的 live 事件（banner / 恢复诊断 / 恢复补写的工具结果）先进缓冲，完成后按**到达顺序**追加。
- [x] 完成后吸底：`follow = true`、`seen = total`、无「到最下」指示条。
- [x] 历史读失败（文件消失 / 权限）→ 降级为不重播 + 一条诊断，**不阻塞启动**，`--continue` 不因此失败。
- [x] 接缝取的是**组装后**的全量事件快照（含恢复补写的合成失败结果），不是组装前的文件快照。
- [x] `ConsoleRequest` 新变体后 TUI 与 plain 两侧的 match 各补一个 arm（编译期强制）；既有断言预期不变，`cargo test --all-targets` 不低于 **664 passed**。
- [x] `cargo clippy --all-targets` 干净；`cargo fmt --check` 只留 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移（**不**顺手格式化）。
- [x] 不改事件 schema、不动 plain / headless 的行为。

## Comments

- 2026-09-23 实现落地：新增 `ConsoleRequest::Replay { events }` + `ConsoleHandle::replay`（plain 侧空 arm）；`TuiState` 新增 `Replay` 态与 `live_buffer`，`apply` 改为返回本次产出的源行数；`replay_batch` 双闸（512 条 / 2000 行，先到先停），主循环在重播未完时用 `biased` + 恒就绪分支取一批、不等 tick；`live_event` 在重播期间**丢弃 `Logged`、缓冲其余**、`finish_replay` 按到达顺序 flush 并 `to_bottom`；`key` 优先走 `replay_key`（`Ctrl-C` 退出、`Ctrl-D`/`Esc`/`Enter`/`PgUp` 等忽略、编辑键与草稿照常），`mouse` 与 `submit` 在重播期间直接返回；`wording::history_progress_line` 三级降级（<38 列 `恢复中`、<58 列 `恢复中 n/m`、否则 `恢复历史 n/m`）。CLI 在 `assemble` 之后、banner 之前以 `harness.events()` 推 `console.replay`（仅 `parsed.resume`）；`TuiOptions.reopened` 让重开的 TUI 先等来这条请求再渲染，否则组装期恢复推上渲染通道的同一批事件会被先画一遍、再被快照画一遍（spec review 发现的顺序缺陷，已修）。
- **`live_event` 与 spec §3 的调和**：§3 把「恢复补写的工具结果」列为要缓冲的 live 事件，但 §1 又要求快照含恢复结果——两条同时成立会把同一次调用画两遍。落地口径是**快照拥有它、live 的那份丢弃**；只缓冲永不落日志的 `Notice` / `Diagnostic` / `Delta`。`a_logged_event_arriving_mid_replay_is_not_painted_twice` 锁住这条。
- 断言：`tests/history_replay.rs`（26 条）+ `tests/wording.rs` 的进度阶梯一条。全仓 `cargo test --all-targets` = **704 passed / 0 failed**（开工前基线 **677**）。断言只证明「进度行处于部分状态 / 完成后消失」，不钉每批几条（spec `Testing Decisions`）；「恰好一批」边界由 `a_history_that_ends_exactly_on_a_batch_boundary_still_converges` 覆盖收敛性。
- 「历史读失败降级」：payload 是内存里的 `Harness::events()` 组装快照，重播路径本身**没有会失败的文件读**，所以 spec §9 的降级在实现里没有触发点；日志不可读会在更早的 `EventLog::open` / `assemble` 处失败，与今天一致，本特性不改那条路径。日志残尾由 `EventLog::open` 的修复保证。
- clippy 干净；fmt 只留 `src/context/repo_map.rs` 既有漂移。
