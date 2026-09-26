# 回合条：单位索引、焦点派生、点击跳转

Type: implement
Status: ready-for-agent
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

（实现时把偏离 spec 的地方记在这里）
