# grilling：保真度、面板与 header 的历史重建

Type: grilling
Status: open
Blocked by: 02
Part of: ../map.md

## Question

定下「重播出来的转录长什么样」与「重播顺带重建哪些新界面状态」。

**冻结输入**（charting）：**原样重播全部块**（消息 / 工具 / 思考提示 / 权限询问与裁决 / hook / usage / 轮次分节），不做「只给人看的子集」过滤；**重建信息面板**与 **header 的模式**；banner 在重播完成后追加。

## 需要定

1. **保真度的边界**：`Transcript` 吃 `RenderEvent::Logged` 就会产出与 live 相同的 `Block`——确认这是「原样」的实现口径；有没有**必须例外**的块（例如合成器的 reasoning、执行者的事件、`SessionStarted` 骨架）？live 时它们长什么样，历史里就必须长什么样。
2. **信息面板**：重播的 `UsageRecorded` 累加是否等于历史总用量（与 live 的累加器同一口径：`cached`/`miss` 是 `input` 的拆分，不重复计）；`last_input`（上下文分子）取历史里最后一条 usage；`turns` 数历史里的 `TurnEnded`。**与 live 的衔接**：重播的用量只累一次，重播完成后继续累加新事件。
3. **header 的模式**：从历史里**最后一条**模式事件推（`ContextInjected{PlanMode}` → 计划；`HistorySuperseded{ModeChange}` → 询问）；没有模式事件时保持注入初值「询问」。给出精确规则与「历史里进过计划模式又出来」的例子。
4. **`SessionFacts` 的一致性**：`session_id` / `cwd`（实为会话目录）/ `model` / `context_window` / `budget_limit` 在重开时本来就来自组装期——确认它们与历史内容不冲突（例如历史里的讨论会话 model 是两人串）。
5. **重开后的滚动位置**：重播完成后是否强制吸底（冻结项与 `Not yet specified` 都指这里）；重播期间的上滚是否打断（结合票 01 的分帧）。
6. **空 / 极短 / 全是讨论的会话**：空日志、只有 `SessionStarted`、只有讨论轮次——各自重播后屏幕上是什么。
7. **与 live 的视觉连续性**：重播结束后，live 的流式尾部 `live` 缓冲从空开始；有没有需要复位的状态（`follow` / `seen` / `indicator`）。

## 先读

- `map.md` 的 Notes（冻结项）
- `src/render/tui.rs` 的 `apply` / `panel.observe` / `mode` 更新分支 / `draw_transcript`
- `src/render/transcript.rs` 的 `push` / `push_logged`（块的唯一决定者）
- `src/render/panel.rs` 的 `observe` / `lines`
- `src/render/wording.rs` 的 `context_*` / `token_pair` / `mode_field`
- `research：分帧重播的接缝事实与成本实测`（票 02）的答案
- `.scratch/tui-ux/issues/02-grilling-collapse-and-detail-contract.md` 的 Answer（块与提示行的契约）

## 答案落点

契约级：重建清单（哪些状态、从哪条事件、什么口径）+ 衔接规则 + 边界表。不要写实现代码。
