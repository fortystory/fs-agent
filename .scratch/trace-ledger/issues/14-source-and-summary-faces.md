# 14 — 来源面与概述面（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: 12, 13

> 规格：[`../spec.md`](../spec.md) §6（分派表补齐、来源面、概述面、原文面不做）。
> 祖先链为什么固定截到三环、为什么**不押在稳定行键上**，在
> [检视器的面与内容](08-grilling-inspector.md) §8；层级的**键盘跳转**归票 19。
> 分派表在票 13 已经落成三张，本票补齐剩下的（`Context` 两面、`Tool` 加「来源」）。

## 目标

一份详情能回答「这一行是被什么带出来的」：切到**来源面**看到一条只读的祖先链 ——
本行 → 它上面那条迭代的回复 → 那一回合 → 上面那条用户消息，每一环带行首文字与位置。
注入详情也终于有面了（注入 / 概述）。

## 现状（2026-10-09 核实，改前先复核）

- 票 13 之后，详情有面机制与三张分派表；`Context` / `File` / `Diff` / `Todo` 四种仍是今天的单面形状。
- 一个回合最多 557 次迭代，**链深不封顶** —— 所以链必须固定截断并写清截到哪。
- 票 12 落地后，「块身份 → 源行」那份账可查，于是每一环的**行首文字**能取到而不必扫全表。
- 块与源行的对应今天只覆盖**块**（`trace_links` 那条链是屏幕行 → 源行 → 详情，反向没有通用入口）。

## 落点

详情的面生成那一步、分派表那处常量、`render::tui` 里取行首文字的那条查询路径。

## 具体行为

1. **来源面**：四环只读链（本行 / 上面那条迭代的回复 / 那一回合 / 上面那条用户消息），
   每环给出**行首文字**与它在账本上的位置；**固定截到三环**并在面上写清「更深的链不再展开」。
2. **链断时**：某一环拿不到（那一回合还没有用户消息、开场段落里没有迭代）就**到此为止**，
   不写「不可用」占位 —— 链本身就是渐短的。
3. **`Tool` 的分派表加「来源」面**，位置在「计时」与「概述」之间。
4. **`Context` 的分派表落成两行**（注入 / 概述，默认落在注入 = 今天那一面）。
5. **概述面**：一条简述 —— 是什么、多长、什么时候、谁说的；工具那一面给工具名与成功/失败。
   它是分派表的最后一格。
6. **来源面不押在行身份上**：查不到行首文字时（裁剪掉了那一行）**退成只给位置**，
   仍然有用 —— 每一环的**内容**才是身份，位置是附带的。
7. **`File` / `Diff` / `Todo` 仍是单面、不画标签条**（它们的主体本身就是一份完整的可滚动对象）。

## 验收

- [x] 点一条工具调用能切到「来源」面，看到四环只读链，每环带行首文字与位置。
- [x] 链上写明截到三环；一个 557 次迭代的回合不会画出一长串。
- [x] 开场段落里的一条注入：链在某一环自然断掉，不出现占位符。
- [x] 被 `CAP` 裁掉的那一环退成只给位置，面上其余环照旧。
- [x] 注入详情现在有标签条（注入 / 概述），默认落在注入。
- [x] 文件 / 改动 / 待办三种详情**仍然没有标签条**，且与今天逐字相同。
- [x] 分派表补齐成规格里那张七行的表，测试断言每种记录的面集合等于表里那一行。
- [x] 切面**不改变**谁被冻住、关掉时还原给谁（既有那几条详情相关测试一字不改仍绿）。
- [x] `cargo test` 全绿、`cargo clippy` 干净、`cargo fmt --check` 只留既有漂移。

## 落地记录

**2026-10-10 落地（来源面与概述面）。**

**落点**：`src/render/tui.rs`（`FaceId::Injection` / `FaceId::Source`、`DETAIL_FACES` 的
`Context` 与 `Tool` 两行、`AnchorKind` / `Anchor` / `ChainLink` / `LedgerChain`、
`TuiState::note_anchor` / `ancestor_chain` / `anchor_text`、`source_section` / `chain_row_line`、
`anchor_kind`、`trace_hit_at`、`open_detail` 多收一个「本行在账本上的行号」、
`push_view_lines` 回报轨迹页第一条内容行的账本行号、`paint_group_header` 记单位锚、
`detail_summary_row` 的注入那一条）、`src/render/pane.rs`（`pushed` 水位、`ledger_row` /
`ledger_line`）、`src/render/wording.rs`（面名、链的四环标签、`chain_row` / `chain_link` /
`chain_arrow` / `chain_deeper` / `chain_unknown`、`summary_injection`）、
`src/render/panel.rs`（零改动，条形的共享留给票 15）。测试：`tests/render_layout.rs`
（三条新的面断言 + 注入标签条 + 单面那条改判据）、`tests/history_replay.rs` 与
`tests/render_layout.rs` 两处 `TOOL_FACES` 常量、`src/render/tui.rs` 与
`src/render/pane.rs` 的四条单测。

### 与票面 / 规格不同的地方

1. **验收第 3 条拿「开场段落里的一条注入」举例，而注入没有来源面。** 分派表（规格 §6、08 票
   §2）里 `Context` = 注入 · 概述 —— 来源面只挂 `Tool`。所以「链在某一环自然断掉」这条验收
   落在**开场段落里的一次工具调用**上（它上面既没有迭代的回复，也没有回合与用户消息），
   断言链上只剩本行、没有「←」、也没有「不可用」这种占位。票面的那两句以分派表为准。
2. **链的「三环」= 三个祖先环，加本行共四行。** 票面先说「四环只读链」又说「固定截到三环」，
   08 票 §8 与 `spec.md` §6 也是这个说法。读法取「本行 + 至多三环祖先（上面那条迭代的回复 /
   那一回合 / 上面那条用户消息）」，于是链长恒 ≤ 4 行加至多一行说明；一个几百次迭代的回合
   不会画出一长串。这一条写进 `chain_row_line` 与 `LedgerChain` 的注释。
3. **「更深的链不再展开」的判据是第一环之上还有别的迭代回复。** 链只取「本行上方最近的
   一条回复」，而一个回合可以有几百次迭代 —— 上面还有回复时说明链被截断过，那一句才出现；
   没有更早的迭代时链是自然走完的，不写它（跟「链断时不写占位」同一条纪律）。
4. **锚表与位置用「账本行号」，不是窗格下标。** `CAP` 裁剪只挪窗口不挪账本，所以
   `Pane` 多记一个只增的推入水位（`pushed`），行号 = 水位 − 窗口长度 + 下标；被裁掉的那一环
   于是仍报得出「第 N 行」，只是行首文字回窗格里取（取不到就只给位置）。锚只记三类记录
   （用户消息 / 迭代的回复 / 单位组头），不记窗格里的每一行 —— 链深不封顶，但锚只与
   会话的迭代数同阶。宽度重放时锚与窗格一起清、按同样顺序重建。
5. **注入的概述面补上了（票 14 第 5 条）。** 它此前只有「一条记录」那句兜底：现在
   「是什么」是 `summary_injection`（一条注入），「多长」是加载进来那段正文的字数，
   「什么时候 / 谁说的」本来就有（注入带着到达时刻与发言者）。工具 / 消息 / 思考三种的概述面
   核对过，本来就合「是什么 · 多长 · 什么时候 · 谁说的」这一条（票 13 落的），零改动。
6. **`Schema` 面不在本票的范围。** 规格 §6 给工具六面（含 Schema），而票 13 与票 14 都只要求
   参数 / 输出 / 计时 / 来源 / 概述这五面 —— 派给工具的 Schema 面（08 票 §12 判「变形做」）
   没有落在这一轮的任何一张票里，所以分派表的工具那一行是五面。这一条不是本票能定的，记在
   这里免得下一轮再采一遍。
7. **一条既有断言的判据改了一处**：`a_detail_with_one_face_draws_no_bar_at_all` 原先
   拿**注入**详情验「单面不画标签条」，而注入现在有两面。改成拿**待办**详情验，另加一条
   `an_injection_detail_grows_a_label_bar_and_still_opens_on_the_injection` 钉注入那两面与
   默认面。这是票 14 明写的行为变化，不是实现偏了。

### 验收对应的测试

| 验收 | 测试 |
| --- | --- |
| 工具调用的来源面有四环只读链，每环带文字与位置 | `a_tool_detail_reads_the_read_only_chain_that_carried_this_row`（`tests/render_layout.rs`） |
| 链写明截到三环、几百次迭代不画一长串 | 同上（`chain_deeper` 在位、`chain_reply` 只出现一次）+ `the_chain_stops_at_three_links_even_when_there_are_more_iterations`（`src/render/tui.rs`） |
| 链断时到此为止、不写占位 | `a_chain_that_breaks_ends_there_instead_of_a_placeholder`（`tests/render_layout.rs`）+ `a_chain_that_cannot_reach_an_ancestor_just_ends`（`src/render/tui.rs`） |
| 被 `CAP` 裁掉的那一环只给位置 | `a_chain_link_keeps_its_position_after_the_cap_clips_its_row`（`src/render/tui.rs`）+ `the_ledger_row_survives_the_cap_while_the_line_does_not`（`src/render/pane.rs`） |
| 注入详情有标签条、默认落注入 | `an_injection_detail_grows_a_label_bar_and_still_opens_on_the_injection`（`tests/render_layout.rs`） |
| 文件 / 改动 / 待办仍然单面、与今天逐字相同 | `a_detail_with_one_face_draws_no_bar_at_all`、`a_single_face_detail_keeps_todays_body_verbatim`（`src/render/tui.rs` 单测） |
| 分派表是那七行、面集合与表一一对应 | `every_record_kind_gets_the_faces_the_table_names`（`src/render/tui.rs`） |
| 切面不改变谁被冻住、关掉还原给谁 | 既有那几条详情相关测试一字不改（`the_detail_overlay_freezes_the_transcript`、`closing_a_trace_detail_returns_to_where_it_was_opened`） |
