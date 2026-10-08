//! 问卷的答案回显：一次 `ask_user_question` 的结果，在转录里长成一条**用户发言**
//! （`.scratch/ui-trim/spec.md`）。
//!
//! 两个接缝各测一半：`Transcript` 说「哪些事件变成一个块」—— 两个前端共用它，`--continue`
//! 的重放也走它 —— 而 `draw_frame` 说它在屏幕上长什么样。plain 那一半住在
//! `render_plain.rs`：同一个块，画的是一行 `[用户] …`。

use heng::config::FileViewerSettings;
use heng::render::{Block, RenderEvent, SessionFacts, Transcript, TuiState, draw_frame};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellWidth};
use serde_json::json;

fn facts() -> SessionFacts {
    SessionFacts {
        switchable: true,
        session_id: "01J8ZQ4K7M".to_owned(),
        session_dir: "~/code/fortystory/heng".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        context_window: 200_000,
        mode: heng::permissions::Mode::Ask,
        budget_limit: Some(100_000),
        number_style: heng::render::wording::NumberStyle::Cn,
        file_viewer: FileViewerSettings::default(),
        speaker_order: Vec::new(),
    }
}

fn state() -> TuiState {
    TuiState::new(facts(), std::path::PathBuf::from("/x/heng"), None)
}

/// 一次 `ask_user_question` 的调用：两道题，第一道带 `header`，第二道只有 `question`。
fn ask_args() -> serde_json::Value {
    json!({
        "questions": [
            {
                "id": "pick",
                "header": "用哪个方案",
                "question": "这两条路走哪一条？",
                "options": [{"label": "A"}, {"label": "B"}]
            },
            {"id": "note", "question": "还有别的要说吗？", "options": []}
        ]
    })
}

/// 那两道题的一份答案：第一道两个标签都选了，第二道是自定义文本。
fn answered_body() -> String {
    json!({
        "answers": [
            {"id": "pick", "selected": ["A", "B"]},
            {"id": "note", "selected": [], "custom": "没有"}
        ]
    })
    .to_string()
}

/// 一次工具调用开始的日志事件。
fn call(seq: u64, id: &str, tool: &str, args: serde_json::Value) -> RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId, ToolCallId};
    RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: tool.to_owned(),
            args,
        },
    ))
}

/// 一次工具调用完成的日志事件。
fn result(seq: u64, id: &str, ok: bool, output: Option<&str>) -> RenderEvent {
    use heng::events::{Event, EventPayload, SpeakerId, ToolCallId};
    RenderEvent::Logged(Event::new(
        seq,
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok,
            output: output.map(str::to_owned),
            error: None,
            duration_ms: 3,
        },
    ))
}

/// 一个块是不是那次答卷。
fn answer_of(blocks: &[Block]) -> Option<&str> {
    blocks.iter().find_map(|block| match block {
        Block::Answer { text } => Some(text.as_str()),
        _ => None,
    })
}

#[test]
fn an_answer_becomes_a_user_turn_in_the_transcript() {
    // 一道题一行，`题面：答案`：`header` 优先，没有就用 `question`（结果那边只有题的 `id`）。
    let mut transcript = Transcript::new();
    let mut blocks = transcript.push(call(1, "call-1", "ask_user_question", ask_args()));
    blocks.extend(transcript.push(result(2, "call-1", true, Some(&answered_body()))));

    assert!(
        matches!(blocks.first(), Some(Block::Tool(_))),
        "工具行照旧画出来：{blocks:?}"
    );
    assert_eq!(
        answer_of(&blocks),
        Some("用哪个方案：A、B\n还有别的要说吗？：没有"),
        "答案跟在它后面，读作他说的那句话"
    );
}

#[test]
fn a_skipped_question_says_so_instead_of_leaving_a_blank() {
    // 显式跳过是一次刻意的「不答」，与一个从没走到的问题不同（spec §7）—— 那一行读作
    // 「已跳过」，而不是一个空的答案。
    let mut transcript = Transcript::new();
    transcript.push(call(1, "call-1", "ask_user_question", ask_args()));
    let body = json!({"answers": [{"id": "note", "selected": []}]}).to_string();
    let blocks = transcript.push(result(2, "call-1", true, Some(&body)));

    assert_eq!(answer_of(&blocks), Some("还有别的要说吗？：已跳过"));
}

#[test]
fn the_answer_rows_are_dropped_for_every_other_tool() {
    // 判据只有一条：**名字**。别的工具的输出不管长什么样都不给这一块 —— 一段恰好是
    // `{"answers": […]}` 的正文不该在转录里冒充用户说话。
    let mut transcript = Transcript::new();
    transcript.push(call(1, "call-1", "bash", json!({"command": "ls"})));
    let blocks = transcript.push(result(
        2,
        "call-1",
        true,
        Some(&json!({"answers": [{"id": "x", "selected": ["y"]}]}).to_string()),
    ));

    assert_eq!(answer_of(&blocks), None);
}

#[test]
fn an_unreadable_result_keeps_only_the_tool_row() {
    // 结果被裁剪过、或者那次调用失败了：读不出来就什么都不给 —— 那正是今天的样子，
    // 而不是半行答案。
    for output in [None, Some("这不是 JSON")] {
        let mut transcript = Transcript::new();
        transcript.push(call(1, "call-1", "ask_user_question", ask_args()));
        let blocks = transcript.push(result(2, "call-1", false, output));
        assert_eq!(answer_of(&blocks), None, "output={output:?}");
    }
}

/// 一帧的缓冲区。
fn frame_of(width: u16, height: u16, state: &mut TuiState) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            draw_frame(frame, state);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

/// 缓冲的一整行，按终端读它的方式读 —— 一个宽字素前进两列。
fn row_of(buffer: &Buffer, y: u16, width: u16) -> String {
    let mut row = String::new();
    let mut x = 0;
    while x < width {
        let symbol = buffer[(x, y)].symbol();
        row.push_str(symbol);
        x += symbol.cell_width().max(1);
    }
    row
}

/// 一帧画出来的那些行。
fn screen(width: u16, height: u16, state: &mut TuiState) -> Vec<String> {
    let buffer = frame_of(width, height, state);
    (0..height).map(|y| row_of(&buffer, y, width)).collect()
}

#[test]
fn the_screen_shows_the_answer_as_the_user_speaking() {
    // 人看得见的那一半：答完之后，屏幕上读得出自己选了什么（`.scratch/ui-trim/spec.md`）。
    let mut state = state();
    state.apply(call(1, "call-1", "ask_user_question", ask_args()));
    state.apply(result(2, "call-1", true, Some(&answered_body())));

    let text = screen(120, 24, &mut state).join("\n");
    assert!(text.contains("用哪个方案：A、B"), "{text}");
    assert!(text.contains("还有别的要说吗？：没有"), "{text}");
}

#[test]
fn the_answer_is_a_bubble_against_the_right_edge_like_any_user_turn() {
    // 它是**用户发言**那一档：气泡（靠右、底色），不是一条过程行
    // （`.scratch/trace-tab/spec.md` §2 的补记）。
    use heng::render::palette;
    use heng::render::width::text_columns;

    let mut state = state();
    state.apply(call(1, "call-1", "ask_user_question", ask_args()));
    state.apply(result(2, "call-1", true, Some(&answered_body())));

    let buffer = frame_of(120, 24, &mut state);
    let (row, line) = (0..24)
        .map(|y| (y, row_of(&buffer, y, 120)))
        .find(|(_, line)| line.contains("用哪个方案"))
        .expect("那一行在屏幕上");
    let at = line.find("用哪个方案").expect("刚刚找到过");
    let mut left = text_columns(&line[..at]) as u16;
    while left > 0 && buffer[(left - 1, row)].bg == palette::BUBBLE {
        left -= 1;
    }
    let main = {
        use heng::render::layout::plan;
        use ratatui::layout::Rect;
        plan(Rect::new(0, 0, 120, 24), 1, true).main
    };
    assert!(
        left >= main.x + main.width / 3,
        "气泡贴着主列右缘，前面是一大片留白：第 {left} 列才起（主列从第 {} 列起）",
        main.x
    );
    assert_eq!(buffer[(left, row)].bg, palette::BUBBLE, "底色是那块气泡底");
}
