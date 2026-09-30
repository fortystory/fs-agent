//! 「沙箱可用」这一份会话配置。
//!
//! `workspace` 档的存在与否只问一件事：沙箱是不是可用（`.scratch/workspace-mode` 的
//! spec §6）。几份端到端测试都要组装这一档，而它们都不真跑命令，所以这里给一份状态为
//! 「可用」的设置就够 —— `bwrap` 指向哪个程序无所谓。

use std::path::PathBuf;

use fs_agent::config::{SandboxAvailability, SandboxMode, SandboxSettings};

/// 一份「沙箱可用」的 `[sandbox]` 设置。
pub fn available() -> SandboxSettings {
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Available {
        bwrap: PathBuf::from("/bin/true"),
    };
    settings
}
