# 18 — 键盘归属层与选中（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: 12

> 规格：[`../spec.md`](../spec.md) §4（键盘归属：那一层的判据、位置、交还与拿回）与 §9（选中：
> 粒度、画法、点击、粘性）。
> 判据两层与「交还 / 拿回」的形状在 [长历史与行选择](10-grilling-history-and-selection.md) §3–§8；
> 为什么是事件侧派生的身份、为什么整行 `ACCENT + BOLD` 而不画反显，在同一票的 §1 与 §6。
> **依赖票 12**（选中的落点与粘性都靠块身份）。**可与票 16 / 17 并行**。

## 目标

轨迹页第一次有了「我在这里」：它成为键盘的一层归属，`↑` / `↓`（或 `j` / `k`）在**块**之间移动选中，
`Enter` 开详情，`g` / `G` 跳两端；`Esc` 依次退「搜索 → 过滤 → 选中 → 把键盘还给输入区」，
点账本任何位置把键盘拿回来；而选中**认块不认位置**，所以改宽度、新块到达、切页都不打断我。

## 现状（2026-10-09 核实，改前先复核）

- 键盘阶梯有十六层守卫，**没有任何一层读主列页签** —— 轨迹页今天只在「当前显示那一页」那一支
  拿到翻页三键与滚轮，`↑`/`↓`/`j`/`k`/`Enter` 全部落进输入区。
- 要收编的键今天各属别处：`↑`/`↓` 是编辑器光标、`Enter` 是提交、`Space`、`/` 弹记号菜单、
  `Tab` / `Shift+Tab` 切模式。**全局无绑定**的有 `j` / `k`、`n` / `N`、`g` / `G`、`[` / `]`、`{` / `}`。
  `Home` / `End` 是编辑器的行首行尾，**不征用**。
- 焦点今天有**三套**互不相同的机制（左栏那个布尔、两页各自的焦点索引、反显的隐式归属）——
  这是**第四套**；细节记在票 10 的作答里，实现时按它写。
- 全屏唯一的反显是「键盘所在」，消费者只有问卷与记号菜单，**详情覆盖层今天都不画**。
- 整行 `ACCENT + BOLD` 有三处先例：左栏文件页、改动页、页签条。
- 点一行开详情是今天的语义，**明确保护**。

## 落点

键盘阶梯（插在覆盖层之下、记号菜单之上）、状态里的选中与焦点两个字段、
轨迹页持有时的绘制那一笔、指针分派（点空白 / 点行）。

## 具体行为

1. **新增一层归属**，判据两半：**主列当前显示页是轨迹** 且 **没有处于「已交还」状态**。
   位置在**详情覆盖层之下、记号菜单之上**。
2. **覆盖层立着时它持有键盘，嵌套在轨迹页层之上**；关掉覆盖层**恢复轨迹页层的原状态**
   （交还过就还是交还着）。
3. **焦点只有一个布尔状态**：默认持有；`Esc` 退完搜索 / 过滤 / 选中三层把它**交还**给输入区；
   **点账本任何位置**拿回（点一行 = 拿回 + 开详情，点空白 = 拿回 + 清选中）；重新成为当前显示页
   时**自动回到持有**。
4. **选中的粒度 = 块**：一个块的第一条源行是落点，附属行（错误正文那行、还在长的增量行）跟着
   所属块一起高亮、**不能单独选中**。
5. **画法**：整行 `ACCENT` + `BOLD`。**不新开通道**（底色是内容语义）、**不画反显**
   （会把选中洗白，而且全屏唯一的反显另有其主）。
6. **这一层不画「键盘所在」的反显** —— 选中行自己就是那个锚点。
7. **键位**：`↑`/`↓` 与 `j`/`k` 移动选中；`Enter` 开详情；`g` 跳第一条行；`G` 跳最新**并恢复跟随**；
   翻页三键与滚轮**照旧且不动选中**。
8. **粘性**：宽度变化重放之后**认块不认位置**、留在原处；切主页签不动；滚动与翻页不推动选中；
   滚出视口**不拉回来**（视口照跟随走）；选中的块不在新过滤集里时**留着、只是不画**。
9. **点击语义照旧**：点一行 = 开详情（不顺手选中）。

## 验收

- [x] 轨迹页是当前显示页时 `↑`/`↓` 与 `j`/`k` 移动选中，`Enter` 开详情。
- [x] 选中的是**一个块**：附属行跟着高亮，跳过它们不误停。
- [x] 选中行整行是 `ACCENT` + `BOLD`，且**账本里没有反显**。
- [x] `Esc` 四层顺序：搜索 → 过滤 → 选中 → 交还键盘；退完就没有第五层。
- [x] 交还之后 `↑`/`↓` 回到输入区移光标、轨迹页那些键不生效；**点账本任何位置**能拿回。
- [x] 重新成为当前显示页时自动回到持有，不必再点一次。
- [x] 改宽度（触发重放）之后选中**仍在原来那一块**。
- [x] 新块到达时视口照跟随走、选中留在原地；翻页与滚轮不改变选中。
- [x] 覆盖层开着时轨迹页那层不生效；关掉之后恢复它原来的持有 / 交还状态。
- [x] 点一行仍然只是开详情，**不顺手留下高亮**。
- [x] `cargo test` 全绿、`cargo clippy` 干净、`cargo fmt --check` 只留既有漂移。

## 落地记录

**2026-10-10 落地。** 三个落点：`src/render/tui.rs`（两个状态位、键盘阶梯那一层、指针分派、
轨迹页绘制那一笔、`first_source_of` 的反向查询）、`src/render/pane.rs`（`reveal_source`）、
`tests/render_layout.rs`（十三条帧层验收）。25 条新测试（13 帧层 + 12 低层，外加一个公共
fixture），既有断言
**一条都没改** —— 这一层不动屏上已有的任何东西，只在选中时多亮一行。

### 与票面不同的四处

1. **输入区那个光标照旧在轨迹页上露面**（`keyboard_in_the_input` 一个字没动）。这一层是**部分
   持有**：它只收编 `↑`/`↓`/`j`/`k`/`Enter`/`g`/`G`/`Esc`，打字、`Backspace`、`Tab`、翻页三键
   与所有 `Ctrl-*` 都照旧落进输入区（票 10 §5 的「未列出的一律照旧」）。而 `keyboard_in_the_input`
   那条判据说的是「你敲的字会落进草稿」（`docs/render.md` 的**键盘**一节）—— 在那个意义上轨迹页
   上它仍然是真的；把它算成「键盘不在输入区」会让光标在**还能打字**的地方消失。
2. **窗格多了一个入口 `reveal_source`**（票面的「落点」只列了 tui 侧那四处）。`↑`/`↓` 换落点时
   视口要跟过去（票 10 §8「滚出之后再按 ↑/↓ 就跳回来」、§13「选中永远可见或可找回」），而
   「这一条来源行在不在窗口里」只有窗格自己知道。它只做一件事：整条已经露在窗口里就**一个显示
   行都不动**，否则滚到最近的那一边 —— 不把选中那一行拽到顶上（那是重排视口，不是跟过去）。
3. **问卷与中间那几种模态立着时这一层整个让位**（`pending.is_none()`）。它们才是那一刻的键盘
   主人（`.scratch/questionnaire-reading/spec.md`「没有浮层时归问卷、浮层立着时归浮层」），而这一层
   扣着 `Enter` 与方向键不放会让一道正在等的题按不动 —— 既有的
   `the_detail_holds_the_keyboard_and_closing_it_hands_the_questionnaire_back` 当场抓到了这一条。
   关掉它们之后原样还给轨迹页（这一位没被动过）。
4. **`Esc` 只落了两层，层序留在原地**：搜索与过滤归票 20，所以 `trace_escape` 里那两道的位置
   以注释与顺序留出来，只实现「有选中则清选中 → 交还键盘」。票面第 3 条那四层就是这么落的。

### 判据上另外定下来的四件小事

- **组头与小标题也是落点**（它们是有一行身份的块）。`↑`/`↓` 会在它们身上停一下，而在那儿
  `Enter` 安静地什么都不做 —— 它们本来就点不开（票 10 §5 把组头的 `Space` 与 `[`/`]` 交给票
  21 / 19）。反过来 `g` 落在账本第一条行上，而它常常正是「开场」那条小标题。
- **「点空白」的判据是「这一下没点中一条能开详情的行」**：空白格、组头、小标题都算，于是点它们
  = 拿回 + 清选中（票 10 §7 那两半）。
- **点输入区把键盘还回去，而选中留着**：与左栏两页的**焦点行**同一档 —— 换的是「键盘在谁那里」，
  不是读者放下的位置（`CONTEXT.md` 的**焦点行**）。左栏与轨迹页这两个布尔因此**互斥**：点左栏
  放下轨迹页那一层，点账本反过来。
- **选中被 `CAP` 裁出窗口、或被过滤掉**时都不清它：`↑`/`↓` 找不到落点就从**视口顶端那一条有身份
  的源行**接着走（这条也让「还没有选中时按 `↓`」有一个确定的起点：读者正在看的那一块）。

### 撞见的一个旁支缺陷（**不在本票范围，没修**）

`TraceGroups::preamble` 在**宽度重放**时被清空（`rerender_if_width_changed` 里的
`TraceGroups::default()`），而重放清单里的 `Painted::SectionHeader` 只把那一行画回去，
**没有把「开场那一段已经开过了」这件事交还给它**。于是重放之后再到达一块开场段的块
（`Notice` / 注入 / 命令 / 诊断），`note_section_header` 会**再开一条 `开场` 小标题**。
复现：先喂一块开场段、再画第一帧（此刻窗格宽度从初值变成真宽度 → 整本账重推）、再喂第二块，
屏上就有两条 `开场`。

这是票 16 那条路径上的**既有**缺陷：本次改动没有碰 `rerender_if_width_changed`、
`note_section_header`、`paint_section_header` 或 `TraceGroups` 里任何一个字节。一级 / 二级组头
不走这一支（它们的开组判据读的是块本身），所以只影响小标题。本票不修，记在这里。

### 验收

- [x] 轨迹页是当前显示页时 `↑` / `↓` 与 `j` / `k` 移动选中，`Enter` 开详情
      （`the_arrows_walk_the_blocks_and_enter_opens_the_selected_one`、
      `the_selection_walks_block_by_block_and_never_lands_on_an_extra_row`）。
- [x] 选中的是**一个块**：附属行跟着高亮，跳过它们不误停
      （`a_block_is_the_unit_so_its_extra_row_is_never_a_landing_point`：
      一步一块地走完整本账，`no such file` 那一行只与它的主行一起亮、且总是那两条；
      `a_source_row_knows_which_block_it_belongs_to` 从身份那一侧钉住同一件事）。
- [x] 选中行整行是 `ACCENT` + `BOLD`，且**账本里没有反显**
      （`the_selected_row_goes_accent_and_bold_and_nothing_is_reversed`：逐格比对，
      连行首那 9 列时刻戳一起亮，而整屏一格 `REVERSED` 都没有）。
- [x] `Esc` 的顺序是「清选中 → 交还键盘」，退完这一层不再认键
      （`escape_clears_the_selection_then_hands_the_keyboard_back`、
      `escape_clears_then_hands_back_and_then_stops_answering`）。**搜索与过滤那两层归票 20**，
      位置与顺序留在 `trace_escape` 里（见上面第 4 条）。
- [x] 交还之后 `↑` / `↓` 回到输入区移光标、轨迹页那些键不生效，**点账本任何位置**能拿回
      （同一条 `Esc` 测试里的光标落行比对；`clicking_the_ledger_takes_the_keyboard_back`
      点一行 = 拿回 + 开详情；`clicking_a_blank_spot_in_the_ledger_clears_the_selection`
      点空白 = 拿回 + 清选中；`clicking_the_input_area_takes_the_keyboard_back_from_the_trace_page`
      点输入区也归还，而选中留着）。
- [x] 重新成为当前显示页时自动回到持有
      （`the_trace_page_takes_the_keyboard_back_when_it_becomes_the_current_page`；
      `switching_the_main_tab_keeps_the_selection` 顺手钉住「切主页签不动选中」）。
- [x] 改宽度（触发重放）之后选中仍在原来那一块
      （`a_rebuild_after_a_width_change_keeps_the_selected_block`、
      `a_rebuild_keeps_the_selected_block` —— 后者同时断言重放之后它换到了新的那一条源行上）。
- [x] 新块到达时视口照跟随走、选中留在原地；翻页与滚轮不改变选中
      （`paging_wheeling_and_new_blocks_leave_the_selection_alone`；
      `g_and_shift_g_are_the_two_ends_of_the_ledger` 钉住 `g` / `G` 那一对落点与 `G` 的恢复跟随）。
- [x] 覆盖层开着时轨迹页那层不生效，关掉之后原样
      （`the_detail_overlay_holds_the_keyboard_and_hands_the_page_back_untouched`；
      `opening_and_closing_a_detail_leaves_the_pages_holding_alone` 从状态那一侧把
      「交还过就还是交还着」也钉了一遍）。
- [x] 点一行仍然只是开详情，**不顺手留下高亮**（`clicking_the_ledger_takes_the_keyboard_back`）。
- [x] 问卷 / 中间那几种模态立着时让位（`a_pending_question_keeps_the_trace_layer_out_of_the_way`）、
      键盘归属在两个布尔之间互斥（`the_keyboards_belonging_is_exclusive_between_the_columns`）、
      点得开与点不开的行各归各（`enter_on_a_row_without_a_detail_opens_nothing`）。
- [x] `cargo test` 全绿（含那 25 条新测试）、`cargo clippy --all-targets` 与基线**逐条相同**
      （43 条既有警告，一条不多）、`cargo fmt --check` 干净、`scripts/check-doc-size.py` 与
      `scripts/check-language.py` 通过。

### 交给后面的票

- 票 20（搜索与过滤）：`trace_escape` 上面那两道位置；选中与过滤集**互不清**这条纪律今天已经
  是行为（选中只在 `Esc` 与点空白时清），过滤视图里照它办。
- 票 19（层级跳转）：`[` / `]` 落在这一层的 `trace_key` 里；组头的 `BlockId` 已经在
  `first_source_of` 的账上。
- 票 21（折叠）：`Space` 同样落在 `trace_key` 里；折行会与它的主行共享一个身份，于是选中那一笔
  与 `↑` / `↓` 的走法**不用改**。
