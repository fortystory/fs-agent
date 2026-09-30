//! 运行时注册表与那唯一的派发点（spec §3、§7）。
//!
//! 注册表是会话携带的一个值，绝不是全局静态：它在启动时组装、在前缀缓存的整个生命周期里固定
//! 不变，也是动态工具插进来的挂载点。
//!
//! 派发拆成三步，好让护栏能在工具层完全不碰读集合的前提下被强制：
//!
//! 1. [`Registry::facts`] 把一次调用解析成 [`CallFacts`]：声明的效果、解析后的写目标、读路径
//!    与 argv。模型给的路径就是在这里碰上会话 cwd 的，所以读文件系统的也是这里；下游的一切都
//!    只是这些值上的函数。
//! 2. [`CallFacts::guardrails`] 拒掉解析不了的目标，以及一次路径还没被读过就要写的写，并返回
//!    这次调用挣到的读权限路径。它查读集合与目标是否存在，从不查注册表。
//! 3. [`Registry::dispatch`] 跑这次已决策的调用，握着共用的逐路径写锁，并报告一次失败的匹配有
//!    没有收回某条路径的读权限。
//!
//! 把第 1 步的决策施加到发起调用的那个 agent 身上的是循环 —— 这也正是只有 `agent` 模块写事件
//! 流的原因。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::context::repo_map::RepoMapInput;
use crate::context::skills::Skills;
use crate::permissions::{Escalation, PathError};
use crate::provider::ToolSpec;
use crate::questions::UserQuestions;

use super::paths::{PathLocks, SessionPaths};
use super::sandbox::{self, Sandbox};
use super::tool::{
    BashLimits, Effect, ExecutorSpawner, ReadPathResolver, ReadSet, Tool, ToolContext, ToolError,
    ToolOutput, WritePathResolver,
};

/// 一次「改前先读」拒绝开头的那段文本。
///
/// 拒绝被记录下来的地方只有结果本身 —— 没有给它留字段 —— 所以产生它的地方与统计它的可观测
/// 查询共用这一个常量，也就是编辑匹配等级遵循的同一条「约定文本」规矩（spec §18）。前缀一漂，
/// 计数就会悄悄变成零。
pub const READ_BEFORE_WRITE_PREFIX: &str = "改前先读：";

/// 某一个会话的工具表。
#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 挂上一个工具。运行时注册正是动态工具不必再开第二个注册表就能存在的原因。
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.spec().name, Arc::from(tool));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(Arc::as_ref)
    }

    /// 一个 spawn 出来的执行者拿到的表（spec §16）：同一批工具，减去那些不可委派的。
    ///
    /// 是一个新值、不是一个视图，因为表正是会话派发进去的那件东西，也是请求里 `tools` 数组
    /// 据以构造的那件东西。它是已组装工具表的一个纯函数，所以每个执行者看到的声明顺序都一样，
    /// 前缀缓存也就能一直命中。
    pub fn for_executor(&self) -> Registry {
        let tools = self
            .tools
            .iter()
            .filter(|(_, tool)| tool.delegable())
            .map(|(name, tool)| (name.clone(), Arc::clone(tool)))
            .collect();
        Registry { tools }
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// 每一条声明，按稳定的名字顺序，就是 provider 想要的样子。
    ///
    /// 稳定顺序要紧：工具数组是缓存前缀的一部分。
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    /// 把一次调用解析成权限门与护栏都要读的那些事实：声明的效果、解析后的写目标、解析后的读
    /// 路径，以及一个命令类工具将要跑的 argv。
    ///
    /// 路径解析是唯一不纯的部分（它做 canonicalize），所以它发生在这里、只发生一次，下游的门
    /// 也仍是这些值上的纯函数。解析不了的目标保留它的字面形式，并报在
    /// [`CallFacts::path_error`] 里：门仍然看得到这次调用，并按路径上限拒掉它，而不是记下一条
    /// 这次调用根本用不上的裁决。
    pub fn facts(
        &self,
        tool_name: &str,
        args: &Value,
        paths: &SessionPaths,
        home: Option<&Path>,
    ) -> Result<CallFacts, ToolError> {
        let Some(tool) = self.get(tool_name) else {
            return Err(ToolError::message(format!("没有注册的工具：{tool_name}")));
        };

        let effect = tool.effect(args);
        let mut write_targets = Vec::new();
        let mut path_error: Option<PathError> = None;
        if let Effect::WritePaths(inputs) = &effect {
            for input in inputs {
                match paths.resolve_write(input) {
                    Ok(path) => write_targets.push(path),
                    Err(error) => {
                        path_error.get_or_insert(PathError::write(error));
                        write_targets.push(paths.unresolved(input));
                    }
                }
            }
        }
        // 按路径排序，这样两次多路径的写不会互相死锁。
        write_targets.sort();
        write_targets.dedup();

        // 读集合的候选。循环只在这次调用成功时才记录它们：一次失败的读不能给后面的写发许可。
        // 工作区解析不了的读，与写的一样，是路径上限上的拒绝 —— 不同的是它带的方向，门据此
        // 给 `outside_read` 那一条裁决。
        let mut read_paths = Vec::new();
        for path in tool.read_paths(args) {
            match paths.resolve_read(&path) {
                Ok(resolved) => read_paths.push(resolved),
                Err(error) => {
                    path_error.get_or_insert(PathError::read(error));
                }
            }
        }

        // 升级申请里的路径在这里变成绝对路径：它由模型写，可能是 `~` 或相对路径，而门要拿它
        // 与遮罩目录比、沙箱要拿它挂可写根，两处必须是同一批字符串。
        let escalation = tool.escalation(args)?.map(|raw| Escalation {
            justification: raw.justification,
            writable_paths: raw
                .writable_paths
                .iter()
                .map(|path| sandbox::escalation_path(path, paths.cwd(), home))
                .collect(),
        });

        Ok(CallFacts {
            tool_name: tool_name.to_owned(),
            effect,
            write_targets,
            read_paths,
            argv: tool.command(args),
            path_error,
            escalation,
        })
    }

    /// 跑一次护栏已经施加过的调用。
    ///
    /// 工具是重新查出、而不是借出来的，这样调用方可以在别处可变地借用它自己的读集合时，仍握着这
    /// 个决策。
    pub async fn dispatch(&self, call: &PendingCall, allowed: &AllowedCall) -> DispatchOutcome {
        let Some(tool) = self.get(&call.tool_name) else {
            return DispatchOutcome::failure(
                ToolError::message(format!("没有注册的工具：{}", call.tool_name)),
                false,
            );
        };

        // 先取工作区锁，再取路径锁，两者都握完整次调用。`Exclusive` 是唯一会取工作区锁的效果。
        let mut guards = Vec::new();
        if allowed.exclusive {
            guards.push(call.locks.lock_exclusive().await);
        }
        for path in &allowed.write_targets {
            guards.push(call.locks.lock(path).await);
        }

        // 这一次调用额外放开的可写根：升级批准的那批路径（如果有）。`Sandbox` 是每次调用
        // 现构造的值，所以这批路径只活这一次调用；门已经把它们判过一遍，写死的边界在那之前
        // 就被拒了。
        let sandbox = call.sandbox.with_grants(&allowed.sandbox_grants);
        let ctx = ToolContext {
            read_paths: &call.paths,
            write_paths: &call.paths,
            outputs_dir: &call.outputs_dir,
            cwd: call.paths.cwd(),
            skills: &call.skills,
            repo_map: &call.repo_map,
            bash: &call.bash,
            sandbox: &sandbox,
            executor: call.executor.as_deref(),
            questions: call.questions.as_deref(),
            tool_call_id: &call.tool_call_id,
            args: &call.args,
        };
        let result = tool.call(&ctx, call.args.clone()).await;
        drop(guards);

        match result {
            Ok(output) => DispatchOutcome::success(output),
            Err(error) => {
                // 对这次调用正要写的某条路径匹配失败，意味着 agent 对它的图景已经陈旧。
                let invalidated = !allowed.write_targets.is_empty() && error.invalidates_reads();
                DispatchOutcome::failure(error, invalidated)
            }
        }
    }
}

/// 一次调用的护栏裁决。
#[derive(Debug, Clone)]
pub enum GuardedCall {
    /// 这次调用可以带着这些路径跑。
    Run(AllowedCall),
    /// 这次调用根本到不了工具；那个错误就是必须交出的结果。
    Refused(ToolError),
}

/// 一次被允许的调用可以碰什么，都已经对着会话 cwd 解析过。
#[derive(Debug, Clone, Default)]
pub struct AllowedCall {
    /// 解析后的写目标，握整次调用。
    pub write_targets: Vec<PathBuf>,
    /// 解析后的读，**这次调用成功时**才记进 agent 的读集合。
    pub read_paths: Vec<PathBuf>,
    /// 这次调用索要工作区级的锁时为真。
    pub exclusive: bool,
    /// 这一次调用额外放开的沙箱可写根 —— 升级批准的那批路径（`.scratch/workspace-mode`
    /// 的 spec §4）。缺省为空：没有升级就没有额外的东西。
    pub sandbox_grants: Vec<PathBuf>,
}

/// 一次解析完的调用：权限门与护栏要读的一切。
///
/// 由 [`Registry::facts`] 构造 —— 模型给的路径在那里碰上会话 cwd，不再只是字符串。
#[derive(Debug, Clone)]
pub struct CallFacts {
    /// 模型要的那个工具名。
    pub tool_name: String,
    /// 工具声明的那个工作区效果。
    pub effect: Effect,
    /// 解析后的绝对写目标（效果不是 `WritePaths` 时为空）。解析不了的目标保留它的字面形式。
    pub write_targets: Vec<PathBuf>,
    /// 解析后的绝对读路径。
    pub read_paths: Vec<PathBuf>,
    /// 一个命令类工具将要跑的 argv，当它跑命令时。
    pub argv: Option<Vec<String>>,
    /// 第一个没能对着会话 cwd 解析的目标，带着它是读还是写。门按方向给裁决（读看
    /// `outside_read`、写看档位），而万一日后绕过了门，护栏会拒掉它。
    pub path_error: Option<PathError>,
    /// 这一次调用带上的升级申请 —— 当它带了一个，且路径已经解析成绝对路径。
    pub escalation: Option<Escalation>,
}

impl CallFacts {
    /// 共享护栏：先是收容，于是解析不了的目标永远到不了工具；然后是改前先读 —— 对每个调用者
    /// 都强制，而不是靠约定。
    ///
    /// 只有本来就存在的目标才需要先读过：覆写一个 agent 没看过的文件，正是这里要防的失败，
    /// 而新建一个目标既没有东西可覆盖、也没有东西可读。它查读集合与每个目标是否存在；不需要
    /// 注册表。
    pub fn guardrails(&self, read_set: &ReadSet) -> GuardedCall {
        if let Some(error) = &self.path_error {
            return GuardedCall::Refused(error.error.clone());
        }

        if let Some(path) = self
            .write_targets
            .iter()
            .find(|path| path.exists() && !read_set.contains(path))
        {
            return GuardedCall::Refused(ToolError::message(format!(
                "{READ_BEFORE_WRITE_PREFIX}{} 已存在，但本次会话里还没读过它；先读它一遍再改",
                path.display()
            )));
        }

        GuardedCall::Run(AllowedCall {
            write_targets: self.write_targets.clone(),
            read_paths: self.read_paths.clone(),
            exclusive: matches!(self.effect, Effect::Exclusive),
            sandbox_grants: Vec::new(),
        })
    }
}

/// 派发器需要知道的、关于一次循环已经记录下来的调用的全部东西。
#[derive(Clone)]
pub struct PendingCall {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: Value,
    pub outputs_dir: PathBuf,
    pub paths: SessionPaths,
    pub locks: PathLocks,
    /// 会话已发现的技能库。像路径表一样按句柄克隆，所以需要它的工具不必伸手进会话。
    pub skills: Arc<Skills>,
    /// `repo_map` 工具的会话输入（spec §9）。持有所有权，因为排序上下文是每次调用从事件流
    /// 重算的，而不是共享的。
    pub repo_map: RepoMapInput,
    /// 一次 `bash` 调用跑在其下的墙钟上限（spec §7）。持有所有权：它是一对 `Copy` 数字，而
    /// 每次调用现构造它，就把配置挡在注册表之外。
    pub bash: BashLimits,
    /// 这一次调用跑在哪个沙箱里（沙箱 spec §7）。与 `bash` 同一形状：从会话配置里现构造出来的
    /// 值，而不是一个够得着会话的句柄。
    pub sandbox: Sandbox,
    /// 跑一个嵌套执行者的端口，给 `task` 调用（spec §16）。由循环每次调用现构造 —— 循环才是
    /// 知道一个执行者需要哪个 provider 与哪个渲染器的那一层。
    pub executor: Option<Arc<dyn ExecutorSpawner>>,
    /// 把模型发起的提问交给用户的端口，给 `ask_user_question` 调用（spec §7）。是会话的那一
    /// 个，像技能一样按句柄克隆：作答的不是循环，所以它只携带这个端口。
    pub questions: Option<Arc<dyn UserQuestions>>,
}

/// 手写的，因为这个端口是个不透明的句柄：关于它，一条诊断能说的有用的话只有「挂没挂」。
impl std::fmt::Debug for PendingCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingCall")
            .field("tool_call_id", &self.tool_call_id)
            .field("tool_name", &self.tool_name)
            .field("args", &self.args)
            .field("outputs_dir", &self.outputs_dir)
            .field("paths", &self.paths)
            .field("locks", &self.locks)
            .field("skills", &self.skills)
            .field("repo_map", &self.repo_map)
            .field("bash", &self.bash)
            .field("sandbox", &self.sandbox)
            .field("executor", &self.executor.is_some())
            .field("questions", &self.questions.is_some())
            .finish()
    }
}

/// 一次派发产出了什么。
#[derive(Debug)]
pub struct DispatchOutcome {
    pub result: Result<ToolOutput, ToolError>,
    /// 这次失败有没有收回某条路径的读权限。
    pub invalidated_reads: bool,
}

impl DispatchOutcome {
    pub fn success(output: ToolOutput) -> Self {
        Self {
            result: Ok(output),
            invalidated_reads: false,
        }
    }

    pub fn failure(error: ToolError, invalidated_reads: bool) -> Self {
        Self {
            result: Err(error),
            invalidated_reads,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.result.is_ok()
    }
}
