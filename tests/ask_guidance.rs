//! 身份里那句提问指引（`.scratch/ask-guidance/seed.md`，2026-10-08 维护者定）。
//!
//! 钉住两件事：主会话身份**点名**了 `ask_user_question`（这一句是「模型想拍板却把问题写在
//! 正文里」的那条对策），以及另外两段公开身份**不带**它（合成器没有工具；讨论者的身份里不带
//! 这条引导，讨论该把问题答完而不是中途弹问卷）。

use heng::agent::{ASK_GUIDANCE, agent_identity};
use heng::discussion::{debater_identity, synthesizer_identity};

#[test]
fn the_program_identity_carries_the_ask_guidance() {
    let identity = agent_identity();
    assert!(identity.contains(ASK_GUIDANCE), "{identity}");
    assert!(
        identity.contains("ask_user_question"),
        "那句指引该点名工具：{identity}"
    );
}

/// 它落在 `todo` 那一段之后 —— 两段都是「怎么跟用户协作」这一档，而顺序一变又会作废一次
/// 缓存前缀，所以这里把它钉住。
#[test]
fn the_ask_guidance_sits_after_the_todo_rule() {
    let identity = agent_identity();
    let todo = identity.find("开工前先把计划写下来").expect("todo 那段在");
    let ask = identity.find(ASK_GUIDANCE).expect("提问那段在");
    assert!(todo < ask, "提问那一段排在 todo 之后：{identity}");
}

#[test]
fn the_other_public_identities_leave_it_out() {
    for (who, identity) in [
        ("讨论者", debater_identity("kimi")),
        ("合成器", synthesizer_identity()),
    ] {
        assert!(
            !identity.contains(ASK_GUIDANCE),
            "{who} 不该带提问那句：{identity}"
        );
    }
}
