//! 身份里那句时间指引（`.scratch/time-mcp/spec.md` §5；票 02）。
//!
//! 两件事一起钉住：模型**知道该问谁**，而身份里**没有任何时间的值**。后者是这份 feature 的支点
//! —— 身份既是缓存前缀的一部分，又是 `replay` 复现当时那次请求时会**再调一次**的那个函数。

use fs_agent::agent::{agent_identity, TIME_GUIDANCE};
use fs_agent::discussion::{debater_identity, synthesizer_identity};

#[test]
fn the_program_identity_carries_the_time_guidance() {
    let identity = agent_identity();
    assert!(identity.contains(TIME_GUIDANCE), "{identity}");
    assert!(
        identity.contains("get_current_time"),
        "那句指引该点名工具：{identity}"
    );
}

/// 身份里出现 `YYYY-MM-DD` 这种片段，就是有人把时钟读数拼了进去 —— 那会让 `replay` 不再是复现，
/// 也让它每次变一次就把整段缓存前缀作废。
#[test]
fn the_identity_never_carries_a_time_of_its_own() {
    let identity = agent_identity();
    assert!(
        !looks_like_a_date(&identity),
        "身份里不该有任何时间的值：{identity}"
    );
}

/// 另外三段身份**不拼**它：合成器明写「没有工具」，讨论者与执行者的工具体系各自独立
/// （`.scratch/time-mcp/spec.md` §5）。执行者那一段不在公开 API 上，断言在
/// [`tests/executor.rs`](../../tests/executor.rs) 里从一次真派发读。
#[test]
fn the_other_public_identities_leave_it_out() {
    for (who, identity) in [
        ("讨论者", debater_identity("kimi")),
        ("合成器", synthesizer_identity()),
    ] {
        assert!(
            !identity.contains(TIME_GUIDANCE),
            "{who} 不该带时间那句：{identity}"
        );
    }
}

/// `YYYY-MM-DD` —— 只看形状，不看它是不是一个合法日期。
fn looks_like_a_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(10).any(|window| {
        window[..4].iter().all(u8::is_ascii_digit)
            && window[4] == b'-'
            && window[5..7].iter().all(u8::is_ascii_digit)
            && window[7] == b'-'
            && window[8..].iter().all(u8::is_ascii_digit)
    })
}
