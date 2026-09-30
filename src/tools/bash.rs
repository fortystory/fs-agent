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

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use crate::permissions::Escalation;
use crate::provider::ToolSpec;

use super::process;
use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、循环与测试不会互相漂离。
pub const BASH_TOOL: &str = "bash";

/// 工具描述里那段沙箱说明（`.scratch/sandbox/spec.md` §8）。
///
/// 模型可见、进请求前缀，所以它是**常量**：加一句是常量成本，一次定死，不随会话变化
/// （工具声明是前缀缓存的一部分）。它要说清四件事 —— 命令跑在沙箱里、区外只读、被拒绝
/// 说明越界而不是命令写错、以及 `/tmp` 每次调用都是新的。
pub const SANDBOX_NOTE: &str =
    "命令跑在一个文件沙箱里：工作区与一列缓存目录可写，区外只读；被沙箱拒绝说明命令越界了，\
     不是命令写错了。`/tmp` 每次调用都是新的：同一条命令内可用、跨命令不保留。（会话把 \
     `[sandbox] mode` 设成 \"off\" 时这一层是关着的。）";

/// 工具描述里那段**升级手势**（`.scratch/workspace-mode/spec.md` §4）。
///
/// 与 [`SANDBOX_NOTE`] 同一个理由：模型可见、进请求前缀，所以是常量、一次定死。它要说清
/// 四件事 —— 被拒是结论、越界只有带理由重试一次这一条路、不许先绕道去聊天里问、以及不许
/// 在没被拒的时候投机性升级。
pub const ESCALATION_NOTE: &str =
    "命令被沙箱拒绝（内核说只读文件系统）就是这条命令的结论：那条路走不通。要越界只有一条路\
     ——带上 `escalation`（`justification` 与 `writable_paths` 都要写、都要非空）把**同一条\
     命令**原样重试一次，用户会就你声明的那几条路径问一次；批准只对这一次调用生效，拒绝即\
     终局，同一条命令再被拒也不会再问。不许先绕道去聊天里问用户，也不许在没被拒的时候预先\
     声明一个更宽的档位。";

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
            description: format!(
                "在工作区里跑一条 shell 命令，返回它的退出码、标准输出与标准错误。命令通过 \
                 `bash -lc` 非交互地跑：没有 TTY、stdin 是空的，所以别启动交互式程序。\
                 有墙钟超时（默认配置是 120s；会话可以配一个不同的默认值与上限），\
                 超时会把整棵进程树杀掉。非零退出是正常结果。这次调用期间工作区被独占，\
                 所以尽量跑短小、非交互的命令。{SANDBOX_NOTE}{ESCALATION_NOTE}"
            ),
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
                    },
                    "escalation": {
                        "type": "object",
                        "description": "可选，只在命令**被沙箱拒绝**之后带上：申请放开这一次\
                                        调用要写的工作区之外的路径。理由与路径都要写、都要\
                                        非空，成对出现。批准只对这一次调用生效",
                        "properties": {
                            "justification": {
                                "type": "string",
                                "description": "为什么要写那儿，一句话"
                            },
                            "writable_paths": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "这一次调用要放开的路径，就是这些路径本身，\
                                                不做父目录提升；文件写文件，目录写目录"
                            }
                        },
                        "required": ["justification", "writable_paths"]
                    }
                },
                "required": ["command"]
            }),
        }
    }

    /// 一次 shell 什么都能写，所以每次调用都是 `Exclusive`，无论命令文本写了什么（spec §7）。
    fn effect(&self, _args: &Value) -> Effect {
        Effect::Exclusive
    }

    /// 模型带上来的升级申请，当它带了一个（`.scratch/workspace-mode/spec.md` §4）。
    ///
    /// 半截的写法 —— 有理由没路径、路径为空、字段类型不对 —— 是**参数错误**，不是静默
    /// 忽略：一次被吞掉的升级申请会变成一条看起来「命令没跑成但也没人问」的谜。
    fn escalation(&self, args: &Value) -> Result<Option<Escalation>, ToolError> {
        escalation(args)
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
        let outcome = process::run(ctx.cwd, &argv, limit, ctx.sandbox).await?;
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

/// 参数里那次升级申请，当它带了的时候。
///
/// 形状是 `{"justification": "…", "writable_paths": ["…"]}`，两个字段都要非空、必须成对。
/// 路径在这里只做「非空字符串」这一层校验，变成绝对路径是注册表的事（它才知道 cwd 与
/// home），而「这些路径是不是写死的边界」是权限门的事。
fn escalation(args: &Value) -> Result<Option<Escalation>, ToolError> {
    let Some(value) = args.get("escalation").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let payload = value.as_object().ok_or_else(escalation_error)?;
    let justification = payload
        .get("justification")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(escalation_error)?;
    let items = payload
        .get("writable_paths")
        .and_then(Value::as_array)
        .ok_or_else(escalation_error)?;

    let mut writable_paths = Vec::with_capacity(items.len());
    for item in items {
        let path = item
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(escalation_error)?;
        writable_paths.push(PathBuf::from(path));
    }
    if writable_paths.is_empty() {
        return Err(escalation_error());
    }

    Ok(Some(Escalation {
        justification: justification.to_owned(),
        writable_paths,
    }))
}

/// 半截或形状不对的升级申请：一次参数错误，而不是被吞掉。
fn escalation_error() -> ToolError {
    ToolError::message(format!(
        "{BASH_TOOL}：`escalation` 要写成 \
         {{\"justification\": \"为什么要写那儿\", \"writable_paths\": [\"要放开的路径\"]}}，\
         两个字段都要非空、必须成对；不给就整段别给"
    ))
}
