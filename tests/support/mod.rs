//! 端到端接缝共享的测试支撑。
//!
//! 假 provider 是验收工具：两家真厂商都没有
//! `seed`，所以脚本化的响应是唯一能可复现地驱动一个
//! 会话的办法。后面的票扩展它的脚本能力，而不新增接缝。

#![allow(dead_code)]
// 每个集成测试 crate 都单独编译这个模块，用到的
// fixture 子集各不相同。
#![allow(unused_imports)]

mod asker;
mod capture;
mod fake_provider;
mod hook;
mod sandbox;

pub use asker::{AlwaysAllow, ScriptedAsker};
pub use capture::CaptureBuf;
pub use fake_provider::{FakeProvider, Reply};
pub use hook::{PostCall, PreCall, ScriptedHook};
pub use sandbox::available as sandbox_available;
