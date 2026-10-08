# 左栏 `todo` 页：摘要、进度行与完整列表弹窗（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-09，两轮 grilling 共 15 问：终点形态与范围 / 页的职责 / 长文本 /
条目超出页高 / 弹窗与打开方式 / 进度行 / 页上画什么 / 已完成折不折 / 弹窗快照还是跟随 /
样式 / 文档落点）。本图只做**规划**，不产代码改动。

> **来源**：[`seed.md`](seed.md)（2026-10-06 一轮 `/ask-matt` 记下的三条意向：进度行、样式、
> 完整列表弹窗）。它那节《现状》核实于 2026-10-06；charting 时补查的事实见下面「起点」。

## 目的地

一份**可执行的 spec**（交给 `/to-spec` 折成构建计划，再由 `/to-tickets` 拆实现票）：把左栏那个
今天**只画字形、id 与被截断的文本**的 `todo` 页定清楚 —— 它列什么、一行画什么、长文本怎么折、
做完的那些去哪、看全时那个弹窗长什么样、指针在哪儿落。**走到这张图的尽头时，实现这一页不再有
任何需要先决定的事**，剩下的只是接线与断言。

**范围**：`src/render/`（`todo.rs`、`tui.rs` 的左栏页与详情覆盖层、`wording.rs`，需要时
`palette.rs`）、[`CONTEXT.md`](../../CONTEXT.md) 的「待办列表」词条、
[`docs/render.md`](../../docs/render.md)、[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md)。
**`todo` 工具的形状、schema、回执、`Effect::ReadOnly` 一个字节不动；事件流不动；模型可见文本
一个字节不动。**

## 笔记

- **领域**：衡（heng）的左栏页与详情覆盖层，喂给它们的是一个内建工具产出的**只读**数据。
- **起点**（charting 实测，每条带证据）：
  1. 列表来自最近一次**非执行者**且已落地的 `todo` 调用（[`src/render/todo.rs:36-49`](../../src/render/todo.rs)）；
     页签是**一只闩**，见过一次非空列表此后整个会话都在，清空列表也不摘（`todo.rs:8-10,51-54`）。
  2. 页今天是**纯读数**：末行一条 `wording::todo_count`（今天写 `已完成 3/11`），条目按页宽
     `truncate_columns` **截断**，塞不下的报 `＋N 项`；**没有滚动、没有命中区域**
     （`todo.rs:56-107`、`src/render/tui.rs:5707`）。文件页与改动页才自己画、记 `rect`
     （[`tui.rs:5710-5718`](../../src/render/tui.rs)）。
  3. 几何：宽档 **40 列**（终端 ≥120）、窄档 **28 列**（≥80），意愿与宽度**相乘**
     （[`src/render/layout.rs:66-73,499-514`](../../src/render/layout.rs)）；页区地板 3 行
     （块字档 5 行），页高 = 内容行减身份与页签条（`layout.rs:102-111,528-556`）。
     diff-page 那张票量过一次：120×24 ⇒ **40×15**，80×24 ⇒ **28×19**。
  4. 详情覆盖层今天有**六种 `DetailKind`**（Thinking / Context / Message / File / Diff / Tool）
     与**三种 `DetailOpener`**（Trace / Files / Changes）（[`tui.rs:8397-8449`](../../src/render/tui.rs)）。
     加一变体 = enum + `detail_body` 的 switch + 一个 `open_*_detail` + 一个 `DetailOpener` 变体
     （照 `DetailOpener::Files`，它什么都不冻）。模板在
     [`files-page/spec.md` §6](../files-page/spec.md) 与 [`diff-page/spec.md` §6](../diff-page/spec.md)。
  5. 弹窗**正文不折行**：排版归调用方，宽度用 `Regions::detail_text_width()`，折行用
     `pane::wrap_text`；框宽上限 135 列。
  6. 字形：`☐` 待办 / `▸` 进行中 / `✓` 完成（[`src/render/wording.rs:1976-1984`](../../src/render/wording.rs)）；
     `▸` 与转录里可折叠块的标记**同形双义**（`wording.rs:1907`），在 picker 里它还是「当前项」
     记号（`wording.rs:1974`）。
  7. 左栏那一块还要登记进**屏幕文本层**（`note_rows`），拖选复制与搜索都靠它
     （[`tui.rs:5724-5726`](../../src/render/tui.rs)）。
  8. 措辞一个归宿在 [`src/render/wording.rs`](../../src/render/wording.rs)
     （[`chinese-ui/spec.md`](../chinese-ui/spec.md)），模型可见文本也是中文（[ADR 0005](../../docs/adr/0005-model-visible-text-in-chinese.md)）。
- **要咨询的 skills**：`/research`（票 02、03）、`/prototype`（票 04）、`/grilling` +
  `/domain-modeling`（票 05、06、07）；折 spec 时 `/to-spec`，拆票时 `/to-tickets`。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 的对话。

### 冻结项（charting 的 grilling 定下，票里不得重开）

1. **终点是一份 spec**，本图只产决策，不写代码。
2. **范围只在呈现**：列表的来源（最近一次非执行者的 `todo` 调用）、工具的形状、schema、回执、
   `Effect::ReadOnly`、规则段，全部不动。`--continue` 与 `sessions replay` 重放同一份调用、
   重算出同一页这件事照旧成立。
3. **页 = 摘要 + 详情弹窗**：页上是「进度 + 在做 + 接下来」，看全靠弹窗。**不搬进主列**
   （轨迹页那种）。
4. **页上画**：页顶一条进度行 + **全部未完成项**（`in_progress` 置顶，其余保持提交时的原顺序）。
   放不下报 `＋N 项`，**不加滚动**。
5. **长文本折行**：一条内容可以占 2–3 行，续行缩进到字形列，**不再截断**。
6. **已完成项默认折掉，页上不给展开**：想看全部去弹窗。
7. **看全走新增的一种详情**（`DetailKind::Todo`），**鼠标开**：进度行右端一个可点按钮 +
   页内点空白处。**不给键盘一把专门的键**。
8. **弹窗里的列表 = 打开那一刻的快照**；开着时模型又提交了新列表，关掉就看到新的。
9. **样式**：换掉 `▸`（与转录可折叠块同形）；弹窗里已完成项**降暗**，**不划掉**。
10. **进度行挪到页顶**，文案 `3/11 · 1 个在做`，全部做完写 `11/11 完成`。
11. **文档落点**：**回改** [`todo-and-modes/spec.md` §4](../todo-and-modes/spec.md)（新 spec 是
    它的继任者），不是只在本目录留一条补记。

### 会撞的既有决定（折 spec 时回改或落笔，本图不改）

- **[`todo-and-modes/spec.md` §4](../todo-and-modes/spec.md)**：本图冻结项 3–10 大体在改它
  （页签是闩、列表从流推、计数行先留出来）。它整节重写，并在旧处留一行指向本目录。
- **[`tui-visual-language/spec.md`](../tui-visual-language/spec.md)**：语义色板与字形语法。冻结项 9
  动的是字形表里的一格，以及「同形双义靠区域区分」那一条登记；弹窗里的降暗要落在**语义名**上，
  不新起颜色。
- **[`files-page/spec.md` §5/§6](../files-page/spec.md) 与 [`diff-page/spec.md` §6](../diff-page/spec.md)**：
  详情覆盖层、`DetailOpener`、「正文折行归调用方、正文不进事件流与模型上下文」是模板。
- **[`usage-stats-format/spec.md`](../usage-stats-format/spec.md)**：`调用量` 页的 `▓`/`░` 占比条是先例 ——
  **本图不选它**（进度行那一条冻结项 10 里没有）。
- **[`questionnaire-reading/spec.md`](../questionnaire-reading/spec.md)** 与 [ADR 0010](../../docs/adr/0010-questionnaire-keys-dispatch-by-zone.md)：
  「浮层立着时键盘归浮层、底边被 `overlay_floor` 挡在题面之上」—— 左栏这一页新开的弹窗算不算
  那一族，由票 06 问。
- **[`tui-feedback/spec.md` §5](../tui-feedback/spec.md)**：`note_rows` 与屏幕文本层；折行之后每一行
  与屏幕行号的映射不能再靠「一行一项」。
- **[ADR 0003](../../docs/adr/0003-plan-leaves-the-permission-modes.md)**：计划归模型、模式只管写不写 ——
  本图一个字节都不动它，正好是那条 ADR 的又一次兑现。

### Tracker 事实与降级（本图适用）

- map = `.scratch/todo-page/map.md`，child = `.scratch/todo-page/issues/NN-*.md`；
  阻塞 = 票面 `Blocked by: NN`；claim = `Status: claimed`（未认领写 `open`）；resolve = 票底
  `## 作答` + `Status: resolved` + 追加一行到本文 `已定的决定`。
- **没有 native sub-issue / 依赖边**，回退到正文约定：本文 `## 任务清单` 逐条引用子票
  （条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/todo-page/map.md`。
- **执行在范围内的例外（2026-10-09，维护者当场定）**：wayfinder 默认 plan-don't-do，而这次
  **实现票也住在本目录** —— spec 折出来之后由 `/to-tickets` 拆出 `Type: implement` 的构建切片，
  追加进本文 `## 任务清单`（条目数校验仍成立）。wayfinder 的会话**不认领**它们（frontier 扫描
  跳过 `Type: implement`，由 `/implement` 认领）。
- **一处偏离要说在明处**：这次 charting 时维护者就在场，两轮 15 条决定是**当场拍板**的，不是走票
  得来的。它们收进 [grilling：charting 当场拍板](issues/01-charting-decisions.md) 并标了
  `resolved` —— 理由与 [`diff-page` 那张图](../diff-page/map.md) 同一条：那些决定确实已经做完，
  而「细节只住一个地方」（图是 index，不是 store）比「charting 不 resolve 票」更硬。
  除此之外的票都按常规走，**每个会话至多 resolve 一张**（research 票除外）。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**决策票**（七张）：

- [x] [grilling：charting 当场拍板](issues/01-charting-decisions.md)
- [x] [research：给左栏页接上命中区域与一种新详情，需要哪些接线](issues/02-research-wiring-for-todo-page.md)
- [x] [research：别的 coding agent 怎么呈现计划列表](issues/03-research-other-agents-plan-ui.md)
- [x] [prototype：两档宽度下页与弹窗的帧草图](issues/04-prototype-page-and-detail.md)
- [x] [grilling：极小页高与三种空态](issues/05-grilling-cramped-and-empty.md)
- [x] [grilling：指针落点与覆盖层打架](issues/06-grilling-hit-regions-and-overlays.md)
- [x] [grilling：多 speaker 时页上那一份是谁的](issues/07-grilling-which-list-is-the-page.md)

**实现票**（2026-10-09 由 `/to-tickets` 从 [`spec.md`](spec.md) 拆出的六张，`Type: implement`，
由 `/implement` 认领，**不进 frontier**）：

- [x] [08 — 页顶那条进度行（tracer bullet）](issues/08-progress-line.md)
- [x] [09 — 一句话读得全：折行、内容同列与溢出那行](issues/09-wrapped-items.md)
- [x] [10 — 点开整份列表：整页可点与详情覆盖层](issues/10-open-full-list.md)
- [x] [11 — 弹窗标题写清这份是谁的](issues/11-detail-speaker.md)
- [x] [12 — 四种空态与小页区](issues/12-empty-states.md)
- [x] [13 — 收口：回改旧 spec、渲染文档与手工清单](issues/13-close-out.md)

## 已定的决定

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [grilling：charting 当场拍板](issues/01-charting-decisions.md) — 两轮 15 问的答案：终点是一份
  可拆票的 **spec**，实现票随后住在本目录；范围**只在呈现**（工具形状、schema、回执、事件流、
  模型可见文本都不动）；页是**摘要 + 详情弹窗**，不搬进主列；页上是**页顶一条进度行 + 全部未完成项**
  （`in_progress` 置顶，其余按提交顺序），放不下报 `＋N 项`、**不加滚动**；**长文本折行**、续行缩进到
  字形列、**不再截断**；**已完成项折掉且页上不给展开**；看全走**新增的一种详情**、**鼠标开**
  （进度行右端的按钮 + 页内点空白）、**不给专门的键**；弹窗里是**打开那一刻的快照**；**换掉 `▸`**、
  弹窗里已完成项**降暗**（不划掉）；进度行文案 `3/11 · 1 个在做` / `11/11 完成`；文档**回改**
  `todo-and-modes/spec.md` §4。
- [research：给左栏页接上命中区域与一种新详情，需要哪些接线](issues/02-research-wiring-for-todo-page.md) —
  接线的全貌清出来了：命中区域是**四件**（页面状态值、「自己画并 `return`」的分支、画时记
  `rect` 且互相清对方的、`屏幕行→下标` 那两句换算），照抄前四件里除 `rows` 之外的；
  **但「一项恰好一行」这个前提照抄不了** —— 折行之后屏幕行与逻辑项不再是双射。指针分派表在
  `click_at`，落在 `todo` 页矩形里的点击今天被**吞掉**（既有测试 `clicking_a_todo_row_opens_nothing`
  钉着，**它与新命中区域正面冲突，实现时必须一起改**）。加一种 `DetailKind` 要动**五处**
  （enum、`detail_body` 的唯一 switch、一个 `open_*_detail`、`DetailOpener` 变体 +
  `close_detail` 的穷尽 match、标题与颜色），模板自 2026-10-08 没漂移；这一页没有滚动/焦点/rect，
  覆盖层关掉后**什么都不用还**（与 `Files`/`Changes` 同形）。折行入口是 `folded_text`（不是票里
  写的 `pane::wrap_text`）；覆盖层只认 `Esc` 与 `Ctrl-D`。语义色板里**没有 dim 这一档**，两个既有
  先例都指向 `MUTED`。屏幕文本层登记的是**画出来的行** ⇒ 折行后必须给 `note_rows` 传折行标记。
  几何没漂移：120×24 ⇒ 40×15、80×24 ⇒ 28×19；**3 行地板只出现在宽档**（120×12 ⇒ 40×3），
  而那一档下「进度行 + 一条折行条目」正好放满、**没有余量给 `＋N 项`**（喂给 05 票）。
- [research：别的 coding agent 怎么呈现计划列表](issues/03-research-other-agents-plan-ui.md) —
  对照九家：**「一次提交整份」满票一致**（与我们同一条形状，也都不从回给模型的文本里解析）。
  两处**我们是异类**：看全的入口别家用 `Ctrl+T` 这把**专门的键**（Claude Code 上限五条、Gemini
  默认收起），我们是鼠标按钮；完成项别家都是「默认收起 + 一个开关看全」，我们页上连开关都没有 ——
  于是页顶那条进度是页上**唯一**的完成度信息。`n/m` 那种进度文案没有可抄的模板（Claude Code 有
  清单没计数，Gemini 有 indicator 没计数）。可借的只是问法：「几条」而不是「几行」。
- [prototype：两档宽度下页与弹窗的帧草图](issues/04-prototype-page-and-detail.md) — 帧草图在
  [`prototype/frame.py`](prototype/frame.py)（一条命令重画五块），维护者当场对着帧拍板：
  **前缀 `aligned`**（字形 2 + id 位 3，内容严格同列，窄档正文 23 列）；**续行缩进到内容列** ——
  这是对 `pane.rs:442`「续行从第零列起」那条纪律的**有意例外**（本页不是日志；缩进主张的是
  「这片属于哪一项」，那是真结构）；进行中的字形 **`●`**（`▸` 因为与转录可折叠块同形而退出）；
  弹窗里完成项**连续降暗**（落 `MUTED`），不加小标题；标题 `── 待办 ──…── esc 关闭 ──`，正文
  112/72 列用满，滚动交给覆盖层既有那套。**按钮四个退路在窄档全都放得下**（文案 15 + 间隙 9 +
  `详情` 4 = 28）⇒ 那不是几何问题而是可发现性问题，交给 06。折行**照旧硬切**（`/to-tickets` 会断
  成两片，写进 spec 的已知代价）。**40×3 那一档一条都画不出来**（只剩进度行 + `＋9 项`）——
  05 票的输入。
- [grilling：极小页高与三种空态](issues/05-grilling-cramped-and-empty.md) — 四个空态各取定一句：
  极小页区（40×3 / 40×5 / 28×5）= **进度行 + `＋N 项`，一条都不画**（3 行地板只有宽档会到，
  窄档最矮是 28×5，那儿能画一条折行项再报溢出）；全部做完 = **就一行 `11/11 完成`**，页签不动
  （它是闩）；列表被清空 = 一句 **`还没有待办`**；一条放不下 = **画前几行 + `…`**（`…` 是文件页
  既有的记号，**不**计入 `＋N 项`，那条数字因此永远是「一条都没露出来的还有几条」）；空列表时
  **不给弹窗按钮**（开不了）；终端 <80 列**不做替代路径**（可见性是用户自己的意图乘上终端宽度，
  而转录里那次调用本来就在屏幕上）。
- [grilling：指针落点与覆盖层打架](issues/06-grilling-hit-regions-and-overlays.md) — **整页可点**
  （页区里除页签条任何一格都开弹窗，含条目行；拖过是选中不是点击，所以不与拖选抢）⇒ 这是对
  冻结项 7「点空白处」的**读法澄清**，不是重开。可发现性靠按钮 **`详情`**（28 列里
  `15 + 9 + 4 = 28`）而不是靠找那一小块；它照常登记进屏幕文本层（复制时跟着走）。不给提示行
  回执（覆盖层已经在屏幕上了，而 `diff-page` 那次给回执是因为它改了数据）。沿用既有指针分派：
  问卷期间可点、**已有弹窗时第一次点先关掉它**（`click_at` 第二步，spec 里照写别当 bug）。关掉
  后**什么都不还**（`DetailOpener` 照 `Files` 那一支；`close_detail` 的 match 是穷尽的）。不接滚轮 ——
  补偿是整页可点 + 弹窗里能滚。剩下的 `▸`（转录折叠标记与 picker）**不动**。
- [grilling：多 speaker 时页上那一份是谁的](issues/07-grilling-which-list-is-the-page.md) — 页上那份
  **保持「最后一个非执行者落地者胜」，规则一个字不改**；执行者的仍不进左栏（`todo-and-modes` §2）。
  归属**只写进弹窗标题**（`── 待办 · kimi ──`）：28 列挤不进第三样东西，而左栏三页一律不给色，
  给 todo 页开一个 speaker 色会是那一族里唯一的例外。抖动**接受**——讨论协议严格轮流，切换的频率
  就是发言的频率，而「刚才是谁在动手」正是这一页最该告诉读者的事。**结论：这不是投影问题**，是
  呈现的一个属性。实现提醒：`TodoPanel` 要多加一个字段记住那份的 speaker。

## 尚未明确

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。决策票全关之后这一段是空的：
     剩下的取舍（折行断词、`ⓘ` 那种 Ambiguous 字形、真机观感）都住在各票的 `## 作答` 与
     即将折出来的 spec 里，不是还没有决定的事。 -->

- （无。决策票全部关闭，本图走完。）

## 明确不做

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **不动 `todo` 工具**：形状、schema、回执、`Effect::ReadOnly`、`delegable`、规则段（`src/agent.rs`
  那段引导）一个字都不改 —— 写得太潦草是另一个话题。
- **不让前端改状态**：不做「点一项把它标成完成」。列表归模型所有，这一页是**读**的那一面
  （`.scratch/todo-and-modes/spec.md` §2）。
- **不给执行者的列表在左栏开位置**：它今天只住在转录里（[`src/render/todo.rs:11-13`](../../src/render/todo.rs)）。
- **不把这一页搬进主列**，不加键盘遍历（焦点行 / `↑`/`↓` / `Enter` 开弹窗），不接滚轮。
- **不做占比条**：`调用量` 页那套 `▓`/`░` 是先例，本图不选。
- **不给已完成项一个「展开」开关**（哪怕记在配置里）。
- **不改事件 schema、不加事件**：这一页与它的弹窗只给人看，不进 `messages`、不进模型上下文
  （与 files-page / diff-page 的弹窗同一立场）。
- **本图不产代码**：做发生在 `/to-spec` → `/to-tickets` → `/implement`。

## 进度

**决策 7/7（2026-10-09，图走完）**。charting 那张是**当场拍板**的归档；两张 research 由 subagent
当场解决；[prototype：两档宽度下页与弹窗的帧草图](issues/04-prototype-page-and-detail.md) 与后面
三张 grilling 也都是维护者当场拍板，帧草图在 [`prototype/`](prototype/)。**frontier 为空** ——
七张决策票全部关闭（权威查询仍是扫 `issues/` 里 open + unblocked + unclaimed 的票）。

**下一步是交棒**：可以折 [`spec.md`](spec.md)（`/to-spec`），再由 `/to-tickets` 拆出
`Type: implement` 的构建切片追加进本文 `## 任务清单`。折 spec 时要顺手落四件：

1. **回改** [`todo-and-modes/spec.md` §4](../todo-and-modes/spec.md) 整节，并在旧处留一行指向本目录；
2. [`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) 加一节真机观感项
   （[04 票 C 组](issues/04-prototype-page-and-detail.md) 列的五条）；
3. **把两条取舍写进 spec 的取舍段**：折行照旧硬切（`/to-tickets` 会被断成两片）、不给键盘键
   （别家都是 `Ctrl+T`，[03 票](issues/03-research-other-agents-plan-ui.md) 的事实）；
4. **一条实现提醒别漏**：既有测试 `clicking_a_todo_row_opens_nothing`
   （`tests/render_layout.rs:4389-4430`）与本页的整页可点**正面冲突**，必须一起改写
   （[02 票](issues/02-research-wiring-for-todo-page.md) 第 2、9 条）。

**2026-10-09 交棒完成**：[`spec.md`](spec.md) 已经折出来了，五十一条用户故事、九节实现决定、
两个测试 seam，`Status: ready-for-agent`。上面那四件都落在 spec 里（§9 与「补记」）。
随后 `/to-tickets` 拆出**六张实现票**（本节下方），链条 `08 → 09 → 10 → 11 / 12 → 13`，
十张验收清单都在各自的票里。

**frontier：空。** 六张实现票 2026-10-09 全部 `done`（链条 `08 → 09 → 10 → 11 · 12 → 13`），
spec 的 `Status:` 同步改成 `6 done`。此后这一页以 [`spec.md`](spec.md) 为准。
图的使命到此为止；此后实现以 spec 为准，本图留作决策记录。

### 第二处偏离（2026-10-09，维护者当场要求）

wayfinder 的规矩是「每个 session 绝不要 resolve 超过一个 ticket」。维护者以「继续剩下的几个」
要求**一次走完剩下四张**（04 prototype、05/06/07 三张 grilling），于是本会话 resolve 了四张：
每张各自认领、各自留痕（`Status: claimed → resolved`、独立 `## 作答`）、各自的 gist 追加在
`## 已定的决定` 里，**没有合并成一张**。与 [`diff-page` 那张图](../diff-page/map.md) 的第二处
偏离同一性质、不同对象（那边是三张 grilling，这边是一张 prototype + 三张 grilling）。
grilling 规程（`grilling` + `domain-modeling` 两个 skill）在这一会话里加载一次、供三张票共用，
同样明写在这里。