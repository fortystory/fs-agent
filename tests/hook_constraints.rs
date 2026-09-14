//! The hook constraint algebra, tested as a pure function of values (spec §3's
//! "directly tested pure functions" seam).
//!
//! These are the two properties the type exists to make true: the effective
//! verdict is the supremum on `Allow < Ask < Deny`, and no constraint can
//! express a relaxation. The public subset is checked here too, so the closure of
//! a hook's observation surface does not depend on the loop's wiring.

use fs_agent::events::{
    ContextSource, Decision, DecisionSource, Event, EventPayload, HistoryReason, ParticipantId,
    Role, RoundMode, SessionId, SpeakerId, StopReason, ToolCallId, Usage,
};
use fs_agent::hooks::{effective_verdict, public_history, Constraint, HookEvent, Tightening};

/// One payload of every enum variant, with whether a hook may see it.
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

/// The effective verdict as the loop computes it: the supremum of the gate's
/// verdict and whatever the constraint forces. This calls the production merge,
/// so the table below cannot drift from the loop.
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
        // The direction that matters: a hook cannot lower a verdict, even when
        // it only asks. The gate's deny stays the supremum.
        (Deny, Constraint::Tighten(Tightening::Ask), Deny),
        (Deny, Constraint::Tighten(Tightening::Deny), Deny),
    ];

    for (gate, constraint, expected) in cases {
        assert_eq!(
            effective(gate, &constraint),
            expected,
            "gate {gate:?} with {constraint:?}"
        );
    }
}

#[test]
fn only_tighten_forces_a_verdict_and_it_is_never_allow() {
    // `Tightening` is the whole vocabulary of verdicts a hook may express, and
    // the exhaustive match makes a future third case a compile error here. There
    // is no `Allow`, which is why "hooks cannot loosen" needs no runtime check.
    for tightening in [Tightening::Ask, Tightening::Deny] {
        let decision = match tightening {
            Tightening::Ask => Decision::Ask,
            Tightening::Deny => Decision::Deny,
        };
        assert_eq!(decision, tightening.decision());
        assert!(decision > Decision::Allow);
    }

    // Only `Tighten` contributes to the merge; the rest are flow.
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
        assert_eq!(projected.is_some(), is_public, "{kind} visibility");
        if let Some(event) = projected {
            assert_eq!(event.kind(), kind, "the projection keeps its own name");
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
        "the closed subset is tool + permission + session boundary"
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
        "messages, usage, hooks and everything else are not a hook's face"
    );
}
