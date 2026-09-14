//! The event log's external contract: append-only JSONL, `seq` equals line
//! number, and a torn final line is tolerated.

use fs_agent::events::{
    last_assistant_has_tool_calls, pending_tool_calls, read_events, Event, EventLog, EventPayload,
    Role, SessionId, SpeakerId, StopReason, ToolCallId, SCHEMA_VERSION,
};
use std::io::Write;

fn started(session: &str) -> EventPayload {
    EventPayload::SessionStarted {
        session_id: SessionId::new(session),
        cwd: "/tmp/work".to_owned(),
        schema_version: SCHEMA_VERSION,
    }
}

#[test]
fn seq_is_the_jsonl_line_number() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.jsonl");
    let mut log = EventLog::create(&path).unwrap();

    let kimi = SpeakerId::Debater("kimi".into());
    for index in 1..=3u32 {
        log.append(
            if index == 1 {
                SpeakerId::System
            } else {
                kimi.clone()
            },
            if index == 1 {
                started("s-1")
            } else {
                EventPayload::TurnStarted {
                    agent: kimi.clone(),
                    iteration: index,
                }
            },
        )
        .unwrap();
    }

    let events = read_events(&path).unwrap();
    assert_eq!(events.len(), 3);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(
            event.seq,
            index as u64 + 1,
            "seq must equal the line number"
        );
    }

    let raw = std::fs::read_to_string(&path).unwrap();
    let line_count = raw.lines().count();
    assert_eq!(line_count, 3, "one event per JSONL line");
    assert_eq!(log.next_seq(), 4);
}

#[test]
fn torn_final_line_is_tolerated_and_repaired_on_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.jsonl");

    {
        let mut log = EventLog::create(&path).unwrap();
        log.append(SpeakerId::System, started("s-1")).unwrap();
        log.append(
            SpeakerId::User,
            EventPayload::MessageCompleted {
                role: Role::User,
                text: "hi".to_owned(),
                reasoning: None,
            },
        )
        .unwrap();
    }

    // Simulate a crash mid-write: a partial third line with no newline.
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(br#"{"seq":3,"at":"2026-01-01T00"#).unwrap();
    }

    let tolerated = read_events(&path).unwrap();
    assert_eq!(
        tolerated.len(),
        2,
        "the torn final line is dropped, not fatal"
    );

    let mut log = EventLog::open(&path).unwrap();
    assert_eq!(log.next_seq(), 3);
    let appended = log
        .append(
            SpeakerId::System,
            EventPayload::SessionEnded {
                reason: StopReason::Completed,
            },
        )
        .unwrap();
    assert_eq!(
        appended.seq, 3,
        "seq stays equal to the line number after repair"
    );

    let events = read_events(&path).unwrap();
    assert_eq!(events.len(), 3);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.seq, index as u64 + 1);
    }
}

#[test]
fn corruption_before_the_final_line_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.jsonl");
    let mut log = EventLog::create(&path).unwrap();
    log.append(SpeakerId::System, started("s-1")).unwrap();
    log.append(
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: "hi".to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
    drop(log);

    let raw = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<&str> = raw.lines().collect();
    lines[0] = "not json";
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

    assert!(read_events(&path).is_err());
}

#[test]
fn pending_tool_calls_is_a_query_over_the_stream() {
    let kimi = SpeakerId::Debater("kimi".into());
    let started_call = |seq: u64, id: &str| {
        Event::new(
            seq,
            kimi.clone(),
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new(id),
                tool_name: "read_file".to_owned(),
                args: serde_json::Value::Null,
            },
        )
    };
    let completed_call = |seq: u64, id: &str| {
        Event::new(
            seq,
            kimi.clone(),
            EventPayload::ToolCallCompleted {
                tool_call_id: ToolCallId::new(id),
                ok: true,
                output: Some("contents".to_owned()),
                error: None,
                duration_ms: 1,
            },
        )
    };

    let mut events = vec![started_call(1, "c1"), started_call(2, "c2")];
    assert_eq!(
        pending_tool_calls(&events),
        vec![ToolCallId::new("c1"), ToolCallId::new("c2")]
    );

    events.push(completed_call(3, "c1"));
    assert_eq!(pending_tool_calls(&events), vec![ToolCallId::new("c2")]);

    events.push(completed_call(4, "c2"));
    assert!(pending_tool_calls(&events).is_empty());
}

#[test]
fn continuation_is_decided_by_the_last_assistant_message() {
    let kimi = SpeakerId::Debater("kimi".into());
    let assistant = |seq: u64, text: &str| {
        Event::new(
            seq,
            kimi.clone(),
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: text.to_owned(),
                reasoning: None,
            },
        )
    };
    let tool_call = |seq: u64| {
        Event::new(
            seq,
            kimi.clone(),
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("c1"),
                tool_name: "read_file".to_owned(),
                args: serde_json::Value::Null,
            },
        )
    };

    let mut events = vec![assistant(1, "let me check"), tool_call(2)];
    assert!(last_assistant_has_tool_calls(&events, &kimi));

    events.push(assistant(3, "all done"));
    assert!(
        !last_assistant_has_tool_calls(&events, &kimi),
        "a later assistant message without tool calls ends the turn"
    );

    let other = SpeakerId::Debater("deepseek".into());
    assert!(!last_assistant_has_tool_calls(&events, &other));
}
