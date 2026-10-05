# 页高撑满之后左栏三页的形态

Type: prototype
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

`sidebar_page` 的高度现在等于**用量字段数**（`fields`，恒 6、地板 3：`src/render/layout.rs:349-356`、`395-419`）。冻结项 14 把它解耦成「撑满左栏剩余高度」，于是：

| 终端 | 身份占 | 页矩形高 |
| --- | --- | --- |
| 120×24（宽档，有 mark） | 5 | 15 |
| 80×24（窄档，只有文字身份） | 1 | 19 |
| 80×10（内容行剩 9，身份已被阶梯让掉） | 0 | 6 |
| < 80 列，或 `Ctrl-O` 收起 | — | 整栏不存在 |

要定这三页在新高度下的形态：

- **调用量**页（6 个字段）：贴顶还是居中，其余留白怎么处理；
- **todo** 页：今天「装不下就在计数行上面画一行 `＋N 项`」（[`docs/render.md`](../../docs/render.md) 左栏一节），行数多了之后这条规则要不要改；
- **轨迹**页：它拿到的是一个 15 / 19 / 6 行的框，框里画什么归[轨迹视图那张票](01-prototype-trace-page-layout.md)，本票只管框与页签的关系。

另外核实：`tabs` 与 `sidebar_page` 的 `y` 都由 `kind.rows() + TAB_ROWS` 推出（`src/render/layout.rs:341-356`），页高改了之后命中矩形、`SIDEBAR_TOP_GAP` 与「高度不够先丢 mark」那条阶梯是否照旧。

## 产物

`.scratch/trace-tab/prototype/` 下的帧：120×24 / 80×24 / 80×10 三档，各含调用量、todo、轨迹三页（轨迹页内容可用占位）。

## 对下游的一句话

本票定的是**框**。框定完，[轨迹视图在 40/28 列里的排版与密度](01-prototype-trace-page-layout.md) 才能在正确的行数里设计内容。

## 作答

**决定（2026-10-05，HITL：维护者拍了两条）**：`sidebar_page` 的高度 = **左栏内容行 − 身份行 − 页签条**，内容**贴顶**；`SIDEBAR_FIELDS = 6` **不再决定页高** —— 那个数降级成阶梯的地板（`SIDEBAR_MIN_FIELDS = 3`）。

**两处改动**（都在 `src/render/layout.rs`）：

1. `sidebar_page.height`（`:349-356`）从 `fields` 改成 `content_rows − kind.rows() − TAB_ROWS`，其中 `content_rows = area.height − SIDEBAR_TOP_GAP`。
2. `sidebar_content`（`:395-419`）的判据从 `kind.rows() + TAB_ROWS + fields <= content_rows` 改成 `kind.rows() + TAB_ROWS + SIDEBAR_MIN_FIELDS <= content_rows`；`SIDEBAR_FIELDS`（`:71`）与那条「逐字段递减」的支路随之删掉。`SIDEBAR_MIN_FIELDS` 建议改名 `SIDEBAR_MIN_PAGE`（它的新身份是页的地板，不再是字段数）。

**实测**（prototype：[frames.py](prototype/frames.py) 与它的输出 [frames.txt](prototype/frames.txt)，`python3 .scratch/trace-tab/prototype/frames.py` 可重跑）：

| 终端 | 内容行 | 身份（今天 → 决定后） | 页区 y | 页高（今天 → 决定后） |
| --- | ---: | --- | --- | --- |
| 120×24 | 23 | mark 5 → mark 5 | 9 | 6 → **15** |
| 80×24 | 23 | 文字 1 → 文字 1 | 5 | 6 → **19** |
| 120×10 | 9 | 0 → **文字 1** | 4 → 5 | 6 → 5 |
| 80×10 | 9 | 0 → **文字 1** | 4 → 5 | 6 → 5 |
| 79×24 | 23 | — | — | 整栏不存在（`< 80` 或 `Ctrl-O`） |

`MIN_HEIGHT = 10`（`layout.rs:18`）之下的尺寸到不了这一支，所以 `80×9` 那类不必考虑。

**新高度下三页的形态**：

- **调用量页贴顶**：`Panel::lines`（`src/render/panel.rs:67-147`）不看高度、最多返回 6 行，Paragraph 从页区顶部画，充裕档下多出来的行是空白。**它一个字不改** —— 页区不足 6 行时 Paragraph 从尾部裁，丢的正是「缓存 → 输出 → 输入」，与它自己的字段顺序一致。
- **todo 页的规则一个字不改**：`TodoPanel::lines`（`src/render/todo.rs:61-93`）已经按 `area.height` 自适应（计数行占最后一行、条目拿 `room − 1` 行、塞不下才画一行 `＋N 项`）。实测 120×24 下它从今天的「4 项 + `＋4 项` + 计数」变成「8 项全显示 + 计数」—— 页高变大的收益在这里是白送的。
- **轨迹 / 文件页**：仍贴顶一行占位，框里画什么归[prototype：轨迹视图在 40/28 列里的排版与密度](01-prototype-trace-page-layout.md)。

**不改**：页签条的 y 与命中矩形（仍由身份行数 + `TAB_ROWS` 推，`layout.rs:341-348`）、`SIDEBAR_TOP_GAP`、左栏宽度两档、`Ctrl-O` 那一层。极矮档真的变的是**身份行回来了**：`120×10` / `80×10` 从「没有身份」变成一行 `fs-agent 0.1.0`，代价是页区少一行、调用量页丢「缓存」—— 那正是这条阶梯设计上第一个丢的字段。

**给 `/to-spec` 的三件事**：页高公式、地板常量 `3`、「贴顶」；`SIDEBAR_FIELDS = 6` 从代码里消失。
