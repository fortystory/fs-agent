# 四分区全屏 TUI（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 `docs/agents/issue-tracker.md`
Charting: **已完成**（2026-09-14）。本图只做**规划**，不产实现代码。

## Destination

一份 **spec-ready 的四分区全屏 TUI 决策集**，交给 `/to-spec` 折叠成 `.scratch/tui-layout/spec.md` + 实现票。

四个分区（用户的原始提案）：**上 = 基础信息**（logo / 项目路径 / 时间日期 / 版本），**中左 = 对话内容**，**中右 = 信息面板**（模型 / token / 上下文用量），**下 = 输入框 + 快捷键提示**。

**直接动机**：inline viewport 下光标不跟随 `>`、`>` 在后续回合消失（`e25097e` 底部锚定之后**仍然存在**）。用户决定**用换布局绕过**——让对话面板没有光标——而**不是**继续调 inline 光标。因此：

- 本图会**推翻 spec §19 的 inline viewport 决策**（`.scratch/fs-agent-v1/spec.md:526`；用户故事 129 的"否决 alt screen"），需要一条 ADR 记录后果 —— 见票 09。
- 修 inline 光标 bug **不在本图范围内**，也不在 spec 范围内（见 Out of scope）。

## Notes

- **领域**：`fs-agent` —— 自用 coding agent CLI，Rust，从零实现，单进程单二进制。现有渲染层在 `src/render/`（`tui.rs` / `markdown.rs` / `wording.rs` + plain / headless 两个兄弟渲染器）；TUI 栈 = `ratatui 0.30` + `crossterm 0.29`（`event-stream`），**不加新 crate**（本机 registry 只读，`cargo add` 拉不下来）。
- **本轮 charting 的 grilling 结果 —— 已定，票里不得重开**：
  1. **目的地 = spec + 票，纯规划**；可以推翻 spec §19；信息面板**只放模型 / token / 上下文**；布局是**绕开**光标 bug 的手段，不是把 bug 顺手修掉。
  2. **上 alt screen**，接受"终端原生选择只覆盖可见区"。header = 名称+版本 / cwd / 模式 / 时钟。右栏内容 = 模型 / token（已用+预算）/ 上下文用量 / 轮次。**降级顺序：先隐藏右栏，再压 header**；最小尺寸 **40×10**。**删除 TUI 的 inline 路径**（`insert_before` / `paint_scrollback` / 视口保留 / `MoveTo` 底部锚定）。
  3. ~~**不开鼠标捕获**~~ → **2026-09-21 票 03 grilling 改判：开鼠标捕获**（换来①滚轮滚转录，每格 3 行 ②可点击的「到最下」；代价 = 选文本/复制要按住 Shift 拖拽）。转录历史**上限 20 000 源行**（宽度无关；滚动与滚动条另按显示行算），超出丢最旧。**吸底，但用户上滚后不抢**；提交时无条件回到底部。
  4. **多行输入这一轮就要**：**Enter 提交**；**Ctrl-J 换行**（唯一可靠的换行键）；**Shift+Enter 等同提交** —— 2026-09-21 票 04 grilling **修正**：用户选择**不启用**键盘增强协议，Shift+Enter 与 Enter 在协议层不可区分（research §7.2），因此它**不得被广告成「换行」**；输入区**随内容长高、上限 10 行**；**启用 bracketed paste**（粘贴多行不得触发提交，超 100 000 字符先确认）；**Esc 清空多行草稿前先确认**；斜杠命令**只看第一行**。
- **数据源事实（决定了范围边界）**：模型名、token 用量（`UsageRecorded`）、预算、上下文用量（`context::usable_input`）都有来源；**fs-agent 没有 todo 状态、也没有 todo 工具**（`todo_write` 属于 DSH harness，不是 fs-agent 的能力）——所以原始提案里的"todo 列表"在右栏**没有数据源**，见 Out of scope。时间/日期由系统时钟提供。
- **领域约束（不得违反）**：`src/render/wording.rs` 是**人面向中文措辞的唯一真相源**，新布局只能改"显示在哪"，不能绕过它自己拼字符串；ADR 0001 冻结的是**模型可见文本**（debater/synthesizer system prompt、投影的 `[轮 N · 名字]` 前缀、`AgentError.message`、fs-agent 工具结果），**冻结清单上没有渲染层文案**，所以本 effort 不触碰冻结面。
- **验收基线（新票不得让它变红）**：`cargo test` 489 passed / 0 failed；`cargo clippy --all-targets` 干净；`cargo fmt --check` 除 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移外干净（**不要**顺手格式化这两个文件）。渲染相关的既有测试：`tests/render_tui.rs`（26 个）、`tests/render_markdown.rs`、`tests/wording.rs`、`tests/render_delivery.rs`（provider 突发导致渲染器饥饿的回归，修法是 `sse_stream` 每 16 个解码事件 yield 一次）。
- **每张票的答案必须自足**：`/implement` 会在 `/clear` 之后的新会话里读它，看不到本图与 charting 对话。
- **架构深度 = 决策级**：布局几何、模块/接缝归属、可验证的检查清单。**不要**定到逐个私有函数，也不要顺手写实现。
- **跨票约束**：票 05 已要求 `TuiOptions`（`src/render/tui.rs:128`）新增注入字段，票 07 大概率也要动终端初始化与同一个结构 —— 改一张时别忘另一张。票 05 还把「**模式**从流上推」定死了（见 Decisions so far），header 的呈现必须用同一来源，不许另维护一份。票 03 与票 04 必须共用**同一张键位表**（两票票面已互相点名）。

## Frontier

**本图 9 张票全部 `resolved`（01–09），已折叠完成，本图关闭。** 产物：

- **`.scratch/tui-layout/spec.md`**（`Status: ready-for-agent`）—— 目的地本身。
- **实现票 10–16 —— 全部 `Status: done` 并已在 `main` 上落地**（就在本目录 `issues/` 下，编号接在设计票之后）：10 全屏骨架（生命周期 / 几何 / 降级 / 提示行 / 管线 / 删 inline 路径）、11 对话面板（滚动 / 缓存 / 吸底 / 鼠标）、12 输入编辑器（`Input` / 多行 / 粘贴 / 两个确认）、13 信息面板、14 模态覆盖层、15 CLI 斜杠命令、16 测试与验证迁移。
- **`docs/adr/0002-fullscreen-alt-screen-tui.md`** —— 推翻 spec §19 的记录。
- **证据**：`research/01-ratatui-crossterm-fullscreen-api.md`（API 事实与 ⚪ 清单）、`prototype/`（`geometry-table.md` + `chosen-*.txt`，实测像素）。

**不要再往这张图加票。** 实现阶段暴露的问题回到 `spec.md` 与对应实现票去改；本图是决策存档。

实现收尾（2026-09-21）：票 10–16 全部完成，各自过了一轮两轴 `/code-review`。**spec 无需回改** —— 票 16 实测的提示行阶梯（40→3 / 60→5 / 80→6 / 120→7）与 §10 记的一致，`fmt` 基线也仍是 §13 记的既有漂移。票 16 计划里被实测推翻的两处（header 锚点改为取自二进制自己的 `--version`、且在重建后的屏幕上匹配；边框断言按横竖分开数 —— 转录与面板之间的接缝会活下来）记在票面勾选项里。实现期另修掉一个用户报告的回归：**空输入行按回车会退出会话**（`submit()` 把空草稿发成 `None`，被循环读成 stdin 结束）。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里，本文件不复述。按名字引用，不写裸编号。 -->

- [research：ratatui 0.30 / crossterm 0.29 全屏布局、滚动与键盘输入的 API 事实](issues/01-research-ratatui-crossterm-fullscreen-api.md): 全屏自 `ratatui::init()`（raw mode + alt screen + panic hook），`TerminalOptions` 只有 `viewport` 一个字段且 `Fullscreen` 是默认值；`draw` 走 buffer diff，非整屏重写。`insert_before` 与 `Viewport::Inline` 仍在（对 Fullscreen 是 no-op），但 `Paragraph::line_count` 被 unstable 特性挡成 `pub(crate)`、当前依赖下不可调用，票 03 的换行行数需另找入口。完整事实与 ⚪ 清单见 research 文件。
- [grilling：右侧信息面板的内容、数据源与刷新节奏](issues/05-grilling-info-panel-data-and-refresh.md): 静态事实（会话 id / cwd / 模型 / 上下文窗口 / 预算上限）在组装期注入 `TuiOptions`，动态数字从流推；**模式不进注入** —— 进计划模式是 `ContextInjected{PlanMode}`、离开是 `HistorySuperseded{ModeChange}`，两条都在流上，注入的初值一按 Shift+Tab 就过期。上下文分子 = 最近一次请求的 `input_tokens`（provider 实测，不用 `chars/4` 估算）；token 显示已用/上限 + 输入/输出 + 缓存明细，**不显示花费**；标签按 `CONTEXT.md` 用**「回合」**而不是「轮次」（数 `TurnEnded`）。
- [prototype：四分区几何与降级阶梯](issues/02-prototype-four-pane-geometry-degrade-ladder.md): **24 张截图全部是真渲染**（`TestBackend` 在默认特性下无特性门，这解掉了票 01 的未知项，也给票 07/08 留下无 pty 的集成测试后端）。基线几何：右栏 `clamp(26%×w, 26, 30)` + 1 列暗色竖线；header 行数**只由宽度定**（`w≥60` 两行、`w<60` 一行并丢 cwd），这样才不倒序违反「先隐藏右栏、后压 header」。阈值：`w<40 或 h<10` → 唯一「终端太小」；`w<80` → 隐藏右栏；内容宽 <29 → 丢 `（6%）`；行数不足 → 先丢缓存、再丢输入/输出；**40×10 不是「太小」**，渲染完整降级布局。输入区 `clamp(草稿行, 1, min(10, h−header−提示行−1))`。手感项（分割比例、分隔样式、留白、header 分隔符、续行缩进）原为**待用户看图定**；**2026-09-21 用户选定并已重算**：四区各自 `Block` 边框（header 底边**就是**那条横线，两框共用接缝 1 列）+ airy 留白 + ` · ` + 续行缩进 2 格 + 右栏 `clamp(⌊26%w⌋, 25, 31)`。**固定 chrome = 7 行**；右栏最小出现尺寸 **80×16**；`120×24` 草稿涨到 10 行时**右栏整栏消失**（最反直觉的一条）；**40×10 保持冻结**（airy 在 h≤11 降级，含 airy 的完整基线最小 40×12 / 60×13）。产物在 `prototype/`（`chosen-*.txt` 为选定版，旧 `screens-*`/`variant-*` 已标作废）。
- [grilling：多行输入编辑器](issues/04-grilling-multiline-input-editor.md): **Enter 提交、Ctrl-J 换行、Shift+Enter 等同提交**（用户选择不开键盘增强协议，所以它不可区分、不得广告成换行 —— 这修正了 charting 时的冻结项）。编辑器拆成独立的 `Input` 类型（新建 `src/render/editor.rs`），光标的唯一真相源是「逻辑行 + 行内字符偏移」，显示行列渲染时算。键位：`↑/↓` 只移光标（历史专归 `Ctrl-P/N`）、`Home/End/Ctrl-A/Ctrl-E/Ctrl-U/Ctrl-K/Ctrl-W` **全部按当前逻辑行**、`←/→` 跨行、`Backspace/Delete` 跨行合并。粘贴走 `Event::Paste`（crossterm 不归一不清理，`\r\n` 归一与控制字符过滤自己做），**永不提交**，超 100 000 字符先确认；**Esc 清空多行草稿前也先确认**（两者复用既有 pending-question 机制，默认答案「否」）。斜杠命令**只看第一行**，其余行拼进 task。
- [grilling：对话面板的滚动与渲染缓存模型](issues/03-grilling-conversation-pane-scroll-and-cache.md): 上限按**源行**（宽度无关）算 20 000，滚动与滚动条另按显示行；换行沿用 `wrap_take` 逐字符口径，**不开** `unstable-rendered-line-info`；全量换行 + 源行→显示行索引，只在宽度变化/流式增量时重算，新块只增量追加。吸底，任何上滚立即脱离，脱离期间转录区底部右缘显示 `↓ N 行新内容 · 到最下`（整块可点），**提交时无条件回到底部**。`PgUp/PgDn` 整页减 2 行重叠、`Ctrl-G` 到底、滚轮每格 3 行。滚动条**恒预留 1 列**、只在超出时画字符。**非 assistant 的消息不再截断**（就是被压成单行 + `truncate(text, 500)` 的那一支，主要是用户自己的输入；assistant 本来就是全文 Markdown），工具输出保持 4 000 字符 preview —— 这是本 effort 唯一的保真度变更。
- [grilling：既有消息在新布局中的落位](issues/06-grilling-existing-messages-in-new-layout.md): 权限询问改成**模态覆盖层**，四种 pending 问题共用一套（`Permission` / `PlanConflict` / 超长粘贴 / Esc 清空草稿；两个新的默认答案都是「否」）。`wording::status_line` **保留**，状态词并进底部提示行最左；提示集换成六条（`enter 发送 / ctrl-j 换行 / esc 取消 / shift+tab 计划 / PgUp/PgDn 滚动 / ctrl-c 退出`），**降级算法改为「状态词与 `ctrl-c 退出` 先占位、中间从左往右填」**，绝不写 `shift+enter`。通知与诊断**全部进转录**，不引入 toast。退出**不 dump**。**配色沿用现有六种 + BOLD，不需要 truecolor，不加主题配置项。**
- [grilling：渲染管线与交付节拍](issues/07-grilling-render-pipeline-and-cadence.md): 保留 `select!` 骨架，**每轮末尾把广播里排队的消息整批 drain 后再画一帧**（合并突发）；用 `ratatui::init()` 进 alt screen（`init_with_options` 不进）；synchronized update **只包那一次 draw**。脏标记驱动、无脏不画、**不预加帧率上限**；时钟只在分钟变化时置脏。换行缓存**在 draw 闭包里按 `frame.area().width` 失效**，resize 不需要单独分支。退出与 panic 都要 `DisableMouseCapture` + `DisableBracketedPaste`（这两个是我们自己开的）。删掉 inline 路径（`insert_before` 循环 / `paint_scrollback` / `Viewport::Inline` / 底部 `MoveTo`）。
- [grilling：测试与验证策略的迁移](issues/08-grilling-test-and-verification-migration.md): `render_block` 那批用例**全部保留**；编辑器四个用例改写为对着新 `Input` 断言；`a_notice_is_a_scrollback_line_shown_as_it_is` 改名（"scrollback" 随 inline 视口一起没了）。**布局可以进 `cargo test`**（`TestBackend` 无特性门），覆盖尺寸矩阵、降级阶梯、提示行条目数（40→3 / 60→4 / 80→5 / 120→6）、`ctrl-c 退出` 恒在、右栏无数据文案。`scripts/tui-startup-check.py` 的锚点 `ctrl-c` 与 `fs-agent：` **仍然有效**，但判定要改（新底部块带边框，`退出` 后面多了 `│`，`endswith` 会失败），并新增 header 版本串锚点。
- [task：ADR —— 以全屏 alt screen 取代 inline viewport](issues/09-task-adr-alt-screen-over-inline-viewport.md): 写在 `docs/adr/0002-fullscreen-alt-screen-tui.md`，逐条覆盖决策、被推翻的 spec §19、**如实写出的动机（inline 光标 bug）**、代价、被否决的替代方案、与 ADR 0001 的关系。代价写的是「复制要按住 Shift 拖拽」，**不是**已作废的「滚轮不能用」。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

- **转录是否落盘 / 与会话日志复用**：20 000 行上限把转录变成纯内存结构，退出即清（票 06 已定**不 dump**）。如果用户日后要「翻回三天前的输出」，就要接会话日志或 transcript 落盘。现在没这个需求，先记着。
- **输入历史跨会话**：票 04 已定**保持进程内**；真要持久化时，落点与会话目录的关系需要重新设计。留一句备忘，不是待办。

（原先挂在这里的另外三项已经关掉：**主题与配色** → 票 06 定了「沿用现有调色板、不需 truecolor」；**`wording::status_line` 的去留** → 票 06 定了「保留但并入提示行」；**超宽屏第三栏** → 移到 `## Out of scope`。）

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **修 inline viewport 的光标 bug**：用户已选择绕开。`e25097e` 的 `MoveTo(0, rows-1)` 与相关调试痕迹保留原样，不在本 effort 里继续追。
- **todo 列表 / todo 工具**：右栏的原始提案里有它，但 fs-agent 没有 todo 状态也没有 todo 工具 —— 没有数据源。做它等于先造一个工具，那是另一个 effort。
- **鼠标点选文本 / 鼠标驱动的焦点切换**：鼠标捕获**已开**（为了「到最下」与滚轮），但点击只命中「点此到底」那一块，其它区域的点击一律忽略；选文本仍走终端原生的 **Shift+拖拽**。
- **超宽屏第三栏**：右栏宽度被 `clamp` 到最多 31 列，转录吃掉剩余空间。再放第三栏（diff、工具输出之类）是另一个 effort。
- **会话内搜索 / 过滤 / 折叠全部工具输出一类的高级漂移**（除非票 03 查明现有折叠语义后判定必须）。
- **语法高亮与 diff 着色本身**：`tree-sitter-highlight` 的现状不动，只改它渲染到哪一栏。
- **两进程拆分（TUI / agent server）**：spec §19 已否决，理由（IPC 会逼"单一写入者 + `seq` 唯一身份"重做）仍然成立，本图不再重开。
- **本图的执行**：本图只产决策与 spec。"做"发生在 `/to-spec` → `/implement`。
