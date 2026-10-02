//! `Tool` trait，以及调度器据以分流的副作用分类。
//!
//! `effect` 描述的是**工作区**副作用，而不是「有没有副作用」：读一个文件的工具是
//! `ReadOnly`，而一个不碰工作区、只 spawn 一个 agent 的工具同样会是 `ReadOnly`（spec §7）。
//! 调度器按这个值给一批调用分区，于是并行执行只读调用是接线，而不是一次重构。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::context::repo_map::RepoMapInput;
use crate::context::skills::Skills;
use crate::provider::ToolSpec;
use crate::questions::UserQuestions;

use super::sandbox::Sandbox;

/// 一次已规划调用的工作区副作用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// 只读工作区，什么都不写。这些可以并发跑。
    ReadOnly,
    /// 恰好写这些路径（每一个都是工具将要去解析的输入）。
    WritePaths(Vec<PathBuf>),
    /// 独占工作区；同一时刻别的什么都不许跑。
    ///
    /// `bash`（spec §7）是它的持有者：一个 shell 什么都能写，所以派发器为它取工作区级的锁。
    /// `effect()` 是调度器与权限门共用的那一套副作用词汇。
    Exclusive,
}

/// 一次成功工具调用的结果，等着变成一条 `ToolCallCompleted`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
}

impl ToolOutput {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// 一次工具调用为什么失败。`InvalidatesReads` 是派发器会在结构上做出反应的那一个变体：
/// 一次找不到匹配的写意味着 agent 对那个文件的图景已经陈旧，于是它的读权限被收回。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Message(String),
    #[error("{message}")]
    InvalidatesReads { message: String, path: PathBuf },
}

impl ToolError {
    pub fn message(message: impl Into<String>) -> Self {
        ToolError::Message(message.into())
    }

    /// 读权限被收回的那个路径（如果有）。
    pub fn invalidated_path(&self) -> Option<&Path> {
        match self {
            ToolError::InvalidatesReads { path, .. } => Some(path),
            ToolError::Message(_) => None,
        }
    }

    /// 当这次失败意味着该路径的读权限必须被丢掉时为真。
    pub fn invalidates_reads(&self) -> bool {
        self.invalidated_path().is_some()
    }
}

/// 一次 `bash` 调用跑在其下的两条墙钟上限（spec §7）。
///
/// 两条都来自 `SessionConfig`；交到工具手里的是这一对、而不是整份配置，所以一个命令类工具
/// 能读到的会话值，只有它真正被允许使用的那些。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BashLimits {
    /// 模型没给 `timeout_ms` 时用的上限。
    pub default_timeout_ms: u64,
    /// 模型给的 `timeout_ms` 会被夹到的天花板。
    pub max_timeout_ms: u64,
}

impl BashLimits {
    /// 一次调用的上限：模型要的那个值夹到天花板，没给就用配置的默认值。`requested` 为零会由
    /// 调用方在这之前拒掉，所以结果永远至少是一毫秒。
    pub fn timeout(&self, requested_ms: Option<u64>) -> Duration {
        let ms = requested_ms.unwrap_or(self.default_timeout_ms);
        Duration::from_millis(ms.clamp(1, self.max_timeout_ms.max(1)))
    }
}

impl Default for BashLimits {
    /// 配置里的默认值，于是在组装之外构造出来的值也是可用的，而不是一条零毫秒的上限。
    fn default() -> Self {
        Self {
            default_timeout_ms: crate::config::DEFAULT_BASH_TIMEOUT_MS,
            max_timeout_ms: crate::config::MAX_BASH_TIMEOUT_MS,
        }
    }
}

/// 一次调用交到工具手里的东西。工具解析自己的路径（它知道哪个参数带路径），而解析器正是把
/// 模型给的路径关在会话 cwd 里面的那件东西。
pub struct ToolContext<'a> {
    /// 模型可以读的路径。会话 cwd 之外的读一律拒。
    pub read_paths: &'a dyn ReadPathResolver,
    /// 模型可以写的路径。
    pub write_paths: &'a dyn WritePathResolver,
    /// 超大输出与 `.before` 快照落盘的地方。
    pub outputs_dir: &'a Path,
    /// 会话 cwd，用于显示，也给那些相对它跑的工具。
    pub cwd: &'a Path,
    /// 会话已发现的技能库（spec §9）。是组装期发现好的一个值，所以 `skill(name)` 是查表，
    /// 而不是去解析一个路径。
    pub skills: &'a Skills,
    /// `repo_map` 工具的会话输入（spec §9）：排序上下文与配置的预算。捆成一个字段，好让它像
    /// `skills` 一样传递。
    pub repo_map: &'a RepoMapInput,
    /// 一次 `bash` 调用跑在其下的墙钟上限（spec §7）。是会话配置，携带进来，好让工具永不伸手
    /// 去够那个会话。
    pub bash: &'a BashLimits,
    /// 这一次调用跑在哪个沙箱里（沙箱 spec §7）。同样是会话配置在组装期变成的注入值：命令类工具
    /// 把它交给 [`super::process::run`]，而不可用时那一步就是一条工具错误。
    pub sandbox: &'a Sandbox,
    /// 跑一个嵌套执行者的端口，给 `task`（spec §16）。会话没挂端口时是 `None`，那时 `task`
    /// 如实报告，而不是假装在干活。
    pub executor: Option<&'a dyn ExecutorSpawner>,
    /// 把模型发起的提问交给用户的端口，给 `ask_user_question`（spec §7）。会话没挂端口时是
    /// `None` —— headless 组装永不挂 —— 那时工具如实报告，而不是挂在一个没人能给的答案上。
    ///
    /// 是 `Arc` 而不是一个借用（`.scratch/mcp-support/spec.md` §3）：MCP 那侧的
    /// `ClientHandler` 要求 `'static`，它必须在连接建立时就把这条端口握在手里；两条路共用
    /// **同一个**端口值，于是不会漂成两套实现。
    pub questions: Option<Arc<dyn UserQuestions>>,
    pub tool_call_id: &'a str,
    pub args: &'a Value,
}

/// 工具用来跑一个嵌套执行者的端口（spec §16）。
///
/// 它住在这里、由 `agent` 层（`agent::executor`）实现，因为跑一个执行者意味着驱动一整个
/// 回合：那一层是事件流的唯一写入者，也是 provider 的唯一调用方。`tools` 只知道形状，所以
/// 依赖箭头依旧朝下。
#[async_trait]
pub trait ExecutorSpawner: Send + Sync {
    /// 为 `brief` 把一个执行者跑到完成，并回报 —— 回报时已经塑造成 `task` 调用那唯一一条结果。
    ///
    /// 结果就是摘要加上从流上派生的元数据（spec §16）；执行者的过程不会到达任何别人那里。
    async fn spawn(&self, brief: &str) -> Result<ToolOutput, ToolError>;
}

/// 把模型给的读路径解析到会话 cwd 上。
pub trait ReadPathResolver: Send + Sync {
    fn resolve_read(&self, path: &Path) -> Result<PathBuf, ToolError>;
}

/// 把模型给的写路径解析到会话 cwd 上。
pub trait WritePathResolver: Send + Sync {
    fn resolve_write(&self, path: &Path) -> Result<PathBuf, ToolError>;
}

/// 一条路径在这个会话里有没有被读过。读权限是逐 agent 的，且两个方向都不继承（spec §16）。
#[derive(Debug, Default)]
pub struct ReadSet {
    paths: HashSet<PathBuf>,
}

impl ReadSet {
    pub fn record(&mut self, path: impl Into<PathBuf>) {
        self.paths.insert(path.into());
    }

    pub fn record_all(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            self.record(path);
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.paths.contains(path)
    }

    pub fn invalidate(&mut self, path: &Path) {
        self.paths.remove(path);
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

/// 运行时注册表里的一个工具。
///
/// object safe 且异步：注册表存的就是 `Box<dyn Tool>`，而 `call` 收的是擦除后的 JSON，好让
/// 动态工具有一道进来的门，而不必再开第二个 trait。
#[async_trait]
pub trait Tool: Send + Sync {
    /// 线级声明，原样发给 provider。
    fn spec(&self) -> ToolSpec;

    /// 这次调用的工作区副作用。是 args 的纯函数。
    fn effect(&self, args: &Value) -> Effect;

    /// 这次调用看过的路径，好让派发器发放读权限。
    ///
    /// 一个读工具在这里声明自己的路径，而不是由派发器去猜某个参数名 —— 这样工具的线级契约
    /// 依旧是它自己的。
    fn read_paths(&self, _args: &Value) -> Vec<PathBuf> {
        Vec::new()
    }

    /// 这次调用将要执行的 argv，给跑命令的工具。
    ///
    /// `bash` 是它的持有者；别的工具一律答 `None`。权限门的 `CommandPrefix` 范围与 `rm`
    /// 断路器都读它，所以一条命令的 argv 必须在进程启动**之前**就可见 —— 这就是工具在这里
    /// 声明它、而不是由门去猜某个 `command` 字符串的原因。
    fn command(&self, _args: &Value) -> Option<Vec<String>> {
        None
    }

    /// 这一次调用带上的升级申请 —— 当它带了一个。
    ///
    /// `bash` 是唯一的持有者：只有它的 schema 里有 `escalation`，动态工具的 argv 模板是
    /// 使用者在 `config.toml` 里声明的，插不进新参数（`.scratch/workspace-mode/spec.md`
    /// §4）。半截的写法是参数错误，所以这里返回 `Result`：一个被吞掉的升级申请会变成一条
    /// 谁也没问过的命令。
    fn escalation(
        &self,
        _args: &Value,
    ) -> Result<Option<crate::permissions::Escalation>, ToolError> {
        Ok(None)
    }

    /// 这个工具是不是执行者工具表的一部分。
    ///
    /// 递归深度为一（spec §16），而这一条由工具表强制、不由一条规则强制：`task` 答 `false`，
    /// 于是执行者的工具声明根本没有办法派发出另一个执行者。用规则就是第二道更弱的防线 ——
    /// 工具仍会被声明给模型，而规则里的一个 bug 就是一个递归 bug。
    fn delegable(&self) -> bool {
        true
    }

    /// 参数到手时是擦除过的；每个工具解析自己的形状、报自己的错。
    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError>;
}
