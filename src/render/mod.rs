//! 渲染边界（spec §19）。
//!
//! 每个进程只有**一个**渲染器，启动时选定，三个实现互斥 —— 绝不是并发的订阅者。三者
//! 消费**同一条**广播通道，那条通道在组装期创建并注入（[`channel`] +
//! [`Renderer::spawn`]）。增量文本与进流的事件走那一条通道，于是两者的相对顺序是定的；
//! 增量文本永不进事件流。
//!
//! 三个实现：
//!
//! * [`headless`] —— 机器模式。它的纯粹性是结构性的：它只往两个显式的写出口写，而
//!   `stdout` 只收最终产物，别的什么都不收（票 01 的回归断言）。
//! * [`plain`] —— 给管道或简单终端看的人类转录。
//! * [`tui`] —— ratatui 界面：alt screen 上一条左栏与一条主列，中间一条竖虚线
//!   （ADR 0002；外面那圈框已经拆掉，`.scratch/tui-chrome/spec.md` §1）。它占着键盘，
//!   并把转录养在自己的滚动缓冲里。
//!
//! plain 与 TUI 共用一个 [`transcript`] 层：同一批事件变成同一批 [`transcript::Block`]，
//! 只有画法不同。就是这样才没变成三个各自重推同一套呈现规则的渲染器。
//!
//! `[speaker]` 前缀是**人**这一侧的前缀。它刻意是投影的模型侧前缀之外的另一个生成器
//! （spec §5）：这一个每行都重复，好让交错的多 agent 日志仍然读得下去，而模型那一侧
//! 每个合并块只写一次。

pub mod editor;
pub mod file_index;
pub mod files;
pub mod headless;
pub mod highlight;
pub mod input;
pub mod layout;
pub mod links;
pub mod markdown;
pub mod opener;
pub mod palette;
pub mod pane;
pub mod panel;
pub mod plain;
pub mod selection;
pub mod severity;
pub mod todo;
pub mod token;
pub mod transcript;
pub mod tui;
pub mod viewer;
pub mod width;
pub mod wording;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::events::{Event, SpeakerId};

pub use headless::Headless;
pub use input::{
    AskRequest, CatalogEntry, CatalogKind, ConsoleAsker, ConsoleEvents, ConsoleHandle, ConsolePort,
    ConsoleQuestions, ConsoleRequest, FrontEndEvent, LineReader, PickerKind, PickerOption,
    PickerRequest, QuestionnaireRequest, console, spawn_plain_console, spawn_plain_console_with,
};
pub use plain::{Plain, PlainOptions};
pub use severity::Severity;
pub use todo::TodoPanel;
pub use transcript::{Block, ToolBlock, ToolOutcome, Transcript};
pub use tui::{
    Key, SessionFacts, SpeakerColors, TOKEN_COMMAND, TOKEN_REFERENCE, Tui, TuiOptions, TuiState,
    draw_frame, paint_frame, render_block, render_block_uncoloured,
};

/// 一个慢消费者开始丢事件之前，能缓冲多少个渲染事件。丢掉一个增量是输出降级，绝不是
/// 正确性降级。
pub const RENDER_CHANNEL_CAPACITY: usize = 1024;

/// 在去渲染器的路上绕过事件流的文本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaKind {
    Text,
    Reasoning,
}

/// 渲染器能观察到的一切：增量模型输出，或一个已落地进事件流的完整单位。
#[derive(Debug, Clone)]
pub enum RenderEvent {
    Delta {
        speaker: SpeakerId,
        kind: DeltaKind,
        text: String,
    },
    Logged(Event),
    /// 只给渲染器看的叙述，不是事件。`at` 是它**发生**的那一刻：它不是事件、没有信封可依，
    /// 所以时刻在源头打上（`.scratch/trace-in-main/spec.md` §5）。
    Diagnostic {
        at: DateTime<Utc>,
        message: String,
    },
    /// 工作区刚被改过：一次非只读的工具调用收尾了，或者一次 `/undo` 落了盘。
    ///
    /// 它是一条**静默信号**，不是给人看的一行：不画进转录、不进事件流 —— 握着会话级文件
    /// 索引的那一端靠它知道该重扫一次（`.scratch/files-page/spec.md` §2）。三个渲染器里
    /// 只有 TUI 养着那份索引，另外两个把它当没看见。
    WorkspaceChanged,
    /// 一行不为任何事件说话的界面文字：启动横幅与交互循环的朴素反馈。
    ///
    /// 不是 [`RenderEvent::Diagnostic`]：诊断是系统在报什么，并且被标成那样，而一条告知
    /// 就是那一行本身。`at` 同理，时间在源头打上。
    Notice {
        at: DateTime<Utc>,
        message: String,
    },
    /// 拼给模型的那段**私有身份**（系统提示词），给读的人看的一条记录。
    ///
    /// 它不是事件、也永远不进事件流（身份在每次请求里现拼、从不落盘 —— `build_messages`，
    /// spec §15）。所以这一条是**按当前代码拼的一份**：`--continue` 重开看到的是今天的拼法，
    /// 不是当时那份。它点得开、看得全，但不进模型上下文、也不写 `log.jsonl`。
    Identity {
        text: String,
    },
}

impl RenderEvent {
    /// 一条诊断，时刻取现在。实时路径与不需要钉死时刻的测试走它。
    pub fn diagnostic(message: impl Into<String>) -> Self {
        Self::Diagnostic {
            at: Utc::now(),
            message: message.into(),
        }
    }

    /// 拼好的系统提示词，给读的人看。没有时刻：它不属于任何一刻，是**当前**的拼法。
    pub fn identity(text: impl Into<String>) -> Self {
        Self::Identity { text: text.into() }
    }

    /// 一条界面告知，时刻取现在。
    pub fn notice(message: impl Into<String>) -> Self {
        Self::Notice {
            at: Utc::now(),
            message: message.into(),
        }
    }
}

/// headless 渲染器的两个显式写出口，组装时注入。
pub struct RenderSinks {
    /// 只接最终产物。
    pub stdout_result: Box<dyn std::io::Write + Send>,
    /// 别的所有东西：进度、诊断、事件叙述。
    pub stderr_diagnostic: Box<dyn std::io::Write + Send>,
}

impl std::fmt::Debug for RenderSinks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RenderSinks")
    }
}

/// 渲染通道的发送端。克隆很便宜。
#[derive(Clone)]
pub struct RenderHandle {
    sender: broadcast::Sender<RenderEvent>,
}

impl RenderHandle {
    pub fn text_delta(&self, speaker: &SpeakerId, text: &str) {
        let _ = self.sender.send(RenderEvent::Delta {
            speaker: speaker.clone(),
            kind: DeltaKind::Text,
            text: text.to_owned(),
        });
    }

    pub fn reasoning_delta(&self, speaker: &SpeakerId, text: &str) {
        let _ = self.sender.send(RenderEvent::Delta {
            speaker: speaker.clone(),
            kind: DeltaKind::Reasoning,
            text: text.to_owned(),
        });
    }

    pub fn logged(&self, event: &Event) {
        let _ = self.sender.send(RenderEvent::Logged(event.clone()));
    }

    /// 一条只给渲染器看的诊断，不是事件。时刻在这里打上 —— 它不是事件，没有信封可依
    /// （`.scratch/trace-in-main/spec.md` §5）。
    pub fn diagnostic(&self, message: &str) {
        let _ = self.sender.send(RenderEvent::diagnostic(message));
    }

    /// 一行原样显示的界面文字。
    ///
    /// 这条接缝存在，是为了让持有 harness 的调用方永远不必自己 print：渲染器一旦占住
    /// 终端，第二个写入者就会落进活动区域里（spec §19）。
    pub fn notice(&self, message: &str) {
        let _ = self.sender.send(RenderEvent::notice(message));
    }

    /// 把拼好的系统提示词摆进转录（轨迹页看得到的那一条；不进事件流）。
    pub fn identity(&self, text: &str) {
        let _ = self.sender.send(RenderEvent::identity(text));
    }

    /// 对前端说一句「工作区变了」：不画一行、不进事件流，只请握着文件索引的那一端重扫一次
    /// （`.scratch/files-page/spec.md` §2）。
    ///
    /// 触发点在工具收尾那一层 —— 判据是既有的 [`crate::tools::Effect`]，所以 `bash` 里的
    /// `mv` / `git checkout` 也算，而不必维护一张工具名单。
    pub fn workspace_changed(&self) {
        let _ = self.sender.send(RenderEvent::WorkspaceChanged);
    }
}

/// 渲染通道的一个消费者。
///
/// 三个实现一次起一个，绝不并排：这个选择*就是* [`Renderer`]，而且在组装时做一次。
/// 一个渲染器拥有它的模式需要的那些资源（机器模式的写出口、TUI 的终端），并在每一个
/// [`RenderHandle`] 都被丢掉、通道排空之后返回。
#[async_trait]
pub trait Render: Send {
    async fn consume(self: Box<Self>, receiver: broadcast::Receiver<RenderEvent>);
}

/// 启动时的渲染器选择：三种模式里恰好一个。
///
/// 是值类型而不是 trait 对象，因为这个选择必须在组装前端的地方做出来，也因为这样
/// 「互斥」就是类型的性质，而不是一条约定。
pub enum Renderer {
    /// 机器模式：两个显式写出口，`stdout` 只承载最终产物。
    Headless(RenderSinks),
    /// 给管道或简单终端看的人类转录。
    Plain(PlainOptions),
    /// ratatui 界面；它占着键盘。
    Tui(Box<TuiOptions>),
}

impl Renderer {
    pub fn headless(sinks: RenderSinks) -> Self {
        Renderer::Headless(sinks)
    }

    pub fn plain(options: PlainOptions) -> Self {
        Renderer::Plain(options)
    }

    pub fn tui(options: TuiOptions) -> Self {
        Renderer::Tui(Box::new(options))
    }

    /// 在 `receiver` 上启动选中的渲染器，那条通道由组装期创建。
    pub fn spawn(self, receiver: broadcast::Receiver<RenderEvent>) -> JoinHandle<()> {
        match self {
            Renderer::Headless(sinks) => {
                let renderer = Box::new(Headless::new(sinks));
                tokio::spawn(async move { renderer.consume(receiver).await })
            }
            Renderer::Plain(options) => {
                let renderer = Box::new(Plain::new(options));
                tokio::spawn(async move { renderer.consume(receiver).await })
            }
            Renderer::Tui(options) => {
                let renderer = Box::new(Tui::new(*options));
                tokio::spawn(async move { renderer.consume(receiver).await })
            }
        }
    }
}

/// 创建那唯一一条渲染通道。组装期持有发送端（作为一个 [`RenderHandle`]）；消费端交给
/// 恰好一个 [`Renderer`]。
pub fn channel() -> (RenderHandle, broadcast::Receiver<RenderEvent>) {
    let (sender, receiver) = broadcast::channel(RENDER_CHANNEL_CAPACITY);
    (RenderHandle { sender }, receiver)
}
