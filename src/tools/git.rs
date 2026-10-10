//! 内建的 `git(op, args?)` 工具：在会话工作区里跑一条版本控制操作（`.scratch/tool-coverage`
//! §3；票 10）。
//!
//! **一条工具、两个字段**。模型今天要走 git 只有一条路：经 `bash` 拼一条命令。那条路上的
//! 权限代数是错的 —— `bash` 的 `effect()` 恒为 `Effect::Exclusive`（「一个 shell 什么都能写」），
//! 于是 `git status` 与 `git diff` 这种纯读取也一样要独占工作区、在 `readonly` 档下被拒。
//!
//! [`GitTool::effect`] 按 `op` 分档（先例是 MCP 转发工具那处「按参数返回不同 `Effect`」）：
//! 读 op 是 `Effect::ReadOnly`，四档放行、不取工作区锁；写 op 是 `Effect::Exclusive` ——
//! `add` / `commit` 写的是 `.git/index` 之类，**枚举不出**「恰好这些路径」，所以它拿不到
//! `WritePaths` 那一档更精确的描述。
//!
//! 三条边界：
//!
//! * **不给 `workdir`**：实测 `git -C` 出现 0 次，而 git 自己会向上找仓库根。站位固定是会话
//!   cwd，没有歧义，所以结果也**不加** `cwd:` 首行；真要换目录用 `args` 透传 `-C`。
//! * **代码不与渲染层共用**：渲染层那件宿主侧 git 的三条纪律是「不过权限门、不过沙箱、
//!   不在任何一帧里 await」，与模型侧的要求正相反。这里走工具层现成的带沙箱进程件
//!   （[`super::process`]），共用的只是「起一个子进程」这件事本身。
//! * **`args` 原样透传**：git 自己的开关（`--stat` / `--oneline` / `-n` / `-C`）不必重新学
//!   一套参数名，而验证它们能做到什么的正是 git 自己。

use async_trait::async_trait;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::process;
use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、描述与测试不互相漂离。
pub const GIT_TOOL: &str = "git";

/// 只读的四个操作。它们四档放行、也不取工作区锁。
pub const READ_OPS: [&str; 4] = ["status", "diff", "log", "show"];

/// 会写工作区的三个操作。它们独占工作区，且与读 op **同批声明、同批实现** —— 声明了不实现
/// 会让模型调用到一个不存在的操作。
pub const WRITE_OPS: [&str; 3] = ["add", "commit", "stash"];

/// git 的所有 op，按声明里的次序。`rev-parse` / `branch` / `ls-files` 与那组搭测试仓库用的
/// `config` / `init` / `user` / `safe` **不在表里** —— 留给 `bash`（spec §3）。
///
/// **由前两个常量派生**，不另写一遍：加一个 op 时漏改这张表，声明与分档就会对不上，而那正是
/// 一条工具唯一的两种分档依据。
pub const OPS: [&str; 7] = [
    READ_OPS[0],
    READ_OPS[1],
    READ_OPS[2],
    READ_OPS[3],
    WRITE_OPS[0],
    WRITE_OPS[1],
    WRITE_OPS[2],
];

/// 通过系统 git 跑一条操作。
pub struct GitTool;

#[async_trait]
impl Tool for GitTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: GIT_TOOL.to_owned(),
            description: "在工作区里跑一条版本控制操作，返回它的退出码、标准输出与标准错误。\
                          一个字段 `op`、可选的 `args` 原样透传给 git（`--stat`、`--oneline`、\
                          `-n`、换目录的 `-C` 都照给）。读的那几个（`status` / `diff` / `log` /\
                          `show`）是只读的，在每一档权限模式下都放行、也不占工作区锁；写的那几个\
                          （`add` / `commit` / `stash`）要独占工作区，`readonly` 档下会被拒。\
                          站位固定是会话工作区，git 自己向上找仓库根；`op` 只认这张表里的那七个，\
                          别的（`rev-parse` / `branch` / `ls-files`，以及搭仓库用的 `init` /\
                          `config` / `user`）走 `bash`。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "op": {
                        "type": "string",
                        "description": "要做哪一条：读 `status` / `diff` / `log` / `show`，\
                                        写 `add` / `commit` / `stash`",
                        "enum": OPS
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "可选，原样透传给 git 的开关，例如 \
                                        [\"--oneline\", \"-5\"] 或 [\"-C\", \"src\", \"status\"]"
                    }
                },
                "required": ["op"]
            }),
        }
    }

    /// 按 `op` 分档（先例：MCP 转发工具那处的「按参数返回不同 `Effect`」）。
    ///
    /// 写 op 与**认不出来的 op** 都落在 `Exclusive` 那一支：分档必须在 [`Tool::validate`] 判完
    /// 参数之后才有意义，而在它之前这里要有一个**安全**的答案。
    fn effect(&self, args: &Value) -> Effect {
        match requested_op(args) {
            Some(op) if READ_OPS.contains(&op.as_str()) => Effect::ReadOnly,
            _ => Effect::Exclusive,
        }
    }

    /// 一个不认识的 `op` 是**参数错误**，而它必须在 [`Tool::effect`] 分档之前就判出来
    /// （那条路是 [`crate::tools::Registry::facts`]，它在权限门之前跑）。
    fn validate(&self, args: &Value) -> Result<(), ToolError> {
        let Some(op) = requested_op(args) else {
            return Err(unknown_op(args));
        };
        if !OPS.contains(&op.as_str()) {
            return Err(unknown_op(args));
        }
        transparent_args(args)?;
        Ok(())
    }

    /// 门与断路器在进程起来**之前**看到的那条 argv。
    ///
    /// 给了它，权限规则就能写 `CommandPrefix = ["git", "status"]` 这一种作用域
    /// （`permissions.rs` 的 `Scope::CommandPrefix` 就拿这条 argv）—— 而 `rm` 断路器对一条普通
    /// argv 只看首元素，于是 `git` 不会被误伤。这条不在 spec 的参数面里，是接线的一部分。
    fn command(&self, args: &Value) -> Option<Vec<String>> {
        argv(args)
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        // 参数在进到这里之前已经由 `validate()` 判过一遍；这里再判一次是为了这个模块自己能
        // 站得住（派发器之外还有别处会构造 `ToolContext`）。
        self.validate(&args)?;
        let limit = ctx.bash.timeout(None);
        let outcome = process::run(
            ctx.cwd,
            ctx.cwd,
            &argv(&args).unwrap_or_default(),
            limit,
            ctx.sandbox,
        )
        .await?;
        Ok(ToolOutput::new(outcome.report()))
    }
}

/// 这次调用要跑的那一条 argv，从 args 构造，好让 [`Tool::command`] 与 [`Tool::call`] 对
/// 「将要执行什么」不可能有分歧。
///
/// 参数不对时答 `None`：那条路径上已经有一条参数错误在等着（`validate()` 或 `call()`）。
fn argv(args: &Value) -> Option<Vec<String>> {
    let op = requested_op(args)?;
    if !OPS.contains(&op.as_str()) {
        return None;
    }
    let mut argv = vec!["git".to_owned(), op];
    argv.extend(transparent_args(args).ok()?);
    Some(argv)
}

/// 模型给的 `op`，剪掉首尾空白之后的那一份。
fn requested_op(args: &Value) -> Option<String> {
    args.get("op")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|op| !op.is_empty())
        .map(str::to_owned)
}

/// `args` 里的那些字符串，一个都不改。
///
/// 半截的写法（数组里有别的东西）是参数错误而不是静默丢掉那一个：一次被吞掉的 `-c` 会变成
/// 一条与模型想做的完全不同的 git 操作。
fn transparent_args(args: &Value) -> Result<Vec<String>, ToolError> {
    let Some(value) = args.get("args").filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let items = value.as_array().ok_or_else(|| {
        ToolError::message(format!(
            "{GIT_TOOL}：`args` 是一个字符串数组，原样透传给 git（比如 \
             [\"--oneline\", \"-5\"]）；不给就整段别给"
        ))
    })?;
    items
        .iter()
        .map(|item| {
            item.as_str().map(str::to_owned).ok_or_else(|| {
                ToolError::message(format!(
                    "{GIT_TOOL}：`args` 里的每一项都要是一段字符串（那是直接交给 git 的参数）"
                ))
            })
        })
        .collect()
}

/// 不认识的 `op`：一条参数错误，说清它认识哪几个。
fn unknown_op(args: &Value) -> ToolError {
    let given = args.get("op").and_then(Value::as_str).unwrap_or("");
    ToolError::message(format!(
        "{GIT_TOOL}：`op` 只认这七个：{}（`rev-parse` / `branch` / `ls-files` 与 \
         `init` / `config` / `user` 那组走 `bash`）；收到的是 `{given}`",
        OPS.join(" / ")
    ))
}
