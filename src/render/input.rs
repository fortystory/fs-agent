//! 循环与占着终端的那一侧前端之间的键盘接缝（spec §19，用户故事 133）。
//!
//! 两件事塑出了它：
//!
//! * **输入归渲染器。** TUI 模式下终端处在 raw 模式，一个 task 占着每一个键；plain
//!   模式下由一个读取 task 占着 stdin。无论哪种，循环都从不直接碰终端。
//! * **循环按需索取。** 只有在循环准备好要一行时才读一行，只有在门已经问了问题时才读
//!   一个答案。一个提前读的读取者会把权限问题的答案当成下一个提示吞掉 —— 所以这里是
//!   请求驱动的，不是一条流。
//!
//! 循环那一侧是 [`ConsoleHandle`]；前端那一侧是 [`ConsolePort`]。[`ConsoleAsker`] 在
//! 同一个 handle 上实现权限门的 [`Asker`] 接缝，所以一个问题与一条提示走的是同一个键盘。

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::events::Event;
use crate::permissions::{Answer, Asker, PermissionRequest};
use crate::questions::{UserAnswer, UserAnswers, UserQuestion, UserQuestions};

/// 一个问题，加上它的答案回来时走的那条一次性通道。
///
/// 这个问题是 [`PermissionRequest`] 而不是它自己的一个枚举：门的 `Ask` 是循环唯一会通过
/// 这条通道摆到用户面前的东西，而它曾经有过第二个变体，纯粹是为了携带那个已经不存在的
/// 模式的计划文件冲突。
#[derive(Debug)]
pub struct AskRequest {
    pub request: PermissionRequest,
    pub reply: oneshot::Sender<Answer>,
}

/// 一份由模型发起、前端必须摆到用户面前的问卷（spec §7）。
///
/// 它有自己的回复通道，而不是第二份 [`AskRequest`] 的形状，这是刻意的：问卷的答案不是
/// 一个 [`Answer`]，而让门去携带一个它永远产不出的形状，正是第三个 asker 存在起来要避开
/// 的事（spec §19）。
#[derive(Debug)]
pub struct QuestionnaireRequest {
    pub questions: Vec<UserQuestion>,
    /// 那些答案，或者一条模型可读的「没有答案」的理由（输入结束了，或这次运行被取消
    /// 了）。丢掉 sender 被读作同一个「没有答案」，于是一次被取消的运行永远不会把工具挂
    /// 在那里。
    pub reply: oneshot::Sender<Result<UserAnswers, String>>,
}

/// 一次「从这份清单里挑一个」的问话（spec §9）。
///
/// 与 [`Ask`](ConsoleRequest::Ask) 同形的**一问一答**通道，只是答案是一个下标而不是一个许可。
/// 走自己的那一格而不是复用问卷：问卷是**模型发起**的（答完它那条工具调用就结束了），选择
/// 器是**人发起**的会话属性（答完回循环，改这一场会话用什么模型）。这两件事的生命周期不同，
/// 共用一条通道会让其中一个悬着等人回答。
#[derive(Debug)]
pub struct PickerRequest {
    /// 「模型」/「思考强度」。
    pub title: String,
    pub options: Vec<PickerOption>,
    /// 选中的那一行的下标，`None` = 取消（`Esc` 或框外点击）。
    pub reply: oneshot::Sender<Option<usize>>,
}

/// 清单里的一行。
///
/// `detail` 是**为什么**这一行长这样（挂在哪个 profile、缺哪个环境变量），而不是装饰 —— 它
/// 回答的是「我点了它会发生什么」。`enabled: false` 的行画成灰的且点了不响应：缺密钥的模型
/// 切过去组不出 provider，与其给一次请求时的失败，不如在清单上就说明白。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerOption {
    pub label: String,
    pub detail: String,
    /// 当前这一场会话用的就是它。
    pub current: bool,
    pub enabled: bool,
}

/// 菜单里一个 `/` 名字的**来处**：三样东西共用一个菜单，而它们该看得出是三类。
///
/// 它只影响名字的颜色（颜色住在 `/` 菜单那一节的调色板里）—— 分派仍旧只看名字，与类别无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogKind {
    /// 程序自带的命令。
    Command,
    /// 会话里装着的技能。
    Skill,
    /// 某个 MCP server 的提示词模板。
    Template,
}

/// 开头的 `/` 能变成的一个名字，以及一句说它是干什么的话。
///
/// 这份目录是**循环的**列表，不是渲染器的：把一次提交变成动作的是循环，所以知道存在哪些
/// 名字的也是循环。前端只负责把它们摆出来 —— 它从不决定其中一个是什么意思（spec §6）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// 不带斜杠的名字，与必须打进去的样子一模一样。
    pub name: String,
    /// 菜单第二列的一句话。没什么可说时是空的。
    pub description: String,
    /// 这个名字从哪儿来：命令、技能，还是 MCP 模板（票 09）。
    pub kind: CatalogKind,
}

impl CatalogEntry {
    /// 一个内建命令（`wording::BUILT_IN_COMMANDS` 里的那批）。
    pub fn command(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            kind: CatalogKind::Command,
        }
    }

    /// 一个会话里装着的技能。
    pub fn skill(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            kind: CatalogKind::Skill,
        }
    }

    /// 一个 MCP server 的提示词模板（`name` 已经是 `server:prompt` 的形状）。
    pub fn template(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            kind: CatalogKind::Template,
        }
    }
}

/// 循环从前端要的东西。
#[derive(Debug)]
pub enum ConsoleRequest {
    /// 下一行用户输入。
    ///
    /// **`None` 就是输入结束**，没别的意思：一个空行是 `Some(String::new())`。一个输入
    /// 不会结束的前端 —— TUI，在空草稿上按 Enter 只是又一个空草稿 —— 绝不能发 `None`，
    /// 否则循环会把它读成 stdin 关掉了，于是停下（spec §6）。
    Prompt {
        reply: oneshot::Sender<Option<String>>,
    },
    /// 把一个问题摆到用户面前。
    Ask(AskRequest),
    /// 把模型的问卷摆到用户面前（spec §7）。它的答案类型与 [`Ask`](Self::Ask) 的不同，
    /// 所以走自己的通道。
    Questionnaire(QuestionnaireRequest),
    /// 把一份清单摆到用户面前，让他挑一个（spec §9）。答 `None` 就是取消。
    Picker(PickerRequest),
    /// 换完模型/档位之后循环推回来的新事实（spec §3）。
    ///
    /// 这是**发完就完**的通知，与 [`Muted`](Self::Muted) / [`RunState`](Self::RunState) 同一格：
    /// 循环推、前端收，没有答案要等。之所以要推，是因为这三样是注入的只读值 —— 换了模型之后
    /// 上下文窗口可能变（1M ↔ 256K），而它是状态行那个 `n%` 的分母，前端自己算不出来。
    SessionUpdate {
        model: String,
        effort: Option<crate::config::ReasoningEffort>,
        context_window: u64,
        /// 新的发言者名册（spec §4）：名字是 provider profile 的名字，换厂商就换名。
        speakers: Vec<String>,
    },
    /// 开头的 `/` 能变成哪些名字。
    ///
    /// 组装之后立刻推一次，因为技能来自会话，没有什么能更早把它们列出来。一个不画菜单的
    /// 前端 —— plain 那条控制台 —— 与它无关。
    Catalog { entries: Vec<CatalogEntry> },
    /// 输入区是不是禁言了（`.scratch/goal-loop/spec.md` §5）。
    ///
    /// 无人值守的目标循环跑着的时候，打字插话不是这个功能的一部分（那是
    /// `.scratch/interjection-flow` 那条种子）：键位照旧响应，只是不落字。知道这件事的只有
    /// 循环，所以它自己说。
    Muted { muted: bool },
    /// 循环是不是**在一次运行里面** —— 一个回合，或它正在驱动的一场讨论。
    ///
    /// 知道这件事的只有循环，所以它自己说，而不是让前端去推断（spec §6）。从渲染流上推
    /// 断，对不是回合的任何东西都失效 —— 合成器那次单一调用会吐增量，却不结束任何
    /// `TurnEnded`；从「没有未决的提示」推断，在启动时失效，那时循环还没要过它的第一行。
    /// 两个错都会把 `Ctrl-C` 变成空闲循环直接丢掉的手势，读起来就是一个死掉的键盘。
    RunState { running: bool },
    /// 重新打开的会话开局带着的事件流，供前端铺成历史
    /// （`.scratch/tui-history-replay/spec.md` §1）。
    ///
    /// 组装之后、启动横幅之前推一次，因为载荷是**组装后**的快照 —— 它包含 `--continue`
    /// 的恢复为悬空工具调用写下的合成结果，所以不能在组装前取。一个不养转录的前端 ——
    /// plain 那条控制台 —— 与它无关。
    Replay { events: Vec<Event> },
}

/// 一个手势，由前端按自己的节奏推上来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontEndEvent {
    /// 用户打断了这次运行（Esc / Ctrl-C）。
    Cancel,
    /// Shift+Tab：循环权限模式 `readonly → ask → auto → readonly`（spec §12；
    /// `.scratch/todo-and-modes/spec.md` §1）。它是会话上的一个值，永远不是事件：循环应用
    /// 它，什么都不注入。
    CycleMode,
    /// 用户要求离开。
    Quit,
    /// 点状态行那两格，或按 `ctrl-t`：打开模型/档位的选择器（spec §7）。
    ///
    /// **只带载荷，不带实现**：前端不知道现在忙不忙（渲染器不知道什么在跑），所以它只是说
    /// 「有人要开这个」。判忙闲、发清单、真的切换都在循环那一侧。
    OpenPicker(PickerKind),
}

/// 要开哪一份清单。`Copy` 是刻意的：加上它不破坏 [`FrontEndEvent`] 的 `Copy`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    Model,
    Effort,
}

/// 键盘上属于循环的那一端。
///
/// 请求发送端与手势接收端是**两个值**，不是一个：循环同时 select 一条提示与一个没人要的
/// 手势，而合成一个结构体会被借两次。
pub struct ConsoleHandle {
    requests: mpsc::UnboundedSender<ConsoleRequest>,
}

impl ConsoleHandle {
    /// 读下一行用户输入；输入结束时是 `None`。
    pub async fn prompt(&self) -> Option<String> {
        let (reply, answer) = oneshot::channel();
        self.requests.send(ConsoleRequest::Prompt { reply }).ok()?;
        answer.await.ok().flatten()
    }

    /// 告诉前端存在哪些 `/<name>`。
    ///
    /// 发完就完：一个已经走了的前端不是错误，也没有什么答案要等。内建项排在列表最前，然后
    /// 是会话的技能，所以菜单读起来的顺序就是循环会尝试它们的顺序。
    pub fn catalog(&self, entries: Vec<CatalogEntry>) {
        let _ = self.requests.send(ConsoleRequest::Catalog { entries });
    }

    /// 说循环是不是在一次运行里面。
    ///
    /// 与 [`catalog`](Self::catalog) 一样发完就完：这个事实是一则通知，不是一个问题，而
    /// 一个已经走了的前端不是错误。它必须被*推*过去 —— 知道一次运行何时开始何时结束的只有
    /// 循环，也没有哪条侧通道（渲染流、一个未决的提示）对每一种运行都说得出来。
    pub fn set_running(&self, running: bool) {
        let _ = self.requests.send(ConsoleRequest::RunState { running });
    }

    /// 说输入区是不是禁言了。
    ///
    /// 与 [`set_running`](Self::set_running) 同一个形状、同一个理由：知道目标循环何时开始与
    /// 结束的只有循环。不想落字的那个前端忽略它。
    pub fn set_muted(&self, muted: bool) {
        let _ = self.requests.send(ConsoleRequest::Muted { muted });
    }

    /// 把重新打开的会话组装出来的历史交给前端。
    ///
    /// 与 [`catalog`](Self::catalog) 一样发完就完：把历史铺开是前端自己的事，也没有什么
    /// 答案要等。载荷是按 `seq` 序的**整条**组装好的流；空的一条意味着没东西可重放，前端
    /// 照它一如既往的样子行事。
    pub fn replay(&self, events: Vec<Event>) {
        let _ = self.requests.send(ConsoleRequest::Replay { events });
    }

    /// 把一份清单摆到用户面前并等一个下标（`None` = 取消）。
    ///
    /// 候选由**循环**算好送过来（spec §8）：渲染器从不伸手去够配置，而「哪些模型可用」要读
    /// `Config`、`caps_for` 与环境变量。前端只画拿到的清单。
    ///
    /// 一个已经走了的前端 —— 或者根本不支持选择器的那条路（plain）—— 读作 `None`，也就是
    /// 取消，而不是一次挂住的等待。
    pub async fn picker(
        &self,
        title: impl Into<String>,
        options: Vec<PickerOption>,
    ) -> Option<usize> {
        let (reply, answer) = oneshot::channel();
        if self
            .requests
            .send(ConsoleRequest::Picker(PickerRequest {
                title: title.into(),
                options,
                reply,
            }))
            .is_err()
        {
            return None;
        }
        answer.await.ok().flatten()
    }

    /// 把换完模型/档位之后的新事实推给前端（spec §3）。发完就完。
    pub fn session_update(
        &self,
        model: String,
        effort: Option<crate::config::ReasoningEffort>,
        context_window: u64,
        speakers: Vec<String>,
    ) {
        let _ = self.requests.send(ConsoleRequest::SessionUpdate {
            model,
            effort,
            context_window,
            speakers,
        });
    }
}

/// 前端按自己的节奏推上来的那些手势。
pub struct ConsoleEvents {
    events: mpsc::UnboundedReceiver<FrontEndEvent>,
}

impl ConsoleEvents {
    /// 下一个手势；前端走了之后是 `None`。
    pub async fn recv(&mut self) -> Option<FrontEndEvent> {
        self.events.recv().await
    }
}

/// 键盘上属于前端的那一端。只该有一个 task 持有它。
pub struct ConsolePort {
    requests: mpsc::UnboundedReceiver<ConsoleRequest>,
    events: mpsc::UnboundedSender<FrontEndEvent>,
}

impl ConsolePort {
    /// 等循环下一个想要的东西。
    pub async fn recv(&mut self) -> Option<ConsoleRequest> {
        self.requests.recv().await
    }

    /// 推一个循环没要过的手势（Esc、Shift+Tab、Ctrl-C）。
    pub fn emit(&self, event: FrontEndEvent) {
        let _ = self.events.send(event);
    }
}

/// 创建这一对。handle 与手势接收端给循环，端口给前端。
pub fn console() -> (ConsoleHandle, ConsolePort, ConsoleEvents) {
    let (request_tx, request_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    (
        ConsoleHandle {
            requests: request_tx,
        },
        ConsolePort {
            requests: request_rx,
            events: event_tx,
        },
        ConsoleEvents { events: event_rx },
    )
}

/// 权限门的那个接缝，经过前端作答（spec §12）。
///
/// 有一个问题从这里走 —— 门的 `Ask` —— 走的正是这个前端已经占着的那一个键盘。
pub struct ConsoleAsker {
    requests: mpsc::UnboundedSender<ConsoleRequest>,
}

impl ConsoleAsker {
    pub fn new(requests: mpsc::UnboundedSender<ConsoleRequest>) -> Self {
        Self { requests }
    }

    /// 从一个 handle 造一个 asker，好让 CLI 把同一个键盘接进循环与权限门两处。
    pub fn from_handle(handle: &ConsoleHandle) -> Self {
        Self::new(handle.requests.clone())
    }

    async fn put(&self, request: PermissionRequest) -> Answer {
        let (reply, answer) = oneshot::channel();
        if self
            .requests
            .send(ConsoleRequest::Ask(AskRequest { request, reply }))
            .is_err()
        {
            return Answer::Deny;
        }
        answer.await.unwrap_or(Answer::Deny)
    }
}

#[async_trait]
impl Asker for ConsoleAsker {
    async fn ask(&self, request: &PermissionRequest) -> Answer {
        self.put(request.clone()).await
    }
}

/// 模型提问的那个接缝，经过前端作答（spec §7）。
///
/// 它是 [`ConsoleAsker`] 在模型的问题上的镜像：循环那一侧问，前端答。错误文本是**模型
/// 可见**的 —— 它成为 `ask_user_question` 那次调用的结果 —— 所以它按 ADR 0005 走中文，
/// 并且刻意不走措辞层。
pub struct ConsoleQuestions {
    requests: mpsc::UnboundedSender<ConsoleRequest>,
}

impl ConsoleQuestions {
    pub fn new(requests: mpsc::UnboundedSender<ConsoleRequest>) -> Self {
        Self { requests }
    }

    /// 从一个 handle 造这个端口，好让 CLI 把同一个键盘接进循环与模型的问题两处。
    pub fn from_handle(handle: &ConsoleHandle) -> Self {
        Self::new(handle.requests.clone())
    }
}

#[async_trait]
impl UserQuestions for ConsoleQuestions {
    async fn ask(&self, questions: &[UserQuestion]) -> Result<UserAnswers, String> {
        let (reply, answer) = oneshot::channel();
        if self
            .requests
            .send(ConsoleRequest::Questionnaire(QuestionnaireRequest {
                questions: questions.to_vec(),
                reply,
            }))
            .is_err()
        {
            return Err("没有接上任何问卷作答者".to_owned());
        }
        match answer.await {
            Ok(result) => result,
            // 前端没作答就走了 —— 这次运行被取消了，或者输入结束了。永远不要挂在那里等一个
            // 已经不在的人。
            Err(_) => Err("这份问卷没被作答就搁下了".to_owned()),
        }
    }
}

/// 一行输入的来源，用 `None` 表示输入结束。
///
/// 这是 plain 前端唯一的输入原语。它是一个值而不是直接读 `stdin`，这样整条面向行的控制
/// 台 —— 提示、权限问题、问卷 —— 都能在测试里由脚本驱动，就像 TUI 的键盘被一次一个事件
/// 驱动一样。
pub type LineReader = Box<
    dyn FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send>>
        + Send,
>;

/// 为 plain 模式跑起这条面向行的前端。
///
/// stdin 是行缓冲的，所以没有 raw 模式也没有按键事件：端口每来一个请求读一行，无论循环是
/// 要一条提示还是门要一个答案。提示去 stderr，绝不去 stdout —— stdout 只承载最终产物
/// （spec §19）。
pub fn spawn_plain_console(port: ConsolePort) -> tokio::task::JoinHandle<()> {
    spawn_plain_console_with(port, Box::new(|| Box::pin(read_stdin_line())))
}

/// 注入式行读取器版本的 [`spawn_plain_console`]。
///
/// 这个读取器就是这条前端全部的输入面，所以把它注入进来正是让逐行那条路 —— 包括模型的
/// 问卷 —— 不用管道也能测的原因。
pub fn spawn_plain_console_with(
    mut port: ConsolePort,
    mut reader: LineReader,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(request) = port.recv().await {
            match request {
                ConsoleRequest::Prompt { reply } => {
                    let _ = reply.send(next_line(&mut reader, "> ").await);
                }
                ConsoleRequest::Ask(ask) => {
                    let answer = answer_question(&mut reader, &ask.request).await;
                    let _ = ask.reply.send(answer);
                }
                // 模型的问卷，一问一行地答。输入结束时它返回一个错误，而不是永远转下去
                // （spec §19）。
                ConsoleRequest::Questionnaire(request) => {
                    let answers = answer_questionnaire(&mut reader, &request.questions).await;
                    let _ = request.reply.send(answers);
                }
                // 选择器是 TUI 独占的交互：这条路径上没有键盘，于是答案是取消而不是一个
                // 下标（spec「明确不做」）。想在这条路径上换模型就用 `/model <id>`。
                ConsoleRequest::Picker(request) => {
                    let _ = request.reply.send(None);
                }
                // 没有状态行，所以换完模型的新事实无处可去（plain 前端不显示它们）。
                ConsoleRequest::SessionUpdate { .. } => {}
                // 面向行的前端没有菜单：名字靠那句未知命令的文案去发现。
                ConsoleRequest::Catalog { .. } => {}
                // 禁言对这条前端是**构造性**的：它只在循环要一行的时候读，而无人值守的循环
                // 在它跑着的那段时间里从不索要一行（`.scratch/goal-loop/spec.md` §5）。所以
                // 这里不需要做任何事 —— 它本来就不读。
                ConsoleRequest::Muted { .. } => {}
                // 这条前端上没有任何东西读按键事件，所以也没有手势会被读成错误的分支
                // （spec §6）。
                ConsoleRequest::RunState { .. } => {}
                // 没有转录可以铺历史：面向行的前端来一个事件印一个，什么都不留
                // （`.scratch/tui-history-replay/spec.md` §1）。
                ConsoleRequest::Replay { .. } => {}
            }
        }
    })
}

/// 把 `prompt` 写到 stderr 并取下一行。输入结束时是 `None`。
async fn next_line(reader: &mut LineReader, prompt: &str) -> Option<String> {
    use std::io::Write;
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    reader().await
}

/// 从 stdin 读一行。EOF 时是 `None`。
async fn read_stdin_line() -> Option<String> {
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_owned()),
            Err(_) => None,
        }
    })
    .await
    .ok()
    .flatten()
}

/// 把一个权限问题放到终端上并读答案。
///
/// 不是明确 yes 的一律按「不动手」那一侧读：一个打错了的答案绝不能批准一次写。
async fn answer_question(reader: &mut LineReader, request: &PermissionRequest) -> Answer {
    let prompt = crate::render::wording::permission_prompt_with_context(
        &request.tool_name,
        &crate::render::transcript::summarize_args(&request.args),
        &request.reason,
        request.speaker.as_ref(),
        request.escalation.as_ref(),
    );
    match next_line(reader, &prompt).await.as_deref() {
        Some("y") | Some("yes") => Answer::Allow,
        Some("a") | Some("always") => Answer::AlwaysAllow,
        _ => Answer::Deny,
    }
}

/// 把模型的问卷放到终端上，一题一行，并把读到的编码下来（spec §7、§19）。
///
/// 这里没有分页：每个问题依次印出来、读进来，于是 TUI 的「一屏一问」变成「一提示一问」。
/// 空行是跳过；一个数字选一个选项；别的都是自定义文本。带选项的多选问题会再读第二行、可选
/// 的一行，好让 `selected` 与 `custom` 都能被作答 —— 这就是 spec §7 要求的那条补充。输入
/// 结束不是一个答案，所以它让这次调用失败，而不是干等。
async fn answer_questionnaire(
    reader: &mut LineReader,
    questions: &[UserQuestion],
) -> Result<UserAnswers, String> {
    let mut answers = Vec::with_capacity(questions.len());
    for (index, question) in questions.iter().enumerate() {
        print_questionnaire_question(index, questions.len(), question);
        let Some(answer) = read_plain_answer(reader, question).await else {
            return Err("问卷还没答完，输入就结束了".to_owned());
        };
        answers.push(answer);
    }
    Ok(UserAnswers { answers })
}

/// 读一个问题的一个答案；输入结束时是 `None`。
///
/// 带选项的多选问题要**两**行：先是一串数字（或自定义文本），然后是一条可选的补充。那
/// 第二行是面向行的前端唯一能说出「选这个，外加这段文字」的办法（spec §7）；其余每个问题
/// 都只要一行，这样常见情形就短。
async fn read_plain_answer(reader: &mut LineReader, question: &UserQuestion) -> Option<UserAnswer> {
    if question.multi_select && !question.options.is_empty() {
        let selection = next_line(
            reader,
            crate::render::wording::questionnaire_plain_options_prompt(true),
        )
        .await?;
        let supplement = next_line(
            reader,
            crate::render::wording::questionnaire_plain_supplement_prompt(),
        )
        .await?;
        return Some(encode_plain_answer(question, &selection, Some(&supplement)));
    }
    let prompt = if question.options.is_empty() {
        crate::render::wording::questionnaire_plain_answer_prompt()
    } else {
        crate::render::wording::questionnaire_plain_options_prompt(false)
    };
    let line = next_line(reader, prompt).await?;
    Some(encode_plain_answer(question, &line, None))
}

/// 把一个问题与它编了号的选项印到 stderr。
fn print_questionnaire_question(index: usize, total: usize, question: &UserQuestion) {
    if let Some(header) = question
        .header
        .as_deref()
        .map(str::trim)
        .filter(|header| !header.is_empty())
    {
        eprintln!("{header}");
    }
    eprintln!(
        "[{}] {}",
        crate::render::wording::questionnaire_progress(index, total),
        question.question.trim()
    );
    for (at, choice) in question.options.iter().enumerate() {
        eprintln!(
            "  {}",
            crate::render::wording::questionnaire_option(
                at + 1,
                &choice.label,
                choice.description.as_deref(),
            )
        );
    }
}

/// 把 plain 控制台的一个答案编码下来。
///
/// 第一行空白是一次跳过（`selected: []`，没有 `custom`）。一行是合法的选项号列表就选那些
/// 选项；别的都是自定义文本，在单选问题上这意味着选择为空，因为自定义文本覆盖它
/// （spec §7）。`supplement` 是多选问题的第二行：有它就加进自定义文本，于是 `selected`
/// 与 `custom` 一起走。
fn encode_plain_answer(
    question: &UserQuestion,
    line: &str,
    supplement: Option<&str>,
) -> UserAnswer {
    let line = line.trim();
    let supplement = supplement
        .map(str::trim)
        .filter(|supplement| !supplement.is_empty());

    let mut selected = Vec::new();
    let mut custom: Option<String> = None;
    if question.options.is_empty() {
        if !line.is_empty() {
            custom = Some(line.to_owned());
        }
    } else if let Some(chosen) = chosen_options(question, line) {
        selected = chosen;
    } else if !line.is_empty() {
        custom = Some(line.to_owned());
    }
    if let Some(supplement) = supplement {
        custom = Some(match custom {
            Some(first) => format!("{first} {supplement}"),
            None => supplement.to_owned(),
        });
    }
    UserAnswer {
        id: question.id.clone(),
        selected,
        custom,
    }
}

/// 一行选项号点到的那些标签；这一行不是一列合法数字时是 `None`（那它就是自定义文本）。
///
/// 单选问题正好取一个数字；多选取一串逗号分隔的。
fn chosen_options(question: &UserQuestion, line: &str) -> Option<Vec<String>> {
    let mut selected = Vec::new();
    let tokens: Vec<&str> = if question.multi_select {
        line.split(',').collect()
    } else {
        vec![line]
    };
    for token in tokens {
        let number: usize = token.trim().parse().ok()?;
        if number == 0 || number > question.options.len() {
            return None;
        }
        let label = question.options[number - 1].label.clone();
        if !selected.contains(&label) {
            selected.push(label);
        }
    }
    (!selected.is_empty()).then_some(selected)
}
