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
fn the_arrows_page_between_questions_and_the_footer_says_where_we_are() {
    // 确认之后**不**自动翻页（spec §4）：翻页归 `→` 与页脚那三个按钮。
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

    // 空格确认高亮那个选项之后仍停在这一题 —— 并存的意义之一就是还能补一句话（回车在过去
    // 承担这件事，§11 之后它改成「往前走」）。
    state.key(Key::Char(' '));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("First?"), "确认不翻页：{text}");
    assert!(text.contains("1 / 3"), "{text}");

    // 翻页是 `→` 的事。
    state.key(Key::Right);
    let text = screen(120, 24, &mut state).join("\n");
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

    // 空格确认第一个答案，而且**不翻页、不提交**（spec §4 那半条没变）。
    state.key(Key::Char(' '));
    assert!(answer(&mut rx).is_none(), "还有题没处理，提交被拒");

    // `→` 往前走；第二题真作答之后仍有一题没着落，还是不能提交。
    state.key(Key::Right);
    state.key(Key::Char(' '));
    assert!(answer(&mut rx).is_none(), "还有一题没处理，提交被拒");

    // 走到末题：`→` 只往前走，而末题上它**不提交**。
    state.key(Key::Right);
    assert!(answer(&mut rx).is_none(), "末题上 `→` 不提交");

    // 回车把还没作答的第三题记成跳过，于是每题都有着落，整份交出去。
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
fn answering_undoes_a_skip_so_the_text_travels_back() {
    // §11 把「跳过压过一切」那条反过来了：**作答即撤销跳过**（只清不回滚），否则
    // `answers()` 里「跳过无条件优先」会把回头改过的答案吞掉。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );

    // 先明确跳过：`Tab` 还是那个「跳过这个，继续」。
    state.key(Key::Tab);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(
        text.contains("已跳过"),
        "页脚说这一题交回去的是什么：{text}"
    );

    // 再作答：跳过被撤销，打进去的文本跟着回去。
    walk_into_the_input(&mut state, 2);
    state.key(Key::Char('x'));
    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("作答了").answers;
    assert_eq!(
        answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: Vec::new(),
            custom: Some("x".to_owned()),
        }]
    );
}

#[test]
fn typing_and_a_single_select_choice_travel_back_together() {
    // 单选与多选一个形状：自定义文本与所选**并存**交回（spec §3）。
    let mut single = state();
    let mut single_rx = ask(
        &mut single,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    single.key(Key::Char(' '));
    let text = screen(120, 24, &mut single).join("\n");
    assert!(text.contains("● 1. a"), "那个选择被显示成已选中：{text}");
    walk_into_the_input(&mut single, 2);
    single.key(Key::Char('x'));
    let text = screen(120, 24, &mut single).join("\n");
    assert!(
        text.contains("● 1. a"),
        "打字没动单选的那个选择，因为文本与它并存：{text}"
    );
    assert!(text.contains("自定义：x"), "{text}");
    single.key(Key::Enter);
    assert_eq!(
        answer(&mut single_rx).expect("作答了").answers,
        vec![UserAnswer {
            id: "one".to_owned(),
            selected: vec!["a".to_owned()],
            custom: Some("x".to_owned()),
        }]
    );

    // 多选：那个选择留着，自定义文本与它一起来。
    let mut multi = state();
    let mut multi_rx = ask(
        &mut multi,
        vec![question("one", "Pick?", &["a", "b"], true)],
    );
    multi.key(Key::Char(' '));
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
fn confirming_a_selected_option_takes_it_back() {
    // 在已选项上再确认一次就是取消它（spec §3）—— 单选与多选在按键语义上是同一条。
    let mut state = state();
    let _rx = ask(
        &mut state,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    state.key(Key::Char(' '));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("● 1. a"), "第一次确认选中它：{text}");

    state.key(Key::Char(' '));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("○ 1. a"), "再确认一次就取消：{text}");
}

#[test]
fn confirming_another_option_replaces_the_single_select_one() {
    // 单选仍是单选：确认另一个选项是**替换**，不是叠加。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    state.key(Key::Char(' '));
    state.key(Key::Char('j'));
    state.key(Key::Char(' '));
    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("作答了").answers;
    assert_eq!(
        answers[0].selected,
        vec!["b".to_owned()],
        "只有后确认的那一个留下"
    );
}

#[test]
fn enter_advances_in_both_zones_and_never_changes_a_selection() {
    // §11 推翻了 §4 的「选项区里 `Enter` 与空格完全一致」：回车现在是「处置这一题、往前走」，
    // **两个区域同一条规则**，而空格仍是唯一的选中键。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a", "b"], false),
            question("two", "Second?", &["c"], false),
        ],
    );
    state.key(Key::Enter);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("Second?"), "选项区里回车往前走：{text}");
    assert!(
        !text.contains("● 1. a"),
        "它没有确认高亮那一项 —— 那正是被推翻的那半条：{text}"
    );

    // 翻回来：这一题被记成跳过了，页脚说出来。
    state.key(Key::Left);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("First?"), "{text}");
    assert!(text.contains("已跳过"), "页脚说它交回去的是什么：{text}");

    // 输入区里是同一条规则：末题上回车把没作答的它记成跳过，然后整份交出去。
    state.key(Key::Right);
    walk_into_the_input(&mut state, 1);
    state.key(Key::Enter);
    assert!(answer(&mut rx).is_some(), "输入区里的回车也处置了这一题");
}

#[test]
fn enter_keeps_typed_text_instead_of_marking_the_question_skipped() {
    // 在一道已经打了字的题上按 `Enter` 只是前进：它不确认那个高亮，也不把这题改成「跳过」
    // —— 打进去的自由文本是用户真答的东西（spec §7、§11）。
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

    // 末题上回车：没作答的那一题记成跳过，整份问卷随这一下交出去。
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
                selected: Vec::new(),
                custom: None,
            },
        ]
    );
}

#[test]
fn moving_forward_marks_a_question_skipped_but_moving_back_never_does() {
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![
            question("one", "First?", &["a"], false),
            question("two", "Second?", &["b"], false),
            question("three", "Third?", &["c"], false),
        ],
    );

    // `←` 只移动，不记任何东西；第一题上它更是无处可去。
    state.key(Key::Left);
    assert!(
        screen(120, 24, &mut state).join("\n").contains("First?"),
        "还在第一题"
    );

    // `→` 给**离开时**那道没作答的题记跳过，页脚会说出来。
    state.key(Key::Right);
    state.key(Key::Left);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("First?"), "{text}");
    assert!(text.contains("已跳过"), "往回翻看得见那一笔：{text}");

    // 回头真作答，再往前走时它**不该**被记成跳过。
    state.key(Key::Right);
    state.key(Key::Char(' '));
    state.key(Key::Right);
    state.key(Key::Left);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(!text.contains("已跳过"), "答过的题不被记成跳过：{text}");

    // 走到末题，然后在末题上按 `→`：什么都不做 —— 不记、不前进、不提交。
    state.key(Key::Right);
    assert!(
        screen(120, 24, &mut state).join("\n").contains("Third?"),
        "到末题了"
    );
    state.key(Key::Right);
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("Third?"), "末题上不前进：{text}");
    assert!(answer(&mut rx).is_none(), "末题上 `→` 不提交");

    // 提交仍旧只走回车。
    state.key(Key::Enter);
    assert!(answer(&mut rx).is_some(), "回车把整份交出去");
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

    // 第十个选项往后也够得到：高亮不是靠数字键。空格确认、回车交出整份（§11）。
    state.key(Key::Char(' '));
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

    // 答案保留原串，标记也在内。空格确认、回车交出整份（§11）。
    state.key(Key::Char(' '));
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
    // 线上契约允许没有选项的题：那时的答案就是用户打进去的文本，而空的不算一个答案
    // （spec §7）。回车在新语义下是「把这一题记成跳过」（§11），所以它交回的是 `selected: []`
    // 且没有 `custom` 的那种空答案 —— 而不是一个空字符串。
    let mut skipped = state();
    let mut skipped_rx = ask(
        &mut skipped,
        vec![question("q", "How should it be named?", &[], false)],
    );
    let text = screen(120, 24, &mut skipped).join("\n");
    assert!(text.contains("回答："), "显示出一行自由文本：{text}");
    skipped.key(Key::Enter);
    assert_eq!(
        answer(&mut skipped_rx)
            .expect("跳过是这一题的着落，于是整份交出去")
            .answers,
        vec![UserAnswer {
            id: "q".to_owned(),
            selected: Vec::new(),
            custom: None,
        }],
        "空答案不是空字符串"
    );

    // 数字是普通文本：这个键盘没有数字选择，
    // 这里也没有任何选项等着它去编号。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("q", "How should it be named?", &[], false)],
    );
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
fn escape_asks_twice_before_it_drops_the_questionnaire() {
    // 问卷里 `Esc` 只管「退出这次询问」（spec §5）：第一下举手，第二下 drop 掉 sender。
    // 它**不**取消这次运行 —— 模型拿到「没作答」，继续跑。
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let mut rx = ask(&mut state, vec![question("one", "Pick?", &["a"], false)]);

    state.key(Key::Esc);
    assert!(state.take_events().is_empty(), "第一下不取消这次运行");
    assert!(answer(&mut rx).is_none(), "第一下也不作答");

    state.key(Key::Esc);
    assert!(state.take_events().is_empty(), "第二下也不是取消运行");
    assert!(rx.try_recv().is_err(), "发送端被丢掉了：工具读作「没作答」");
    assert!(!state.should_quit(), "它也不是退出这次运行");
}

#[test]
fn escape_from_the_text_input_goes_back_to_the_options() {
    // 输入区那一下 `Esc` 同时做两件事：回选项区（文本不清）**并且**举手，所以任何区域
    // 连按两下 `Esc` 都是退出询问（spec §5）。
    let mut state = state();
    let mut rx = ask(
        &mut state,
        vec![question("one", "Pick?", &["a", "b"], false)],
    );
    state.key(Key::Char('j'));
    state.key(Key::Char('j')); // → 输入区
    state.key(Key::Char('x'));

    state.key(Key::Esc); // 回选项区 + 举手
    state.key(Key::Char('j')); // 在选项区里这是移动，不是文本
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("自定义：x"), "已经输入的文本不清：\n{text}");
    assert!(
        !text.contains("自定义：xj"),
        "`Esc` 之后键盘在选项区：\n{text}"
    );

    state.key(Key::Esc); // 第二下：退出这次询问
    assert!(rx.try_recv().is_err(), "第二下 drop 掉发送端");
}

#[test]
fn the_questionnaire_gesture_expires_with_the_window() {
    let mut state = state();
    let mut rx = ask(&mut state, vec![question("one", "Pick?", &["a"], false)]);
    state.key(Key::Esc);
    state.expire_exit_gesture(); // 超时作废：不等真实时间
    state.key(Key::Esc);
    assert!(answer(&mut rx).is_none(), "作废之后那一下只是新的一次起手");
    state.key(Key::Esc);
    assert!(rx.try_recv().is_err(), "再过一下才真的退出询问");
}

#[test]
fn the_two_gestures_are_exclusive() {
    // 举着「退出询问」时按 `Ctrl-C` 不作数：它走 `Ctrl-C` 自己的第一下 —— 取消这次运行、
    // 并举起退出手（spec §5）。
    let mut state = state();
    state.request(ConsoleRequest::RunState { running: true });
    let mut rx = ask(&mut state, vec![question("one", "Pick?", &["a"], false)]);

    state.key(Key::Esc); // 举「退出询问」
    state.key(Key::CtrlC); // 不作数 → 取消 + 举退出手
    assert_eq!(state.take_events(), vec![FrontEndEvent::Cancel]);
    assert!(answer(&mut rx).is_none(), "那一手没被兑现：问卷还在");
    assert!(!state.should_quit());
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
    state.key(Key::Enter); // 第一题已经打了字，回车只往前走
    state.key(Key::Char(' ')); // 第二题用空格真作答
    state.key(Key::Enter); // 每题都有着落，交出去
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
    assert!(text.contains("● 1. a"), "单选里空格确认高亮：\n{text}");
    assert!(
        text.contains("First?"),
        "确认不翻页，翻页是 `→` 的事：\n{text}"
    );
    single.key(Key::Right);
    // 第二题没作答，回车把它记成跳过 —— 于是整份问卷随这一下交出去。
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
    assert_eq!(
        answers[0].selected,
        vec!["c".to_owned()],
        "两次 `j` 落在第三项"
    );

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
    assert!(
        text.contains("自定义：j"),
        "输入区里的 `j` 是文本：\n{text}"
    );

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
fn escape_leaves_a_question_without_options_in_the_text_input() {
    // 没有选项的题只有输入区，所以 `Esc` 不该把它推到一个不存在的选项区
    // （`.scratch/questionnaire-keys/spec.md` §1、§5）。
    let mut state = state();
    let mut rx = ask(&mut state, vec![question("q", "Name?", &[], false)]);
    state.key(Key::Char('x'));
    state.key(Key::Esc); // 举手，同时「回选项区」—— 但这道题没有选项区
    state.key(Key::Char('y'));
    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("回答：xy"), "`Esc` 之后仍然能打字：\n{text}");

    state.key(Key::Esc); // 第二下：退出这次询问
    assert!(rx.try_recv().is_err(), "第二下 drop 掉发送端");
}

#[test]
fn a_wrapped_custom_answer_stays_inside_the_pane() {
    // 输入区自己折行也占行（spec §7）：续行要看得到，页脚也不能被挤掉。
    let long = "y".repeat(120);
    let mut state = state();
    let _rx = ask(&mut state, vec![question("q", "Pick?", &["a", "b"], false)]);
    state.key(Key::Char('j'));
    state.key(Key::Char('j')); // → 输入区
    for ch in long.chars() {
        state.key(Key::Char(ch));
    }

    let text = screen(60, 24, &mut state).join("\n");
    let ys = text.matches('y').count();
    assert!(
        ys >= 100,
        "折行之后输入区看得见更多字符（只看到 {ys} 个）：\n{text}"
    );
    assert!(text.contains("1 / 1"), "页脚没被挤掉：\n{text}");
}

#[test]
fn a_wrapped_option_keeps_all_of_its_lines_in_the_window() {
    // 超长选项折行而不是被截断；高亮落在它上面时它**整块**都在窗口里
    // （`.scratch/questionnaire-keys/spec.md` §7）。
    let long = "x".repeat(200);
    let mut state = state();
    let _rx = ask(
        &mut state,
        vec![question("q", "Which?", &["short", &long], false)],
    );
    state.key(Key::Char('j')); // 高亮落到那个长选项上

    let text = screen(60, 24, &mut state).join("\n");
    let xs = text.matches('x').count();
    assert!(
        xs >= 190,
        "长选项整块都在窗口里（只看到 {xs} 个 x）：\n{text}"
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
    assert!(
        text.contains("回答：zk"),
        "没有选项的题只有输入区：\n{text}"
    );

    state.key(Key::Enter);
    let answers = answer(&mut rx).expect("作答了").answers;
    assert_eq!(answers[0].custom, Some("zk".to_owned()));
}
