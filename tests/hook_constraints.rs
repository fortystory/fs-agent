//! 钩子约束的代数，当作值上的纯函数来测（spec §3 说的
//! 「直接测的纯函数」那条接缝）。
//!
//! 这个类型的存在意义就是让两条性质成立：生效裁决是
//! `Allow < Ask < Deny` 上的上确界，而任何约束都表达不出
//! 放松。公开子集也在这里检查，于是钩子观察面的封闭性
//! 不依赖循环怎么接线。

use fs_agent::events::{
    ContextSource, Decision, DecisionSource, Event, EventPayload, HistoryReason, ParticipantId,
    Role, RoundMode, SessionId, SpeakerId, StopReason, ToolCallId, Usage,
};
use fs_agent::hooks::{effective_verdict, public_history, Constraint, HookEvent, Tightening};

/// 每个枚举变体各一条 payload，外加钩子能不能看见它。
fn all_payloads() -> Vec<(&'static str, EventPayload, bool)> {
    let session = SessionId::new("s");
    let participant = ParticipantId::new("kimi");
    let tool_call = ToolCallId::new("call-1");
    vec![
        (
            "SessionStarted",
            EventPayload::SessionStarted {
                session_id: session.clone(),
                cwd: "/workspace".to_owned(),
                schema_version: 1,
            },
            true,
        ),
        (
            "ContextInjected",
            EventPayload::ContextInjected {
                source: ContextSource::AgentsMd,
                content: "rules".to_owned(),
            },
            false,
        ),
        (
            "SessionEnded",
            EventPayload::SessionEnded {
                reason: StopReason::Completed,
            },
            true,
        ),
        (
            "RoundStarted",
            EventPayload::RoundStarted {
                round: 1,
                mode: RoundMode::Independent,
            },
            false,
        ),
        (
            "RoundEnded",
            EventPayload::RoundEnded {
                round: 1,
                reason: StopReason::NoDivergence,
            },
            false,
        ),
        (
            "DivergenceRecorded",
            EventPayload::DivergenceRecorded {
                round: 1,
                topic: "t".to_owned(),
                positions: vec!["a".to_owned()],
            },
            false,
        ),
        (
            "TurnStarted",
            EventPayload::TurnStarted {
                agent: SpeakerId::Debater(participant.clone()),
                iteration: 1,
            },
            false,
        ),
        (
            "MessageCompleted",
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: "hi".to_owned(),
                reasoning: Some("because".to_owned()),
            },
            false,
        ),
        (
            "ToolCallStarted",
            EventPayload::ToolCallStarted {
                tool_call_id: tool_call.clone(),
                tool_name: "read_file".to_owned(),
                args: serde_json::json!({ "file_path": "a.txt" }),
            },
            true,
        ),
        (
            "ToolCallCompleted",
            EventPayload::ToolCallCompleted {
                tool_call_id: tool_call.clone(),
                ok: true,
                output: Some("contents".to_owned()),
                error: None,
                duration_ms: 3,
            },
            true,
        ),
        (
            "UsageRecorded",
            EventPayload::UsageRecorded {
                usage: Usage::default(),
            },
            false,
        ),
        (
            "TurnEnded",
            EventPayload::TurnEnded {
                reason: StopReason::Completed,
            },
            false,
        ),
        (
            "PermissionAsked",
            EventPayload::PermissionAsked {
                request_id: "perm-1".to_owned(),
                tool_call_id: tool_call.clone(),
                request: serde_json::json!({ "tool": "read_file" }),
            },
            true,
        ),
        (
            "PermissionDecided",
            EventPayload::PermissionDecided {
                request_id: "perm-1".to_owned(),
                decision: Decision::Deny,
                source: DecisionSource::Policy,
                reason: Some("because".to_owned()),
            },
            true,
        ),
        (
            "HookExecuted",
            EventPayload::HookExecuted {
                point: "pre_tool_use".to_owned(),
                command: "policy".to_owned(),
                outcome: "continue".to_owned(),
            },
            false,
        ),
        (
            "ExecutorSpawned",
            EventPayload::ExecutorSpawned {
                executor_id: participant.clone(),
                parent: participant.clone(),
                brief: "do it".to_owned(),
            },
            false,
        ),
        (
            "ExecutorFinished",
            EventPayload::ExecutorFinished {
                executor_id: participant.clone(),
                reason: StopReason::Completed,
                summary: "done".to_owned(),
            },
            false,
        ),
        (
            "AgentError",
            EventPayload::AgentError {
                message: "bad call".to_owned(),
                recoverable: true,
            },
            true,
        ),
        (
            "SessionError",
            EventPayload::SessionError {
                code: "protocol".to_owned(),
                detail: "detail".to_owned(),
            },
            false,
        ),
        (
            "HistorySuperseded",
            EventPayload::HistorySuperseded {
                targets: vec![1],
                reason: HistoryReason::Undo,
                summary: None,
            },
            false,
        ),
    ]
}

/// 循环算出来的生效裁决：权限门的裁决与约束逼出来的裁决取
/// 上确界。这里调的是生产代码里那一次合并，
/// 所以下面那张表不会跟循环漂开。
fn effective(gate: Decision, constraint: &Constraint) -> Decision {
    effective_verdict(gate, constraint.tightening())
}

#[test]
fn the_merge_is_the_supremum_on_allow_ask_deny() {
    use Decision::{Allow, Ask, Deny};

    let cases = [
        (Allow, Constraint::Continue, Allow),
        (Allow, Constraint::Tighten(Tightening::Ask), Ask),
        (Allow, Constraint::Tighten(Tightening::Deny), Deny),
        (Ask, Constraint::Continue, Ask),
        (Ask, Constraint::Tighten(Tightening::Ask), Ask),
        (Ask, Constraint::Tighten(Tightening::Deny), Deny),
        (Deny, Constraint::Continue, Deny),
        // 要紧的方向是这一边：钩子压不低一个裁决，哪怕它
        // 只是问一声。权限门的拒绝仍然是上确界。
        (Deny, Constraint::Tighten(Tightening::Ask), Deny),
        (Deny, Constraint::Tighten(Tightening::Deny), Deny),
    ];

    for (gate, constraint, expected) in cases {
        assert_eq!(
            effective(gate, &constraint),
            expected,
            "权限门 {gate:?} 配上 {constraint:?}"
        );
    }
}

#[test]
fn only_tighten_forces_a_verdict_and_it_is_never_allow() {
    // `Tightening` 就是钩子能表达的全部裁决词汇，而这里的
    // 穷尽匹配会让将来长出第三个变体时变成编译错误。这里
    // 没有 `Allow`，所以「钩子松不开」不需要任何运行时检查。
    for tightening in [Tightening::Ask, Tightening::Deny] {
        let decision = match tightening {
            Tightening::Ask => Decision::Ask,
            Tightening::Deny => Decision::Deny,
        };
        assert_eq!(decision, tightening.decision());
        assert!(decision > Decision::Allow);
    }

    // 只有 `Tighten` 参与那次合并；其余几个讲的是流程。
    for constraint in [
        Constraint::Continue,
        Constraint::Rewrite(serde_json::json!({})),
        Constraint::Skip,
        Constraint::Stop,
    ] {
        assert_eq!(constraint.tightening(), None, "{constraint:?}");
    }
}

#[test]
fn the_public_subset_is_exactly_seven_tool_permission_and_session_events() {
    let mut public = Vec::new();
    for (kind, payload, is_public) in all_payloads() {
        let projected = HookEvent::from_payload(&payload);
        assert_eq!(projected.is_some(), is_public, "{kind} 的可见性");
        if let Some(event) = projected {
            assert_eq!(event.kind(), kind, "投影保留它自己的名字");
            public.push(kind);
        }
    }

    assert_eq!(
        public,
        vec![
            "SessionStarted",
            "SessionEnded",
            "ToolCallStarted",
            "ToolCallCompleted",
            "PermissionAsked",
            "PermissionDecided",
            "AgentError",
        ],
        "这个封闭子集是工具 + 权限 + 会话边界"
    );
}

#[test]
fn public_history_keeps_only_the_public_events_in_seq_order() {
    let events: Vec<Event> = all_payloads()
        .into_iter()
        .enumerate()
        .map(|(index, (_, payload, _))| Event::new(index as u64 + 1, SpeakerId::System, payload))
        .collect();

    let kinds: Vec<&str> = public_history(&events)
        .iter()
        .map(HookEvent::kind)
        .collect();

    assert_eq!(
        kinds,
        vec![
            "SessionStarted",
            "SessionEnded",
            "ToolCallStarted",
            "ToolCallCompleted",
            "PermissionAsked",
            "PermissionDecided",
            "AgentError",
        ],
        "消息、用量、钩子以及其余一切都不是钩子能看到的那一面"
    );
}
