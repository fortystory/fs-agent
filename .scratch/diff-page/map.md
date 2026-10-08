# 左栏「改动」页：改动的文件 + 点开看那份 diff（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-08，三轮 grilling 共 16 问：终点形态与范围 / 改动口径 / 列表内容 /
弹窗形态 / 顺序 / 刷新时机 / 谁跑 git / 上色档次 / 页签出现与不可用态 / 键盘 / 取数时机 /
外部工具那一档的形状与选中方式 / 页签名）。本图只做**规划**，不产代码改动。

> **来源**：[`seed.md`](seed.md)（2026-10-07 在一轮 `/ask-matt` 里记下的一句话意向，同日核实过
> 现状）。那次核实的《现状》一节仍然成立，charting 时补查的四组事实见
> [grilling：这一页的形态与手势](issues/01-charting-decisions.md) 与
> [research：宿主侧子进程、ANSI 与 `[ui]` 配置的接线](issues/03-research-host-side-subprocess.md)
> 的「已知起点」。

## 目的地

一份**可执行的 spec**（交给 `/to-spec` 折成构建计划，再由 `/to-tickets` 拆实现票）：把左栏那个
今天还不存在的 **`改动` 页**定清楚 —— 它列什么算「改了」、一行画什么、怎么排、鼠标与键盘各做
什么、点开一份 diff 看多深、那份数据什么时候重取、以及「把这份 diff 交给外部工具」那一档长
什么样。**走到这张图的尽头时，实现这一页不再有任何需要先决定的事**，剩下的只是接线与断言。

**范围**：`src/render/`（新页本身、`wording.rs`、`palette.rs`、`layout.rs`，以及一个新的
宿主侧取数件）、[`CONTEXT.md`](../../CONTEXT.md) 的相关词条、[`docs/render.md`](../../docs/render.md)
的「改动页」一节、[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) 的锚点。
**事件 schema 不动；模型可见文本一个字节不动；`FileIndex` 的遍历规则一个字不改。**

## 笔记

- **领域**：衡（heng）的 TUI 外壳 + 一层新引入的 git 取数（宿主侧、只读）。
- **起点**（charting 实测，每条带证据）：
  1. 左栏今天三页：`Tab::{Usage, Todo, Files}`（[`src/render/tui.rs:5789`](../../src/render/tui.rs)），
     页签文案是 `wording::TAB_USAGE` / `TAB_TODO` / `TAB_FILES`
     （[`src/render/wording.rs:1874`](../../src/render/wording.rs)）。页签**点出来、不给键位**。
  2. **页签条放得下第四签**：算式是 `调用量`6 + `┆`1 + `todo`4 + `┆`1 + `文件`4 = 16 列，再加
     `┆ diff`… → `改动`（4 列）后 21 列，窄档 28 列还剩 7 列填 `┄`（`draw_tab_bar`，
     [`src/render/tui.rs:5435`](../../src/render/tui.rs)；窄档见
     [`src/render/layout.rs:66`](../../src/render/layout.rs)）。
  3. **仓库里没有 git 集成**：`src/` 里 `git` 只作为命令名出现在权限策略与文案里，
     没有任何地方跑过 `git` 子进程。
  4. **渲染层没有「跑一次子进程、收 stdout、带超时」的形状**：`opener.rs` 起 `xdg-open` 时
     三条 stdio 全 null、**不等退出**（`child.wait()` 丢给后台线程），没有超时也没有 kill；
     `viewer.rs` 的 nvim 那一档是交互式 pty（一屏外来终端）。完整形状在
     `src/tools/process.rs`（tokio + 进程组 + 超时），但它在**工具域**、过沙箱与权限门，
     渲染器借不到 —— 所以「宿主侧跑一次 git」是**新的活**。
  5. **diff 上色的件是现成的，但今天没有调用方**：`diff_tag(&str) -> DiffTag` 吃一整行吐
     四值（`Context`/`Added`/`Removed`/`Hunk`），`DiffTag::style` 吐的是**背景**样式（正好与
     语法前景叠加）；`highlight_diff` 吃整段吐逐行 span，但它**写死按 Rust 高亮**。
     diff 的三个颜色**不在 `palette.rs` 里**（写死在 `highlight.rs`）。
  6. **弹窗那一半有模板**：详情覆盖层五个 `DetailKind`（含 files-page 加的 `File`），
     `detail_body` 是唯一 switch；`file_body_lines` 就是「前缀列 + 折行」的模板
     （行号列从折行预算里扣、插在折行之前）。加一个变体 = enum 加一项 + switch 加一支 +
     一个 `open_*_detail` + 一个 `DetailOpener` 变体（照 `DetailOpener::Files`，它什么都不冻）。
  7. **刷新信号已经有**：非只读工具调用收尾时那条静默的 `WorkspaceChanged`
     （[`files-page` §2](../files-page/spec.md)）今天喂着 `FileIndex` 的重扫。
- **要咨询的 skills**：`/research`（票 02、03）、`/prototype`（票 04）；折 spec 时 `/to-spec`。
- **每张票的答案必须自足**：`/to-spec` 会在别处的会话里读它，看不到本图与 charting 对话。

### 冻结项（charting 的 grilling 定下，票里不得重开）

理由与逐条细节在 [grilling：这一页的形态与手势](issues/01-charting-decisions.md)：

1. **终点是一份 spec**，本图只产决策，不写代码。
2. **页是左栏常驻第四签，页签写 `改动`**（不是条件出现，不是英文 `diff`）。
3. **「改了」= 相对 HEAD**：已暂存 + 未暂存 + **未跟踪**（`??`）；不是「工作区 vs index」。
4. **列表一行 = 文件名 + 状态字形**（`M`/`A`/`D`/`??`），**不带** `+N/-M` 统计。
5. **列表顺序 = 先按状态分组，组内路径字典序**（不是纯字典序，不是会话碰过的先后）。
6. **弹窗 = 那一份 diff，整份可滚**；不做逐 hunk 折叠，也不给「下一个文件」。
7. **键盘照文件页那套**（点左栏接管键盘、焦点行、`↑`/`↓`、`Esc` 还键盘不取消回合），
   但 **`Enter` = 开那份 diff**；`@路径` 插入是文件页的语义，**不搬过来**。
8. **正文点开时才取**（列表只跑 status，点开一个文件才为它单跑一次 `git diff HEAD -- <path>`）。
9. **git 由渲染器自己在宿主侧跑**：不进事件流、不进 `messages`、不过权限门与沙箱（只读、不写盘）。
10. **刷新**：借既有的 `WorkspaceChanged` + 提交之后，**另给一个手动刷新键**（键位未定，见票 05）。
11. **上色内置兜底**：现成的 `diff_tag` 背景色 + **按扩展名挑语言**的 `highlight_code`
    （`highlight_diff` 写死 Rust 那处要改）；另有一档 **`[ui] diff_viewer`**：写成外部命令时
    把那份 diff 交给它、**一次性输出当静态正文**（不是内嵌一屏外来终端）。
12. **不可用态**：不是 git 仓库 / `git` 不在 `PATH` → **页签仍在**，页里写一句。
13. **范围**：这一页**只读**，不做任何 git 写操作，不做 conflict / rebase / 历史浏览，
    不动文件索引，不把 diff 塞进模型上下文。

### 会撞的既有决定（`/to-spec` 时回改，不在本图改）

- **[`files-page/spec.md`](../files-page/spec.md) §6**：详情覆盖层那套（`DetailKind::File`、
  `DetailOpener::Files`、`file_body_lines` 的「前缀列 + 折行」、有界截断与空/失败态文案）是
  这一页弹窗的模板；「渲染器直接读盘、不进事件流、不进模型上下文」那条立场**照旧适用**。
- **[`nvim-file-viewer/spec.md`](../nvim-file-viewer/spec.md) 与 [ADR 0013](../../docs/adr/0013-nvim-file-viewer-is-an-alien-screen.md)**：
  「`[ui]` 开关挑外部呈现」的先例是它；但 diff 这一档选了**一次性输出**，不是 alien screen ——
  两档的代价不同（不占键盘、不需要 pty、不需要 `respond_to`）。
- **[`clickable-links/spec.md`](../clickable-links/spec.md) 与 `src/render/opener.rs`**：
  「宿主侧起进程、不过权限门、不进流、失败给一句回执」的先例是它。新那件的差别：
  **要收 stdout、要等退出、要超时**。
- **[`tui-visual-language/spec.md`](../tui-visual-language/spec.md)**：语义色板是唯一的取色处，
  **diff 三色今天写在 `highlight.rs` 里**——收不收进色板由 spec 定，不新起体系。
- **[`docs/highlight.md`](../../docs/highlight.md)**：`diff_tag` / `DiffTag::style` /
  `highlight_diff` 是当年为**工具输出**留的一层，自述「仍然没有调用方」；这一页是它的第一个
  真调用方。
- **[`input-tokens/spec.md`](../input-tokens/spec.md) §1**：`FileIndex` 的遍历规则（忽略规则、
  字典序、隐藏文件与 `.git/` 的挡法）是它的契约，本图**只消费、不改**。
  **注意这一页不消费它** —— diff 是 git 的口径，不是遍历的口径。
- **[`todo-and-modes/spec.md`](../todo-and-modes/spec.md) §4**：`todo` 页签「第一次提交才出现」
  的先例 —— `改动` 页**有意不照抄**（常驻）。
- **[`sidebar-toggle/spec.md`](../sidebar-toggle/spec.md)**：左栏两档宽度与 `Ctrl-O` 意愿不动。

### Tracker 事实与降级（本图适用）

- map = `.scratch/diff-page/map.md`，child = `.scratch/diff-page/issues/NN-*.md`；
  阻塞 = 票面 `Blocked by: NN`；claim = `Status: claimed`；resolve = 票底 `## 作答` +
  `Status: resolved` + 追加一行到本文 `已定的决定`。
- **没有 native sub-issue / 依赖边**，回退到正文约定：本文 `## 任务清单` 逐条引用子票
  （条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/diff-page/map.md`。
- **一处偏离要说在明处**：这次 charting 时维护者就在场，16 条决定是**当场拍板**的，不是走票得来
  的。它们收进 [grilling：这一页的形态与手势](issues/01-charting-decisions.md) 并标了
  `resolved` —— 那些决定确实已经做完，而「细节只住一个地方」（图是 index，不是 store）比
  「charting 不 resolve 票」更硬。除此之外的票都按常规走。
- **第二处偏离（2026-10-08）**：最后三张票（[grilling：刷新、陈旧与手动那把键](issues/05-grilling-refresh-and-staleness.md)、
  [grilling：改动页与文件页之间要不要一个指针](issues/07-grilling-page-relationship.md)、
  [grilling：未跟踪的粒度](issues/08-grilling-untracked-granularity.md)）是**在同一个会话里一起走完的**，
  而 wayfinder 的规矩是「每个 session 绝不要 resolve 超过一个 ticket」。理由：维护者明确要求一次
  走完，而这三张都是同一族的短 grilling（合起来七问，一轮问完）。三张的认领、作答与关闭各自
  留痕，没有合并成一张。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**决策票**（八张）：

- [x] [grilling：这一页的形态与手势（charting 当场拍板）](issues/01-charting-decisions.md)
- [x] [research：git 的三个读数与输出的稳定性](issues/02-research-git-readouts.md)
- [x] [research：宿主侧子进程、ANSI 与 `diff_viewer` 的配置接线](issues/03-research-host-side-subprocess.md)
- [x] [prototype：列表与弹窗在两档宽度下的排版](issues/04-prototype-layout.md)
- [x] [grilling：刷新、陈旧与手动那把键](issues/05-grilling-refresh-and-staleness.md)
- [x] [grilling：外部工具那一档（`diff_viewer`）的形状](issues/06-grilling-external-viewer.md)
- [x] [grilling：改动页与文件页之间要不要一个指针](issues/07-grilling-page-relationship.md)
- [x] [grilling：未跟踪的粒度（目录一行，还是展开成文件）](issues/08-grilling-untracked-granularity.md)

**实现票**（`/to-tickets` 从 [`spec.md`](spec.md) 拆出的七张，2026-10-08）：

- [x] [09 — 改动页画出一列改动的文件（tracer bullet）](issues/09-diff-page-list.md)
- [x] [10 — 键盘交给这一页（焦点行）](issues/10-focus-row-and-keyboard.md)
- [x] [11 — 点开一份 diff](issues/11-open-the-diff.md)
- [x] [12 — 那份 diff 上色](issues/12-diff-coloring.md)
- [x] [13 — 刷新与手动重取](issues/13-refresh-and-manual-reread.md)
- [x] [14 — 交给外部工具那一档](issues/14-external-diff-viewer.md)
- [x] [15 — 收口：真机清单与验收](issues/15-close-out.md)

**frontier = 空（决策票这一侧，2026-10-08，图走完）**：八张决策票全部关闭，权威查询仍是扫
`issues/` 里 open + unblocked + unclaimed 的票。

**实现票 7/7 done（2026-10-08）**：frontier 曾经是 [09 — 改动页画出一列改动的文件](issues/09-diff-page-list.md) —— 只有它
无阻塞；其余六张各有 `Blocked by:`（10←09、11←10、12←11、13←09、14←11、15←12 与 13 与 14）。
`Type: implement` 的票由 `/implement` 认领，不走 wayfinder 的会话。

## 已定的决定

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [grilling：这一页的形态与手势](issues/01-charting-decisions.md) — charting 那 16 问的答案：
  页是**常驻第四签 `改动`**（28 列下 21 列，放得下）；「改了」=**相对 HEAD**（已暂存 + 未暂存 +
  未跟踪）；列表一行 = **文件名 + 状态字形**、**先按状态分组再字典序**、不带统计；点开是
  **那一份 diff、整份可滚**（照详情覆盖层 `File` 那条路），不给「下一个文件」；键盘照文件页那套
  但 `Enter` = 开 diff；正文**点开时才取**；**git 由渲染器在宿主侧自己跑**（不进流、不过权限门）；
  刷新借 `WorkspaceChanged` + 提交 + 一把手动键；**上色内置兜底**（现成 diff 三色 + 按扩展名
  语法高亮），另有一档 `[ui] diff_viewer` 外部工具（一次性输出当静态正文）；不是 git 仓库时
  **页签仍在、页里写一句**；范围只读。
- [research：git 的三个读数与输出的稳定性](issues/02-research-git-readouts.md) — 三个读数确定：
  列文件走 `git status --porcelain=v2 -z`（`-z` 是 NUL 结尾、路径原样），正文走
  `git diff HEAD -- <path>`（**对未跟踪文件是空输出 + 退出码 0** —— 未跟踪那一档得直接读盘）；
  要锁的开关逐条实测过（`--no-color`、`--no-ext-diff`、`--src-prefix=a/ --dst-prefix=b/`、
  `--no-pager`、`--unified=3`、`-unormal`/`-uall`、`git --no-optional-locks`、清
  `GIT_DIR`/`GIT_WORK_TREE`）。三处出乎意料：**`git status` 默认会写回 index**（不锁就是动用户的
  仓库）；**`--porcelain=v2` 仍跟随用户配置**（`status.relativePaths`、`status.showUntrackedFiles`），
  只有 v1 有「不受配置影响」的明文保证；**错误文案随 locale 本地化**（别靠英文短语判定）。
  失败态：非仓库 `status`=128 / `diff HEAD`=129，无 HEAD 的空仓库里 `diff HEAD`=128 而 `status` 照常，
  `git` 不在 `PATH` 是 `ErrorKind::NotFound`。超时依据：最坏实测 10 万未跟踪文件 + `-uall` ≈ 60 ms
  （真仓库 10537 文件 23–25 ms）⇒ 1 秒已很宽。
- [research：宿主侧子进程、ANSI 与 `diff_viewer` 的配置接线](issues/03-research-host-side-subprocess.md) —
  **零新依赖**：`tokio` 的 `time`/`process`/`rt` 与 `libc` 都在，超时用 `tokio::time::timeout` +
  `process_group(0)` + `kill_on_drop` + `killpg`（一个临时 example 真编译跑通过，探针已删；
  `ProcessGroup` 今天在 `process.rs` 是**私有**的，借用要提公共或复制）；异步回填的现成形状只有
  `file_index` 那一套（三态 + 位 + `mpsc` + `*_loaded`），**git 取数必须 `tokio::spawn` 出去、
  不能在任何一支里 await**（一帧的 `select!` 只等通道与两个定时器，节拍 60 ms）；进程卫生里
  `GIT_DIR`/`GIT_WORK_TREE` **必清**（实测污染会改语义）；**`ansi_line` / `DiffTag::ansi` 是产出
  ANSI 的表、不是解析器**（`#[cfg(test)]` 外无调用方）⇒ 外部工具吐的 ANSI **今天没有现成解析件**；
  `[ui]` 项加一个要动 8 处（含 `SessionFacts` 与 10 个完整字面量），带参命令在 `[tools.*]` 里是
  argv 数组、在 `[ui]` 里是新文法；`WorkspaceChanged` 那条链逐处行号在 findings 表里，
  合并守卫照抄「一个位 + 在飞时不发、位留着补发」；**`layout.rs` 与启动检查脚本都不用动**，
  但「新增左栏页」的清单**不存在**（手工清单要自己加一节）。

- [prototype：列表与弹窗在两档宽度下的排版](issues/04-prototype-layout.md) — 帧草图是**可执行的**
  （`.scratch/diff-page/prototype/frame.py`，一条命令重画两档）：宽档 120×24 ⇒ 页区 **40 列 × 15 行**、
  窄档 80×24 ⇒ **28 列 × 19 行**；字形列**占满两格**（`M `/`A `/`D `/`??`，名字严格同列）、
  路径取**全路径、超宽尾部 `…`**（窄档下名字拿 25 列，实测那组路径一条没截）、
  分组标题**占整行、不带计数、不给色**、空组不画；`XY` 折字形取「工作区那一位优先」；
  **放不下就画满 + 末行写「还有 M 处改动」**（不加滚动，那是新机制）；弹窗标题只放路径
  （未跟踪标「**新文件**」）、正文宽用 `Regions::detail_text_width()`（框宽 − 4 —— 那处落差
  files-page 票 05 **已经修掉**，照抄即可）、行号列 = `最长行号位数 + 1`（新增/上下文取新号、
  删除取旧号、hunk 头与头行留空）、**头行四行不留**（`@@` 那行留）；四句文案取定
  （`没有改动` / `这里不是 git 仓库` / `找不到 git` / `正在读取改动…`）。

- [grilling：外部工具那一档（`diff_viewer`）的形状](issues/06-grilling-external-viewer.md) —
  **做全档**：`diff_viewer = "delta"` 配 `diff_viewer_args = ["--paging=never"]`（程序名 + argv
  数组，按整元素替换、绝不过 shell；不写 = 内置），**diff 走 stdin**；我们**只给环境变量**
  （`PAGER=cat`、`GIT_PAGER=cat`、`COLUMNS=<正文宽>`）、一条命令行参数都不注入；输出按
  **「只认 SGR 子集」**画（其他 CSI 整段丢掉，认不出的颜色档退成 PLAIN），画完仍走我们自己的
  折行与截断；失败/超时（2 秒）**回退内置 + 提示行一句回执**；标题右端**标出工具名**；
  **拖选复制出来仍是文本**（ANSI 解成 span，不是留在文本里）。前提事实：**这台机器上
  `delta` / `diff-so-fancy` / `colordiff` / `difft` 一个都没装**，所以「真跑一遍 delta」跑不了，
  替身是 [`prototype/external_tool.py`](prototype/external_tool.py)（假工具吐真 SGR + 解析读数）。
  这一档的颜色**不经过语义色板** ⇒ 折 spec 时落一条 **ADR**（与
  [ADR 0013](../../docs/adr/0013-nvim-file-viewer-is-an-alien-screen.md) 同族）。

- [grilling：刷新、陈旧与手动那把键](issues/05-grilling-refresh-and-staleness.md) — 手动刷新 =
  **键盘在改动页时的 `r`**（`r` 今天没人用，落在文件页那套键盘归属上），回执走提示行一句；
  **陈旧不表达**（不画「上次取数时刻」、不做阈值提示）；**不做 watch**（工作区与 `.git/index`
  的 mtime / inotify 都不盯 —— 进「明确不做」）；合并与不丢照抄 `file_index` 那道守卫
  （一个位 + 在飞时不发、位留着补发），但**用自己的一位**、不共用 `file_scan_wanted`；
  **页面不在前台也照旧重取**；`/undo`、提交之后、执行者与讨论者的调用都算触发点；
  **失败保留上一次读数 + 提示行回执**（页里只在还没取到数时写 `正在读取改动…`）。
- [grilling：改动页与文件页之间要不要一个指针](issues/07-grilling-page-relationship.md) —
  **不做任何跨页指针**：点一行与 `Enter` 都只开那份 diff（两页各自回答一个问题）；`@路径`
  插入**不搬过来**（这一页的键位就五个：`↑` `↓` `Enter` `Esc` `r`）；文件页也**不反向指路**
  （那要它知道 git 状态）。后两条一起进「明确不做」。
- [grilling：未跟踪的粒度（目录一行，还是展开成文件）](issues/08-grilling-untracked-granularity.md) —
  **全展开（`-uall`）**：列表里一行永远是一个文件，与「一行 = 一个文件 + 一个状态字形」这条
  不变式一致；代价实测很小（10 万未跟踪文件 + `-uall` ≈ 60 ms），而 `.gitignore` 挡住的目录
  根本不出现。上限不另立，照 [prototype：列表与弹窗在两档宽度下的排版](issues/04-prototype-layout.md)
  那条「画满 + 末行一句」。

## 尚未明确

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

- **已暂存 / 未暂存的区分**：porcelain 的 `XY` 两列能表达两处都有改动（`MM`），而冻结项只定了
  「一个状态字形 + 按状态分组」。要不要把「已暂存」当独立的组、或者用两个字形，留给 spec。
- **改名 / 复制 / 冲突的字形**：`R` / `C` / `U` 今天没进字形集。冲突**不做解决**（范围），但它
  出现在列表里怎么画还没有答案。
- **一份 diff 读到第几行截断**：那个数（以及「还有 M 行」里 M 怎么算）随 spec 定 —— 它是选一个
  数，不是决策。二进制那句话与「几千个改动文件怎么办」已由
  [prototype：列表与弹窗在两档宽度下的排版](issues/04-prototype-layout.md) 取定。
- **页签上要不要带读数**（比如改动件数）——`调用量` / `文件` 都不带，倾向不带。

## 明确不做

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **任何 git 写操作**：暂存、提交、丢弃、切分支、`git add -p` 那一类都不做（它们要过权限门，
  是另一条路）。
- **conflict 解决、rebase、`git log` 的历史浏览**。
- **把 diff 塞进模型上下文**：这一页与它的弹窗只给人看，不进 `messages`、不进事件流、不打码
  （与文件页弹窗同一立场，[`files-page/spec.md`](../files-page/spec.md) §6）。
- **逐 hunk 折叠、弹窗里翻上/下一个文件、搜索与过滤**。
- **外部工具的一屏外来终端**（内嵌 nvim 那一档的复用）：`diff_viewer` 是一**次性输出**，
  不占键盘、不需要 pty。
- **外部分页器那一套**（`delta` 自己的 pager / 交互模式）：输出交回来之后由我们画。
- **改动页与文件页共用一份索引**：git 的口径与遍历的口径不是一回事，两页各自取数。
- **两页之间的跨页指针**：点改动页的一行跳去文件页、在改动页插 `@路径`、或让文件页的焦点行
  去看 diff —— 三条都不做（[grilling：改动页与文件页之间要不要一个指针](issues/07-grilling-page-relationship.md)：
  两页各自回答一个问题，要看文件本身去 `文件` 页）。
- **watch 工作区**：不盯文件树的 mtime、也不盯 `.git/index`，所以**在外部 shell 或编辑器里
  做的改动不会自己冒出来** —— 那是 `r` 与 `WorkspaceChanged` 分工的结果
  （[grilling：刷新、陈旧与手动那把键](issues/05-grilling-refresh-and-staleness.md)）。
- **本图的执行**：本图只产决策。「做」发生在 `/to-spec` → 实现票 → `/implement`。

## 进度

**决策 8/8（2026-10-08，图走完）。** 图上八张决策票全部关闭：charting 那张是**当场拍板**的归档，
两张 research 由子代理解决（findings 在 [`research/`](research/)），
[prototype：列表与弹窗在两档宽度下的排版](issues/04-prototype-layout.md) 与三张 grilling
（[外部工具那一档](issues/06-grilling-external-viewer.md)、
[刷新与陈旧](issues/05-grilling-refresh-and-staleness.md)、
[两页之间的指针](issues/07-grilling-page-relationship.md)、
[未跟踪的粒度](issues/08-grilling-untracked-granularity.md)）由维护者当场拍板，两份帧草图在
[`prototype/`](prototype/)。

**下一步是交棒**：设计票全关 ⇒ 可以折 [`spec.md`](spec.md)（`/to-spec`），再由 `/to-tickets`
拆出 `Type: implement` 的构建切片交给 `/implement`。折 spec 时有两件要顺手落：一条 **ADR**
（`diff_viewer` 那一档的颜色不经过语义色板，与
[ADR 0013](../../docs/adr/0013-nvim-file-viewer-is-an-alien-screen.md) 同族）、以及
`docs/tui-manual-checklist.md` 的一节真机观感项（两档下的密度与截断、diff 两色在真配色下够不够分、
外部工具那一档装了真工具之后的观感）。

> **2026-10-08 交棒完成**：[`spec.md`](spec.md) 已经折出来了（同一轮，八张票的答案综合成问题陈述 /
> 方案 / 五十九条用户故事 / 九节实现决定 / 测试决定），`Status: ready-for-agent`。图的使命到此为止；
> 此后实现以 spec 为准，本图留作决策记录。ADR 与手工清单那两件挪进 spec 的 §8 与 §9。
