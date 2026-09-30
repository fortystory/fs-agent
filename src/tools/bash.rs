//! 内建的 `bash(command, timeout_ms?)` 工具：在会话工作区里跑一条 shell 命令（spec §7、§12、
//! §20）。
//!
//! 三条决定定义了这个工具：
//!
//! * `effect()` **永远**是 [`Effect::Exclusive`]：一个 shell 什么都能写，所以派发器取工作区级
//!   的锁，权限门把这次调用当作写。于是 `readonly` 不需要特例就会拒掉它，也没有写豁免可以借了
//!   （旧计划模式那条 `PLAN.md` 豁免已经退场 ——
//!   `docs/adr/0003-plan-leaves-the-permission-modes.md`）。
//! * 命令是作为**一个 argv 元素**跑的 —— `["bash", "-lc", command]` 直接 spawn，绝不是把一条
//!   命令字符串拼进更大的 shell 行里 —— 所以模型加不了第二层 shell 替换。
//! * 超时终止的是**进程组**，不只是那个 shell，所以一个又起了子进程的命令不会把它们留在身后。
//!   同一条守卫在调用被半路丢掉（一次取消手势）时也会杀掉整个组。
//!
//! 后两条是每个命令类工具都需要的机制，所以它们住在 [`super::process`] 里、与动态工具共用
//! （spec §14）；这个模块只是 shell 特有的那部分。

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::process;
use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、循环与测试不会互相漂离。
pub const BASH_TOOL: &str = "bash";

/// 那个 shell 与让它收下命令字符串的那个旗标。`-l` 给命令一份用户的登录环境；`-c` 才是收下
/// 那一个参数的东西。
const SHELL: &str = "bash";
const SHELL_FLAG: &str = "-lc";

/// 通过系统 shell 跑一条命令。
pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: BASH_TOOL.to_owned(),
            description:
                "在工作区里跑一条 shell 命令，返回它的退出码、标准输出与标准错误。命令通过 \
                          `bash -lc` 非交互地跑：没有 TTY、stdin 是空的，所以别启动交互式程序。\
                          有墙钟超时（默认配置是 120s；会话可以配一个不同的默认值与上限），\
                          超时会把整棵进程树杀掉。非零退出是正常结果。这次调用期间工作区被独占，\
                          所以尽量跑短小、非交互的命令。"
                    .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "要跑的 shell 命令，按你写下的原文。它会作为单个参数\
                                        交给 `bash -lc`"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "可选，墙钟上限，单位毫秒。不写就用配置的默认值，并会被\
                                        配置的上限夹住"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    /// 一个 shell 什么都能写，所以每次调用都是 `Exclusive`，无论命令文本写了什么（spec §7）。
    fn effect(&self, _args: &Value) -> Effect {
        Effect::Exclusive
    }

    /// 任何东西启动之前门看到的 argv：那个 shell、它的旗标，以及作为**一个**元素的命令。
    /// `CommandPrefix` 匹配这条 argv，而 `rm` 断路器穿透这层包装去读那条命令字符串。
    fn command(&self, args: &Value) -> Option<Vec<String>> {
        argv(args)
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let argv = argv(&args)
            .ok_or_else(|| ToolError::message(format!("{BASH_TOOL}：需要一个非空的 `command`")))?;
        let requested = requested_timeout_ms(&args)?;
        if requested == Some(0) {
            return Err(ToolError::message(format!(
                "{BASH_TOOL}：`timeout_ms` 必须是正的毫秒数"
            )));
        }
        let limit = ctx.bash.timeout(requested);
        let outcome = process::run(ctx.cwd, &argv, limit).await?;
        Ok(ToolOutput::new(outcome.report()))
    }
}

/// 这次调用要跑的那一条 argv，从 args 构造，好让 [`Tool::command`] 与 [`Tool::call`] 对「将
/// 要执行什么」不可能有分歧。
fn argv(args: &Value) -> Option<Vec<String>> {
    let command = args
        .get("command")
        .and_then(Value::as_str)
        .filter(|command| !command.trim().is_empty())?;
    Some(vec![
        SHELL.to_owned(),
        SHELL_FLAG.to_owned(),
        command.to_owned(),
    ])
}

/// 模型给的 `timeout_ms`，当它给了的话。一个存在但不是非负整数的值，是参数错误，而不是悄悄
/// 回退。
fn requested_timeout_ms(args: &Value) -> Result<Option<u64>, ToolError> {
    match args.get("timeout_ms") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_u64().map(Some).ok_or_else(|| {
            ToolError::message(format!("{BASH_TOOL}：`timeout_ms` 必须是正的整数毫秒数"))
        }),
    }
}
