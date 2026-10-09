# 15 — 用量面（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: 13

> 规格：[`../spec.md`](../spec.md) §6（用量面：桶的集合、两节、口径句、降级）。
> 桶的集合与算法为什么与左栏用量面板同源、「本次」为什么与行尾那个 `合计` 是同一个数，在
> [检视器的面与内容](08-grilling-inspector.md) §5；行尾那条用量尾巴的形状在
> [ADR 0016](../../../../docs/adr/0016-usage-rides-the-row-of-its-call.md)。

## 目标

一条消息或一段思考的用量能**单独看**：五个桶（输入 / 缓存读 / 未命中 / 输出 / 推理）分
「本次」与「这一趟会话」两节，末尾一句口径把容易被加错的三个数说清；而「本次」与行尾那个
`合计` 是同一个数，两处对不上时读者不用担心是自己看错。

## 现状（2026-10-09 核实，改前先复核）

- 用量是五个桶：输入 / 输出 / 缓存读 / 未命中 / 推理（**可空** —— 供应商不一定报）。
- 左栏用量面板与这一面**必须用同一套算法**（面板那边是「观察 + 累积」两个入口），否则两处会漂移。
- 现有条形画法（实心 / 空心两档块字符）只在面板那条路径上有一条先例；条宽上限 10 列。
- **工具调用没有用量**：用量事件带发言者不带工具调用 id，行尾那条尾巴也只挂在消息与思考上 ——
  所以这一面挂**消息与思考**详情，**不挂工具详情**。

## 落点

详情的面生成那一步（用量那一节的行）、条形画法（与面板共用）、措辞那套（桶名、两节小标题、
末尾口径句、`—（供应商未报）`）。

## 具体行为

1. **两节**：「本次」与「这一趟会话（到这条记录为止）」，每节五行（输入 / 缓存读 / 未命中 /
   输出 / 推理）。
2. **末尾一句口径**：`输入 = 缓存读 + 未命中；推理已含在输出里，这两条不能再相加。`
   写死这一句是因为读者最容易在这里数两遍。
3. **「本次」的口径**：一条消息跨 ≥ 2 次调用时，「本次」就是那几次之和 —— 与行尾那个 `合计`
   **同一个数**（同源同口径，不另算一套）。
4. **推理可空时**写 `—（供应商未报）` 且**不画条形** —— 没有分母的条就是骗人。
5. **条形与面板共用**画法与宽度上限；不新造第二套条形。
6. **`Message` 与 `Thinking` 两行分派表各加「用量」面**（正文/思考 · 用量 · 计时 · 概述）。
   工具详情**不加**（它没有用量）。

## 验收

- [x] 点一条消息能切到「用量」面，看到两节各五行。
- [x] 末尾那句口径逐字在位。
- [x] 「本次」那节的四个数与同一行行尾那条 `合计` **相等**（一条跨多次调用的消息也要相等）。
- [x] 推理为空的会话里那一格写 `—（供应商未报）` 且**没有条形**。
- [x] 工具详情**没有**用量面。
- [x] 这一面的数字与左栏用量面板对同一段会话给出同一个累计值。
- [x] `cargo test` 全绿、`cargo clippy` 干净、`cargo fmt --check` 只留既有漂移。

## 落地记录

**2026-10-10 落地（用量面）。**

**落点**：`src/render/tui.rs`（`FaceId::Usage`、`DETAIL_FACES` 的 `Message` / `Thinking` 两行插进
「用量」、`UsageScope`、`usage_section` / `usage_rows`、`TuiState::turn_ordinal` /
`turn_totals` / `turn_total`、`Painted::Block` 与 `SettledThinking` 各带一份用量账、
`BlockPaint`（`emit_block` 的实参收成一个结构体，否则它超了 clippy 的参数上限）、
`open_detail` 里填「本次」、`DetailFacts.usage`、`detail_faces` / `face_sections` 多收一个
`NumberStyle`）、`src/render/panel.rs`（`proportion_bar` / `BAR_COLUMNS` / `label_columns` 从
「只有面板用」变成 `pub(crate)`，另加 `Panel::total()`）、`src/render/wording.rs`（面名、两节
小标题、五个桶名、`—（供应商未报）`、末尾那句口径）。测试：`tests/render_layout.rs`（三条新的
用量断言，另加 `MESSAGE_FACES` / `THINKING_FACES` / `tab_to_timing` 跟着新分派表改）、
`src/render/tui.rs`（一条用量面的单测；`every_record_kind_gets_the_faces_the_table_names` 的
判据从「每面一节」放宽成「每节非空」—— 用量面本来就是两节读数加末尾那句口径）。

### 与票面 / 规格不同的地方

1. **「这一趟会话（到这条记录为止）」是这条记录到达那一刻的会话累计**，不是面板的当前值。
   两条路读同一个累计器（`Panel::total`），但详情把那一刻的数**记在记录上**
   （`Painted::Block` / `SettledThinking` 随块一起记住）—— 否则一次宽度变化（整本账按新宽度
   重放）会让历史记录显示成「整个会话」的累计。这与 `at`、计时那两笔账是同一条纪律。
   验收「与左栏用量面板给出同一个累计值」因此读的是**同一段会话**：测试里那条消息之后还有
   一个回合，两处都读「到那条记录为止」的那一个数。
2. **「本次」那一节在打开那一刻才算。** 一条消息画出来的时候，它所属的那次发言可能还没收尾
   （后面还有调用），所以 `Detail` 里只记「属于哪一次发言」，`open_detail` 按它在
   `turn_totals`（已收尾的发言）或 `turn_usage`（还在跑的那一次）里查出与行尾那个 `合计`
   **同一个数**（票 15 第 3 条）。
3. **分母取这一节里最大的那个桶。** 票面只规定「条形与面板共用画法与宽度上限」，没规定分母
   —— 用量面没有天然的分母（面板那边是上下文窗口与额度）。拿这一节的最大值当刻度至少让
   「谁大谁小」读得出来；这一节全零时不画条（没有分母的条不画）。
4. **「推理」那一行整条让给那一句话。** 供应商没报推理 token 时那一格写
   `—（供应商未报）`（15 列），比给数字与条形留的那 12 列宽 —— 它本来就不画条，所以那一行的
   余下宽度全给它、左对齐跟在标签后面。窄屏上仍可能被省略号收尾（`panel::row` 的 `fit`），
   那是既有那条纪律。
5. **用户自己那条消息的「本次」是空的（全 0）。** 它不是一次调用（没有配它的模型调用），
   所以不属于任何一次发言（`turn = None`）—— 读者要看的是它后面那次发言，行尾那条尾巴也
   正是挂在那次调用的行上。票面没有点名这一档，这里按「这条记录自己花了多少」落。
6. **`emit_block` 的实参收成 `BlockPaint`。** 用量账进来之后它有八个参数，超了 clippy 的
   七参数上限，所以「除块之外那几笔账」打包成一个结构体 —— 那几笔账的去处本来就同一个
   （`DetailFacts`）。行为零变化。

### 验收对应的测试

| 验收 | 测试 |
| --- | --- |
| 两节各五行、末尾那句口径逐字 | `a_message_detail_reads_its_usage_in_two_sections`（`tests/render_layout.rs`）、`the_usage_face_names_the_two_sections_and_the_ledger_note`（`src/render/tui.rs`） |
| 「本次」与行尾那个 `合计` 相等（跨多次调用） | `a_message_detail_reads_its_usage_in_two_sections`（那条消息所属那次发言是两次调用，行尾 `合计 in=5200 out=60` 与「本次」的输入 / 输出是同一个数） |
| 推理为 `None` 写 `—（供应商未报）` 且没有条形 | `a_reasoning_count_the_provider_never_reported_draws_no_bar`、`the_usage_face_names_the_two_sections_and_the_ledger_note` |
| 工具详情没有用量面 | `a_tool_detail_has_no_usage_face` |
| 与左栏面板同一个累计值 | `a_message_detail_reads_its_usage_in_two_sections`（面板那一帧与用量面上是同一个 compact 值） |
| 分派表两行各多一面 | `every_record_kind_gets_the_faces_the_table_names`、`a_message_detail_walks_its_faces_with_the_tab_key` |
| 条形与面板共用画法与宽度上限 | 两处都走 `panel::proportion_bar` 与 `panel::BAR_COLUMNS`（`src/render/panel.rs`） |
