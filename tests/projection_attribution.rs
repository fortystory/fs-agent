//! 投影与发言归属，当作流上的纯函数来测
//! （spec §5；Testing Decisions 的「直接测的纯函数」那条接缝）。
//!
//! 这里测的契约是一个 agent 会重放的那些 `messages`：谁变成
//! `assistant`、谁被降级成 `user`、另一个发言者的工具往返里
//! 活下来什么，以及发言者自己那次往返与钩子反馈
//! 如何合并。模型面前缀的确切拼法故意不去断言
//! （spec 的 Testing Decisions）：这些测试钉的是要紧的那些
//! 性质 —— 归属、可见性、合并的边界与顺序。

use heng::config::GenerationParams;
use heng::events::{
    ContextSource, EventLog, EventPayload, Role, RoundMode, SessionId, SpeakerId, StopReason,
    ToolCallId, hook_format,
};
use heng::provider::capability::{ModelCaps, caps_for};
use heng::provider::openai::build_body;
use heng::provider::projection::project;
use heng::provider::{ChatRequest, Message, ToolChoice};

fn deepseek() -> SpeakerId {
    SpeakerId::Debater("deepseek".into())
}

fn kimi() -> SpeakerId {
    SpeakerId::Debater("kimi".into())
}

/// 第三个参与者的视角：合成器，它的身份是 `System`（spec §2）。
fn synthesizer() -> SpeakerId {
    SpeakerId::System
}

fn caps() -> ModelCaps {
    caps_for("deepseek-flash").unwrap()
}

/// 从一个脚本搭出一份日志，并让临时目录与它一起活下来。
fn log(script: impl FnOnce(&mut EventLog)) -> (tempfile::TempDir, EventLog) {
    let dir = tempfile::tempdir().unwrap();
    let mut log = EventLog::create(dir.path().join("log.jsonl")).unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SessionStarted {
            session_id: SessionId::new("s-1"),
            cwd: "/workspace".to_owned(),
            schema_version: heng::events::SCHEMA_VERSION,
        },
    )
    .unwrap();
    script(&mut log);
    (dir, log)
}

fn user_says(log: &mut EventLog, text: &str) {
    log.append(
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: text.to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
}

fn say(log: &mut EventLog, speaker: &SpeakerId, text: &str) {
    say_with_reasoning(log, speaker, text, None);
}

fn say_with_reasoning(
    log: &mut EventLog,
    speaker: &SpeakerId,
    text: &str,
    reasoning: Option<&str>,
) {
    log.append(
        speaker.clone(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: reasoning.map(str::to_owned),
        },
    )
    .unwrap();
}

fn call(log: &mut EventLog, speaker: &SpeakerId, id: &str, tool: &str, args: serde_json::Value) {
    log.append(
        speaker.clone(),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new(id),
            tool_name: tool.to_owned(),
            args,
        },
    )
    .unwrap();
}

fn result(log: &mut EventLog, speaker: &SpeakerId, id: &str, output: &str) {
    log.append(
        speaker.clone(),
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new(id),
            ok: true,
            output: Some(output.to_owned()),
            error: None,
            duration_ms: 1,
        },
    )
    .unwrap();
}

fn user_messages(messages: &[Message]) -> Vec<&Message> {
    messages
        .iter()
        .filter(|message| matches!(message, Message::User { .. }))
        .collect()
}

fn text_of(message: &Message) -> &str {
    match message {
        Message::User { content, .. } | Message::System { content, .. } => content,
        Message::Assistant { content, .. } => content.as_deref().unwrap_or(""),
        Message::Tool { content, .. } => content,
    }
}

// --- 另一个发言者的回合 -----------------------------------------------------

#[test]
fn another_speakers_text_is_kept_but_its_tool_round_trip_shrinks_to_one_summary_line() {
    let (_dir, log) = log(|log| {
        log.append(
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 1,
                mode: RoundMode::Independent,
            },
        )
        .unwrap();
        say_with_reasoning(log, &deepseek(), "I checked the file", Some("SECRET-THINK"));
        call(
            log,
            &deepseek(),
            "call-1",
            "read_file",
            serde_json::json!({ "file_path": "src/lib.rs" }),
        );
        result(log, &deepseek(), "call-1", "SECRET-BODY");
    });

    let messages = project(&log.events(), &kimi(), &caps());

    assert_eq!(messages.len(), 1, "只有那一块合并后的别人：{messages:?}");
    let user = user_messages(&messages)[0];
    let content = text_of(user);
    assert!(content.contains("I checked the file"), "{content}");
    assert!(
        content.contains("read_file") && content.contains("src/lib.rs"),
        "这次工具调用活成一行摘要：{content}"
    );
    assert!(
        content.contains("轮 1") && content.contains("deepseek"),
        "每一段别人的发言都带归属：{content}"
    );
    assert!(!content.contains("SECRET-BODY"), "结果正文不被投影");
    assert!(
        !content.contains("SECRET-THINK"),
        "另一个发言者的推理不被投影"
    );
    assert!(
        !messages.iter().any(|m| matches!(m, Message::Tool { .. })),
        "配对的那条工具结果被丢掉，而不是被改写成一对残缺"
    );
}

// --- 发言者自己的回合 -------------------------------------------------------

#[test]
fn own_reasoning_is_replayed_and_the_tool_round_trip_merges_with_post_hook_feedback() {
    let (_dir, log) = log(|log| {
        say_with_reasoning(log, &kimi(), "reading", Some("my-thoughts"));
        call(
            log,
            &kimi(),
            "call-1",
            "read_file",
            serde_json::json!({ "file_path": "src/lib.rs" }),
        );
        result(log, &kimi(), "call-1", "file body");
        log.append(
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "lint".to_owned(),
                outcome: hook_format::feedback("trailing whitespace"),
            },
        )
        .unwrap();
    });

    let messages = project(&log.events(), &kimi(), &caps());

    assert_eq!(messages.len(), 2, "{messages:?}");
    match &messages[0] {
        Message::Assistant {
            content,
            reasoning_content,
            tool_calls,
            name,
        } => {
            assert_eq!(content.as_deref(), Some("reading"));
            assert_eq!(
                reasoning_content.as_deref(),
                Some("my-thoughts"),
                "模型自己的推理必须被重放"
            );
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call-1");
            assert_eq!(name.as_deref(), Some("kimi"));
        }
        other => panic!("期望一条 assistant 消息，实际得到 {other:?}"),
    }
    match &messages[1] {
        Message::Tool {
            tool_call_id,
            content,
        } => {
            assert_eq!(tool_call_id, "call-1");
            // 正好一条工具消息同时扛着结果与钩子的
            // 反馈：provider 每次 `tool_call` 只允许一条 `tool` 消息。
            assert!(content.starts_with("file body"), "{content}");
            assert!(
                content.contains(hook_format::FEEDBACK_MARKER)
                    && content.contains("trailing whitespace"),
                "{content}"
            );
        }
        other => panic!("期望那条合并后的工具消息，实际得到 {other:?}"),
    }
}

#[test]
fn a_failed_post_hook_loses_its_feedback_but_keeps_the_result() {
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "reading");
        call(log, &kimi(), "call-1", "read_file", serde_json::json!({}));
        result(log, &kimi(), "call-1", "file body");
        log.append(
            kimi(),
            EventPayload::HookExecuted {
                point: hook_format::POINT_POST.to_owned(),
                command: "lint".to_owned(),
                outcome: hook_format::failed("lint exploded"),
            },
        )
        .unwrap();
    });

    let messages = project(&log.events(), &kimi(), &caps());

    let tool = messages
        .iter()
        .find_map(|message| match message {
            Message::Tool { content, .. } => Some(content),
            _ => None,
        })
        .expect("结果仍然有它那一条工具消息");
    assert_eq!(tool, "file body");
    assert!(!tool.contains("lint exploded"));
}

#[test]
fn an_interleaved_other_speaker_never_leaves_a_tool_call_without_its_result() {
    // 一个合法的回合从不交错，但一份手搭出来、或者将来会坏掉的
    // 日志，绝不能产出线上检查会拒的那种没配对的 `tool_call`。
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "working");
        call(log, &kimi(), "call-1", "read_file", serde_json::json!({}));
        say(log, &deepseek(), "interjecting");
        result(log, &kimi(), "call-1", "body");
    });

    let messages = project(&log.events(), &kimi(), &caps());

    let calls: Vec<String> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Assistant { tool_calls, .. } => Some(tool_calls.iter().map(|c| c.id.clone())),
            _ => None,
        })
        .flatten()
        .collect();
    let results: Vec<String> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Tool { tool_call_id, .. } => Some(tool_call_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, vec!["call-1".to_owned()]);
    assert_eq!(
        results,
        vec!["call-1".to_owned()],
        "这个 assistant 组保住它那一条工具消息：{messages:?}"
    );
}

// --- 自己与别人，在同一批事件上 ---------------------------------------------

#[test]
fn the_same_events_project_differently_for_each_speaker_and_each_projection_is_stable() {
    let (_dir, log) = log(|log| {
        log.append(
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 1,
                mode: RoundMode::Independent,
            },
        )
        .unwrap();
        say_with_reasoning(log, &deepseek(), "deepseek's answer", Some("d-think"));
        call(
            log,
            &deepseek(),
            "d-call",
            "read_file",
            serde_json::json!({}),
        );
        result(log, &deepseek(), "d-call", "d-body");
        say_with_reasoning(log, &kimi(), "kimi's answer", Some("k-think"));
    });

    let for_kimi = project(&log.events(), &kimi(), &caps());
    let for_deepseek = project(&log.events(), &deepseek(), &caps());

    assert_ne!(for_kimi, for_deepseek, "同一批事件必须投影成不同的窗口");

    // 投影是一个纯函数：重跑一遍逐字节稳定，
    // 而「能从流 + 规则重算」买到的就是这个。
    assert_eq!(for_kimi, project(&log.events(), &kimi(), &caps()));
    assert_eq!(for_deepseek, project(&log.events(), &deepseek(), &caps()));

    // kimi 看到 deepseek 被压成一块 user，以及它自己那一轮
    // 带着它自己的推理。
    let kimi_text = for_kimi.iter().map(text_of).collect::<Vec<_>>().join("\n");
    assert!(kimi_text.contains("deepseek's answer"), "{kimi_text}");
    assert!(!kimi_text.contains("d-think"), "{kimi_text}");
    let kimi_own = for_kimi
        .iter()
        .find(|message| matches!(message, Message::Assistant { .. }))
        .expect("kimi 自己的消息");
    match kimi_own {
        Message::Assistant {
            reasoning_content, ..
        } => assert_eq!(reasoning_content.as_deref(), Some("k-think")),
        other => panic!("期望 assistant，实际得到 {other:?}"),
    }
    // deepseek 看到自己那次工具往返，kimi 只看到一句摘要。
    assert!(
        for_deepseek
            .iter()
            .any(|message| matches!(message, Message::Tool { .. }))
    );
    assert!(
        !for_kimi
            .iter()
            .any(|message| matches!(message, Message::Tool { .. }))
    );
}

// --- 合并的规矩 -------------------------------------------------------------

#[test]
fn the_pinned_head_never_merges_and_a_round_is_a_hard_boundary() {
    let (_dir, log) = log(|log| {
        user_says(log, "the task");
        log.append(
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 1,
                mode: RoundMode::Independent,
            },
        )
        .unwrap();
        say(log, &kimi(), "round 1 from kimi");
        say(log, &deepseek(), "round 1 from deepseek");
        log.append(
            SpeakerId::System,
            EventPayload::RoundEnded {
                round: 1,
                reason: StopReason::NoDivergence,
            },
        )
        .unwrap();
        log.append(
            SpeakerId::System,
            EventPayload::RoundStarted {
                round: 2,
                mode: RoundMode::Targeted,
            },
        )
        .unwrap();
        say(log, &kimi(), "round 2 from kimi");
    });

    let messages = project(&log.events(), &synthesizer(), &caps());
    let users = user_messages(&messages);

    assert_eq!(users.len(), 3, "{messages:?}");
    assert_eq!(
        text_of(users[0]),
        "the task",
        "被钉住的头部逐字节稳定，什么都不吸收"
    );
    let round_1 = text_of(users[1]);
    assert!(
        round_1.contains("round 1 from kimi") && round_1.contains("round 1 from deepseek"),
        "连续的别人会合并，没有条数门槛：{round_1}"
    );
    assert!(!round_1.contains("round 2"), "{round_1}");

    let round_2 = text_of(users[2]);
    assert!(round_2.contains("round 2 from kimi"), "{round_2}");
    assert!(!round_2.contains("round 1"), "{round_2}");
}

#[test]
fn the_first_user_message_does_not_absorb_a_later_speakers_speech() {
    // 这里没有轮次边界，所以唯一挡住头部随着
    // 另一个发言者的文本长大的就是那个钉子。
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "first");
        say(log, &deepseek(), "second");
    });

    let messages = project(&log.events(), &synthesizer(), &caps());
    let users = user_messages(&messages);

    assert_eq!(users.len(), 2, "{messages:?}");
    assert_eq!(text_of(users[0]), "first");
    assert_eq!(text_of(users[1]), "second");
}

#[test]
fn a_context_injection_is_pinned_and_does_not_merge() {
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "before");
        log.append(
            SpeakerId::User,
            EventPayload::ContextInjected {
                source: ContextSource::PlanMode,
                content: "read PLAN.md before acting".to_owned(),
            },
        )
        .unwrap();
        say(log, &kimi(), "after");
    });

    let messages = project(&log.events(), &synthesizer(), &caps());
    let users = user_messages(&messages);

    assert_eq!(users.len(), 3, "这条注入保持是它自己的消息：{messages:?}");
    assert_eq!(text_of(users[1]), "read PLAN.md before acting");
    assert!(text_of(users[0]).contains("before"));
    assert!(text_of(users[2]).contains("after"));
}

// --- 执行者的可见性 ---------------------------------------------------------

#[test]
fn an_executors_events_stay_out_of_a_debaters_projection_but_not_its_own() {
    let executor = SpeakerId::Executor("e-1".into());
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "kimi's take");
        say_with_reasoning(log, &executor, "executor working", Some("e-think"));
        call(log, &executor, "e-call", "read_file", serde_json::json!({}));
        result(log, &executor, "e-call", "e-body");
    });

    let for_synthesizer = project(&log.events(), &synthesizer(), &caps());
    let synthesizer_text = for_synthesizer
        .iter()
        .map(text_of)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(synthesizer_text.contains("kimi's take"));
    assert!(
        !synthesizer_text.contains("executor working"),
        "{synthesizer_text}"
    );
    assert!(!synthesizer_text.contains("e-body"), "{synthesizer_text}");

    let for_executor = project(&log.events(), &executor, &caps());
    assert!(
        for_executor
            .iter()
            .any(|message| matches!(message, Message::Assistant { .. }))
    );
    assert!(
        for_executor
            .iter()
            .any(|message| matches!(message, Message::Tool { .. }))
    );
}

// --- 名字 -------------------------------------------------------------------

#[test]
fn names_are_sanitized_and_never_carried_on_tool_messages() {
    let odd = SpeakerId::Debater("ki mi/../x\u{4f60}".into());
    let (_dir, log) = log(|log| {
        say(log, &odd, "hello");
        call(log, &odd, "call-1", "read_file", serde_json::json!({}));
        result(log, &odd, "call-1", "body");
    });

    let messages = project(&log.events(), &odd, &caps());
    let own = messages
        .iter()
        .find(|message| matches!(message, Message::Assistant { .. }))
        .expect("发言者自己的那条 assistant 消息");
    let name = match own {
        Message::Assistant { name, .. } => name.as_deref().unwrap(),
        other => panic!("期望 assistant，实际得到 {other:?}"),
    };
    assert!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        "name 必须被打码成 [A-Za-z0-9_-]：{name}"
    );
    assert!(name.len() <= 64, "name 必须保持短：{name}");
}

// --- 历史 -------------------------------------------------------------------

#[test]
fn superseded_ranges_are_excluded_from_the_projection() {
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "old answer");
        say(log, &kimi(), "replacement answer");
        log.append(
            SpeakerId::System,
            EventPayload::HistorySuperseded {
                targets: vec![2],
                reason: heng::events::HistoryReason::Regenerate,
                summary: None,
            },
        )
        .unwrap();
    });

    let messages = project(&log.events(), &synthesizer(), &caps());
    let text = messages.iter().map(text_of).collect::<Vec<_>>().join("\n");
    assert!(!text.contains("old answer"), "{text}");
    assert!(text.contains("replacement answer"), "{text}");
}

#[test]
fn retiring_a_tool_call_leaves_no_empty_assistant_message() {
    // 一个只有工具调用的 assistant 回合，而那次调用被退休了：
    // 这个组里什么都不剩，而线上没有一条既没有 content 也没有
    // 调用的 assistant 消息的形状，所以一条都不发出。
    let (_dir, log) = log(|log| {
        call(
            log,
            &kimi(),
            "call-1",
            "read_file",
            serde_json::json!({"file_path": "a.txt"}),
        );
        result(log, &kimi(), "call-1", "a");
        log.append(
            SpeakerId::User,
            EventPayload::HistorySuperseded {
                targets: vec![2, 3],
                reason: heng::events::HistoryReason::Undo,
                summary: None,
            },
        )
        .unwrap();
        say(log, &kimi(), "carried on");
    });

    let messages = project(&log.events(), &kimi(), &caps());
    let assistants = messages
        .iter()
        .filter(|message| matches!(message, Message::Assistant { .. }))
        .count();
    assert_eq!(assistants, 1, "{messages:?}");
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::Tool { .. })),
        "那条被退休的结果也一并没了：{messages:?}"
    );
}

#[test]
fn retiring_a_tool_call_keeps_the_text_the_assistant_said() {
    // assistant 说了句话、然后调了个工具；撤销那次调用
    // 退休的是那次调用与它的结果，不是那句话。
    let (_dir, log) = log(|log| {
        say(log, &kimi(), "let me look");
        call(
            log,
            &kimi(),
            "call-1",
            "read_file",
            serde_json::json!({"file_path": "a.txt"}),
        );
        result(log, &kimi(), "call-1", "a");
        log.append(
            SpeakerId::User,
            EventPayload::HistorySuperseded {
                targets: vec![3, 4],
                reason: heng::events::HistoryReason::Undo,
                summary: None,
            },
        )
        .unwrap();
    });

    let messages = project(&log.events(), &kimi(), &caps());
    let assistant = messages
        .iter()
        .find(|message| matches!(message, Message::Assistant { .. }))
        .expect("那句话活了下来");
    match assistant {
        Message::Assistant {
            content,
            tool_calls,
            ..
        } => {
            assert_eq!(content.as_deref(), Some("let me look"));
            assert!(tool_calls.is_empty());
        }
        other => panic!("期望 assistant，实际得到 {other:?}"),
    }
}

// --- 线上序列化 -------------------------------------------------------------
#[test]
fn projected_messages_serialize_into_both_vendors_wire_shapes() {
    let (_dir, log) = log(|log| {
        say_with_reasoning(log, &kimi(), "answer", Some("thought"));
        call(log, &kimi(), "call-1", "read_file", serde_json::json!({}));
        result(log, &kimi(), "call-1", "body");
        say(log, &deepseek(), "deepseek's reply");
    });

    let request = ChatRequest {
        model: "fake-model".to_owned(),
        messages: project(&log.events(), &kimi(), &caps()),
        tools: Vec::new(),
        tool_choice: ToolChoice::Auto,
        params: GenerationParams {
            max_output_tokens: Some(100),
            ..GenerationParams::default()
        },
        cache_key: None,
    };

    for (vendor, caps) in [
        ("kimi", caps_for("kimi-k3").unwrap()),
        ("deepseek", caps_for("deepseek-v4-pro").unwrap()),
    ] {
        let (body, warnings) = build_body(&request, caps);
        assert!(warnings.is_empty(), "{vendor}：{warnings:?}");
        let messages = body["messages"].as_array().unwrap();

        let assistant = messages
            .iter()
            .find(|message| message["role"] == "assistant")
            .unwrap_or_else(|| panic!("{vendor}：一条 assistant 消息"));
        assert_eq!(assistant["reasoning_content"], "thought", "{vendor}");
        assert_eq!(assistant["name"], "kimi", "{vendor}");

        let tool = messages
            .iter()
            .find(|message| message["role"] == "tool")
            .unwrap_or_else(|| panic!("{vendor}：一条工具消息"));
        assert!(
            tool.get("name").is_none(),
            "{vendor}：工具消息不能带 `name`：{tool}"
        );
        assert_eq!(tool["tool_call_id"], "call-1", "{vendor}");

        let user = messages
            .iter()
            .find(|message| message["role"] == "user")
            .unwrap_or_else(|| panic!("{vendor}：别人的那条消息"));
        assert_eq!(user["name"], "deepseek", "{vendor}");
    }

    // 输出 token 那个字段真的是按厂商不同的。
    let (kimi_body, _) = build_body(&request, caps_for("kimi-k3").unwrap());
    let (deepseek_body, _) = build_body(&request, caps_for("deepseek-v4-pro").unwrap());
    assert_eq!(kimi_body["max_completion_tokens"], 100);
    assert_eq!(deepseek_body["max_tokens"], 100);
}

#[test]
fn reasoning_replay_is_data_on_the_capability_table_not_a_vendor_branch() {
    let (_dir, log) = log(|log| {
        say_with_reasoning(log, &kimi(), "answer", Some("thought"));
    });

    let mut caps = caps();
    caps.requires_reasoning_replay = false;

    let messages = project(&log.events(), &kimi(), &caps);
    match &messages[0] {
        Message::Assistant {
            reasoning_content, ..
        } => assert!(
            reasoning_content.is_none(),
            "不要求重放的模型一条都拿不到：{reasoning_content:?}"
        ),
        other => panic!("期望 assistant，实际得到 {other:?}"),
    }
    // 同一批事件配上表里那个事实，就产出了重放。
    let messages = project(&log.events(), &kimi(), &caps_for("deepseek-flash").unwrap());
    match &messages[0] {
        Message::Assistant {
            reasoning_content, ..
        } => assert_eq!(reasoning_content.as_deref(), Some("thought")),
        other => panic!("期望 assistant，实际得到 {other:?}"),
    }
}

#[test]
fn a_mid_session_injection_does_not_merge_into_an_executors_brief() {
    // 简报是执行者的第一次发言，不是被钉住的头部的一部分：
    // 头部是开头那一串注入，而稍后到的计划模式注入必须
    // 独立成条，而不是被追加到简报后面（spec §5、§10）。
    let executor = SpeakerId::Executor("e-1".into());
    let (_dir, log) = log(|log| {
        log.append(
            SpeakerId::User,
            EventPayload::ContextInjected {
                source: ContextSource::AgentsMd,
                content: "project rules".to_owned(),
            },
        )
        .unwrap();
        log.append(
            executor.clone(),
            EventPayload::ExecutorSpawned {
                executor_id: heng::events::ParticipantId::new("e-1"),
                parent: heng::events::ParticipantId::new("kimi"),
                brief: "count the modules under src".to_owned(),
            },
        )
        .unwrap();
        log.append(
            SpeakerId::User,
            EventPayload::ContextInjected {
                source: ContextSource::PlanMode,
                content: "read PLAN.md before acting".to_owned(),
            },
        )
        .unwrap();
    });

    let messages = project(&log.events(), &executor, &caps());
    let users = user_messages(&messages);

    assert_eq!(users.len(), 3, "头部、简报、注入：{messages:?}");
    assert!(text_of(users[0]).contains("project rules"));
    assert_eq!(text_of(users[1]), "count the modules under src");
    assert_eq!(text_of(users[2]), "read PLAN.md before acting");
}

#[test]
fn a_personas_name_reaches_the_model_verbatim_while_the_wire_field_stays_sanitized() {
    // 讨论者是一个人物：`[discussion] debaters = [{ name = "保守", … }]` 给一方
    // 起名，而同一场讨论的两方必须在前缀里保持分得开 ——
    // 一个被弄坏的名字会被读成同一个发言者两遍。厂商的字符集
    // 唯一没有文档的地方就是 `name` 这个字段，所以它单独过一道
    // 打码（spec §5）。
    let persona = SpeakerId::Debater("保守".into());
    let (dir, mut log) = log(|_| {});
    log.append(
        persona.clone(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "保守的看法".to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: "问题".to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
    log.append(
        persona.clone(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "再看一次".to_owned(),
            reasoning: None,
        },
    )
    .unwrap();

    let events = log.events();
    let messages = project(&events, &kimi(), &caps());
    let merged = messages
        .iter()
        .find(|message| {
            matches!(
                message,
                Message::User { content, .. } if content.contains("保守的看法")
            )
        })
        .expect("这个人物的发言到了对面");
    let Message::User { content, name, .. } = merged else {
        unreachable!()
    };
    assert!(
        content.contains("保守"),
        "前缀保留用户挑的那个名字：{content}"
    );
    assert_eq!(
        name.as_deref(),
        Some("--"),
        "线上那个字段是打码后的形状，而正文才是归属的保证"
    );
    drop(dir);
}

#[test]
fn a_persona_is_private_to_the_side_it_describes() {
    // 灵魂记在**流上**（好让重放能把那次调用重建出来），署名是
    // 它所描述的那个讨论者 —— 而投影不把它交给任何别人：
    // 对面正在*反对*这个人物，而合成器读的是答案，
    // 不是人物（spec §5、§15）。
    let (dir, mut log) = log(|_| {});
    let persona = SpeakerId::Debater("张三".into());
    log.append(
        persona.clone(),
        EventPayload::ContextInjected {
            source: ContextSource::Persona("张三".into()),
            content: "你的性格设定：法外狂徒".to_owned(),
        },
    )
    .unwrap();
    log.append(
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: "问题".to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
    log.append(
        persona.clone(),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "张三的作答".to_owned(),
            reasoning: None,
        },
    )
    .unwrap();
    let events = log.events();

    let own = project(&events, &persona, &caps());
    assert!(
        own.iter().any(|message| matches!(
            message,
            Message::User { content, .. } if content.contains("法外狂徒")
        )),
        "人物自己的那段注入到了它手里：{own:?}"
    );
    for other in [kimi(), synthesizer()] {
        let seen = project(&events, &other, &caps());
        assert!(
            !seen.iter().any(|message| matches!(
                message,
                Message::User { content, .. } if content.contains("法外狂徒")
            )),
            "{other} 绝不能读到另一方的灵魂：{seen:?}"
        );
    }
    drop(dir);
}
