# 07 — 历史与 live 的接缝：分隔行、信息面板与 header 模式

Type: implement
Status: ready-for-agent
Blocked by: 06
Part of: ../map.md

**What to build:** 重开后会话在「旧的」与「新的」之间有一条明确的分界：重播结束、flush 缓冲**之前**插一条 `── 以上为历史 ──`（暗灰，渲染层合成的 `Notice`，**不落日志**），启动 banner 排在它之后；信息面板的 token 累计与回合数把历史算进去并继续累加（不重置）；header 的模式取历史最后一条模式事件——包括恢复补写的那条。

**Blocked by:** 06 —— 分隔行要插在「重播结束、flush 缓冲之前」那个时刻，面板与模式的验收也要先有历史事件进得来。

**来源：** `.scratch/tui-history-replay/spec.md` 的 §6–§7 与票 03 的四条契约。**分隔行、面板与模式都是「逐条 apply 的自然结果」**——本票的代码量主要在分隔行的插入时机与那一条 `Notice` 的文案，其余是把口径固化成断言。

**验收：**

- [ ] 历史块之后、banner 之前出现 `── 以上为历史 ──`。
- [ ] **仅当**这次重播至少产出一个块才插；空日志 / 只有 `SessionStarted` 骨架时不插。
- [ ] 顺序断言：`[历史块] → [分隔行] → [缓冲的 banner / 诊断] → …`。
- [ ] 分隔行是渲染层合成、不落日志：下一次 `--continue` 不会把旧的那条从历史里带出来（而是新插一条）。
- [ ] 历史里**没有** `Notice` / `Diagnostic` 的重建：banner、技能加载、渲染器丢弃、恢复诊断不出现在历史段。
- [ ] 信息面板：历史 `UsageRecorded` / `TurnEnded` 累加后的总量、最后输入量、回合数正确（经右栏文本断言）；与 live 衔接**不重置**；`cached` / `miss` 是输入量的拆分、不重复计。
- [ ] header 模式：历史含 `PlanMode` 注入后跟一条 `ModeChange` → 重开后是**「询问」**；只有 `PlanMode` → **「计划」**；没有任何模式事件 → 保持初值「询问」。
- [ ] 恢复补写的 `ModeChange` 也算：被杀在计划模式的会话重开后显示「询问」，与 harness「重开从配置模式开始」的实际策略一致。
- [ ] 上下文窗口 / 预算上限取**当前**配置、如实显示不调和（配置变过时不自洽是已知代价，不是缺陷）。
- [ ] 完成后仍吸底（`follow = true`、`seen = total`、无指示条）；live 缓冲结束时为空。
- [ ] `cargo test --all-targets` 不低于 **664 passed**；clippy 干净；fmt 只留既有漂移。
