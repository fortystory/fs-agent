//! `ask_user_question` 对 TUI 底部输入区的接管（spec §7、§19）。
//!
//! 接缝就是 `render_layout.rs` 测过的那一条：一个 `TuiState` 进
//! [`draw_frame`]，一块定尺的 `TestBackend` 缓冲出来，所以每一条
//! 断言说的都是人看得见的东西。键盘那些断言说的是循环将会收到的
//! 那个答案，那是这次接管的另一半。
//!
//! 它住在自己的文件里，而不是住在 `render_layout.rs` 里，好让拥有那个
//! 文件的票能继续改它，而不跟这一个撞车。

use fs_agent::questions::{UserAnswer, UserAnswers, UserQuestion};
use fs_agent::render::{
    draw_frame, ConsoleRequest, FrontEndEvent, Key, QuestionnaireRequest, SessionFacts, TuiState,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use ratatui::Terminal;
use tokio::sync::oneshot;

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/fs-agent".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        // 会话被组装时所处的模式；`ask` 是默认，想要另一档的
        // 测试在自己的 facts 里说清楚。
        mode: fs_agent::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        number_style: fs_agent::render::wording::NumberStyle::Cn,
        speaker_order: Vec::new(),
    }
}

fn state() -> TuiState {
    TuiState::new(facts(), std::path::PathBuf::from("/x/fs-agent"), None)
}

/// 一道题，带选项与多选旗标。
fn question(id: &str, text: &str, options: &[&str], multi_select: bool) -> UserQuestion {
    UserQuestion {
        id: id.to_owned(),
        question: text.to_owned(),
        header: None,
        options: options
            .iter()
            .map(|label| fs_agent::questions::Choice {
                label: (*label).to_owned(),
                description: None,
            })
            .collect(),
        multi_select,
    }
}

/// 把一份问卷交给状态，与循环的做法一样。
fn ask(
    state: &mut TuiState,
    questions: Vec<UserQuestion>,
) -> oneshot::Receiver<Result<UserAnswers, String>> {
    let (reply, answers) = oneshot::channel();
    state.request(ConsoleRequest::Questionnaire(QuestionnaireRequest {
        questions,
        reply,
    }));
    answers
}

/// 把键盘送进输入区：有选项的题上，`j` 一直走到越过末项就落到那里（spec §1）。
///
/// 区域立起来之后，字符只在输入区进文本，所以「打开就能打字」的那些老测试都要先走这一下。
fn walk_into_the_input(state: &mut TuiState, options: usize) {
    for _ in 0..options {
        state.key(Key::Char('j'));
    }
}

/// 状态发出的那个答案 —— 如果它已经发出了的话。
fn answer(rx: &mut oneshot::Receiver<Result<UserAnswers, String>>) -> Option<UserAnswers> {
    match rx.try_recv() {
        Ok(Ok(answers)) => Some(answers),
        Ok(Err(reason)) => panic!("状态拒答：{reason}"),
        Err(oneshot::error::TryRecvError::Empty) => None,
        Err(oneshot::error::TryRecvError::Closed) => {
            panic!("状态没作答就把问卷丢了")
        }
    }
}

fn buffer(width: u16, height: u16, state: &mut TuiState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|frame| draw_frame(frame, state))
        .expect("一帧");
    terminal.backend().buffer().clone()
}

fn cells(frame: &Buffer, y: u16, from: u16, to: u16) -> String {
    let mut text = String::new();
    let mut x = from;
    while x < to {
        let symbol = frame[(x, y)].symbol();
        text.push_str(symbol);
        x += symbol.cell_width().max(1);
    }
    text
}

fn screen(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let frame = buffer(width, height, state);
    (0..height).map(|y| cells(&frame, y, 0, width)).collect()
}

#[test]
fn the_question_and_its_options_take_over_the_bottom_input_area() {
    let mut state = state();
    let _rx = ask(
        &mut state,
        vec![question(
            "q",
            "Which framework?",
            &["serde", "manual"],
            false,
        )],
    );

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("Which framework?"), "{text}");
    assert!(text.contains("1. serde"), "选项是带编号的：{text}");
    assert!(text.contains("2. manual"), "{text}");

    // 它在主列脚下那块输入区里，不是中间的覆盖层：
    // 它的那一行坐在输入区上面那条横线之下，而屏幕上如今一个框角
    // 都没有 —— 外壳没有外框，问卷自己也没有边框
    // （`.scratch/tui-chrome/spec.md` §1）。
    let input_rule = rows
        .iter()
        .position(|row| row.ends_with('┄'))
        .expect("输入区上面那条横线");
    let question_row = rows
        .iter()
        .position(|row| row.contains("Which framework?"))
        .expect("这个问句在屏幕上");
    assert!(
        question_row > input_rule,
        "这个问句画在底部输入区里，不是浮在中间的覆盖层：\n{text}"
    );
    assert!(
        !text.contains(['┌', '┐', '└', '┘']),
        "没有任何框浮在转录上面：\n{text}"
    );
}

#[test]
fn a_single_select_choice_advances_and_the_footer_pages() {
    let mut state = state();
    let _rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a", "b"], false),
            question("two", "Second?", &["c", "d"], false),
            question("three", "Third?", &["e", "f"], false),
        ],
    );

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("1 / 3"), "页脚在翻页：{text}");

    // 在单选题上确认高亮那个选项，就前进到
    // 下一题。
    state.key(Key::Enter);
    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(text.contains("Second?"), "{text}");
    assert!(text.contains("2 / 3"), "{text}");
    assert!(!text.contains("First?"), "同一时刻屏幕上只有一道题：{text}");
}

#[test]
fn submit_is_refused_until_every_question_is_answered_or_skipped() {
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a"], false),
            question("two", "Second?", &["b"], false),
            question("three", "Third?", &["c"], false),
        ],
    );

    // 确认第一个答案会前进，但在它后面那些题还没处理完时，
    // 绝不能提交。
    state.key(Key::Enter);
    assert!(answer(&mut rx).is_none(), "还有题没处理，提交被拒");

    // 第二个也一样：答案已经作出，提交仍然要另外按一次。
    state.key(Key::Enter);
    assert!(answer(&mut rx).is_none(), "还有一题没处理，提交被拒");

    // 显式跳最后一题，才让整份问卷完成；跳过这个动作
    // 本身也不提交。
    state.key(Key::Tab);
    assert!(answer(&mut rx).is_none(), "跳过最后一题的那一下不提交");

    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("所有题都处理完了就提交");
    assert_eq!(
        answers.answers,
        vec![
            UserAnswer {
                id: "one".to_owned(),
                selected: vec!["a".to_owned()],
                custom: None,
            },
            UserAnswer {
                id: "two".to_owned(),
                selected: vec!["b".to_owned()],
                custom: None,
            },
            UserAnswer {
                id: "three".to_owned(),
                selected: Vec::new(),
                custom: None,
            },
        ]
    );
}

#[test]
fn a_skipped_question_is_no_answer_even_after_typing() {
    // 跳过是一个显式的「不作答」，而且它压过跳过之前打进去的
    // 文本：答案是 `selected: []`，没有 `custom`（spec §7）。先打字、
    // 再决定不作答，绝不能把那文本偷渡进结果里。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );

    state.key(Key::Char('x'));
    state.key(Key::Tab);
    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("这次跳过让整份问卷完成");
    assert_eq!(
        answers.answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: Vec::new(),
            custom: None,
        }]
    );
}

#[test]
fn typing_overrides_a_single_select_choice_and_supplements_a_multi_select_one() {
    // 单选：自定义文本赢，所以回来的 `selected` 是空的（spec §7）。
    let mut single = state();
    let mut single_rx = ask(
        &mut single,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    single.key(Key::Enter);
    let text = screen(120, 24, &mut single).join("\n");
    assert!(text.contains("● 1. a"), "那个选择被显示成已选中：{text}");
    walk_into_the_input(&mut single, 2);
    single.key(Key::Char('x'));
    let text = screen(120, 24, &mut single).join("\n");
    assert!(
        text.contains("○ 1. a"),
        "打字清掉了单选的那个选择，因为自定义文本覆盖它：{text}"
    );
    assert!(text.contains("自定义：x"), "{text}");
    single.key(Key::Enter);
    assert_eq!(
        answer(&mut single_rx).expect("作答了").answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: Vec::new(),
            custom: Some("x".to_owned()),
        }]
    );

    // 多选：那个选择留着，自定义文本与它一起来。
    let mut multi = state();
    let mut multi_rx = ask(
        &mut multi,
        vec![question("one", "Pick?", &["a", "b"], true)],
    );
    multi.key(Key::Enter);
    walk_into_the_input(&mut multi, 2);
    multi.key(Key::Char('x'));
    let text = screen(120, 24, &mut multi).join("\n");
    assert!(
        text.contains("[x] 1. a"),
        "打字没动多选的那个选择，因为自定义文本补充它：{text}"
    );
    multi.key(Key::Enter);
    assert_eq!(
        answer(&mut multi_rx).expect("作答了").answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: vec!["a".to_owned()],
            custom: Some("x".to_owned()),
        }]
    );
}

#[test]
fn enter_keeps_typed_text_instead_of_re_confirming_an_option() {
    // 在一道已经打了字的题上按 `Enter` 只是前进；它不确认那个
    // 高亮，否则打进去的自由文本会被某个选项悄悄替换掉
    // （spec §7）。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a", "b"], false),
            question("two", "Second?", &["c"], false),
        ],
    );
    walk_into_the_input(&mut state, 2);
    for ch in "typed".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("Second?"), "Enter 走到下一题：\n{text}");

    state.key(Key::Enter);
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("作答了").answers,
        vec![
            UserAnswer {
                id: "one".to_owned(),
                selected: Vec::new(),
                custom: Some("typed".to_owned()),
            },
            UserAnswer {
                id: "two".to_owned(),
                selected: vec!["c".to_owned()],
                custom: None,
            },
        ]
    );
}

#[test]
fn the_option_window_scrolls_so_the_highlighted_option_stays_visible() {
    // 底部那块是有上限的（布局里的 `MAX_INPUT_ROWS`），所以一道
    // 选项装不下的题必须滚动它的选项窗口：题头与
    // 题目原地不动，而高亮那个选项永远在屏幕上
    // （spec §7）。
    let labels: Vec<String> = (1..=20).map(|n| format!("opt-{n:02}")).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut state = state();
    let mut rx = ask(&mut state, vec![question("many", "Which?", &refs, false)]);

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("1. opt-01"), "窗口从顶上开始：\n{text}");

    for _ in 0..15 {
        state.key(Key::Down);
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("16. opt-16"),
        "高亮那个选项被滚进了视野：\n{text}"
    );
    assert!(text.contains("Which?"), "题目钉在窗口上方不动：\n{text}");

    // 第十个选项往后也够得到：高亮不是靠数字键。
    state.key(Key::Enter);
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("作答了").answers,
        vec![UserAnswer {
            id: "many".to_owned(),
            selected: vec!["opt-16".to_owned()],
            custom: None,
        }]
    );
}

#[test]
fn digits_are_free_text_not_selection_keys() {
    // 定下来的键盘没有数字选择，所以数字是普通的字符，
    // 就算在一道给了选项的题上也一样 —— 这也意味着在
    // 没有选项的题上，没有任何数字会被吞掉（spec §7）。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("q", "How should it be named?", &["a", "b"], false)],
    );
    walk_into_the_input(&mut state, 2);
    for ch in "v2".chars() {
        state.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("自定义：v2"),
        "那个数字落进自由文本那一栏：\n{text}"
    );
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("作答了").answers,
        vec![UserAnswer {
            id: "q".to_owned(),
            selected: Vec::new(),
            custom: Some("v2".to_owned()),
        }]
    );
}

#[test]
fn the_recommended_marker_is_display_only() {
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question(
            "one",
            "Pick?",
            &["serde (Recommended)", "manual"],
            false,
        )],
    );

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(
        text.contains("推荐"),
        "那个标记被显示成一个展示用的徽记：{text}"
    );
    assert!(
        !text.contains("(Recommended)"),
        "这个选项读出来不是那个原始标记：{text}"
    );

    // 答案保留原串，标记也在内。
    state.key(Key::Enter);
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("作答了").answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: vec!["serde (Recommended)".to_owned()],
            custom: None,
        }]
    );
}

#[test]
fn answering_hands_the_bottom_back_to_the_resident_input() {
    let mut state = state();
    let mut rx = ask(&mut state, vec![question("one", "Pick?", &["a"], false)]);
    state.key(Key::Enter);
    state.key(Key::Enter);
    assert!(answer(&mut rx).is_some());

    let rows = screen(120, 24, &mut state);
    let text = rows.join("\n");
    assert!(!text.contains("Pick?"), "接管退场了：\n{text}");
    assert!(
        text.contains("esc 取消"),
        "常驻输入区那几句提示回来了：\n{text}"
    );
}

#[test]
fn a_question_with_no_options_is_answered_with_free_text() {
    // 线上契约允许没有选项的题：那时的答案就是用户
    // 打进去的文本，而空的不算一个答案（spec §7）。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("q", "How should it be named?", &[], false)],
    );

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("回答："), "显示出一行自由文本：{text}");

    state.key(Key::Enter);
    assert!(answer(&mut rx).is_none(), "一道空的自由文本题不算作过答");
    // 数字是普通文本：这个键盘没有数字选择，
    // 这里也没有任何选项等着它去编号。
    for ch in "v2-name".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("作答了").answers,
        vec![UserAnswer {
            id: "q".to_owned(),
            selected: Vec::new(),
            custom: Some("v2-name".to_owned()),
        }]
    );
}

#[test]
fn an_empty_questionnaire_is_refused_rather_than_panicking() {
    // 工具在够到端口之前就拒了空列表，所以这里只是
    // 兜底：一份没东西可画的问卷绝不能往空里索引。
    let mut state = state();
    let mut rx = ask(&mut state, Vec::new());
    match rx.try_recv() {
        Ok(Err(reason)) => assert!(reason.contains("至少要有一道题"), "{reason}"),
        other => panic!("期望一次拒绝，实际得到 {other:?}"),
    }
}

#[test]
fn escape_still_cancels_the_run_and_never_answers_the_questionnaire() {
    // `Esc` 保持它的含义（spec §6、§19）：它不放弃这道题，
    // 它取消这次运行，而那次被取消的调用在别处拿到它那唯一一条结果。
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let mut rx = ask(&mut state, vec![question("one", "Pick?", &["a"], false)]);

    state.key(Key::Esc);
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    assert!(answer(&mut rx).is_none(), "Esc 取消这次运行，它不作答");

    // 运行的结束把问卷撤回，读作「没有答案」。
    state.request(ConsoleRequest::RunState { running: false });
    let rows = screen(120, 24, &mut state);
    assert!(!rows.join("\n").contains("Pick?"), "接管跟着这次运行一起走");
    assert!(rx.try_recv().is_err(), "发送端被丢掉了，而不是作了答");
}

#[test]
fn a_space_is_text_once_the_custom_answer_has_focus() {
    // 票 33：一道**有选项**的题上，人已经在自定义栏里打了字（中文答案里夹英文词，
    // `llm wiki` 这种），空格是自由文本的一部分，不是「确认高亮选项」。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a", "b"], false),
            question("two", "Second?", &["c"], false),
        ],
    );
    walk_into_the_input(&mut state, 2);
    for ch in "llm".chars() {
        state.key(Key::Char(ch));
    }
    state.key(Key::Char(' '));
    for ch in "wiki".chars() {
        state.key(Key::Char(ch));
    }
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("First?"), "空格不把人推到下一题：\n{text}");
    assert!(
        text.contains("自定义：llm wiki"),
        "空格落进了自由文本：\n{text}"
    );

    // 一路答完：那个空格随答案交回去，而不是被高亮那个选项替换掉。
    state.key(Key::Enter);
    state.key(Key::Enter);
    state.key(Key::Enter);
    assert_eq!(
        answer(&mut rx).expect("作答了").answers,
        vec![
            UserAnswer {
                id: "one".to_owned(),
                selected: Vec::new(),
                custom: Some("llm wiki".to_owned()),
            },
            UserAnswer {
                id: "two".to_owned(),
                selected: vec!["c".to_owned()],
                custom: None,
            },
        ]
    );
}

#[test]
fn a_space_still_confirms_while_nobody_is_typing() {
    // 票 33 的另一半：焦点不在自定义栏时空格还是那个「确认」。别把它修成
    // 「空格永远只是文本」—— 「不用 `Enter` 也能作答」是既有键位的一部分。
    let mut single = state();
    let mut single_rx = ask(
        &mut single,
        vec![
            question("one", "First?", &["a", "b"], false),
            question("two", "Second?", &["c"], false),
        ],
    );
    single.key(Key::Char(' '));
    let text = screen(120, 24, &mut single).join("\n");
    assert!(text.contains("Second?"), "单选里空格确认并前进：\n{text}");
    single.key(Key::Enter);
    single.key(Key::Enter);
    assert_eq!(
        answer(&mut single_rx).expect("作答了").answers[0].selected,
        vec!["a".to_owned()],
        "它确认的是高亮那个选项"
    );

    // 多选：空格是高亮选项的开关，不翻页。
    let mut multi = state();
    let mut multi_rx = ask(
        &mut multi,
        vec![question("one", "Pick?", &["a", "b"], true)],
    );
    multi.key(Key::Char(' '));
    let text = screen(120, 24, &mut multi).join("\n");
    assert!(text.contains("[x] 1. a"), "多选里空格切换高亮：\n{text}");
    assert!(text.contains("Pick?"), "多选里它不翻页：\n{text}");
    assert!(answer(&mut multi_rx).is_none(), "还没提交，就不该有答案");

    // 没有选项的题：空格本来就是普通字符。
    let mut free = state();
    let mut free_rx = ask(
        &mut free,
        vec![question("q", "How should it be named?", &[], false)],
    );
    for ch in "two words".chars() {
        free.key(Key::Char(ch));
    }
    free.key(Key::Enter);
    assert_eq!(
        answer(&mut free_rx).expect("作答了").answers,
        vec![UserAnswer {
            id: "q".to_owned(),
            selected: Vec::new(),
            custom: Some("two words".to_owned()),
        }]
    );
}

#[test]
fn moving_the_highlight_takes_the_focus_back_to_the_options() {
    // 票 34（票 01 改写）：`↑`/`↓` 从输入区回来时把高亮挪到选项上，于是空格又是「确认」，
    // 而不是继续往自由文本里塞字符。落进输入区的路现在是「越过选项的两端」。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("one", "First?", &["a", "b"], false)],
    );
    state.key(Key::Down); // a → b
    state.key(Key::Down); // b 之后越过边界 → 输入区
    state.key(Key::Down); // 回来，两端环绕，于是绕到 a
    state.key(Key::Char(' '));

    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("作答了").answers;
    assert_eq!(
        answers[0].selected,
        vec!["a".to_owned()],
        "空格确认的是回到选项区之后的高亮"
    );
    assert_eq!(answers[0].custom, None, "空格没被塞进自由文本");
}

#[test]
fn j_k_and_ctrl_n_ctrl_p_walk_the_same_path_in_the_options_zone() {
    // 选项区里 `j`/`k` 与 Emacs 的 `Ctrl-N`/`Ctrl-P` 是同一条分派。
    let mut vim = state();
    let mut vim_rx = ask(
        &mut vim,
        vec![question("one", "Pick?", &["a", "b", "c"], false)],
    );
    vim.key(Key::Char('j'));
    vim.key(Key::Char('j'));
    vim.key(Key::Char(' '));
    vim.key(Key::Enter);
    let answers = answer(&mut vim_rx).expect("作答了").answers;
    assert_eq!(answers[0].selected, vec!["c".to_owned()], "两次 `j` 落在第三项");

    let mut emacs = state();
    let mut emacs_rx = ask(
        &mut emacs,
        vec![question("one", "Pick?", &["a", "b", "c"], false)],
    );
    emacs.key(Key::CtrlN);
    emacs.key(Key::CtrlN);
    emacs.key(Key::CtrlP);
    emacs.key(Key::Char(' '));
    emacs.key(Key::Enter);
    let answers = answer(&mut emacs_rx).expect("作答了").answers;
    assert_eq!(
        answers[0].selected,
        vec!["b".to_owned()],
        "`Ctrl-P` 与 `k` 一样往回走一格"
    );
}

#[test]
fn walking_past_the_last_option_hands_the_keyboard_to_the_text_input() {
    // 越过两端就是进输入区（spec §1），而 `j` 到了那里就是一个字符。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    state.key(Key::Char('j')); // a → b
    state.key(Key::Char('j')); // b 之后 → 输入区
    state.key(Key::Char('j')); // 输入区里 `j` 是文本
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("自定义：j"), "输入区里的 `j` 是文本：\n{text}");

    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("作答了").answers;
    assert_eq!(answers[0].custom, Some("j".to_owned()));
}

#[test]
fn ctrl_n_and_ctrl_p_go_quiet_in_the_text_input() {
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    state.key(Key::Char('j'));
    state.key(Key::Char('j')); // → 输入区
    state.key(Key::CtrlN);
    state.key(Key::CtrlP);
    state.key(Key::Char('x'));
    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("作答了").answers;
    assert_eq!(
        answers[0].custom,
        Some("x".to_owned()),
        "`Ctrl-N`/`Ctrl-P` 在输入区里什么都不做"
    );
}

#[test]
fn printable_characters_and_backspace_are_swallowed_in_the_options_zone() {
    // 键盘只在输入区让给文本（10-01 的那条意向）。
    let mut swallowed = state();
    let mut swallowed_rx = ask(
        &mut swallowed,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    swallowed.key(Key::Char('z'));
    let text = screen(120, 24, &mut swallowed).join("\n");
    assert!(
        !text.contains("自定义：z"),
        "选项区里的字符没进文本：\n{text}"
    );
    swallowed.key(Key::Tab);
    swallowed.key(Key::Enter);
    let answers = answer(&mut swallowed_rx)
        .expect("跳过让整份问卷完成")
        .answers;
    assert_eq!(
        answers[0],
        UserAnswer {
            id: "one".to_owned(),
            selected: Vec::new(),
            custom: None,
        }
    );

    // 回到选项区之后 `Backspace` 也静默：已经打进输入区的文本不会被它删掉。
    let mut backspace = state();
    let mut backspace_rx = ask(
        &mut backspace,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    backspace.key(Key::Char('j'));
    backspace.key(Key::Char('j')); // → 输入区
    backspace.key(Key::Char('x'));
    backspace.key(Key::Char('y'));
    backspace.key(Key::Up); // 回选项区
    backspace.key(Key::Backspace);
    backspace.key(Key::Enter);
    let answers = answer(&mut backspace_rx).expect("作答了").answers;
    assert_eq!(
        answers[0].custom,
        Some("xy".to_owned()),
        "`Backspace` 在选项区里静默"
    );
}

#[test]
fn a_question_without_options_starts_in_the_text_input() {
    let mut state = state();
    let mut rx = ask(&mut state, vec![question("q", "Name?", &[], false)]);
    state.key(Key::Char('z'));
    state.key(Key::Down); // 没有选项可挪，静默
    state.key(Key::Char('k')); // 输入区里是文本
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("回答：zk"), "没有选项的题只有输入区：\n{text}");

    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("作答了").answers;
    assert_eq!(answers[0].custom, Some("zk".to_owned()));
}
