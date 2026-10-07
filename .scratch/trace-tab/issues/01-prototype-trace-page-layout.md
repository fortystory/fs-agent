# 轨迹视图在 40/28 列里的排版与密度

Type: prototype
Status: resolved
Part of: ../map.md
Blocked by: 03

## 问题

轨迹视图要把整条流画进 **40 列**（宽档）或 **28 列**（窄档），而它装的是转录今天在主列约 77 列里画的东西（冻结项 2：全量块）。要定的是**每种块在这两档里长什么样**：

- 用户 / assistant 的消息正文：长段落怎么折、要不要一行题头；
- 工具调用与结果：今天转录里是「调用一行 + 结果一行」，行上带 `▸` 可点开详情；
- 思考（`reasoning`）：今天一行「思考」加详情入口；
- 上下文注入（`ContextInjected`）：今天一行来源名加详情；
- 骨架与旁注：回合 / 轮次边界、`Usage`、`Executor*`、`Hook`、权限裁决、`Sandbox`、`HistoryReason` 那类叙述；
- 分隔：块与块之间靠什么分开（空行 / 缩进 / 颜色 / 前后缀）。

还要回答两件事：**markdown 是否仍排版**（表格、代码块、列表在 28 列下值不值得），以及**密度**（一行一条摘要，还是允许一条折成多行）。

## 产物

`.scratch/trace-tab/prototype/` 下的几帧真实宽度草图：40 列与 28 列各至少「一个有内容的会话」与「一个空会话」，加一张「块 → 一行画法」的对照表。用假数据，不接真渲染器。

## 接受的边界

列宽不动（冻结项 3）；看全文仍走详情覆盖层（居中主列）；键盘不入，只有滚轮（冻结项 13）。

## 作答

**决定（2026-10-05，HITL：维护者拍了三条）**：

1. **消息正文只画首行 + `…`**，并**新增 `DetailKind::Message`**（全文进详情覆盖层）；
2. **前缀分档**：宽档（40 列）照旧 `[名字] `，窄档（28 列）**去方括号**（`用户 ` / `助手 ` / `讨论1 `）；
3. **markdown 照排** —— 复用 `to_lines_indented(text, width)`，表格列宽自适应、超宽代码行折且保缩进。

**为什么范围比票面小**（三条实测）：

- 工具行、思考行、上下文注入行与全部叙述行（`RoundStarted` / `TurnStarted` / `Permission*` / `Hook` / `Executor*` / `Usage` / `Sandbox` / `History` / `Notice` / `Diagnostic`）**今天就已经各占一行**：`tool_block_lines`（`src/render/tui.rs:4888-4932`）只返回一个 `RenderedLine::linked`，详情全在 `DetailKind::Tool` 里；`paint_block`（`4656-4843`）其余分支也都各返一行。所以「块 → 一行画法」表里真正要决定的只有**消息正文**。
- **消息正文是唯一的多行块**：assistant 走 markdown、其余按 `\n` 分行（`4658-4696`）。
- `Block::Message` **今天没有详情入口** —— `DetailKind` 只有 `Thinking` / `Context` / `Tool`（`tui.rs:5058-5076`）。这正是决定 1 要补的那一个。

**三档帧**（prototype：[trace-pages.py](../prototype/trace-pages.py) 与 [trace-pages.txt](../prototype/trace-pages.txt)，`python3 .scratch/trace-tab/prototype/trace-pages.py` 可重跑）：

- 40 列：一条 5 行的回答在「照抄」画法里折成 5 行、把两行工具行挤出屏幕，在决定后的画法里占 1 行；
- 28 列：同一条回答「照抄」占 **6 行**、决定后占 1 行；短前缀让 `read_file src/render/` 多露出 2 列。

**给 `/to-spec` 的落点**：

- 消息在轨迹视图的分支：首行截断 + `…`，行上挂 `DetailKind::Message { speaker, text }`；对话视图仍画全文（那里宽度够，冻结项 2 的分工不变）。
- 前缀：`speaker_line`（`4590-4600`）与 `attribute_document` / `attribute_speech`（`4491-4571`）都把前缀写死成 `[{name}] `，要多一档「这个视图多宽」的输入（或一个 `PrefixStyle`）。
- markdown：轨迹视图把左栏宽度喂给 `to_lines_indented`（`4677`），渲染器不新增分支。

**未证实**：28 列下**表格**实际挤成什么样没跑真渲染器（原型只做折行近似）。若真机上不可读，「宽档照排、窄档退成纯文本」是现成的退路（本票问题里的选项 (c)）。

**补记（2026-10-05，看过整屏示例之后由维护者追加）**：**轨迹视图里按轮次隔行底色** —— 每个单位（回合 / 轮次，与回合条同一判据）的所有行共用一块深背景，相邻单位交替，像表格的隔行换色。它是**单位级**而不是屏幕行级：底色在行生成期按「源行 → 单位序号」打上（那份索引已经存在，正是回合条用的那一份），所以滚动时色块跟着内容走、不会重排。只在轨迹视图铺（对话视图不铺）；色值实现期对着真终端定（256 色两块深灰起步），退化终端不做底色。

整屏示例：[带色版](../prototype/split-view.txt)（`cat` 可见底色与三色前缀）与[无色版](../prototype/split-view-plain.txt)。
