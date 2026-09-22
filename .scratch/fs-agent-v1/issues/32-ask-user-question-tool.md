# 32: `ask_user_question` —— 模型发起的用户提问，接管底部输入区

**What to build:** 让**模型**能在干活途中向用户提问：一个模型面工具 `ask_user_question`，答案是那条
`tool_call` 的唯一结果；TUI 上**接管底部输入区**把它变成问卷界面（一屏一问、分页、可跳过）。这是
**第三类发起者**——现有两类是 harness 发起的（权限询问、计划冲突，走中段覆盖层）与渲染器发起的
（超长粘贴、`Esc` 清草稿）。对照物是 DSH 的 `ask_user_question` + `dsh-client-ui-user-questions`，
事实清点见 `.scratch/ask-user-question/research/01-dsh-ask-user-question-and-composer-takeover.md`
（路线与分叉见同目录 `seed.md`）。

Blocked by: None（它依赖的 §7 工具表 / §19 渲染与 CLI 组装 / 渲染接缝在票 07、18、19 里都已 done）

Status: done

**参考:** spec §7（工具 trait 与副作用）、§19（渲染与 CLI 组装：输入归渲染器、渲染器只管两个 sink）、
§6（取消传播）、§2（事件 schema，**本次不改**）、§12 与 User Story 8（无交互能力时降级而不是挂住）、
`docs/render.md`、`dsh-tool-ask-user` / `dsh-client-ui-user-questions` 的已安装产物

## 已定的决定（grilling 第 1–2 轮，12 条）

**词汇**
- [x] `CONTEXT.md` 新增**用户提问（User Question）**词条：模型发起、结果是工具结果；并把现有的权限 /
  计划冲突明确为**询问（Ask）**——一个是 harness 的闸门（答案决定去留），一个是模型的问题（答案是
  上下文）。工具名 `ask_user_question`。

**工具契约（wire，snake_case，与现有工具一致）**
- [x] `questions`（required，≥1 条），每条 `{ id, question, header?, options?: [{label, description?}], multi_select? }`；
  `multi_select` 默认 `false`
- [x] 结果 = `{"answers":[{"id","selected","custom"?}]}` 序列化成 JSON 文本，作为那条 `tool_call` 的
  唯一结果
- [x] **三条编码约定写进工具描述**：`selected: []` 无 `custom` = 跳过；单选下自定义文本**覆盖**已选
  （`selected: []` + `custom`）、多选下**补充**；推荐项放第一位且 label 末尾追加 `(Recommended)`，
  而**答案值保留原串**
- [x] **不收** `detail` / `intent`（DSH 里它们是 harness 专属，模型面工具根本不暴露，`execute` 还会丢弃）

**呈现**
- [x] TUI：接管底部输入区；一屏一问、分页（footer `2 / 3`）；**每题必须显式作答或跳过才能提交**，
  本题未处理时提交键禁用；单选选中即前进；单选下打字清空已选、多选下保留；`(Recommended)` 只做
  **显示**标记
- [x] `Esc` / `Ctrl-C` 保持现有含义：取消**这次运行**（§6）。**不引入**「只放弃这次提问」的第三个手势，
  也**不新增错误类型**——取消后那条调用走 §6 的第 5 条路径拿到它唯一的结果
- [x] **现有四类问题不动**（继续走中段覆盖层）：**票 30 不受影响**

**渲染器与降级**
- [x] plain：**逐行问答**（它本来就读 stdin / 写 stderr）
- [x] headless：**不把这个工具挂进工具表**（给模型一个必然失败的工具只是浪费一次调用）
- [x] 兜底：真到了没有 answerer 的组装里，返回**模型可读的错误结果**，绝不挂住

**谁能问 / 流**
- [x] 只有主会话能问：**执行者的工具表里不挂它**（与「执行者的表里没有 `task`」同一模式）
- [x] **零 schema 改动**：问题在 `tool_call` args、答案在 result。`--continue` 撞上「进程死在问题挂着
  时」⇒ 复用既有的**悬空 `tool_call` 合成 unknown 结果**

## 测试

- [x] 工具层：合法调用返回 JSON 答案文本；`questions` 为空 / 缺 `id` / `id` 重复被拒；没有 answerer
  时返回错误结果而不是挂住；描述里含那三条约定
- [x] 接缝层：`ToolContext` 的端口被注入到工具；执行者的表里没有它；headless 组装的表里没有它
- [x] TUI 层（`TestBackend` 帧断言）：接管出现／提问与选项在底部／分页 `/ 3`／未处理时不能提交／
  跳过与作答的编码／推荐标记不改答案值／作答完成后交回常驻输入区
- [x] plain 层：逐行问答读到答案；EOF 时返回错误结果
- [x] 端到端：假 provider 起一个真会话，模型调 `ask_user_question`，断言流上那条 `tool_call` 有且
  只有一条结果，且结果就是答案 JSON

## Comments

**2026-09-22（grilling）** 12 条决定全部由访谈定下，逐条在上。三条**不是分叉而是架构强制**的，
记在这里免得被当成漏项：问卷状态活在 `TuiState` 里（本仓库没有「重挂载」，所以 DSH 那套草稿 store
与键控不适用）；**不设等待超时**（DSH 也没有——人在键盘前，超时就是错的；headless 又不挂这个工具，
不存在挂死）；答案以 JSON 文本回来。

**代价（已知并接受）**：plain 逐行 + TUI 接管 + headless 不挂 = **三条路径都要建、都要测**，这是
这个设计里最贵的一块。它的手感（键位、分页、提交）属「必须亲眼看到」，所以 TUI 侧用 `TestBackend`
帧断言，必要时再拿 `/prototype` 兜。

**2026-09-22（实现）** 清单全部落地，Status: done。四个 red→green slice，按票面顺序：

- **(A) 工具 wire + 无端口错误**：先写 `tests/ask_user_question.rs::a_question_round_trips_as_the_answers_json`，
  编译失败（`E0432`：`fs_agent::questions` 与 `AskUserQuestionTool` 都不存在）⇒ red；建 `src/questions.rs`
  与 `src/tools/ask_user.rs` 后 8 条工具层测试 green。结果 JSON 逐字断言
  `{"answers":[{"id":"framework","selected":["serde"]}]}`；`custom` 缺席即省略。
- **(B) 端口与注入**：`tests/ask_user_question.rs` 里加 e2e 与表测试，`SessionScaffold` 缺 `questions`
  字段（`E0560`）⇒ red；打通 `SessionScaffold → OpenedSession → Session::questions() → PendingCall →
  ToolContext.questions → registry::dispatch` 后 12 条 green。
- **(C) TUI 接管**：`tests/ask_user_question_tui.rs` 9 条先全部失败（`ConsoleRequest::Questionnaire`
  当时只回一个占位 `Err`，屏幕上看不到接管）⇒ red；实现 `Pending::Questionnaire` / `Questionnaire` /
  `QuestionDraft`、`questionnaire_key`、`questionnaire_lines`、`bottom_rows` 与底部块绘制后 green。
- **(D) plain + headless 表**：plain 三测先因找不到 `spawn_plain_console_with` / `LineReader`
  （`E0432`）⇒ red；把 plain 控制台的输入原语抽成注入的 `LineReader`、实现逐行问卷与
  `encode_plain_answer` / `chosen_options` 后 green。headless 不挂工具由 (B) 的两条表测试与
  一条 provider-request 测试盖住。

**新增测试（精确名字）**

- `tests/ask_user_question.rs`（15）：`a_question_round_trips_as_the_answers_json`、
  `custom_text_and_multi_select_round_trip`、`the_description_carries_the_three_encoding_conventions`、
  `the_tool_is_read_only_and_only_the_main_session_may_ask`、`an_empty_question_list_is_refused`、
  `a_question_without_an_id_is_refused`、`duplicate_question_ids_are_refused`、
  `without_a_question_port_the_call_fails_instead_of_hanging`、
  `the_table_decides_whether_the_model_may_ask`、`an_executors_table_has_no_way_to_ask`、
  `the_model_can_ask_and_the_answer_is_the_tools_one_result`（e2e：假 provider 真实会话，断言那条
  `tool_call` 有且只有一条结果、且结果就是答案 JSON）、
  `a_headless_session_does_not_advertise_the_tool_to_the_model`、
  `the_plain_console_answers_a_questionnaire_line_by_line`、
  `the_plain_console_reads_a_multi_select_and_a_skip`、
  `the_plain_console_fails_at_end_of_input_instead_of_looping`。
- `tests/ask_user_question_tui.rs`（9，`TestBackend` 帧断言）：
  `the_question_and_its_options_take_over_the_bottom_input_area`、
  `a_single_select_choice_advances_and_the_footer_pages`、
  `submit_is_refused_until_every_question_is_answered_or_skipped`、
  `typing_overrides_a_single_select_choice_and_supplements_a_multi_select_one`、
  `the_recommended_marker_is_display_only`、`answering_hands_the_bottom_back_to_the_resident_input`、
  `a_question_with_no_options_is_answered_with_free_text`、
  `an_empty_questionnaire_is_refused_rather_than_panicking`、
  `escape_still_cancels_the_run_and_never_answers_the_questionnaire`。
- `tests/wording.rs`（3）：`a_questionnaire_reads_in_chinese_and_pages`、
  `a_recommended_option_keeps_its_value_when_displayed`、
  `the_plain_console_asks_a_questionnaire_in_chinese`。

**Mutation checks（翻条件→变红→翻回）**

1. 关掉重复 `id` 检查（`if false && …`）＋把 `delegable()` 改成 `true` ⇒
   `duplicate_question_ids_are_refused`、`the_tool_is_read_only_and_only_the_main_session_may_ask` 同时红；
   翻回后绿。
2. 把 `agent.rs` 的 `questions: session.questions().cloned()` 改成 `None`（等于不注入端口）⇒
   `the_model_can_ask_and_the_answer_is_the_tools_one_result` 在 `assert!(ok)` 处红；翻回后绿。
3. 让单选下打字**不清空**已选（`type_custom` 的 `if !multi_select` → `if false`）⇒ 测试**仍然绿**，
   暴露它只断言了编码后的答案（编码层本来就会用 `custom` 覆盖 `selected`）。于是给该测试补上帧断言
   （打字后 `● 1. a` 必须变成 `○ 1. a`），再跑同一 mutation ⇒ 红；翻回后绿。
4. 去掉单选选择后的 `self.advance()` ⇒ `a_single_select_choice_advances_and_the_footer_pages` 红；
   翻回后绿。
5. 实现自审时发现的一个真 bug：`press` 把所有 `1-9` 一律当选项键，于是**无选项题目里打数字会被吞掉**。
   修法是让 `choose` 返回「是否真的选中」，没选中就落回 `type_custom`；`a_question_with_no_options_…`
   的输入从 `my-name` 改成 `v2-name` 钉住它。再把回退那两行去掉（还原旧行为）⇒ 该测试红；翻回后绿。

**命令与结果**

- `cargo clippy --all-targets` → 0 warnings。
- 单个文件按 slice 跑（见上）；最后 `cargo test --all-targets` → 0 failures（全部 `test result: ok`）。
- `cargo build --bin fs-agent` 后 `python3 scripts/tui-startup-check.py target/debug/fs-agent 2` →
   4/4 GREEN（`0/4 red`）。
- 只对本次改过的文件跑了 `rustfmt`；`src/context/repo_map.rs` 的既有 fmt 漂移不是本票引入，未动。

**没做／做得不一样的地方**

- **TUI 测试放在新文件 `tests/ask_user_question_tui.rs`**，而不是加进 `tests/render_layout.rs`：后者是
  票 30 的文件，本仓可能有并行 session，独立文件避免互相覆盖；`screen`/`buffer` 等 helper 照它抄了一份。
- **为 plain 加了一条注入接缝**：`LineReader`（`Box<dyn FnMut() -> …>`）＋ `spawn_plain_console_with`。
  原来的 `spawn_plain_console` 签名与行为不变，CLI 两个调用点不变；没有这条接缝，plain 的逐行问卷只能
  靠管道进程测。权限询问也一起改走注入 reader（行为不变）。
- **键位是我定的**（票面只规定「一屏一问、分页、每题必须作答或跳过、单选前进、打字清空/保留」）：
  `1-9` 选择/切换、`Tab` 跳过并前进、`←→` 换题、`Enter` 在还有未处理题时前进/全处理完即提交、
  可打印字符进自定义文本、`Backspace` 删。**超过 9 个选项时第 10 个起显示但按不到**（TUI 键表上限；
  wire 与 plain 无此限制）——这一条票面没写，记在这里。
- `(Recommended)` 在 TUI/plain 里**从显示中剥掉**、换成中文徽标 `（推荐）`，答案值仍是原串（含
  标记）——这是 research 里 DSH `parseRecommendedLabel` 的读法，也是「只做显示标记」的自然实现。
- 讨论的讨论者（`SpeakerId::Debater`，属主会话）也挂了这个工具；只有执行者没有。票面「只有主会话能问」
  按此落地。
- 顺手在 `docs/render.md` 的键盘一节补了一条 `ConsoleQuestions`/接管说明，保持该文档与实现一致。
- 事件 schema、`Asker`、`Question`/`AnswerChoice` 一个字没动；`--continue` 的悬空 `tool_call`
  合成路径原样复用（没有新增分支）。

**2026-09-22（复审修复）** 两个审查轴的 9 条逐条落地（未 commit；不勾选框）：

1. **`tests/custom_tools.rs` 的分离符不变量漏了新工具**：`builtin_names_never_contain_the_separator`
   从 `builtin(false)` 改成 `builtin(true)`——该不变量管的是**每个内建**（spec §14），不是 headless
   广告的那张表。同文件 `the_declared_tool_is_registered_and_recognizable` 里那句「内建表里没有它」
   也一并改成 `builtin(true)`（那句同样在说内建表）。由现有两条测试钉住。
2. **spec §7 工具数**：`第十个` → `第八个`（§7 列表七个、`builtin()` 注册七个）。
3. **`Questionnaire::answers()` 无视 `skipped`**：先加
   `a_skipped_question_is_no_answer_even_after_typing`（打字后 Tab 跳过 → `selected: []` 且无
   `custom`），跑出红（`custom: Some("x")`）再修：`answers()` 第一个判断就是 `draft.skipped`，直接
   返回空答案（spec §7）。
4. **键位换成已定的那套**：删掉 `1-9` 选键与 `choose`，改成 `↑/↓` 移动高亮（`QuestionDraft.highlight`）、
   `Enter`/`Space` 确认、`Tab` 跳过、`←/→` 翻页、可打印字符进自由文本、`Backspace` 删。数字从此处处是
   普通文本，任何选项都可选（无 9 个上限）。单选取中即前进、未处理不能提交、`Esc`/`Ctrl-C` 原样。
   更新了原来按数字的测试；新增/加强：`digits_are_free_text_not_selection_keys`、
   `the_option_window_scrolls_so_the_highlighted_option_stays_visible` 里第 16 个选项可选、
   `enter_keeps_typed_text_instead_of_re_confirming_an_option`（见下「做得不一样」）。
5. **选项溢出**：先加 `the_option_window_scrolls_so_the_highlighted_option_stays_visible`
   （20 个选项、移到第 16 个），跑出红（旧 `.take()` 只画到 `opt-06`，16 不可见）再修：
   `questionnaire_lines` 拆出 pinned 前缀（header+题干）与 pinned 答案行，新增
   `questionnaire_window` 只滚动中间选项窗口，`option_window_start` 保证高亮始终在窗内；绘制改走窗。
6. **`questionnaire_hint` 不再总是承诺「提交」**：改成 `questionnaire_hint(ready)` /
   `questionnaire_status(index, total, ready)`，`ready` 由 TUI 传 `all_handled()`——未处理完显示
   `enter 继续`，全处理完才是 `enter 提交`。`wording.rs` 测试同时钉两态。
7. **plain 表达多选「补充」**：多选且有选项的题目现在读**两行**——第一行编号（或自定义文本），第二行是
   可选补充；`encode_plain_answer(question, line, supplement)` 把补充并入 `custom`，于是 `selected`
   与 `custom` 可同时出现。提示词明说「下一行补充」（`questionnaire_plain_supplement_prompt`）。
   由 `the_plain_console_lets_a_multi_select_answer_options_and_text_together` 钉住；
   `the_plain_console_reads_a_multi_select_and_a_skip` 的脚本补了第三行（多选现在吃两行）。
8. **词表补全**：`CONTEXT.md` 的 `## 提问` 增加问卷（Questionnaire）、问卷请求
   （QuestionnaireRequest）、作答草稿（QuestionDraft）、问题选项（questions::Choice）、问卷文案
   （`questionnaire_*`）。`questions::Choice` 与 `wording::Choice` **两个名字都保留**（改任一个都更
   大），在词条里写明二者同名不同物。
9. **抽出唯一的选项行生成器**：`wording::questionnaire_option(number, label, description)`，plain
   打印器与 TUI 的 `questionnaire_parts` 都调它，`(Recommended)`→徽标的显示约定只在这一个地方。

**做得不一样 / 顺带**

- 第 4 条落地时发现一个真 bug：若 `Enter` 无条件先 `confirm_highlight`，单选中「打了字再按 Enter 继续」
  会用高亮选项覆盖掉刚打的文本。改成「当前题已作答（已选或已输入）⇒ `Enter` 只前进；未作答且有选项
  ⇒ 才确认」，并加 `enter_keeps_typed_text_instead_of_re_confirming_an_option` 钉住。
- `Space` 在有选项的题上确认，在无选项的题上仍是普通空格——「Space 确认」与「可打印字符进文本框」在
  空格上互斥，按题面「Space confirms it」优先，自由文本题没有可确认的东西。
- 多选第二行是**恒定**读的（不是「有编号才读」），这样两行形状稳定、提示词可如实描述；代价是既有
  plain 多选测试要多喂一行。
- 命令与结果：`cargo clippy --all-targets` → 0 warnings；`cargo test --all-targets` → 0 failures
  （`ask_user_question_tui` 13 条全绿）；`cargo build --bin fs-agent` 后
  `python3 scripts/tui-startup-check.py target/debug/fs-agent 2` → `0/4 red`（4/4 GREEN）。
