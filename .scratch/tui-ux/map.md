# TUI 使用体验与视觉效果优化（wayfinder map）

Label: `wayfinder:map`
Tracker: local markdown —— 见 `docs/agents/issue-tracker.md`
Charting: **已完成**（2026-09-23，三轮 grilling）。本图只做**规划**，不产实现代码。
**✅ 本图已完成（2026-09-23）**：8 张子票全部 resolved、`Not yet specified` 为空 ⇒ 路线 clear；下一步是 `/to-spec`（见 `## 进度`）。**不要再往这张图加票。**

> **交棒已发生（2026-09-26 补记）**：这八张票的增量已经折进 **[`.scratch/tui-layout/spec.md`](../tui-layout/spec.md) §14「使用体验与视觉效果」**，该 spec 的 §2 几何数字与 Mark header 阈值也按本图（含 `prototype/geometry/geometry-table.md` 的「新」列）回改过；实现由 `/implement` 逐票落地（提交见 `7e437a0` / `940cd43` / `0641c57` / `b48871c` / `538c4aa` / `530a191`）。本图至此**只作决策存档**，不再是待办。

## Destination

一份 **spec-ready 的 TUI 使用体验与视觉效果决策集**，交给 `/to-spec` 折叠进既有 TUI spec（`.scratch/tui-layout/spec.md`，本图会推翻它若干条已实现的决定）并生成实现票。

五项原始诉求：

1. 中段权限覆盖层与底部 `ask_user_question` 问卷**支持鼠标点击作答**。
2. 优化 TUI 界面：上/中/下三块之间**不再留 airy 空行**；Mark header 里 **logo 与会话路径之间加一行空行、会话路径下方那行空行删掉**。
3. 加 `Ctrl-D` 退出 TUI，按下后弹**确认**。
4. **折叠**模型思考过程与工具输出：只写「xxx 正在思考」「xxx 思考完成」「xxx 调用 xxx」「xxx 调用 xxx 失败」；点击提示后用**覆盖层**看详情。
5. **不同角色名字用不同的文字颜色**。

**判据**：改动**只落在 TUI 渲染层**（`src/render/tui.rs` 与 `pane` / `layout` / `editor` / `panel` 等同侪）；**plain 与 headless 一个字节不动**；**不改 `events` schema**。

## Notes

- **领域**：`fs-agent` —— 自用 coding agent CLI，Rust，从零实现。TUI 栈 = `ratatui 0.30` + `crossterm 0.29`（`event-stream`），**不加新 crate**。相关既有产物：`.scratch/tui-layout/`（四分区全屏 TUI 的 map + spec + 实现票 10–16，全部已落地）、`.scratch/fs-agent-v1/issues/30`（问题覆盖层改成「标题 + 正文 + 候选键行」）、`32`（`ask_user_question` 接管底部输入区）、`docs/tui-manual-checklist.md`、`scripts/tui-startup-check.py`。

- **每张票的答案必须自足**：`/implement` 会在 `/clear` 之后的新会话里读它，看不到本图与 charting 对话。

- **要咨询的 skills**：`/grilling`（HITL 票默认）、`/domain-modeling`（术语，若「折叠 / 详情 / 角色」被引入词汇表）、`/prototype`（形态票）、`/research`（research 票）。核对实现时用 `/code-review` 双轴。

### Tracker 事实与降级（本图适用）

- 本仓库 issue tracker = **local markdown**：map = `.scratch/tui-ux/map.md`，child = `.scratch/tui-ux/issues/NN-*.md`，阻塞 = 票面 `Blocked by: NN`，claim = 票面 `Status: claimed`，resolve = `## Answer` + `Status: resolved` + 追加到本文件 `Decisions so far`。
- **`Status:` 与 triage 共用**：按 `docs/agents/issue-tracker.md`，triage 状态也记在 `Status:`（canonical 角色串，如 `ready-for-agent`），category 记在新 `Category: bug|enhancement` 行。因此 **frontier 判定 = 非 `resolved`/`done`/`closed` + unblocked + unclaimed**（`ready-for-agent` 仍是可拿的前沿票）；wayfinder 认领时把它改成 `claimed`。
- **该后端没有 native sub-issue / 原生依赖边**（`docs/agents/issue-tracker.md` 只定义正文约定）。因此按既定规则**回退**：本文件用 `## 任务清单` 逐条引用子票（条目数 == 子票文件数），每张子票顶部写 `Part of: ../map.md`（相对 `issues/` 解析）。**阻塞关系仍以正文 `Blocked by:` 为权威**（没有脚本可建的原生边可依据）。
- 校验脚本：`scripts/wayfinder-check.py .scratch/tui-ux/map.md`，校验「任务清单条目数 == 子票数」「每张票 `Part of` 指向本图」「每个 `Blocked by:` 都能解析」，并打印 `closed/total`；对不上时非零退出。

### 冻结项（charting 三轮 grilling 定下，票里不得重开）

1. **只做 TUI**；plain / headless 的**可见输出一个字节不动**；不改 `events` schema。
   - 票 01 查明：`Block::Message` 现在被 `plain.rs:81-85` 与 `tui.rs:2105-2109` 用无 `..` 的字段列表穷尽解构，**给它加字段会编译期强制在 `plain.rs` 出现一处解构改动**。该改动若只补字段名、不改变任何输出，不算违反本条；`headless.rs` 不消费 `Block`，完全不受影响。
2. **删 airy**：`AIRY_ROWS`（header⇄中段、中段⇄底部各 1 行）两处都删，中部多出 2 行给转录。这**推翻 tui-layout spec §2** 的 airy 与几何表数字（原型数字是唯一来源，先改原型再改 spec）。
3. **Mark header 内部间距对调**：logo 5 行 + 空 1 行 + 信息 1 行，**净高仍 7**（只挪 `draw_mark` 的信息行，几何不变）。
4. **`Ctrl-D` 退出**：**忙时忽略**；空闲弹确认，**默认答案「否」**、`Esc` = 否、`y` = 退出。底部提示行**加 `ctrl-d 退出`**，与 `ctrl-c 退出` 同级保留。`Ctrl-C` 现有行为（忙时取消、闲时退出）**不变**。
5. **折叠**：折叠 reasoning（思考）与工具**输出正文**；工具调用行**保留参数摘要**（`kimi 调用 bash command=…`）。提示进**转录**、可回看、可点击。
6. **详情用覆盖层**（不是就地展开）；内容 = reasoning 全文 / 工具参数 + `outputs/<tool_call_id>.txt` 全文（指针失效降级为事件里的 head/tail 预览）；覆盖层**内部可滚动**（`↑/↓`、`PgUp/PgDn`、滚轮），`Esc` / 再点关闭；**不可叠在待答问题（权限模态 / 问卷）之上**。
7. **reasoning 沿用已有 `MessageCompleted.reasoning`**（`src/events.rs:319`，`src/agent.rs:571` 写入、入流前打码、可 `--continue` 重放）——**不新增事件、不记录逐段增量**。本图只把它从 `Transcript`/`Block` 送到 TUI。
8. **鼠标作答语义**：单击即选中并**立即作答/前进**（单选）；多选单击切换勾选、**仍需提交**；`Esc` 仍是安全答案。中段覆盖层与底部问卷是**两套绘制**，命中各自实现。
9. **角色配色**（只染名字前缀，不加 truecolor / 主题配置）：讨论者 1 = `LightCyan`、讨论者 2 = `LightMagenta`、执行者 = `LightYellow`、用户 = `LightGreen`、系统 = `Gray`。
10. 本图包含一张**测试与验证迁移**票（沿用 tui-layout 图票 08 的先例）。

### 既有决定会被本图推翻的地方（/to-spec 时回改，不在本图改）

- **tui-layout spec §2**（四分区几何与降级阶梯）：airy 删除 → `CHROME` / `fits_airy` / `max_input_rows` / `LOGO_MIN_HEIGHT` 与几何表要重算；`120×24` 草稿 10 行时「右栏消失」可能不再成立。
- **tui-layout spec §3**（对话面板）：工具输出 preview 从「直接显示」变成「折在详情里」；转录新增思考提示行。
- **tui-layout spec §4**（滚动与鼠标）：从「除『点此到底』外所有点击一律忽略」改为在覆盖层 / 问卷上有明确命中。
- **tui-layout spec §6**（键位表）：新增 `Ctrl-D`。
- **tui-layout spec §9**（模态覆盖层）：新增「详情覆盖层」这一类，并给四种问题加鼠标命中。
- **tui-layout spec §10**（底部提示行与配色）：提示集加 `ctrl-d 退出`；角色名字配色打破「六种颜色 + BOLD」的现状。
- **tui-layout spec §2 的既有漂移**（本图票 05 发现，不是本图引入）：那张几何表写在 **Mark header 落地之前**，所以 `60×24` 的 header 内容行表里是 2、当前代码已是 7。回改 §2 时一并修正。

### 基线与纪律

- 验收基线：`cargo test --all-targets` 当前 **633 passed / 0 failed**（2026-09-23 charting 实测；票据 31 收尾时是 601，票据 32 落地后已涨）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 除 `src/context/repo_map.rs` 与 `tests/repo_map.rs` 的既有漂移外干净（**不要**顺手格式化这两个文件）。
- 渲染相关的既有测试：`tests/render_tui.rs`（34 个 `#[test]`）、`tests/render_layout.rs`（46 个）、`tests/render_editor.rs`、`tests/render_markdown.rs`、`tests/wording.rs`、`tests/render_delivery.rs`；pty 启动检查在 `scripts/tui-startup-check.py`；真终端手测在 `docs/tui-manual-checklist.md`。
- **提交用限定路径**（`git commit -F - -- <路径>`）：本工作区出现过并行 session 全量暂存把在制品卷进无关提交。
- 改了 spec 的决定就**回改 spec 正文**，不要只写在票的评论区。

## 任务清单

<!-- 逐条引用子票；条目数必须等于 issues/ 下的子票文件数（scripts/wayfinder-check.py 校验）。Frontier 的权威查询仍是扫描 issues/ 里 open + unblocked + unclaimed 的票。 -->

- [x] [research：折叠与详情层的数据来源与命中接缝](issues/01-research-collapse-detail-data-sources.md)
- [x] [grilling：折叠与详情覆盖层的交互契约](issues/02-grilling-collapse-and-detail-contract.md)
- [x] [prototype：折叠提示行、详情覆盖层与工具行的形态](issues/03-prototype-collapse-hint-and-detail-overlay.md)
- [x] [grilling：鼠标点击作答的命中与焦点契约](issues/04-grilling-mouse-click-answer-hit-contract.md)
- [x] [prototype：删 airy 后的几何、降级阶梯与 Mark 间距](issues/05-prototype-geometry-without-airy.md)
- [x] [grilling：Ctrl-D 退出的确认与提示行](issues/06-grilling-ctrl-d-exit-confirmation.md)
- [x] [grilling：角色配色的落点与绘制边界](issues/07-grilling-speaker-name-colours.md)
- [x] [grilling：测试与验证迁移](issues/08-grilling-test-and-verification-migration.md)

共 **8** 张子票，当前 **8 resolved / 0 open**。

## Decisions so far

<!-- 索引：每条一行，够判断相关性即可；细节住在票里，本文件不复述。按名字引用，不写裸编号。 -->

- [research：折叠与详情层的数据来源与命中接缝](issues/01-research-collapse-detail-data-sources.md): 完成态 reasoning 的**唯一**到达路径是 `MessageCompleted.reasoning`（无独立渲染事件）；`Transcript::push_logged` 构造 `Block::Message` 时用一个 `..` 丢掉它——那一个 `..` 就是从流到 TUI 的全部距离。只有工具调用的 iteration **也会**发 `MessageCompleted`，但空正文的 assistant 消息现在渲染 0 行，没有可挂「思考完成」的行。工具全文**不在事件里**（事件只存 `.preview`，指针不随行）；`SessionFacts.cwd` 实际是**会话目录**，`cwd.join("outputs")` 即全文目录，但渲染层目前零文件系统访问。命中只有 `indicator` 的单矩形先例，`Pane` 没有块级索引；覆盖层的候选键行恒为最后一行，问卷选项的显示行号可由 `prefix.len() + (i − option_window_start)` 推出。完整事实（68 条、逐条标约束哪张票）与 8 条 ⚪ 见 research 文件。
- [grilling：折叠与详情覆盖层的交互契约](issues/02-grilling-collapse-and-detail-contract.md): 契约 11 条定稿——**一个思考段一行**（reasoning delta 起「正在思考」，正文 delta 到达即**原地定格**「思考完成」，交错时后续 reasoning 开新行）；提示是**普通转录源行**（计入 20 000 上限、参与吸底），只有完成态可点。`xxx` = `speaker_label`；工具行改写为「调用」、失败只在行尾加「失败」，**post-hook 反馈留在折叠态**（Yellow），输出/错误/无结果进详情。详情覆盖层是一个**独立「查看态」、不是 `pending`**（独占键盘与鼠标、`Esc` 或再点同一行关闭、打开时转录冻结），分节显示思考全文 / 工具参数 / 工具全文；全文按 `tool_call_id` 读 `outputs/<id>.txt`，读不到降级为事件预览 + 「全文不可用」，上限 200 000 字符。失败行 `Red`、思考提示 `DarkGray`。合成器「有思考流、无思考全文」时仍定格并明说「本次未记录思考全文」。`--continue` 不重播历史 → out of scope 另立票；覆盖层几何留给票 03。
- [prototype：折叠提示行、详情覆盖层与工具行的形态](issues/03-prototype-collapse-hint-and-detail-overlay.md): 形态定稿——可点提示用**行首 `▸ ` 标记**（流式期「正在思考」不加、不可点）；思考行 `[kimi] … 正在思考` → 原地定格 `[kimi] ✓ 思考完成`（`DarkGray`）；工具行 `▸ [kimi] 调用 bash command=…`（`BOLD`），失败在**行尾**加 `失败`（`Red`）；详情覆盖层用 `── 思考 ──` 分节横线、标题为被点行原文、**宽度上限 90 列**、底部 `↕ n/m · esc 关闭`。产物：`.scratch/tui-ux/prototype/` 下 13 张 `variant-*` + 6 张 `chosen-*`（ratatui `TestBackend` 真渲染；颜色/属性在每张快照的 `styles` 一节列出）。四分区几何仍是近似，归票 05。
- [grilling：鼠标点击作答的命中与焦点契约](issues/04-grilling-mouse-click-answer-hit-contract.md): 契约 9 条——命中矩形**绘制时当帧记录**（沿用 `indicator` 先例），`mouse()` 分派 **详情覆盖层 > 待答问题 > 转录**，任何时候都不滚背后转录。中段覆盖层按**每个 `[key] label` 区间**命中（等价按键；`modal()==None` 或被裁的部分不可点）；问卷**整行可点**（单选单击确认并在有下一题时前进、**最后一题只确认不自动提交**、多选只切换不前进），底部状态行扩成可点条 `← 上一题` / `下一题 →` / `all_handled()` 的 `提交`，滚轮 = 移高亮；点击自定义文本行才显示光标、切题复位；run 结束撤回后旧命中自动消失。§8 给了验证票的断言点。
- [prototype：删 airy 后的几何、降级阶梯与 Mark 间距](issues/05-prototype-geometry-without-airy.md): 几何定稿——删 airy（`AIRY_ROWS` / `fits_airy` 整体删），凡旧 `airy=on` 的尺寸中段 **+2**（`120×24` 7→9、`80×16` 4→6、`40×10`/`120×10` 不变）。**两条旧结论被推翻**：右栏最小尺寸 `80×16` → **`80×14`**；`LOGO_MIN_HEIGHT` 20 → **18**（Mark 从 h≥18 出现，用户确认公式值）。仍成立：降级顺序不变、`120×24` 草稿 10 行右栏仍消失（但输入上限 7→9）、`40×10` 仍是完整降级布局。Mark 间距改为 logo 5 + 空 1 + 信息 1，净高仍 7。**顺带查出既有文档漂移**：spec §2 几何表早于 Mark header，表里 `60×24` header=2 而当前代码已是 7。产物在 `.scratch/tui-ux/prototype/geometry/`。
- [grilling：Ctrl-D 退出的确认与提示行](issues/06-grilling-ctrl-d-exit-confirmation.md): 新增 `Key::CtrlD` 与第五种 `Pending::Exit`（渲染器自有，`RunState{running:false}` 撤回不管它）。空闲 → 弹确认（title `退出会话`、body `会话记录会保留；未发送的草稿会丢弃`、`[y] 退出` / `[n] 取消`、默认否）；**忙 → 忽略**；四种问题 / 问卷 → 忽略；**详情覆盖层打开 → 关闭详情覆盖层**（对票 02「其它键忽略」的例外，已回改票 02 的 Answer）。提示行拆成 `EXIT_HINT_IDLE = "ctrl-c/ctrl-d 退出"`（空闲）与 `EXIT_HINT_BUSY = "ctrl-c 退出"`（忙时 / 查看行，因为忙时 `Ctrl-D` 被忽略），`hint_line` 改为接收 exit 字符串；空闲新梯子 40→3 / 60→4（`就绪` 反而放不下）/ 80→6 / 120→7，忙时行不变。
- [grilling：角色配色的落点与绘制边界](issues/07-grilling-speaker-name-colours.md): 契约 5 条——**凡出现 `speaker_label` 的行都染名字**（消息 / 工具行 / 全部 speaker narration；无 speaker 的行不动），名字用角色色、**正文仍按原语义色**（需把单串拆成「名字 span + 正文 span」）；讨论者序号靠 `SessionFacts` 新增的 `speaker_order`（组装期有序名册）取 `DEBATER_PALETTE = [LightCyan, LightMagenta]`，会话中途 `/discuss` 抽到名册外人时按首次出现补未占用槽、用尽回退 `Gray`；执行者 `LightYellow`、用户 `LightGreen`、系统 `Gray`。**只染转录**（面板 / header / 问卷 / 详情标题不动）、**不加 BOLD**（浅色背景的可读性是已知取舍）。`speaker_color` 住画家侧，`wording::speaker_label` 保持纯文本。
- [grilling：测试与验证迁移](issues/08-grilling-test-and-verification-migration.md): 分层定死——**行为进 `cargo test`**（合成 `Mouse`/`Key` + `TestBackend`）、**终端归属进 pty**（`GESTURES` 新增 `ctrl-d → y` 第三条退出路径）、**手感进手工清单**。逐条列出：新增断言（鼠标命中 / 折叠与详情 / 几何新数字 / 提示行梯子 / `fg` 精确配色 / Ctrl-D 边界）、既有测试改动（`tests/wording.rs` 的提示行梯子；`tests/render_layout.rs` 的 airy 断言、高草稿注释、Mark 阈值、floor 尺寸）、pty 锚点复核、手工清单 ④/⑦/⑨/⑩（顺手把 ⑩ 的 `41×19` 漂移改成实测 `42×18`）与新增项、基线 633 passed、以及给 `/to-spec` 的 §2–§10 回改汇总（含两处既有漂移）。

## Not yet specified

<!-- 在范围内、但现在还说不精确的雾。随前沿推进毕业成票。不要预先切成票那么大。 -->

<!-- 当前**没有**未指定的雾：本图 8 张票全部 resolved，原有的两条雾（悬停反馈、详情覆盖层的内容操作）已判出 scope（见 `## Out of scope`）。 -->

## Out of scope

<!-- 有意识排除在本次 effort 之外的工作；永不毕业。 -->

- **plain / headless 的呈现改动**：本图判据是只动 TUI；管道化输出保持逐行、无颜色、无折叠。
- **事件 schema 改动 / reasoning 逐段增量落流**：沿用 OpenHands 先例（增量是 UX affordance，不进持久记录）；完成后的整段已有 `MessageCompleted.reasoning`。
- **主题 / 配色配置项与 truecolor**：角色配色用现有 ratatui 命名色，不引入主题系统。
- **鼠标点选文本 / 鼠标驱动的焦点切换**：选文本仍走终端原生 Shift+拖拽；本图只加「点击作答」「点击看详情」两类明确命中。
- **鼠标悬停反馈（hover）**：终端要开 any-motion 才有 `MouseEventKind::Moved`；本图只做点击命中。原 `Not yet specified` 的雾，判出 scope。
- **详情覆盖层里的复制 / 导出 / 搜索**：本图只做「点开看全文」；这些内容操作与既有的「会话内搜索 / 过滤」同族，超出目的地。原 `Not yet specified` 的雾，判出 scope。
- **修 inline viewport 的光标 bug**：tui-layout 图已选择绕开，理由（ADR 0002）不变。
- **会话内搜索 / 过滤 / 分页浏览工具输出**：折叠与详情是本次要做的；搜索、过滤、全文检索仍排除。
- **todo 列表 / todo 工具**：fs-agent 没有 todo 状态也没有 todo 工具，没有数据源。
- **两进程拆分（TUI / agent server）**：`.scratch/fs-agent-v1/spec.md` §19 已否决，理由仍成立。
- **TUI 启动时把历史事件重播进 pane（`--continue` 的历史可见性）**：票 01 查明现状**根本不重播**（全仓库唯一 `render.logged` 在 `src/agent.rs:2148`），所以「`--continue` 后重建思考行」无处可建。这是独立能力（转录重建、吸底、20 000 上限都要重定），不属本图目的地；已按「小需求另立一张票」的规则开在 **`.scratch/tui-history-replay/`**，它现在是新 map `fs-agent 重新打开会话：历史加载与详情弹窗`（2026-09-23 charting）的子票 01；本图不开子票。
- **本图的执行**：本图只产决策与 spec-ready 结论。「做」发生在 `/to-spec` → 实现票 → `/implement`。

## 进度

**100%** —— **本图完成（2026-09-23）**：8/8 张子票全部 resolved，`Not yet specified` 为空 ⇒ 通往 destination 的决策已 clear。

**下一步 = handoff，不是 build**：`/to-spec` 把互链的 decisions 折成可建计划（按 `grilling：测试与验证迁移` §7 回改 tui-layout spec §2–§10，并修正 Mark header 的既有漂移）→ `/to-tickets` → 每票一次 `/implement`（fresh session、票间 `/clear`）→ `/code-review` 双轴。**本图不再加票。**

**待确认**：无。八张票的决定都已在 live exchange 里由维护者拍板；剩下的只是执行。
