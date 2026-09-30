//! 工具边界（spec §7）：`Tool` trait、运行时注册表、编辑匹配阶梯，以及那个强制执行共享
//! 护栏的派发点。
//!
//! 注册表是会话携带的运行时值（绝不是全局静态），所以动态工具有了挂载点，而工具表在
//! 前缀缓存的整个生命周期里保持不变。
//!
//! 依赖形状：`edit` 与 `paths` 不依赖这里的任何东西，`bash` 只依赖 `tool`（并 spawn 一个
//! 进程），`file` 依赖 `edit`、`tool` 与 `paths`，`registry` 坐在它们之上。`tools` 里没有
//! 任何东西写事件或读环境；`bash` 把命令交给一个继承调用者环境的 shell —— 那是这个边界
//! 里唯一伸手到进程之外的地方。

pub mod ask_user;
pub mod bash;
pub mod custom;
pub mod edit;
pub mod file;
pub mod paths;
pub mod process;
pub mod registry;
pub mod repo_map;
pub mod sandbox;
pub mod skill;
pub mod task;
pub mod todo;
pub mod tool;

pub use ask_user::{AskUserQuestionTool, ASK_USER_QUESTION_TOOL};
pub use bash::{BashTool, BASH_TOOL};
pub use custom::{is_custom_tool, CustomTool};
pub use file::{
    before_artifact, EditCall, EditFile, ReadFile, WriteFile, EDIT_FILE, MATCH_LEVEL_PREFIX,
    READ_FILE, WRITE_FILE, WROTE_PATH_PREFIX,
};
pub use paths::{write_owner_only, PathLocks, SessionPaths};
pub use process::{CommandOutcome, EXIT_CODE_PREFIX, STDERR_HEADER, STDOUT_HEADER, TIMEOUT_PREFIX};
pub use registry::{
    AllowedCall, CallFacts, DispatchOutcome, GuardedCall, PendingCall, Registry,
    READ_BEFORE_WRITE_PREFIX,
};
pub use repo_map::RepoMapTool;
pub use sandbox::Sandbox;
pub use skill::SkillTool;
pub use task::{TaskTool, TASK_TOOL};
pub use todo::{TodoTool, TODO_TOOL};
pub use tool::{
    BashLimits, Effect, ExecutorSpawner, ReadPathResolver, ReadSet, Tool, ToolContext, ToolError,
    ToolOutput, WritePathResolver,
};

/// v1 的内建工具。
///
/// `skill` 与 `repo_map` 是包在会话携带值外面的一层无状态壳：`skill` 经 [`ToolContext`] 读
/// 已发现的技能库，`repo_map` 用同样的方式读会话的排序上下文。`task` 是同一个形状 ——
/// 包在循环注入的执行者端口外面的无状态壳 —— 也是执行者的工具表里唯一拿不到的那个工具
/// （spec §16）。`bash` 同样无状态：它的两条上限和仓库地图的预算一样，经 [`ToolContext`]
/// 递进来。
///
/// `can_ask` 是组装期的事实「这个会话有提问端口」（spec §7、§19）。它是个参数，而不是
/// `ask_user_question` 在调用时自己去发现的东西，因为 headless 会话根本不该声明这个工具：
/// 给模型一个注定失败的调用等于白费一次调用。这与「让 `task` 不进执行者的工具表」是同一个
/// 「由工具表决定」的机制。
///
/// `todo` 刻意**不**放在 `can_ask` 后面：维护一份列表不需要有人作答，所以三个渲染器都挂它
/// —— 这正是它与 `ask_user_question` 的分界（`.scratch/todo-and-modes/spec.md` §2）。
pub fn builtin(can_ask: bool) -> Registry {
    let mut registry = Registry::new();
    registry.register(Box::new(ReadFile));
    registry.register(Box::new(WriteFile));
    registry.register(Box::new(EditFile));
    registry.register(Box::new(BashTool));
    registry.register(Box::new(SkillTool));
    registry.register(Box::new(RepoMapTool::new()));
    registry.register(Box::new(TaskTool));
    registry.register(Box::new(TodoTool));
    if can_ask {
        registry.register(Box::new(AskUserQuestionTool));
    }
    registry
}

/// 内建工具表，加上每一个动态声明的工具（spec §14）。
///
/// 这里是工具表的组装点：一条声明只在此处、不在别处变成一个看起来普普通通的工具，而之后
/// 工具表不再变化 —— 工具数组是缓存前缀的一部分（spec §14）。`can_ask` 径直传给
/// [`builtin`]，所以「模型是否拿得到 `ask_user_question`」就在建表的那一处决定。
pub fn with_dynamic(declarations: &[crate::config::ToolDeclaration], can_ask: bool) -> Registry {
    let mut registry = builtin(can_ask);
    for declaration in declarations {
        registry.register(Box::new(CustomTool::new(declaration.clone())));
    }
    registry
}
