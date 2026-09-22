# 30: 问题覆盖层的形态与 `/` 补全菜单（补记：无票落地的工作）

**What to build:** 这一票是**补记**。工作区在票 24–29 之外还改了两处前端，而翻遍 issue tracker 找不到任何一张票认领它们（六张票的正文与评论、`docs/discussion.md` 里都搜不到「覆盖层 / 标题 / choices」）：

1. **四种等待回答的问题**（权限 / 计划冲突 / 超长粘贴 / Esc 清空草稿）从「`Block` 边框 + 居中折行段落」（tui-layout 票 14 的形态）改成**标题 + 正文 + 候选键行**：`wording::permission_title` / `permission_summary` / `permission_call`、`plan_conflict_title` / `_body`、`paste_title` / `_body`、`clear_draft_title` / `_body`、`question_prompt`、`choices_text`，配一张 `Choice` 表。
2. **`/` 变成一个真正的补全菜单**：`CatalogEntry`、`ConsoleRequest::Catalog`、`editor::SlashToken` / `complete_slash`、`Skills::entries()`，加布局与编辑器测试。票 26 只写了「`/` 补全菜单列出 `discuss`（内置命令表加一项）」，它的措辞以菜单已在为前提——而 HEAD 上**没有菜单**，所以实际是建了一个。

**已经落地的**：上面两条已在工作区里做完，随票 24–29 一起提交（形态由本次 `/code-review` 的 Spec 轴点出无主）。

**剩下的才是这张票要干的活：**

- [x] **`docs/tui-manual-checklist.md` 跟上**：①.7 与 ⑤.2 逐字引用的旧措辞（「清空输入？[y] 清空 / [n] 保留」「粘贴 N 字符？…」）现在已经不是那个形态，要改成标题 + 正文 + 候选键行；并补 `/` 菜单的手工项（↑/↓/Tab/Enter 选择、resize 后命中矩形仍对）。
- [x] **复核 `scripts/tui-startup-check.py` 的划分**：菜单是键驱动的，按该脚本 docstring 的口径属于手工面；确认一句，别让「只有 pty 看得见」与「只有手工看得见」的分界含糊。

Blocked by: None

Status: done

**参考:** spec §19（渲染与 CLI 组装）、`docs/render.md`（措辞层的家）、票 26、`docs/tui-manual-checklist.md`、tui-layout 票 14

## Comments

**2026-09-22（补记）** 为什么单独补一张票，而不是把票 26 或 29 的勾选项改成「也顺带做了覆盖层重构」：那两张票的正文都没写这两处形态，事后追认会让票面说谎——而本 spec 的 Further Notes 刚记下「先写答案、后来由别的票修正它，但没回改正文」是那张地图 24 处不一致里约一半的根因。补记一张、把「已落地」与「未落地」分开写，是同一条教训的应用。

**为什么不是「不算数」**：这两处不是顺手改的格式，它们改了用户在**最需要读懂**的地方看到的东西（权限询问的候选键、`/` 的可发现性），值得在 tracker 里有一条能从「为什么长这样」回答到「还剩什么」的记录。

**2026-09-22（收尾）** 两个勾选项都做完了，只动 `docs/tui-manual-checklist.md` 与本票，没有 `.rs` 改动。

- **①.7 与 ⑤.2**：旧的一句式引用换成「标题 + 正文 + 候选键行、各占一行」。①.7 现在写标题 `清空输入`、正文 `草稿有多行，Esc 会把它们全部丢掉`、候选键行 `[y] 清空` / `[n] 保留`；⑤.2 写标题 `粘贴确认`、正文 `粘贴 {n} 字符`（`{n}` 是实际字符数）、候选键行 `[y] 粘贴` / `[n] 取消`。逐字对过 `src/render/wording.rs` 的 `clear_draft_title` / `clear_draft_body` / `CLEAR_CHOICES` / `paste_title` / `paste_body` / `PASTE_CHOICES`；候选键行的两键之间是三个空格（`tui.rs::choices_row`），清单按文件既有习惯只列键与标签，不逐列复刻间距。
- **权限与计划冲突**：`tui.rs::Pending::modal()` 里权限是 `permission_title` / `permission_summary` / `permission_call` 三行加 `PERMISSION_CHOICES`，计划冲突是 `plan_conflict_title` / `plan_conflict_body` 两行加 `PLAN_CHOICES`。清单里只有 ② 权限模态，它只点名 `y` / `a` / `n`、没引旧形态，仍然准确，所以**没动**；计划冲突本来就没有手工项，也没有引到不存在的形状。按「不改仍然准确的项」只重写了 ①.7 与 ⑤.2。
- **新增 ⑪ `/` 菜单**（接在 ⑩ 之后）：只列真终端能验的——敲 `/` 出菜单且裸 `/` 无高亮、继续打字筛选并随光标移动、`↑`/`↓` 的高亮走位与绕回及其反白底色、`Tab` 只填不提交而 `Enter` 填并提交、`Esc` 只收菜单不动草稿、无匹配时整体消失且不残留半框、框的完整与位置（窄终端丢说明列、放不下就不画），以及**开着菜单 resize** 后仍锚在新光标行且不出屏。resize 沿用 ④/⑨/⑩ 的既有口径：拖窗口发 SIGWINCH；用 `stty` 改尺寸则改完随便敲一个键触发重画（`stty` 不发 SIGWINCH，TUI 只在「脏」时重画，spec §11）。`cargo test` 已在 `tests/render_layout.rs` 固定 120×24 buffer 上断言框、筛选、按键结果与宽字角落，清单不重复这些，只留按键时序、真实配色与重画。
- **手工 / pty 的分界**：`scripts/tui-startup-check.py` 的 docstring 写的是它只管「第一帧」与「退出时交还的终端状态」，并且明说「光标、鼠标、resize 留在手工清单」。菜单是键驱动、只在按键后出现且锚在光标上，只有第一帧的脚本看不到它，所以留在手工面——这一句写进了 ⑪ 的开头。没有找到任何 pty 可观测的部分。
- **未动**：权限 / 计划冲突的覆盖层文案不在这份清单里逐字引用，② 因此没改；没有发现需要记录的菜单缺陷（未改任何 `.rs`）。
