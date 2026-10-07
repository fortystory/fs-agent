//! 思考用哪种语言：四段身份共用**同一处**条款（ADR 0005）。
//!
//! 模型自己产出的散文也是散文。思考文本进事件流（`MessageCompleted.reasoning`）、也进转录的
//! 详情弹窗（`src/render/tui.rs` 的 `DetailKind::Thinking`，TUI 里那一节写着 `── 思考 ──`），
//! 所以它和回答一样归 ADR 0005 那条「英文只留给不是散文的东西」管。
//!
//! 四段身份（本程序、讨论者、合成器、执行者）各自拼上 [`THINKING_IN_CHINESE`]，而这里断言
//! 公开的那三段确实拼上了 —— 漏掉一处，那个发言者的思考就会独自漂回英文（推理里夹着大量
//! 标识符与代码，它比回答更容易漂）。第四处是执行者：它的身份不在公开 API 上，所以那条断言
//! 住在 `tests/executor.rs`，从一次真实派发发出去的请求里读。

use heng::agent::{THINKING_IN_CHINESE, agent_identity};
use heng::discussion::{debater_identity, synthesizer_identity};

#[test]
fn every_public_identity_carries_the_thinking_rule() {
    for (who, identity) in [
        ("本程序", agent_identity()),
        ("讨论者", debater_identity("kimi")),
        ("合成器", synthesizer_identity()),
    ] {
        assert!(
            identity.contains(THINKING_IN_CHINESE),
            "{who} 的身份里没有那段思考语言的条款：{identity}"
        );
    }
}
