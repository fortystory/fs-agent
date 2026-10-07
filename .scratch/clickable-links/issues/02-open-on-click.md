# 点一下就打开：命中热区 → 解析目标 → 宿主 spawn `xdg-open`

Type: implement
Status: done
Blocked by: 01

> 规格：[`.scratch/clickable-links/spec.md`](../spec.md) §1（交互与范围）、§3（解析与打开）、
> §4（回执）、§5（边界与安全）。
> 票 [01](01-recognise-and-underline.md) 落地之前不开工 —— 本票认的是它记下来的候选。

## 目标

- 左键单击（按下与抬起落在同一格、没跨过拖选门槛）落在**对话视图转录区**的某个候选上：
  解析目标 → 产生一个打开请求 → `Tui::run` 取走并 spawn `xdg-open`。
- 拖选照旧只复制：**永不**打开（判定顺序不动，见下）。
- 目标判据：URL 原样；路径要**工作区内且真存在**（目录也算）。区外、不存在、`path:line`、
  其它 scheme —— 什么都不发生，**不留回执**。
- 看得见的反馈：成功一句 `已打开 <目标>`，失败一句 `打开失败：<原因>`，都走提示行那条回执槽位。
- 不经 shell、不经 `bash` 工具、不过权限门、不进沙箱。

## 现状（2026-10-06 量的，改前先复核）

- 指针链路：`mouse`（[`:2640-2658`](../../../src/render/tui.rs)）→ `press_at` `:2723` →
  `drag_to` `:2731` → `release_at` `:2745`：`drag.selecting` 为真先 `copy`、否则
  `click_at`。`DRAG_THRESHOLD` 当前是 **2**（挪一格仍算手抖、两格之上才起选 ——
  `selection.rs:135`），所以「点在链接上没挪动」这条判据有整整两格的容差。
- 点击分派：`click_at`（`:2782`）—— 详情覆盖层 → 问卷/待答 → 左栏 → `regions.action_at`
  → 两个「回到末尾」指示器 → 文件页 → 轨迹页那条 `over_trace`（`:2852`）→ 别的什么都不做。
- 宿主侧动作的现成模板：`copy`（`:2761`）只**算出**要写出去的字节放进 `clipboard`，
  `take_clipboard`（`:2775`）由 `Tui::run`（`:484-491`）取走并 `execute!`。挂起请求
  （`take_suspend_request`，`:3532`）同形。
- 回执：`hint_line`（`:4149`）把 `copy_receipt`（`:4176`）排在提示集合之前；措辞在
  `render/wording.rs:1275`。
- 会话 cwd 在 `TuiState::cwd`（[`:658`](../../../src/render/tui.rs)）。
- 外部进程的先例：`render/viewer.rs:160` 的 `NvimViewer::spawn`（pty 那一档）与
  `tools/process.rs`（**不经 shell** 的 argv 直传纪律，见该文件开头的文档注释）。
- 全仓没有任何 `xdg-open` / `Command::new("open")` 代码。

## 落点

- `src/render/opener.rs`（新）：argv 拼装的**纯函数**、真正的 spawn、失败原因。
- `src/render/opener.rs` 或 `links.rs`：目标解析（cwd 拼接、规范化、存在性、区内外判定）——
  纯函数的部分要能单测，落点实现定。
- `src/render/tui.rs`：`click_at` 里一条新分派、`open` 槽位与 `take_open_request()`、
  `Tui::run` 里的执行段、回执槽位。
- `src/render/wording.rs`：两句回执措辞。
- `tests/render_tui.rs`：点击分派的用例。

## 具体行为

### 1. 命中（规格 §1、§3）

- 落点必须在**对话视图这一帧画出来的转录矩形**里：与轨迹页那条 `over_trace` 对称的一条
  判据（落点不在里面就什么都不做）。左栏、页签条、状态行、提示行、输入区、详情覆盖层、
  问卷、nvim 浮层各自照旧。
- 命中判据：`click 列 - (块左缘 + lead)` 落在某个候选的列区间里。落在 Markdown 链接的
  **label**（`文字 (url)` 里的 `文字`）上不命中 —— 本层认的是画出来的目标文本。
- 命中之后立即解析（§2）；解析不出来就**当作没命中**：不产生请求、不留回执、不 mark dirty。
- 顺序不动：拖选优先的那条判定留在 `release_at`，本票不改它，但要有测试钉住。

### 2. 解析（规格 §3）

- URL 候选 → 原样作为目标。
- 路径候选 → 以 `TuiState::cwd` 拼成绝对路径、`canonicalize`（`..` 与符号链接一起解开），
  要求 `fs::metadata` 成功**且**结果 `starts_with(cwd 的规范化形式)`。
- 目录也算目标（`xdg-open` 会开文件管理器）。
- 解析不了 / 不存在 / 区外 → `None`，什么都不做。

### 3. 打开（规格 §3、§5）

- **纯函数**拼 argv：`["xdg-open", <target>]` —— 目标作为**一个** argv 元素原样传进去，
  绝不拼进 shell 字符串。文件传绝对路径字符串（不做 `file://` 编码）。
- spawn 一处：stdout / stderr 丢弃；**不等待**它退出（浏览器活多久与我们无关），但**要回收**
  子进程（`wait` 在一个不挡键盘的地方，或 `spawn` 之后显式 reap），不留僵尸。
- spawn 本身失败（`xdg-open` 不在 PATH）→ 失败原因回到回执那一路。
- 打开成功与否**不改**任何会话状态：不动模式、不推 `FrontEndEvent`、不写状态文件、不打日志。

### 4. 回执（规格 §4）

- 成功：`已打开 <目标>`（目标按列宽截断），失败：`打开失败：<一句原因>`。
- 走 `copy_receipt` 那一槽位与寿命（回执只占一句、会被提示行自己的截断规则管着）。
- 两句措辞进 `wording.rs`，与 `已复制 … 字 · … 行` 并排。
- **不命中、解析失败都不留回执** —— 点普通文字本来就该没有反应。

## 验收

- `cargo test`（`tests/render_tui.rs`）：
  - 单击 URL 热区 → `take_open_request()` 是那个 URL；再取一次是 `None`；
  - 单击工作区内存在的路径热区 → 请求是它的绝对路径；
  - 拖过同一个热区 → 没有请求（走的是复制那条路）；
  - 点普通文字 → 没有请求；
  - 点工作区外路径、点 `path:line`、点不存在的路径 → 没有请求；
  - 点左栏 / 状态行 / 轨迹页同一列 → 没有请求。
- argv 拼装与目标解析各有纯函数单测（含带空格与非 ASCII 的路径）。
- **单测绝不真的开浏览器**：真 spawn 那条路不进测试。
- 手工（`docs/tui-manual-checklist.md` 的条目在 [票 03](03-docs-and-walkthrough.md) 里落）：
  真机点一次 `/eli5` 的 `.html` 与一条 `https://` URL。
- `cargo fmt` / `cargo clippy` 干净。

## 评论

- **落地**：新增 `src/render/opener.rs`（`command()` 纯函数 + `open()`：`xdg-open` 的 argv 直传、stdin/stdout/stderr 全丢、**不等它退出**，收尸交给一条后台线程，主循环一步不等）。`links.rs` 的 `Target::resolve(&Path) -> Option<String>`（URL 原样；路径 `canonicalize` 之后要求 `starts_with(cwd)` —— `..` 与符号链接都先解开再判）。`tui.rs`：`TuiState` 加 `open_request` / `opened` / `conversation_rect` 三个字段、`take_open_request()` 与 `note_open_receipt()`、`follow_link()` 一条新分派（排在文件页之后、轨迹页之前）、`Tui::run` 里那段「取走 → spawn → 写回执」。`wording::opened` / `wording::open_failed` 两句。
- **形状**：请求槽位与 `take_clipboard` 同构 —— 状态机只**解析**（一次 `canonicalize`），起进程留给唯一那处运行期代码。于是测试能逐字断言「点这一下要打开什么」，而**一个浏览器都不会开**（票面那条「单测绝不真开浏览器」）。
- **一处改名**：`COPIED_WINDOW` → `RECEIPT_WINDOW`。打开的回执与复制的回执共用同一条寿命与同一个提示行槽位；名字里留着 `COPIED` 就成了谎话（函数 `copy_receipt` 的名字不变，它说的是那一类回执）。
- **判据五条**，一条不成立就当没点（`follow_link` 的顺序）：落在对话转录区里（`conversation_rect`，每帧在 `draw_frame` 开头清空、真画了那页才填 —— 与 `trace_rect` 同一条纪律）、落进的是这一帧真画出来的那一块（覆盖层盖着时最上层块不是转录）、那一行的列扣掉 `lead` 之后落在某个候选里、解析得出目标。**解析不出来什么都不发生、也不留回执**（spec §3）——那与点普通文字是同一种反应。
- **两页不互相抢**：对话页与轨迹页共用同一块矩形，但各自的 rect 只在真画了那一页时才有值，所以 `follow_link` 与 `over_trace` 不会同时命中。
- **验收**：`cargo test` **1366 passed / 0 failed**；`cargo clippy --all-targets` 干净；`cargo fmt --check` 通过。新测试七条 —— `opener.rs` 两条（分号/管道/反引号/$() 在 argv 里只是普通字符；含空格与非 ASCII 的路径仍是一个参数）、`links.rs` 两条（URL 解析成自身；路径只认区内且存在，`target/../Cargo.toml` 先解开再判）、`tests/render_layout.rs` 四条（点 URL 得到它本身、点工作区文件得到绝对路径、点普通文字/不存在/区外都不产生请求、拖过链接只复制不打开、左栏与提示行上点不出请求、回执落进提示行）、`tests/wording.rs` 两句措辞与「回执打头、出口保住」。
- **留给票 03 的**：真机走查（真点一次 `/eli5` 的 `.html`）与文档面 —— `docs/render.md` 的新节、`tui-feedback` §3 的边界补记、手工清单与索引票数。
