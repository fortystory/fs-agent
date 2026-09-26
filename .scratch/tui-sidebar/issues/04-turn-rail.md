# 回合条：单位索引、焦点派生、点击跳转

Type: implement
Status: done
Blocked by: 02

> 规格：`.scratch/tui-sidebar/spec.md` §4 / §6 / §7。帧：`prototype/frames/120x24-rail-3-units.txt`、`-rail-30-units.txt`、**`-rail-30-units-focus-12-gap.txt`（反例，说明为什么窗口要跟着焦点走）**。
> 术语：`CONTEXT.md` 的**回合条（TurnRail）**与**焦点回合（FocusedTurn）**。

## 目标

转录右缘那一列从空变成一条能读、能点的回合条：一格一个单位、最新在下、焦点那格亮着、点一格跳到那一轮的段首。

## 落点

`src/render/tui.rs`（`TuiState` 新增两份索引 + 焦点派生 + `draw_rail` + `HitAction` + 鼠标分派）、`src/render/transcript.rs`（若 `Block` 需要暴露「这是不是一条用户消息 / 回合结束」）、`src/render/wording.rs`（`RAIL_CELL` / `RAIL_FOCUS` / `RAIL_TRUNCATED` 与 `turn_rail_*` 文案）、`tests/render_tui.rs`、`tests/render_layout.rs`。

## 具体行为

1. **单位**：`SessionFacts.speaker_order.len() > 1` → 数 `RoundEnded`（轮次）；否则数 `TurnEnded`（回合）。
2. **两份索引**（与现有 `links: VecDeque<Option<Detail>>` 同形、同随 20 000 源行上限裁剪）：**每源行 → 单位序号**、**每单位 → 段首源行**。段首 = 该单位之前最近的一条用户消息；讨论会话没有用户消息，退化到该轮的首个源行。
3. **画**：条占转录右缘最右 1 列（外框内侧），行数 = 转录行数。字形：普通格 `┊`（`DarkGray`）、焦点格 `┃`（粗、`LightMagenta`）、被裁那一端 `⋮`（`DarkGray`）。**吸底时窗口贴底**（最新一格紧贴状态行上分隔线）；**回看时窗口跟着焦点走**，两端各自按「那一端还有没有被裁掉的单位」画 `⋮`——这是 `frames/120x24-rail-30-units-focus-12-gap.txt` 那个反例的修法，**必须**有回归测试。
4. **焦点（`FocusedTurn`）是派生量**：= 视口顶端那一行所属的单位；吸底时 = 最新单位。**不要**在 `TuiState` 里存一个「选中单位」。
5. **点击**：命中某格 → 滚到该单位段首并**顶端对齐**（最后一格自然夹到底）。点焦点格 = 同样顶端对齐（视口顶端本来就在该单位里，所以多数时候看起来没动静）。命中矩形 = 该格那一行 × 条的 1 列。
6. **空会话**：零个单位 → 那一列什么都不画（不画格、不画 `⋮`）。
7. **历史重播**：条随重播逐格长出，不做特例。

## 测试

- 格数 = 单位数（不足转录行数时上方留空、贴底）；溢出时窗口跟着焦点（视口停在很旧的单位 → 条上仍有且只有一格 `┃`）；`⋮` 只出现在真的还有被裁单位的那一端。
- 焦点派生：滚到第 3 个单位内 → 焦点在 3；吸底 → 焦点在最新；滚动一页跨过单位边界时焦点跟着跳。
- 点击：点第 N 格 → 视口顶端行 = 该单位段首；点焦点格不移动视口（或只做顶端对齐）；命中矩形随窗口移动而变。
- 讨论会话（`speaker_order.len() == 2`）数的是轮次（造一段 `RoundStarted/RoundEnded` 的流断言）。
- `--continue` 重播途中条随历史增长，重播结束后焦点在最新。

## 不做什么

键位（不加键）；讨论会话里轮次的配色或标签；`轨迹` / `文件` 页里的任何内容。

## Comments

**实现完成（2026-09-26）**。落点：`src/render/tui.rs`（`Rail` 结构、`rail_rows()`、`is_user_message()` / `is_boundary()`、`draw_rail()`、`TuiState::focused_unit()` / `jump_to_unit()`、`HitAction::RailUnit`、鼠标分派）、`src/render/pane.rs`（新增 `Pane::scroll_to_source`）、`tests/render_layout.rs`（7 个用例）、`tests/history_replay.rs`（1 个用例）。

**索引怎么建**：与 `links` 同形、同一处裁剪（`prune_links` 一并 `rail.prune(dropped)`，换来的 `dropped` 同时把每段的段首下标往前挪），所以「第 N 条源行属于哪个单位」永远与 `pane.source_at()` 给回来的下标对齐。单位边界在 `apply()` 里认 `TurnEnded`（讨论会话认 `RoundEnded`，两者都在流上，所以由 `facts.speaker_order.len() > 1` 决定数哪一种）。没结束的那个回合里的行落在「还没有单位」的下标上，取焦点时夹到最后一个单位。

**一处规格没写细、由本票定的**：**段首取「单位内第一条用户消息」，取不到才退到「该单位的首个源行」**。规格 §4 写的是「该单位之前最近的一条用户消息」，但用户消息实际上总是**落在本回合自己的范围里**（`Harness::run_turn` 先 `record_user_message` 再 `drive_turn`，而 `TurnEnded` 才是边界），所以「之前」的那条要么不存在、要么是**上一轮**的问题 —— 那会让点击落到错的地方。「单位内第一条」同时满足用户故事 16（点下去跳到自己敲的那句）和讨论会话的退化规则（讨论的题目只记一次，在第 0 轮之前；后面的轮次没有自己的用户消息，于是退到该轮的首行）。两种情形都有用例钉着。

**prototype 挖出来的洞的回归测试**：`the_rail_window_follows_the_focus_wherever_the_viewport_is` 一次翻一页、在**每一个**停靠点断言「有且只有一格 `┃`」，20 页走完，覆盖「视口停在第 12/30 个单位」那个反例；另有一条断言 `⋮` 只出现在真的还有被裁单位的那一端（贴底时在顶、停在最上面时在底）。`rail_rows()` 的分配规则也写成注释留在代码里：焦点格先占一格，每一端被裁再花一格，剩下的按「上少下多、多出来的还给上面」分 —— 贴底时自然退化成「最上面一格 `⋮` + 最新 N−1 格」，与 prototype 的帧一致。

**空会话**：零个单位时那一列什么都不画（不是画空格），用例覆盖。

**没改 `transcript.rs`**：`Block` 已经足够表达「这是不是用户消息」（`Message { speaker: SpeakerId::User }`，注意不是 `ContextInjected` —— 后者也记在 user 名下，但它不是 `Message`，不该被当成段首）与「这是不是边界」；票面留的「若需要暴露」没有需要。

**交互细节**：点击落在**顶端对齐**；最新那个单位会自然夹到底（`Pane::scroll_to_source` 里 `top` 一夹到 `max_top` 就把 `follow` 打开），所以点焦点格「看起来没动静」是预期行为，有用例钉住。`Pane::scroll_to_source` 是新公开的接缝：`scroll()` 按**显示行**走，而段首是**源行**，两者的换算只有 pane 自己知道。

**基线**：`cargo test` **721 passed / 0 failed**（713 → +8）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
