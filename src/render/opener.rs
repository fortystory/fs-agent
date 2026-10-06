//! 把一个目标交给系统默认程序（`.scratch/clickable-links/spec.md` §3、§5）。
//!
//! 这是**宿主侧**的动作：TUI 进程拿着终端、也在沙箱之外。所以这一条路不过 `bash` 工具、
//! 不过权限门、不进沙箱 —— 交给系统的自始至终只有**一个字符串**，而它作为**一个** argv
//! 元素传进去，绝不拼进 shell 字符串：模型的正文是不可信输入，`foo.html; rm -rf ~` 在这里
//! 只是一个（大概不存在的）文件名。
//!
//! 平台：v1 只 Linux x86_64（`README.md`），程序名写死 `xdg-open`，不做配置项（spec
//! 「明确不做」）。

use std::process::{Command, Stdio};

/// 这次打开要跑的命令：程序名与它的**一个**参数。
///
/// 单独拎出来是为了让「目标永远是一个参数、中间没有 shell」这条能被逐字断言 —— 这条纪律
/// 的代价只在这里付一次，别处只剩调用。
pub fn command(target: &str) -> (&'static str, &str) {
    ("xdg-open", target)
}

/// 交给系统默认程序，**不等它退出**。
///
/// 浏览器或文件管理器活多久与我们无关，让它拖住 TUI 才是坏的。但也不能留下一具僵尸，
/// 所以收尸放到一条后台线程上：主循环一步都不等。
pub fn open(target: &str) -> std::io::Result<()> {
    let (program, argument) = command(target);
    let mut child = Command::new(program)
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_is_one_argument_and_never_a_shell_line() {
        // 分号、管道、反引号在 argv 里都只是普通字符：它们到不了 shell。
        for nasty in ["foo.html; rm -rf ~", "x | y", "`whoami`", "$(id)"] {
            assert_eq!(command(nasty), ("xdg-open", nasty));
        }
    }

    #[test]
    fn a_target_with_spaces_survives_as_a_single_argument() {
        assert_eq!(
            command("/home/ada/我的 笔记/页面.html"),
            ("xdg-open", "/home/ada/我的 笔记/页面.html")
        );
    }
}
