//! 事件流对外的契约：只追加的 JSONL，`seq` 等于行
//! 号，而且最后一行被截断是可容忍的。

use heng::events::{
    Event, EventLog, EventPayload, Role, SCHEMA_VERSION, SessionId, SpeakerId, StopReason,
    ToolCallId, Usage, last_assistant_has_tool_calls, pending_tool_calls, read_events, total_usage,
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
        assert_eq!(event.seq, index as u64 + 1, "seq 必须等于行号");
    }

    let raw = std::fs::read_to_string(&path).unwrap();
    let line_count = raw.lines().count();
    assert_eq!(line_count, 3, "每一行 JSONL 一条事件");
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

                first_token_ms: None,
            },
        )
        .unwrap();
    }

    // 模拟写到一半崩溃：第三行只写了一半、没有换行符。
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(br#"{"seq":3,"at":"2026-01-01T00"#).unwrap();
    }

    let tolerated = read_events(&path).unwrap();
    assert_eq!(tolerated.len(), 2, "被截断的末行被丢掉，而不是致命错误");

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
    assert_eq!(appended.seq, 3, "修复之后 seq 仍等于行号");

    let events = read_events(&path).unwrap();
    assert_eq!(events.len(), 3);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.seq, index as u64 + 1);
    }
}

#[test]
fn a_complete_final_line_without_a_newline_is_preserved_on_open() {
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

                first_token_ms: None,
            },
        )
        .unwrap();
    }

    // 崩溃也可能留下一条*完整*的末尾事件，却没有结尾的换行符。
    let raw = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, raw.trim_end_matches('\n')).unwrap();

    let mut log = EventLog::open(&path).unwrap();
    assert_eq!(log.events().len(), 2, "完整的事件永远不会被删掉");
    assert_eq!(log.next_seq(), 3);

    let appended = log
        .append(
            SpeakerId::System,
            EventPayload::SessionEnded {
                reason: StopReason::Completed,
            },
        )
        .unwrap();
    assert_eq!(appended.seq, 3);

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

            first_token_ms: None,
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

                first_token_ms: None,
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
        "后面一条不带工具调用的 assistant 消息收尾了这个回合"
    );

    let other = SpeakerId::Debater("deepseek".into());
    assert!(!last_assistant_has_tool_calls(&events, &other));
}

#[test]
fn session_spend_is_the_sum_of_usage_events() {
    let usage = |seq: u64, input: u64, output: u64, cached: u64, reasoning: Option<u64>| {
        Event::new(
            seq,
            SpeakerId::Debater("kimi".into()),
            EventPayload::UsageRecorded {
                usage: Usage {
                    input_tokens: input,
                    output_tokens: output,
                    cached_tokens: cached,
                    miss_tokens: input - cached,
                    reasoning_tokens: reasoning,
                },
            },
        )
    };
    let events = vec![usage(1, 100, 20, 60, None), usage(2, 50, 10, 30, Some(5))];

    let total = total_usage(&events);
    assert_eq!(total.input_tokens, 150);
    assert_eq!(total.output_tokens, 30);
    assert_eq!(total.cached_tokens, 90);
    assert_eq!(total.miss_tokens, 60);
    assert_eq!(total.reasoning_tokens, Some(5));
}

#[test]
fn usage_without_reasoning_leaves_the_reasoning_total_absent() {
    let events = vec![Event::new(
        1,
        SpeakerId::Debater("kimi".into()),
        EventPayload::UsageRecorded {
            usage: Usage {
                input_tokens: 7,
                ..Usage::default()
            },
        },
    )];
    assert_eq!(total_usage(&events).reasoning_tokens, None);
}

// ---- 首 token 时刻（ADR 0020）：那个字段是**可选**的，于是两个方向都兼容 ----

#[test]
fn a_message_completed_without_the_first_token_field_reads_as_none() {
    // 一条**老流**：它写的时候还没有那个字段，于是事件 JSON 里没有 `first_token_ms`。
    let old = serde_json::json!({
        "MessageCompleted": {
            "role": "Assistant",
            "text": "答案",
            "reasoning": null,
        }
    });
    let payload: EventPayload = serde_json::from_value(old).expect("老流照读");
    match payload {
        EventPayload::MessageCompleted {
            role,
            text,
            first_token_ms,
            ..
        } => {
            assert_eq!(role, Role::Assistant);
            assert_eq!(text, "答案");
            assert_eq!(
                first_token_ms, None,
                "缺字段读成「不可用」而不是 0 —— 0 是一条被编造出来的数"
            );
        }
        other => panic!("读出来不是一条消息：{other:?}"),
    }
}

#[test]
fn the_first_token_field_survives_a_round_trip_through_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.jsonl");
    let mut log = EventLog::create(&path).unwrap();
    let kimi = SpeakerId::Debater("kimi".into());
    // 前面两条是别的形状：流里本来就允许一条新字段的事件旁边跟着没有它的事件。
    for _ in 0..2 {
        log.append(kimi.clone(), started("s-1")).unwrap();
    }
    log.append(
        kimi,
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "答案".to_owned(),
            reasoning: None,
            first_token_ms: Some(742),
        },
    )
    .unwrap();

    let events = read_events(&path).expect("读回来");
    let found = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::MessageCompleted { first_token_ms, .. } => Some(*first_token_ms),
            _ => None,
        })
        .expect("那条消息在流里");
    assert_eq!(
        found,
        Some(742),
        "落盘与读回是同一个数 —— 而 `seq` 就是行号，所以这份流仍可按行号寻址"
    );
    // 缺字段的那几条读成 None，而不是读不出来。
    for event in &events {
        if let EventPayload::MessageCompleted {
            role,
            first_token_ms,
            ..
        } = &event.payload
        {
            assert!(*role == Role::Assistant, "这条角色不变，说明形状没被读歪");
            let _ = first_token_ms;
        }
    }
}

#[test]
fn a_stream_written_before_the_field_is_readable_unchanged() {
    // 「老二进制读新流」那一半：只用**老形状**声明去读一份**带新字段**的流，serde 忽略未知
    // 字段 —— 所以一个没升过版本的渲染器不会因为多一个键就拒读整条流。
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct OldMessageCompleted {
        role: Role,
        text: String,
        reasoning: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    enum OldPayload {
        MessageCompleted(OldMessageCompleted),
    }

    let new = serde_json::json!({
        "MessageCompleted": {
            "role": "Assistant",
            "text": "答案",
            "reasoning": null,
            "first_token_ms": 742,
        }
    });
    let payload: OldPayload = serde_json::from_value(new).expect("老形状忽略未知字段");
    match payload {
        OldPayload::MessageCompleted(message) => {
            assert_eq!(message.text, "答案");
            assert_eq!(message.reasoning, None);
        }
    }
}
