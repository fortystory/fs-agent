# 13 — 首 token 时刻落流，详情覆盖层分成面（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1（一个可选字段）、§6（面：机制、分派表、计时面）、§13（措辞走既有那套）。
> 那个字段为什么搭在完成事件上（而不是只在渲染层记、也不是新增一条事件变体）在
> [ADR 0020](../../../../docs/adr/0020-first-token-time-rides-the-completion.md) 与
> [时间轴与计时](06-grilling-timeline.md) §4；分派表与各面的内容在
> [检视器的面与内容](08-grilling-inspector.md) §2–§6。
> **可与票 12 并行**：面机制与计时面都不依赖块身份 —— 计时面拿不到「这次模型调用」的归属时会
> **自动降级**成只有「这次工具调用」一节，等票 12 之后再补上那一节。

## 目标

一次模型调用「等了多久才动起来」这件事第一次有确切数字：完成事件上多带一个可选的首 token 时刻，
它一路进日志、进重放，老流读成「不可用」；而详情覆盖层从「一份正文」变成**一组面**，
顶部一条标签条切，于是那份新数字有个能读的地方。

## 现状（2026-10-09 核实，改前先复核）

- `EventPayload::MessageCompleted` 已有 `at` 与用量；**没有**首 token 时刻。
- 增量事件有两种（正文与推理），先到者即「屏幕上第一次动」；三处产点在主循环与合成器里，
  **用户消息那一条恒为无**。
- `SCHEMA_VERSION` 只是记号 —— 代码里没有任何按版本分派的读取逻辑。
- 详情覆盖层今天**一份详情一个 `body`**，尺寸算术只有两处（文字区宽、内边距）；
  覆盖层立着时独占键盘，那一支只认 `Esc` / `Ctrl-D` / 方向键 / 翻页 —— 所以 **`Tab` 在那里是空键**。
- 工具详情今天**没有**计时、Schema、来源、概述这四样；工具调用**没有用量**（用量事件带发言者
  不带工具调用 id，行尾那条用量尾巴也只挂在消息与思考上）。

## 落点

事件侧：`MessageCompleted` 的形状、那三处产点、schema 版本号。渲染侧：详情的那一层
（`DetailView` 那种「一份详情一个 `body`」的形状）、标签条画法（复用既有那个）、面名与措辞
（走既有措辞那套）、色板（计时面不需要新色）。

## 具体行为

1. **`MessageCompleted` 加 `first_token_ms: Option<u64>`**：首个增量（正文或推理，先到者）距本次调用
   开始的毫秒数。三处产点各读一次时钟；用户消息那一条恒 `None`。schema 版本号跟着涨一档。
2. **兼容**：老流缺这个字段读成 `None`；老形状读新流忽略未知字段。**不给这批事件新增变体**
   —— 那是单向不兼容，老二进制的 `read_events` 会拒读整条流。
3. **详情分成面**：一份详情变成「一组面 + 当前面」，每面带自己的滚动位置；标签条借走主体一行、
   复用既有的标签条画法，每个标签记一个命中矩形供点击；`Tab` / `Shift+Tab` 切面。
   **覆盖层尺寸一行都不许动。**
4. **分派表先落三张**（穷举的常量，测试断言「每种记录的面集合等于表里那一行」）：
   `Tool` = 参数 · 输出 · 计时 · 概述；`Message` = 正文 · 计时 · 概述；`Thinking` = 思考 · 计时 ·
   概述。**`Context` / `File` / `Diff` / `Todo` 这四张与「来源」面归票 14**（来源要行→块的反查）。
   每张表都要能再加行 —— 票 14 会补两行。
5. **默认面永远是今天打开就看到的那一面**（工具默认落在**参数**）；「概述」排在末尾。
6. **计时面**：每节末尾一行**计时来源**，写清是哪两个时刻相减。首 token / 生成 / 吞吐三项在老流上
   **不存在** → 写 `不可用`，并把原因写进同一行的计时来源，**不填 0、不写占位符**。
   **降级**：拿不到「这次模型调用」的归属时（票 12 之前必然拿不到），这一面**只出现「这次工具调用」
   一节** —— 面上只画它自己能证明的那一段。
7. **概述面**：一行简述（是什么、多长、什么时候、谁说的）。
8. **页脚仍是那一对数加 `esc` 关闭**，**不加面号** —— 标签条已经给了位置。
9. **小节标题只在这一面 ≥ 2 节时画**，判据在生成面那一步就决定。

## 验收

- [x] 一次有增量的模型调用在日志里带上首 token 时刻；用户消息那一条不带
      （产出点在上一阶段落的 `src/agent.rs`；落盘与读回由
      `the_first_token_field_survives_a_round_trip_through_the_log` 钉住）。
- [x] 一条没有该字段的老流读成「不可用」，`sessions replay` / `--continue` / `sessions show` 照旧
      （`a_message_completed_without_the_first_token_field_reads_as_none`、
      `a_stream_written_before_the_field_is_readable_unchanged`，重放那几条在 `tests/history_replay.rs`）。
- [x] 点一行工具调用：详情顶上出现标签条，`Tab` / `Shift+Tab` 与点标签都能切面
      （`the_label_bar_borrows_a_body_row_without_touching_the_box`、
      `a_message_detail_walks_its_faces_with_the_tab_key`、`clicking_a_label_switches_the_face`）。
- [x] 切换到计时面能读到开始时刻 / 总时长 / 首 token / 生成 / 吞吐，末行写清计时来源
      （`the_timing_face_reads_the_numbers_off_this_call`）。
- [x] 老会话上那三项写 `不可用`（不是 0、不是空白）
      （`an_old_stream_reports_the_three_numbers_as_unavailable`、
      `a_stream_without_a_first_token_says_unavailable_instead_of_zero`）。
- [x] 票 12 未落地时计时面只出现「这次工具调用」一节，不出现空的「这次模型调用」
      （`a_tool_timing_face_only_shows_the_section_it_can_prove`、
      `the_tool_timing_face_shows_only_the_section_it_can_prove`）。
- [x] 面数 ≤ 1 的详情**不画**标签条，且与今天逐字相同
      （`a_detail_with_one_face_draws_no_bar_at_all`、`a_single_face_detail_keeps_todays_body_verbatim`）。
- [x] 各面各记自己的滚动位置：切走再切回来还在原处（`each_face_keeps_its_own_scroll_position`）。
- [x] 覆盖层尺寸与今天完全一致（100×30 是 **96×26** —— 票面与 spec §6 那个 `92` 是旧数字，
      见落地记录）；页脚仍是那一对数加 `esc 关闭`（`the_label_bar_borrows_a_body_row_without_touching_the_box`）。
- [x] 分派表是一个可测的常量，测试断言每种记录的面集合等于表里那一行
      （`every_record_kind_gets_the_faces_the_table_names`）。
- [x] `cargo test` 全绿、`cargo clippy` 与基线逐条相同、`cargo fmt --check` 干净。

## 落地记录

**2026-10-10 落地（面那一半，事件侧见下面的阶段记录）。**

**落点**：`src/render/transcript.rs`（`CallTiming` / `ToolTiming` 两个共享层的计时，
`Block::Message` 与 `ToolBlock` 各多带一份 —— 两个渲染器都忽略它们）、`src/render/tui.rs`
（`RecordKind` / `FaceId` / `DETAIL_FACES` 那张分派表、`Section` / `Face`、`DetailView` 与
`detail_face_to` / `detail_face_cycle`、`draw_detail` 里的标签条与逐面滚动、计时面与概述面）、
`src/render/wording.rs`（面名、计时面各行的词、概述行、`clock_ms`）、`src/render/mod.rs`（多导出
两个计时类型）、`src/render/plain.rs`（新字段不画一个字）。测试：`tests/render_layout.rs`（八条
新的面断言）、`tests/history_replay.rs`、`tests/render_tui.rs`、`tests/todo.rs` 的构造点。

### 与票面 / 规格不同的地方

1. **覆盖层尺寸是 96×26，不是 92×26。** 票面与 `spec.md` §6 都写「100×30 仍是 92×26」，而
   实测（`plan(100×30).detail()`）是 `x = 2, y = 2, 宽 96, 高 26`：宽度 = `100 − MODAL_MARGIN(4)`、
   `left = (100 − 96) / 2`、高度 = `30 − (BORDER_COLUMNS + DETAIL_MARGIN_ROWS)`、`top = (30 − 26) / 2`。
   `src/render/layout.rs` 在这次改动里**零 diff**（`git diff HEAD -- src/render/layout.rs` 为空），
   而 `MODAL_MARGIN` 一直是 4、`DETAIL_MAX_WIDTH` 是 135 —— 那两处算术今天就是这几个数，所以
   **92 是旧数字**（`spec.md` 的正文按纪律不动）。验收与测试钉的是今天实测量的 96×26。
2. **降级那一节不画小节标题，于是「这次工具调用」这五个字不在屏幕上。** 第 6 条说工具计时面
   「只出现『这次工具调用』一节」，而第 9 条说小节标题只在这一面 ≥ 2 节时画 —— 两条相乘的结果是：
   那一面**只有它那一节的内容**（开始时刻 / 总时长 / 其中等审批 / 计时来源），标题不出现。
   所以集成测试的判据是「不出现空的『这次模型调用』」而不是「出现『这次工具调用』」——
   后者要等票 14 把「这次模型调用」那一节补进来（那时才有两节、才画标题）。
3. **等审批那一段的两个档**：`approval_ms` 为 `None`（这次调用根本没问过权限）写
   `没有等待审批`；问过而两个时刻相同写 `0.0 s · 两个口径，不是两个可相加的数` —— 那是**真读数**，
   不是拿 0 顶上去（`0.0 s` 那两个时刻确实在同一毫秒里）。「不填 0」那条纪律只针对
   拿不到的首 token / 生成 / 吞吐三项。
4. **五条断言的判据订正**：这一票的测试里有五条当初写的判据与实现的算术不符（覆盖层按旧数字
   断言 92、切面后按「整屏没有那句正文」断言、老流按「整屏出现三次不可用」计数、工具计时面按
   「没有等待审批」断言、滚动按「第 100 行可见」断言）。逐条核实后**改的是断言**，不是实现：
   前三条是判据写偏（底下那条转录行本来就写着同一句正文；计时来源那一行里也有「不可用」；
   票面那个 92 是旧数字），第四条是场景与断言不符（测试构造的是**问过**的权限，而那句话是
   「没问过」的写法），第五条是「一次 `PageDown` 滚多少」按实测几何重新算（见下）。实现一侧
   没有为了这几条改动过。

### 落地时算清楚的两个数（写在这里免得下次再采一遍）

- **120×40 下主体 29 行**：覆盖层 36 = 40 − 4；框内 34 = 36 − 2；除掉上下内边距 32；带标签条时
  主体 = 32 − 3 = 29（标题一行、标签条一行、页脚一行）。于是 `PageDown` 一次 28 行（一页减一行
  重叠），200 行的输出滚 5 次落在第 141 行。
- **`DetailView.height` 是上一帧写回的数**：测试里两次按键之间不画帧时它还是 0（那一帧才量出
  主体高度），所以按真实按键序列在滚动前先渲染一帧。

## 阶段记录

### 2026-10-09 傍晚：事件侧落地（`Status` 仍是 `ready-for-agent`）

[ADR 0020](../../../../docs/adr/0020-first-token-time-rides-the-completion.md) 定的那一笔
先落了 —— 它**不依赖这张票的其余部分**，而其余部分要碰的是详情覆盖层的形状、绘制、键位与指针，
那是另一个自然的单位。

**落点**：`src/events.rs`（字段 + `SCHEMA_VERSION` 1 → 2）、`src/agent.rs`（三处产点）、
`tests/event_log.rs`（三条兼容测试）。**屏上输出一个字符都没变** —— 那个字段还没有任何读者
（读者是本票的计时面，票 14 之后的来源面与票 22 的时间轴）。

- **字段**：首个增量 token 距本次调用开始的毫秒数，正文与推理**先到者**为准。
- **三处产点**：模型回答那处给真值（`send` 之前记 `call_started`，两个增量分支里
  `get_or_insert_with(Instant::now)` 记第一次到达）；**用户自己那条**与 **harness 合成的
  `System` 那条**恒 `None` —— 前者没有人等，后者没有 provider 流。provider 一个增量都没发的
  那次调用也是 `None` 而不是 0。
- **兼容**：`#[serde(default, skip_serializing_if)]`，双向都兼容；三条测试钉住
  （老流缺字段读成 `None`、新字段落盘读回同一个数、只用老形状的声明读带新字段的流不失败）。

**代价照 ADR 认下的那些**：38 处测试里的构造点各多写一行 `first_token_ms: None`（编译器逐个抓，
`SCHEMA_VERSION` 只是记号、代码里没有按它分派的读取逻辑）。

### 这一阶段没做的（票的其余部分，下一个单位）

详情覆盖层分成**面**这件事：结构（`DetailView` 从一份正文变成一组面 + 当前面）、那张按记录种类
穷举的表、标签条、切面键与点击、**各面各记自己的滚动位置**、以及计时面与概述面的**内容**。
计时面那一半还要动共享层：`Block::Message` 得把首 token 时刻与那次调用的起点一路带进
`DetailKind`（今天是块自己的时刻 + 文本），而「工具行属哪次模型调用」那笔账按
[长历史与行选择](10-grilling-history-and-selection.md) §9 的结论**至今没有** —— 所以工具详情的
计时面按票面的降级走：只给「这次工具调用」一节。
