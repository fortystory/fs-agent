# research：手工清单 26 个条目的现状（票 01）

- **被调研对象**：[`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md)（668 行，26 个条目 ①–㉖）
- **调研日期**：2026-10-04
- **一手材料**：`docs/tui-manual-checklist.md`、`src/render/**`、`tests/*.rs`、
  [`scripts/tui-startup-check.py`](../../../scripts/tui-startup-check.py)、`git log`、
  `.scratch/{tui-layout,tui-sidebar,tui-chrome,terminal-title,suspend-gesture,questionnaire-keys,sidebar-toggle,goal-loop,markdown-render,todo-and-modes,exit-gesture}/spec.md`
- **一句话**：**26 条全部还有活读者**（功能都在 `src/` 里），但其中 **2 条（⑨、⑯.3）夹着旧外壳的过时数字**、
  **1 处（④.5）数字双错**；「已被自动化覆盖」这一列里，凡是纯几何 / 文案 / 状态机的都已被
  `tests/render_layout.rs` 等覆盖，**光标、鼠标、resize 三类按 `tui-startup-check.py` 的 docstring 明说是刻意留手工的**，
  所以它们不是重复劳动。

## 分界线怎么读（票面问的第 2 件事）

- [`scripts/tui-startup-check.py`](../../../scripts/tui-startup-check.py) 的 docstring 原文写着：
  「进入那一头……**光标、鼠标与缩放仍留在手工清单里**（`docs/tui-manual-checklist.md`）」，
  又说「这个脚本是会红的那条检查……在 pty 上守进程的两头」。
- [`docs/render.md`](../../../docs/render.md):295 也写着：「真终端必须确认的东西 —— **光标、鼠标、缩放、干净退出** ——
  写成了一份跟着走的清单」。
- **更正一处票面事实**：票面说这条分界「那个脚本的 docstring 与 [`docs/skills.md`](../../../docs/skills.md) 都写明了」。
  实测 `docs/skills.md`（71 行）**通篇没有**「光标 / 鼠标 / resize / 手工清单」任何一处 ——
  它讲的是「一条指令该放在 `AGENTS.md` / skill / hook 哪一边」。写这条分界的是脚本 docstring 与 `docs/render.md`。
  这不影响结论，只影响引用哪一份。

## 26 行表

「功能还在否」= 条目描述的行为在 `src/` 里是否仍成立。
「已被自动化否」= **手工项本身**是否已被 `scripts/tui-startup-check.py` 或 `tests/*.rs` 覆盖；
「自动」= 该条断言已全在自动化里；「手工」= 按上面的分界刻意留手工；「混合」= 机制已在自动化里、
**手感 / 观感 / 真终端那半**仍归手工（这正是它写在清单里的理由）。
「引用元素还准否」= 条目点名的文案、键位、数字、页签与机制是否与今天的代码一致；
**粗体**是实测出的偏差。散文行数是按 `.scratch/docs-slim/map.md` 的口径（空行分隔的整块、
排除标题 / 表格 / 代码块、**列表行并入相邻块**、不另算一段）量出来的。

| 条目 | 功能还在否 | 已被自动化否 | 引用元素还准否 | 散文行数 |
| --- | --- | --- | --- | --- |
| ① 光标 | 在。`src/render/editor.rs`（`PROMPT = "❱ "`、多行草稿、`Ctrl-A/E/U/K/W`、按显示宽度折行） | **手工**（脚本 docstring 明说光标留手工） | 准：`❱ `、`[y] 清空` / `[n] 保留`、标题「清空输入」都在 `wording.rs` | 12 |
| ② 权限模态 | 在。`tui.rs:1048` 组装覆盖层：标题 `权限询问：`（`permission_title`，**不带工具名**）、描述 `调用 bash 查看 …`（`tool_call_line`，与折叠行同一句）、正文 `bash（command=…）`（`permission_call`）、按钮行居中 | 混合：三行结构 / 按钮居中已被 `render_layout.rs` 的 `a_question_splits_into_a_title…`、`the_permission_modals_buttons_are_centred` 覆盖；**焦点回输入区、Shift-Tab 被吃掉是真终端那半** | 准：`[y] 允许` / `[a] 总是允许` / `[n] 拒绝` 在 `PERMISSION_CHOICES`；`permission_asked`（带工具名的旧拼法）只余 plain / 转录侧 | 8 |
| ③ 鼠标 | 在。`EnableMouseCapture`（`tui.rs:556`）、指示条 `↓ {n} 行新内容 · 点此到底`、点击作答 / 折叠行 / 详情 / 问卷 | **手工**（鼠标，脚本 docstring） | 准：指示条文案、`↕ n/m · esc 关闭`、`[图片]` 之外的点名元素都在；Shift 拖拽的代价仍由 ADR 0002 承担 | 27 |
| ④ resize | 在。`a_resize_keeps_the_reader_on_the_same_line` 覆盖锚点；SIGWINCH 重画在 `tui.rs` | **手工**（缩放，脚本 docstring）；锚点那条的**缓冲模型**有自动化 | **一处失准（数字双错）**：④.5「拖到 `120×24` 且草稿写到 10 行：**转录被压到只剩 1 行**」—— 今天的公式是 `转录 = h − 4 − 输入行数`，10 行草稿（封顶 10）下 `24−4−10 = 10` 行，`transcript_rows == 10` 由 `a_tall_draft_costs_the_transcript_and_never_the_sidebar` 钉住。「只剩 1 行」两项都对不上 | 12 |
| ⑤ 粘贴 | 在。`a_paste_never_submits…`、`an_oversized_paste_asks_first…`、`a_tab_in_a_paste…` | 混合：换行归一 / 10 万字符确认 / Tab→4 空格已在 `tests/render_tui.rs`；**真粘贴事件本身**手工 | 准：`粘贴确认` / `粘贴 {n} 字符` / `[y] 粘贴` / `[n] 取消` 都在 | 5 |
| ⑥ Ctrl-J 与 Shift-Enter | 在。`ctrl-j 换行` 在 `KEY_HINTS` | 混合：`no_hint_ever_names_shift_enter` 钉住提示行从不写它；**真终端的协议层不可区分**手工 | 准：提示行只有 `ctrl-j 换行`，`src/` 里 `shift+enter` 零命中 | 4 |
| ⑦ 退出后终端干净 | 在。双击手势在 `tests/render_tui.rs`；回执 `会话 {id}；接着跑：fs-agent -c {id}` 在 `wording.rs:82` | **最自动化**：`scripts/tui-startup-check.py` 每轮跑 `/quit`、空闲 `Ctrl-C`、空闲 `Ctrl-D` 三条 + 忙碌双击（断言退出码 130 + 终端交还）+ `--continue` 收敛；termios 与撤销序列（备用屏幕 / 鼠标 / 括号粘贴 / 标题还原）逐条断言 | 准：`退出会话` 覆盖层已拆（`wording.rs:1718` 那条是 `/quit` 的**帮助文本**，不是覆盖层）；⑦.6 说 panic 路径「靠构造保证」也仍然成立 | 29 |
| ⑧ 忙碌 Ctrl-C | 在。`an_idle_ctrl_c_double_taps_and_a_working_one_cancels_first`、`escape_while_working_is_a_cancel_gesture` | 混合：`Ctrl-C` 先取消已在 `render_tui.rs`；**「进程仍在、输入区可用」的观感**手工 | 准：`已取消 · 再按一次 ctrl-c 退出`、`再按一次 ctrl-c/ctrl-d 退出` 都在 | 4 |
| ⑨ 120×24 草稿 10 行 → 转录被压到 7 行 | 在。几何有自动化：`transcript_rows == 10` | **自动**（`a_tall_draft_costs_the_transcript_and_never_the_sidebar`、`the_input_area_holds_three_rows_before_it_grows_and_the_transcript_pays_for_it` 覆盖了它要验的每一件事） | **整条过时（旧外壳）**：标题与 ⑨.2 写的「转录只剩 **7** 行」是 `CHROME=7` 时代的数；⑨.3 写的「删回空草稿转录回到 **14** 行」同理（今天是 `24−4−3 = 17`）。成因：`tui-chrome` 拆外框与状态行上方那条线，`CHROME` 由 7 降到 4 | 5 |
| ⑩ 左栏三档 | 在。40 / 28 / 隐藏三档 + 内容阶梯在 `layout.rs:367–419` | 混合：档位、字段数、标记亮度有 `render_layout.rs` 多条；**字形、渐变、居中**手工 | 准：`fs-agent 0.1.0`、六个读数、`Cache` / `输出` / `输入` 先丢的顺序都在 | 14 |
| ⑪ `/` 菜单 | 在。`render_layout.rs` 有 14 条菜单测试（打开 / 筛选 / 跟光标 / Tab / Enter / 箭头 / 绕回 / Esc / 问答抢占 / 非 ASCII 角） | 混合：**固定尺寸 buffer 里的框与筛选归 `cargo test`** 是清单自己写的；真终端的按键、重画、位置手工 | 准：`/undo` `/discuss` `/quit`、`回滚上一次编辑`、反白高亮 | 23 |
| ⑫ 折叠提示与详情覆盖层 | 在。折叠行 / 详情 / 截断 / `outputs/<id>.txt` 一族测试在 `render_layout.rs`（`a_collapsing…`、`the_detail_overlay_reads_the_spilled_tool_output`、`a_missing_spilled_file_degrades_to_the_preview`） | 混合：文案、颜色、滚动位置有自动化；**点击精度、滚动惯性、同屏观感**手工 | **一处失准**：⑫.2 写「详情覆盖层**居中于主列**弹出，宽度是 `min(主列宽 − 4, 135)`」—— 这是 `tui-sidebar` 时代的几何；`tui-chrome` 之後覆盖层改成**屏幕居中**，`DETAIL_MAX_WIDTH = 135` 不变，⑳.5 与代码一致、⑫.2 没有跟着改 | 26 |
| ⑬ 鼠标作答的边界 | 在。`a_question_in_the_way_keeps_the_collapsed_lines_unclickable`、`a_click_outside_the_detail_overlay_closes_it`、`every_row_of_a_wrapped_option_answers_that_option` | **手工**（鼠标） | 准：问题拥有指针、点外部关详情、被裁掉那截点不到 | 5 |
| ⑭ `--continue` 重开 | 在。`tests/history_replay.rs`（31 条）+ 脚本的 `--continue` 轮（只判收敛与终端交还） | 混合：**重播内容**在 `cargo test`、**收敛**在脚本，清单自己写明只留观感 | 准：`恢复历史 n/m`、`── 以上为历史 ──`、旧流两条（`计划模式` / `模式变更`）都在 | 20 |
| ⑮ 外壳改版：左栏 tab、状态行、回合条 | 在。`render_layout.rs` 的 `clicking_a_tab_switches_the_sidebar_page`、`only_the_tab_labels_answer_a_click`、`clicking_a_rail_cell_jumps_to_that_turns_question`、`the_rail_window_follows_the_focus…` | 混合：存在 / 档位 / 命中矩形已断言；**字形对齐、点击手感、焦点观感**手工 | 准；⑮.6 的 120 列提示行 `enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 模式 · ctrl-c/ctrl-d 退出`（无 `就绪`）与 `the_hint_row_gives_up_hints_before_it_gives_up_the_way_out` 逐字一致；⑮.7 的文本宽度 117 / 77 / 49 与 `TRAILING_COLUMNS = 2` 一致 | 29 |
| ⑯ 输入区三行、提示符色相与静止的左栏 | 在。`MIN_INPUT_ROWS = 3`、`PULSE_FRAME = 60ms`、色环与下落都留在仓库里但撤出屏幕（`the_colour_ring_is_kept_off_screen`、`the_mark_does_not_move…`） | 混合：三行地板 / 起始色 / mark 静止已由 `render_layout.rs` + `tui.rs` 单测断言；**光标落点、颜色观感、模糊宽度字形**手工 | **一处失准（数字）**：⑯.3「40×10 这一档只有 **2 行**、转录只剩 1 行」—— 今天 `max_input_rows(10)` 夹住的是地板 3 行，`40×10` 的转录是 **3** 行（`every_size_in_the_matrix_draws_the_regions_its_budget_allows` 第一行、`a_floor_sized_terminal_still_draws_the_main_column`）。⑯.1 / ⑯.4 / ⑯.5 / ⑯.8 的常数与守卫（`if state.busy()`）都准 | 32 |
| ⑰ 模式循环与 `todo` 标签 | 在。`shift_tab_cycles_the_status_row_through_the_four_modes_and_back`、`no_key_opens_a_plan_mode_any_more`、`the_todo_tab_appears_with_a_non_empty_list_and_then_stays`、`an_executors_list_stays_out_of_the_sidebar` | 混合：状态行文案 / 标签进退 / 页内容 / 命中矩形有自动化；**按键手感、字形、28 格可读性**手工 | 准：四档 `询问 → 工作区 → 自动 → 只读`、`＋N 项`、`已完成 n/m` 都在；`/plan` 与计划模式弹窗零残留 | 24 |
| ⑱ 沙箱：内核真的拦住了 | 在。`tests/sandbox.rs`（含真 bwrap 那条）+ `wrap()` 逐字断言 | **手工**（性质上：这是一份 2026-10-01 的真机记录，不是可自动化的断言） | 准：`.scratch/sandbox/spec.md:175` / `:185` 反过来引用 ⑱ 第 3 / 第 8 条，编号仍对得上 | 46 |
| ⑲ `workspace` 档与升级手势 | 在。`tests/permission_gate.rs`（四档 × 方向 × 升级矩阵）、`tests/workspace_mode.rs`、`tests/sandbox.rs`（假 bwrap 升级全链路） | 混合：纯函数与端到端都有；**弹窗读起来够不够、真内核下那条出口**手工 | 准：`被沙箱拒绝，申请写工作区之外` / `理由：…` / `要放开的路径：…`、`这一条没有任何通道放宽`、`路径不存在` 都在 | 33 |
| ⑳ 外壳收干净 | 在。`tui.rs:200` `CHROME_LINE = Color::Rgb(0x4a, 0x4a, 0x4a)`，`paint_rule` 画 `┄`、`draw_divide` 画 `┆`，外框不再画 | 混合：几何与字形由 `render_layout.rs`（118 条用例）钉住；**读起来舒不舒服、虚线分不分得清**手工 | 准：⑳.4 点名的常量值一字不差；⑳.1 的「四边没有框线」与矩阵测试里 `!text.contains(['┌','┐','└','┘'])` 一致 | 30 |
| ㉑ 转录里的 Markdown | 在。`tests/render_markdown.rs`（34 条）+ `render_highlight.rs`（8 条）+ 十语言 grammar 表（`highlight.rs:216–321`） | 混合：表格 / 代码块 / 续行由 `tests/render_markdown.rs` 与 `render_layout.rs` 钉住；**语法高亮与 CJK 折行的组合**手工 | 准：`[图片] 图 (https://example.com/a.png)`、语言名右对齐、assistant 续行顶格 / 用户续行缩进都对 | 27 |
| ㉒ 目标循环 | 在。`tests/render_layout.rs` + `tests/goal_loop.rs`（19 条）；`/goal-new` 与 `/loop` 在 `cli.rs` | 混合：几何与状态在自动化；**手感**手工 | 准：`目标 {name} 被主动停下`、`历史压成摘要`、`已提醒模型`、`禁言` 都由 `tests/goal_loop.rs` / `render_layout.rs` 对应；`docs/goals.md:190` 的 ㉒ 引用仍指向本节 | 20 |
| ㉓ 终端标题 | 在。`tests/wording.rs`（拼法 / 40 列封顶）、`tests/render_tui.rs`（状态与目标名来去）、脚本（`CSI 22 t` 保存、`OSC 0` 写入、`CSI 23 t` 还原） | 混合：两条腿已在自动化；**标题看起来对不对、退不退出得回来**手工 | 准：`运行中` / `等你` / `重放中` 在 `wording.rs:1390–1392` | 24 |
| ㉔ 挂起 | 在。`tests/render_tui.rs`（`ctrl_z_asks_for_a_suspend_and_is_taken_once`、`a_suspend_rewrites_the_title_even_when_it_did_not_change`）+ 脚本两轮（TUI / `--plain`，判「停住 + 停之前已交还 + SIGCONT 回来重进重绘 + 出口干净」） | 混合：两条腿已在自动化；**画面回来之后对不对、标题回不回来**手工 | 准：`Ctrl-Z` 排在一切按键之前（`tui.rs:2494–2497`），重放也拦不住它（`ctrl_z_suspends_even_during_a_replay`） | 25 |
| ㉕ 问卷 | 在。`tests/ask_user_question_tui.rs`（30 条）+ `render_layout.rs`（页脚 / 折行 / 滚轮分派） | 混合：状态机与帧断言在自动化；**区域高亮、折行后光标、两句回执**手工 | 准：`j`/`k` + `Ctrl-N`/`Ctrl-P`、`自定义：`、`再按一次 esc 退出询问`、`已取消 · 再按一次 ctrl-c 退出` 都在 | 27 |
| ㉖ 左栏开关 | 在。`render_layout.rs`：`ctrl_o_takes_the_sidebar_away_and_brings_it_back`、`ctrl_o_cannot_bring_the_sidebar_back_below_eighty_columns`、`ctrl_o_works_while_a_run_is_in_flight`、`ctrl_o_is_ignored_while_the_detail_overlay_is_up`、`a_todo_list_landing_while_the_sidebar_is_hidden_does_not_pop_it_back`；`wording.rs`：`ctrl-o 左栏` 排在最末 | 混合：几何 / 页签命中 / 窄档无效 / busy 与覆盖层守卫 / 提示行档位都在自动化；**那 41 列值不值、末条提示出不出现**手工 | 准：`Ctrl-O` 在 `tui.rs:142`（`'o' => Key::CtrlO`）；`ctrl-o 左栏` 只在最宽档（174 列）出现，与清单自己写的可发现性边界一致 | 23 |

（「散文行数」这一列的合计是 **534**：按 map 的口径（空行分隔的整块、排除标题 / 表格 / 代码块、
列表行并入相邻块）量，26 条共 368 行散文 + 164 行列表 + 2 行代码。整份文件的账是：
21 行文件头（第 1–21 行的抬头与准备）+ 26 行条目标题 + 534 行块内容 + 60 行空行 = 668 行。
map 与票面写的「散文 548 行 / 668 行」是另一套口径（正文行数按我的口径是 560 行，见下注），
两者量级相近；**本表用可复现的那一套**。）

> 口径备注：`wc -l` 报 667，是因为文件末尾有换行符；按行数算是 668 行。
> 「正文行数 560」= 668 − 26 行条目标题 − 21 行文件头 − 61 行空行（含文件头前的那一行）。

## 逐条核对过的推翻链

- **`tui-sidebar` 推翻 `tui-layout`**：spec 抬头写着「**推翻了 `.scratch/tui-layout/spec.md` §2 的四条**」——
  面板移到左、四区边框改成一圈外框 + 内部分隔线、取消整宽 header、cwd 与时钟不再显示。
  `tui-layout/spec.md` 自己在 §2 上方有一段「**补记（2026-09-30）**」承认外壳已被推翻。
  这解释了 ⑨ / ⑯.3 里那批旧数字的来源。
- **`tui-chrome` 又推翻 `tui-sidebar` 两处**：spec 抬头写着它推翻「`tui-sidebar/spec.md` §1 的一圈外框 + 内部分隔线
  （只留后者、改成虚线）」与「同文件 §7 / `tui-sidebar/issues/04-turn-rail.md` §2 的『详情、问题吃掉一切指针』」。
  代码侧的两笔账是 `CHROME` 从 7 降到 4、`CHROME_LINE` 取 `#4a4a4a`、覆盖层改屏幕居中。
  这是 ④.5 / ⑨ / ⑫.2 三处过时数字的直接成因。
- **`sidebar-toggle` 推翻 `tui-sidebar` §2 一句**：spec 抬头写明「『去留只由宽度决定』原文不改写、另加带日期的补记；
  宽度那一层本身（40 / 28 / 隐藏三档）不动」。㉖ 与 ⑩ 因此都仍然成立。
- **`exit-gesture`** 拆掉了退出确认覆盖层（`git log` 的 `bb6de14`），⑦ 里「**那个退出确认框已经拆了**」是准确的当前事实。
- **`git log -- docs/tui-manual-checklist.md`** 从 `b48871c`（「tui-ux 落地的 spec 回改、手工清单」）
  到 `6ad1524`（「Ctrl-O 收起与叫回左栏」）共 **26 个提交**，条目确实是逐轮追加的。
  但「后半段更臃肿」这个假设**只弱成立**：⑰–㉖ 的块行数是 20–46，①–⑬ 是 4–27，
  两者区间大半重叠；最长的 ⑱（46 行）是逐字真机记录（第 3、8 条被 `.scratch/sandbox/spec.md` 反引），
  它长是因为内容本身长，不是因为行文松；⑨（5 行）与 ⑥（4 行）反而是全表最短的两条。

## 回答票面的总结句

- **还有活读者、只是写得长（压表达的对象）**：**26 / 26 条**。功能一条都没被推翻；
  被动过的只是**条目里引用的旧数字**，不是条目要验的行为。
- **已经失效（冻结项 4 的规则覆盖不到的那一类）**：**0 条整体失效**，
  但有 **3 处引用失效**，全都指向同一段历史（`tui-chrome` 改外壳之前）：
  - ④.5「120×24 草稿 10 行 → 转录被压到只剩 1 行」（今天 10 行，且 1 行从未成立）；
  - ⑨ 标题与 ⑨.2 / ⑨.3 的 7 / 14 行（今天 10 / 17 行）；
  - ⑫.2「详情覆盖层居中于主列、宽度 `min(主列宽 − 4, 135)`」（今天屏幕居中，⑳.5 已改口、⑫.2 没改）。
  另有 ⑯.3 的「40×10 输入区只有 2 行」（今天 3 行）。
- 因此票 06 面对的是：**删无可删，但改要改 4 处数字**（压表达与「改数」是两件事 ——
  压表达只拆段落、不改信息量，而这 4 处是事实性的信息量已经错了）。

## 给后续票的事实（票 06 / 04 会用到的）

1. **手工清单不是废纸**：26 条全在。按「已被自动化否」那一列的分布是 **1 条全自动（⑨）、
   22 条混合、3 条纯手工（① 光标 / ③ 鼠标 / ⑱ 沙箱真机记录）**；无论哪一类，
   「真终端那半」按脚本 docstring 的口径本来就不该自动化 —— 不能拿「已有 `cargo test`」当删条目的理由。
2. **⑨ 这一条整个是自动化的**：它要验的几何已被两条测试逐值钉住，唯一留下的活读者价值是「旧数字被纠正」。
   它是最接近「可以删」的一条，但删它属于票 06 的决定，本票不判。
3. **编号被外部引用 5 处**：`docs/render.md` ⑭、`docs/goals.md` ㉒、`.scratch/sandbox/spec.md` ⑱（两处）、
   `.scratch/tui-chrome/spec.md` ⑳、`.scratch/sidebar-toggle/spec.md` ㉖。票 06 若动条目编号，
   这 5 处要同步（map 的「会撞的既有决定」只点到了 ⑭ / ㉒ 两处）。
4. **「38 / 40 列」这类数字是活的**：⑮.6 的 120 列提示行、⑮.7 的 117 / 77 / 49、㉖ 的「那 41 列」
   都与 `layout.rs` / `wording.rs` 今天的值一致 —— 压表达时不要顺手改它们。
