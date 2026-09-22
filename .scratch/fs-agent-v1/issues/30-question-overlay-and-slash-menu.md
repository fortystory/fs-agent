# 30: 问题覆盖层的形态与 `/` 补全菜单（补记：无票落地的工作）

**What to build:** 这一票是**补记**。工作区在票 24–29 之外还改了两处前端，而翻遍 issue tracker 找不到任何一张票认领它们（六张票的正文与评论、`docs/discussion.md` 里都搜不到「覆盖层 / 标题 / choices」）：

1. **四种等待回答的问题**（权限 / 计划冲突 / 超长粘贴 / Esc 清空草稿）从「`Block` 边框 + 居中折行段落」（tui-layout 票 14 的形态）改成**标题 + 正文 + 候选键行**：`wording::permission_title` / `permission_summary` / `permission_call`、`plan_conflict_title` / `_body`、`paste_title` / `_body`、`clear_draft_title` / `_body`、`question_prompt`、`choices_text`，配一张 `Choice` 表。
2. **`/` 变成一个真正的补全菜单**：`CatalogEntry`、`ConsoleRequest::Catalog`、`editor::SlashToken` / `complete_slash`、`Skills::entries()`，加布局与编辑器测试。票 26 只写了「`/` 补全菜单列出 `discuss`（内置命令表加一项）」，它的措辞以菜单已在为前提——而 HEAD 上**没有菜单**，所以实际是建了一个。

**已经落地的**：上面两条已在工作区里做完，随票 24–29 一起提交（形态由本次 `/code-review` 的 Spec 轴点出无主）。

**剩下的才是这张票要干的活：**

- [ ] **`docs/tui-manual-checklist.md` 跟上**：①.7 与 ⑤.2 逐字引用的旧措辞（「清空输入？[y] 清空 / [n] 保留」「粘贴 N 字符？…」）现在已经不是那个形态，要改成标题 + 正文 + 候选键行；并补 `/` 菜单的手工项（↑/↓/Tab/Enter 选择、resize 后命中矩形仍对）。
- [ ] **复核 `scripts/tui-startup-check.py` 的划分**：菜单是键驱动的，按该脚本 docstring 的口径属于手工面；确认一句，别让「只有 pty 看得见」与「只有手工看得见」的分界含糊。

Blocked by: None

Status: open

**参考:** spec §19（渲染与 CLI 组装）、`docs/render.md`（措辞层的家）、票 26、`docs/tui-manual-checklist.md`、tui-layout 票 14

## Comments

**2026-09-22（补记）** 为什么单独补一张票，而不是把票 26 或 29 的勾选项改成「也顺带做了覆盖层重构」：那两张票的正文都没写这两处形态，事后追认会让票面说谎——而本 spec 的 Further Notes 刚记下「先写答案、后来由别的票修正它，但没回改正文」是那张地图 24 处不一致里约一半的根因。补记一张、把「已落地」与「未落地」分开写，是同一条教训的应用。

**为什么不是「不算数」**：这两处不是顺手改的格式，它们改了用户在**最需要读懂**的地方看到的东西（权限询问的候选键、`/` 的可发现性），值得在 tracker 里有一条能从「为什么长这样」回答到「还剩什么」的记录。
