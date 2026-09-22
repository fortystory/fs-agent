//! Throwaway cost benchmark for ticket 02.
//!
//! Two jobs, both measured against the *current* product code (no forks):
//!
//! * `replay <log.jsonl|--synth N> [batch]` — parse a log into `Event`s, count the
//!   source lines the real `Transcript` + `render_block` produce, then replay the
//!   whole log through the real `TuiState::apply` in batches of `batch` (default
//!   512), drawing one frame per batch exactly as the `select!` loop does. Also
//!   times an apply-only run so the fixed frame cost can be told apart.
//! * `evict <pre> <post> [width]` — push `pre` source lines, then time `post`
//!   more while the pane is at its 20 000-line cap, and a same-sized run below
//!   the cap for comparison.
//!
//! The synthetic generator emits a repeating six-event turn shape so the total
//! source lines cross the 20 000 cap, which is where `evict` gets expensive.

use std::time::Instant;

use fs_agent::events::{
    Event, EventPayload, ParticipantId, Role, SpeakerId, StopReason, ToolCallId, Usage,
};
use fs_agent::render::pane::{Pane, CAP};
use fs_agent::render::transcript::Transcript;
use fs_agent::render::{draw_frame, render_block, RenderEvent, SessionFacts, TuiState};
use ratatui::backend::TestBackend;
use ratatui::text::Line;
use ratatui::Terminal;

/// The window size every timed run draws into (well above the TUI's minimum).
const FRAME_WIDTH: u16 = 120;
const FRAME_HEIGHT: u16 = 40;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("replay") => {
            let events = load(&args[1]);
            let batch: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(512);
            replay(&events, batch);
        }
        Some("evict") => {
            let pre: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(CAP);
            let post: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20_000);
            evict(pre, post);
        }
        Some("synth") => {
            let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50_000);
            let out = args.get(2).cloned().unwrap_or_else(|| "/tmp/synth.jsonl".to_owned());
            let events = synth(n);
            let mut body = String::new();
            for event in &events {
                body.push_str(&serde_json::to_string(event).expect("serialize"));
                body.push('\n');
            }
            std::fs::write(&out, body).expect("write synth log");
            eprintln!("wrote {} events to {out}", events.len());
        }
        _ => {
            eprintln!("usage: replay <log.jsonl> [batch] | evict <pre> <post> | synth <n> <out>");
            std::process::exit(2);
        }
    }
}

/// `--synth N` generates in memory; anything else is a JSONL path.
fn load(arg: &str) -> Vec<Event> {
    if let Some(n) = arg.strip_prefix("--synth=") {
        return synth(n.parse().expect("event count"));
    }
    let body = std::fs::read_to_string(arg).expect("read log");
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Event>(line).expect("parse event"))
        .collect()
}

fn facts() -> SessionFacts {
    SessionFacts {
        session_id: "bench".to_owned(),
        cwd: "/bench".to_owned(),
        model: "bench-model".to_owned(),
        context_window: 200_000,
        budget_limit: Some(1_000_000),
    }
}

fn replay(events: &[Event], batch: usize) {
    println!("events={}", events.len());

    // 1) Source lines: the same `Transcript::push` + `render_block` path
    //    `TuiState::apply` uses, counted without a terminal.
    let mut transcript = Transcript::new();
    let mut source_lines = 0usize;
    let mut blocks = 0usize;
    for event in events {
        for block in transcript.push(RenderEvent::Logged(event.clone())) {
            blocks += 1;
            source_lines += render_block(&block).len();
        }
    }
    for block in transcript.flush() {
        blocks += 1;
        source_lines += render_block(&block).len();
    }
    println!("blocks={blocks} source_lines={source_lines} caps_flushed={}",
        source_lines / CAP);

    // 2) Apply-only: the real `TuiState::apply` for every event, no frames.
    let mut state = TuiState::new(facts());
    let start = Instant::now();
    for event in events {
        state.apply(RenderEvent::Logged(event.clone()));
    }
    let apply_only = start.elapsed();

    // 3) Apply + one drawn frame per batch: what the `select!` loop actually does.
    let mut state = TuiState::new(facts());
    let mut terminal = Terminal::new(TestBackend::new(FRAME_WIDTH, FRAME_HEIGHT)).expect("terminal");
    let start = Instant::now();
    let mut frames = 0usize;
    for chunk in events.chunks(batch) {
        for event in chunk {
            state.apply(RenderEvent::Logged(event.clone()));
        }
        terminal
            .draw(|frame| draw_frame(frame, &mut state))
            .expect("frame");
        state.mark_clean();
        frames += 1;
    }
    let apply_draw = start.elapsed();

    println!(
        "batch={batch} frames={frames} apply_only_ms={:.3} apply_only_us_per_event={:.3} \
         apply_draw_ms={:.3} apply_draw_us_per_event={:.3}",
        apply_only.as_secs_f64() * 1e3,
        apply_only.as_secs_f64() * 1e6 / events.len() as f64,
        apply_draw.as_secs_f64() * 1e3,
        apply_draw.as_secs_f64() * 1e6 / events.len() as f64,
    );
}

/// Time `post` `Pane::push` calls below the cap and again at the cap.
fn evict(pre: usize, post: usize) {
    let line = || Line::from("a source line of about sixty columns of ordinary text xxxxxxxx");
    let mut pane = Pane::new();
    // A real frame sets the width; without it `wrap_pending` is a no-op and the
    // eviction path takes its cheap branch. 120 is the benchmark frame's width.
    pane.view(FRAME_WIDTH, FRAME_HEIGHT, "");

    for _ in 0..pre {
        pane.push(line());
    }
    let start = Instant::now();
    for _ in 0..post {
        pane.push(line());
    }
    let below = start.elapsed();

    let mut pane = Pane::new();
    pane.view(FRAME_WIDTH, FRAME_HEIGHT, "");
    for _ in 0..CAP {
        pane.push(line());
    }
    let start = Instant::now();
    for _ in 0..post {
        pane.push(line());
    }
    let at_cap = start.elapsed();

    println!(
        "pre={pre} post={post} cap={CAP} below_ms={:.3} below_us_per_push={:.3} \
         at_cap_ms={:.3} at_cap_us_per_push={:.3}",
        below.as_secs_f64() * 1e3,
        below.as_secs_f64() * 1e6 / post as f64,
        at_cap.as_secs_f64() * 1e3,
        at_cap.as_secs_f64() * 1e6 / post as f64,
    );
}

/// A repeating six-event turn: TurnStarted, an assistant MessageCompleted of
/// three lines, UsageRecorded, a tool call plus its result, TurnEnded. Six events
/// yield eight source lines, so `n` events cross the 20 000-line cap.
fn synth(n: usize) -> Vec<Event> {
    let speaker = SpeakerId::Debater(ParticipantId::new("bench"));
    let mut events = Vec::with_capacity(n);
    let mut seq = 1u64;
    let mut push = |payload: EventPayload, events: &mut Vec<Event>| {
        events.push(Event::new(seq, speaker.clone(), payload));
        seq += 1;
    };
    push(
        EventPayload::SessionStarted {
            session_id: fs_agent::events::SessionId::new("bench"),
            cwd: "/bench".to_owned(),
            schema_version: fs_agent::events::SCHEMA_VERSION,
        },
        &mut events,
    );
    let mut iteration = 0u32;
    while events.len() < n {
        push(
            EventPayload::TurnStarted {
                agent: speaker.clone(),
                iteration,
            },
            &mut events,
        );
        push(
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: format!("iteration {iteration} line one\nline two\nline three"),
                reasoning: None,
            },
            &mut events,
        );
        push(
            EventPayload::UsageRecorded {
                usage: Usage {
                    input_tokens: 1_000,
                    output_tokens: 200,
                    cached_tokens: 100,
                    miss_tokens: 900,
                    reasoning_tokens: Some(50),
                },
            },
            &mut events,
        );
        let id = ToolCallId::new(format!("call-{iteration}"));
        push(
            EventPayload::ToolCallStarted {
                tool_call_id: id.clone(),
                tool_name: "read_file".to_owned(),
                args: serde_json::json!({ "path": "src/lib.rs" }),
            },
            &mut events,
        );
        push(
            EventPayload::ToolCallCompleted {
                tool_call_id: id,
                ok: true,
                output: Some("one line of tool output".to_owned()),
                error: None,
                duration_ms: 3,
            },
            &mut events,
        );
        push(EventPayload::TurnEnded { reason: StopReason::Completed }, &mut events);
        iteration += 1;
    }
    events.truncate(n);
    events
}
