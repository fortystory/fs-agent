//! ratatui 界面（`.scratch/tui-sidebar/spec.md` §1–§2，ADR 0002）。
//!
//! 有三条性质是结构性的，不是风格问题：
//!
//! * **alt screen，一帧。** TUI 画一条全高左栏与一条主列，中间是一条竖虚线；四周没有
//!   外框，终端自己就是边界（`.scratch/tui-chrome/spec.md` §1）。左栏放标记与会话的
//!   读数，主列自上而下堆着转录（右边缘带滚动条与回合条）、状态行、输入区与提示行。
//!   转录住在自己的缓冲里，而不是终端的滚动回退里 —— 内联视口那个漂移的光标也正是
//!   这么消掉的：全屏下窗格原点永远是 `(0, 0)`。
//! * **渲染器占着键盘。** 它是唯一读终端事件的 task，并且通过注入的 console 通道
//!   回答循环的请求（[`ConsoleRequest`]）。输入与输出不打架，靠的就是这一条。
//! * **`select!` 管 broadcast 与按键。** 渲染事件、循环的请求与键盘输入是三个互相
//!   独立的来源；`select!` 是它们合流的方式，不用再引入一条顺序未定义的第二通道。
//!   已经排进队列的在画帧之前先排空，所以一个爆发输出的 provider 花掉的是帧而不是
//!   事件。循环里唯一的定时器是标记的脉冲，而且**只在一次运行进行中的时候**才武装
//!   （`.scratch/tui-input-pulse/spec.md` §2）：空闲的会话仍然只等这三个来源，别的
//!   什么都不等。

use async_trait::async_trait;
use futures::StreamExt;
use ratatui::buffer::CellWidth;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as CtEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{
    enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, SetTitle,
};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block as WidgetBlock, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState,
};
use tokio::sync::broadcast;

use crate::events::{ContextSource, Event, EventPayload, Role, StopReason, ToolCallId};
use crate::permissions::{Answer, Mode, PermissionRequest};
use crate::questions::{UserAnswer, UserAnswers, UserQuestion};

use super::editor::{self, Input};
use super::input::{CatalogEntry, ConsolePort, ConsoleRequest, FrontEndEvent};
use super::layout;
use super::pane::{self, Pane};
use super::panel::Panel;
use super::severity::Severity;
use super::transcript::{summarize_args, Block, ToolBlock, Transcript};
use super::width::{text_columns, truncate_columns};
use super::wording::{self, speaker_label};
use super::{DeltaKind, Render, RenderEvent};

/// 流式文本保留多少才裁成一条尾巴。转录不需要整条消息都活着：完成的 `Message` 块会
/// 把它整段重画一遍。
const LIVE_BUFFER: usize = 4_000;

/// 大于这个值的粘贴要先问一句才收下（spec §7）。
const PASTE_CONFIRM_CHARS: usize = 100_000;

/// 一帧吸收多少个排队中的渲染事件。有界的排空让突发输出不至于把键盘饿掉整整一帧的
/// 工作量。
const DRAIN_LIMIT: usize = 4_096;

/// 一次重放批次应用多少个历史事件。
///
/// 重放一个会话是一帧一帧地追赶，而不是阻塞式加载，所以每一趟只取有界的一片然后画。
/// 只按事件数设上限是不够的：512 条巨型工具结果照样是慢帧，所以批次还停在上限
/// [`REPLAY_BATCH_LINES`] 条来源行（`.scratch/tui-history-replay/spec.md` §2）。
const REPLAY_BATCH_EVENTS: usize = 512;

/// 一次重放批次最多产出多少条转录来源行。转录满之后，窗格每行的成本随它的上限一起涨，
/// 所以批次的界限是它画出来多少，而不是只按它吃掉多少事件来算。
const REPLAY_BATCH_LINES: usize = 2_000;

/// TUI 认的按键。
///
/// 刻意用自己的词汇表而不是 crossterm 的：这样状态机不用终端就能测，而换个后端也不会
/// 悄悄挪掉一个绑定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Delete,
    Enter,
    /// `Tab`，它永远只是 `/` 菜单的补全键（spec §6）。
    Tab,
    Esc,
    BackTab,
    CtrlC,
    /// `Ctrl-D`：退出，藏在一次确认后面。运行进行中的时候忽略，所以它永远只是空闲键盘
    /// 的手势（票 06 §1）。
    CtrlD,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// Emacs 风格的行编辑：`Ctrl-A/E` 移动，`Ctrl-U/K/W` 抹掉，`Ctrl-P/N` 翻提示历史。
    CtrlA,
    CtrlE,
    CtrlU,
    CtrlK,
    CtrlW,
    CtrlP,
    CtrlN,
    CtrlG,
    CtrlJ,
    /// `Ctrl-Z`：**挂起** —— 交还终端、停到后台，`fg` 回来重进并重绘
    /// （`.scratch/suspend-gesture/spec.md` §2）。它不归任何视图管，所以
    /// [`TuiState::key`] 在一切守卫之前就把它接走。
    CtrlZ,
    /// `Ctrl-O`：左栏的开关 —— 收起与叫回（`.scratch/sidebar-toggle/spec.md` §3）。它是纯
    /// 视图手势，所以忙闲都生效，也不清举手；只有两个独占键盘的视图（详情覆盖层、历史重放）
    /// 拦得住它。
    CtrlO,
    PageUp,
    PageDown,
}

/// 把一个 crossterm 按键翻成 [`Key`]，TUI 不认的键返回 `None`。
fn map_key(key: KeyEvent) -> Option<Key> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(ch) = key.code {
            return match ch.to_ascii_lowercase() {
                'c' => Some(Key::CtrlC),
                'd' => Some(Key::CtrlD),
                'a' => Some(Key::CtrlA),
                'e' => Some(Key::CtrlE),
                'u' => Some(Key::CtrlU),
                'k' => Some(Key::CtrlK),
                'w' => Some(Key::CtrlW),
                'p' => Some(Key::CtrlP),
                'n' => Some(Key::CtrlN),
                'g' => Some(Key::CtrlG),
                'j' => Some(Key::CtrlJ),
                'z' => Some(Key::CtrlZ),
                'o' => Some(Key::CtrlO),
                _ => None,
            };
        }
    }
    match key.code {
        KeyCode::Esc => Some(Key::Esc),
        KeyCode::Tab => Some(Key::Tab),
        KeyCode::BackTab => Some(Key::BackTab),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Char(ch) => Some(Key::Char(ch)),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Delete => Some(Key::Delete),
        KeyCode::Left => Some(Key::Left),
        KeyCode::Right => Some(Key::Right),
        KeyCode::Up => Some(Key::Up),
        KeyCode::Down => Some(Key::Down),
        KeyCode::Home => Some(Key::Home),
        KeyCode::End => Some(Key::End),
        KeyCode::PageUp => Some(Key::PageUp),
        KeyCode::PageDown => Some(Key::PageDown),
        _ => None,
    }
}

/// 每个发言者的名字用什么颜色画（票 07 §1）。
///
/// 调色板与名册都是注入的；这里只保存一个**组装之后**才第一次出现的发言者拿到的槽位。
/// 讨论可以在组装时就把它的那一对定下来，但会话中途敲的 `/discuss` 点出了注入从没听说
/// 过的人 —— 他们拿走第一个没人认领的调色板槽位，并在余下的会话里一直占着，于是名字
/// 永远不会在读的人眼皮底下换颜色。
///
/// 颜色是**画家**的事：[`wording::speaker_label`] 保持纯文本，转录之外的东西一律不上色
/// （票 07 §3、§4）。公开只因为 [`render_block`] 收它；它怎么建出来归状态机管。
pub struct SpeakerColors {
    /// 名册顺序里的讨论者：调色板的槽位 `n` 属于 `roster[n]`。
    roster: Vec<String>,
    /// 名册没认领的那些调色板槽位，按调色板顺序排：会话期间第一个没人认领的名字拿走
    /// 其中第一个。
    free_slots: Vec<usize>,
    /// 会话期间第一次见到的名字，按出现顺序。状态就这全部：颜色本身可以从它和名册推
    /// 出来。
    extra: Vec<String>,
    /// 没有注入名册，于是没什么可上色的，每个名字都是灰的。这是共享渲染里素的那一半，
    /// 而且它必须保持中性：空名册不是「一个匿名讨论者的会话」，而是「调用方根本没有
    /// 调色板」。
    uncoloured: bool,
}

/// 讨论者的调色板，按名册把槽位发出去的顺序（票 07 §1）。
const DEBATER_PALETTE: [Color; 2] = [Color::LightCyan, Color::LightMagenta];

/// 结构性框线的颜色：比 `DarkGray` 再沉一档（`DarkGray` 在常见配色里 ≈ `#808080`，而框架
/// 不该比内容先被看见）。
///
/// 它刻意是真彩色，也是这个界面里**第二处**（第一处是提示符的色相脉冲）：16 色 ANSI 里比
/// `DarkGray` 更暗的只有 `Black`，那在深色背景上等于消失。亮背景终端要在这一点上调
/// （`.scratch/tui-chrome/spec.md` §3）。
const CHROME_LINE: Color = Color::Rgb(0x4a, 0x4a, 0x4a);

impl SpeakerColors {
    /// 一份名册对应的调色板：按槽位发出去的顺序列出讨论者。没有名册的调用方传一个空的
    /// 进来，于是每个名字都是灰的。
    pub fn new(roster: &[String]) -> Self {
        let roster = roster.to_vec();
        // 名册按位置认领调色板槽位：第 `n` 个讨论者拿槽位 `n`。讨论者比颜色多时多出来的
        // 槽位会绕回来，所以这里的认领是 `slot < len` 而不是整个名册。
        let claimed: Vec<usize> = (0..roster.len().min(DEBATER_PALETTE.len())).collect();
        let free_slots = (0..DEBATER_PALETTE.len())
            .filter(|slot| !claimed.contains(slot))
            .collect();
        Self {
            uncoloured: roster.is_empty(),
            roster,
            free_slots,
            extra: Vec::new(),
        }
    }

    /// 一个发言者名字的颜色。
    ///
    /// 调色板在组装时就定下，所以一个会话的名字到颜色的映射只要它在跑就是稳定的 ——
    /// 包括会话中途才第一次出现的名字（票 07 §1）。
    fn of(&mut self, speaker: &crate::events::SpeakerId) -> Color {
        use crate::events::SpeakerId;
        if self.uncoloured {
            return Color::DarkGray;
        }
        match speaker {
            SpeakerId::Debater(id) => self.debater(id.as_str()),
            SpeakerId::Executor(_) => Color::LightYellow,
            SpeakerId::User => Color::LightGreen,
            SpeakerId::System => Color::Gray,
        }
    }

    fn debater(&mut self, id: &str) -> Color {
        if let Some(slot) = self.roster.iter().position(|name| name == id) {
            return DEBATER_PALETTE[slot % DEBATER_PALETTE.len()];
        }
        // 注入的名册不认识的名字，也就是会话中途敲的 `/discuss` 造出来的那种。它第一次
        // 露面时拿走名册没认领的第一个调色板槽位；下面那句记忆把它变成此后每次露面都相同
        // 的颜色。调色板用光了就是灰色，因为重复的颜色读起来像换了个发言者（票 07 §1）。
        let slot = match self.extra.iter().position(|name| name == id) {
            Some(slot) => slot,
            None => {
                let slot = self.extra.len();
                self.extra.push(id.to_owned());
                slot
            }
        };
        match self.free_slots.get(slot) {
            Some(slot) => DEBATER_PALETTE[*slot],
            None => Color::Gray,
        }
    }
}

/// 左栏与状态行没法从事件流上读到的那些会话值（spec §8）。
///
/// 这里的一切在组装时就已经知道，并且作为一个值注入，因为 seam 就在这里：渲染器从不
/// 伸手去够配置。状态行显示模型，左栏显示各项计数，详情覆盖层从 `session_dir` 里读出
/// 落盘的工具输出。
///
/// 会话中途会变的那些 —— 模式 —— 刻意**不**在这里：注入的副本会在用户第一次按下
/// Shift+Tab 时过期，而流上本来就带着那两次迁移。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionFacts {
    /// 这个终端正在显示的那个会话。
    pub session_id: String,
    /// 会话自己的文件所在的目录：详情覆盖层从它那里拼出落盘的工具输出，而屏幕上不显示
    /// 它（工作目录随旧顶栏一起退场，spec §8）。
    pub session_dir: String,
    /// 这个会话用来作答的模型 —— 讨论里则是两个讨论者的模型，因为状态行只有一栏放它
    /// （spec §5）。
    pub model: String,
    /// 那个模型窗口的输入预算，输出预留已经减掉。
    pub context_window: u64,
    /// 会话组装时所在的权限模式（spec §12）。
    ///
    /// 它被注入是因为状态行显示它、而流上没有任何东西说它：模式是一个 `Session` 值，
    /// 它**曾经**有的那两次迁移 —— 一次 plan 模式注入、一次模式变更取代 —— 都不再存在。
    /// `Shift+Tab` 是唯一能挪动它的东西，而前端施加的步进与循环施加的是同一步
    /// （`.scratch/todo-and-modes/spec.md` §1）。
    pub mode: Mode,
    /// 会话累计的 token 额度，有的话。
    pub budget_limit: Option<u64>,
    /// 左栏那一页数字用哪套书写制式（`.scratch/usage-stats-format/spec.md` §2）。
    ///
    /// 它在组装时注入，与会话中途会变的那些（模式）不同：制式是配置里定下来的一个值，
    /// 面板要按它把同一批计数写成 `123.5万` 或 `1.2M`。
    pub number_style: wording::NumberStyle,
    /// 这个会话的讨论者，按抽出来的顺序。单 agent 会话列出它那一个档案；讨论列出名册
    /// 产出的那一对。它就是发言者颜色的来源，所以它和别的事实一样在组装时注入：
    /// 名册不在流上（票 07 §1）。
    pub speaker_order: Vec<String>,
}

/// TUI 的注入值：console 通道的前端这一端，加上左栏与状态行要显示的那些事实。
pub struct TuiOptions {
    pub port: ConsolePort,
    pub facts: SessionFacts,
    /// 这个会话的工作目录：终端标题的路径段要回答「在哪个目录」
    /// （`.scratch/terminal-title/spec.md` §1）。
    ///
    /// 它**不进** [`SessionFacts`]：那是给面板与状态行看的事实集合，而标题要的另两样东西
    /// （`$HOME` 与工作目录）是渲染器自己的输入，随组装传进来。
    pub cwd: std::path::PathBuf,
    /// 这个会话是不是**被重新打开**的（`--continue`），所以有一次历史重放正沿着 console
    /// 端口过来。
    ///
    /// 它是一个标志，不是历史本身：事件自己走 [`ConsoleRequest::Replay`]，因为只有组装
    /// 完成后的 harness 手里才有恢复之后的那份快照。TUI 需要这个标志，是因为在快照到达
    /// 之前它一条渲染事件都不许落下 —— 连横幅都不许 —— 否则组装已经发出的那些恢复事件
    /// 会被画在它们本该从属的历史之上，然后又被重放画一遍
    /// （`.scratch/tui-history-replay/spec.md` §1、§3）。
    pub reopened: bool,
}

/// TUI 渲染器。
pub struct Tui {
    options: TuiOptions,
}

impl Tui {
    pub fn new(options: TuiOptions) -> Self {
        Self { options }
    }

    pub async fn run(self, mut receiver: broadcast::Receiver<RenderEvent>) {
        let TuiOptions {
            mut port,
            facts,
            cwd,
            reopened,
        } = self.options;
        // 家目录在这里读一次，不进 `TuiState::new` 的调用方：标题那一层是纯函数，读环境
        // 只发生在组装处（`.scratch/terminal-title/spec.md` §5）。
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let mut state = TuiState::new(facts, cwd, home);

        // alt screen、raw 模式，以及一个把它们恢复回去的 panic hook。鼠标上报与括号粘贴
        // 归我们管：`ratatui::init` 这两样都不碰（spec §1）。
        let mut terminal = ratatui::init();
        // 第一版标题在这里就写：它不该跟着那场「等重放」一起等下去，而且快照也从这一刻
        // 起算，绘制路径只管之后的变化（`.scratch/terminal-title/spec.md` §4、§5）。
        let first = state.sync_title().expect("首帧之前还没有标题快照");
        let modes = TerminalModes::enter(&first);
        let mut keys = EventStream::new();

        // 重新打开的会话在画任何东西之前先等这次重放。循环把它作为**第一条** console 请求
        // 推过来，紧接组装之后、横幅之前；与此同时组装已经在渲染通道上发出了那些恢复
        // 结果，而它们已经在那份重放快照里了。在这里等，正是为了不让它们被画在历史之上、
        // 然后再被重放画一遍。端口没了就放弃等待。
        if reopened {
            if let Some(request) = port.recv().await {
                state.request(request);
            }
        }

        // 两个定时器，都**按需**武装（`.scratch/tui-input-pulse/spec.md` §2b，
        // `.scratch/exit-gesture/spec.md` §6）：
        //
        // - `pulse`（60 ms）只在一次运行进行中的时候武装。用 `interval` 而不是每一趟新建
        //   一个 `sleep`：一个突发一千条增量的 provider 会让每轮迭代都重置一次 sleep，
        //   于是提示符恰恰会在会话最忙的时候停住不动。`Delay` 让积压的漏帧不会在循环从
        //   某个长帧里回来时被一次性花掉。
        // - 退出手势的 deadline（500 ms）只在举着手的那些帧里武装，到点就把举手作废。
        //
        // 两个都靠各自 `select!` 分支上的 `if` 保证「不武装就不会唤醒循环」：没武装的
        // 分支永远不会被 poll。空闲会话因此照旧一次唤醒都没有（那个 deadline 是唯一的
        // 有界例外）。
        let mut pulse = tokio::time::interval(PULSE_FRAME);
        pulse.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            let mut closed = false;
            if state.replay_pending() {
                // 批次之间每个来源都会被问一遍 —— 键盘、实时流与循环的请求 —— 否则一次长
                // 重放中的 `Ctrl-C` 就是一个死键。最后一个分支总是就绪的，所以这个 select
                // 从不等待任何东西，下面的批次就以能画出来的最快速度跑
                // （`.scratch/tui-history-replay/spec.md` §2）。
                tokio::select! {
                    biased;
                    received = receiver.recv() => closed = state.take_render_event(received),
                    maybe_event = keys.next() => state.terminal_event(maybe_event),
                    request = port.recv() => closed = state.port_request(request),
                    _ = std::future::ready(()) => {}
                }
                state.replay_batch();
            } else {
                // 三个来源，加上两个按需武装的定时器。曾经住在这里的重绘 tick 随它存在的
                // 那个时钟一起走了 —— 待答的问题从 console 端口来，事件从渲染通道来，
                // 而键就是键，所以没有*别的*东西在等着被注意到（票 05 §1）。两个例外都只是
                // 时间的函数：运行进行中的脉冲，与举着手的那个 deadline。空闲时这个
                // `select!` 又只是三个来源，键盘是唯一能唤醒它的东西
                // （`.scratch/tui-input-pulse/spec.md` §2b、
                // `.scratch/exit-gesture/spec.md` §6）。
                //
                // deadline 在 `select!` 之前取成值：`Instant` 是 `Copy`，取完借用就结束，
                // 分支里才借得到 `&mut state`。没举手时给它一个占位时刻 —— 关掉 poll 的是
                // 分支上那个 `if`，与 `pulse` 同构。
                let deadline = state.exit_deadline();
                tokio::select! {
                    received = receiver.recv() => closed = state.take_render_event(received),
                    maybe_event = keys.next() => state.terminal_event(maybe_event),
                    request = port.recv() => closed = state.port_request(request),
                    _ = pulse.tick(), if state.busy() => state.tick(),
                    _ = tokio::time::sleep_until(tokio::time::Instant::from(
                        deadline.unwrap_or_else(std::time::Instant::now),
                    )), if deadline.is_some() => state.expire_exit_gesture(),
                }
            }

            // 已经排进队列的都并进这一帧。一个在两帧之间突发一千条增量的 provider 花掉
            // 一帧而不是一千帧，而且仍然什么都不丢（spec §11）。重放期间这些是缓冲下来而
            // 不是应用掉，跟 select 看见的那些一模一样。
            let mut drained = 0usize;
            while drained < DRAIN_LIMIT {
                match receiver.try_recv() {
                    Ok(event) => state.live_event(event),
                    Err(broadcast::error::TryRecvError::Lagged(dropped)) => {
                        state.live_event(RenderEvent::Diagnostic(wording::renderer_dropped(
                            dropped,
                        )));
                    }
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => {
                        closed = true;
                        break;
                    }
                }
                drained += 1;
            }
            if closed {
                break;
            }

            for event in state.take_events() {
                port.emit(event);
            }
            if state.take_suspend_request() {
                // 交还终端 → 停到后台 → `fg` 回来后重进并重绘
                // （`.scratch/suspend-gesture/spec.md` §3、§4）。这一步**阻塞**到用户把进程
                // 调回前台，所以它排在绘制段之前：恢复之后那一帧自然就是全量的。
                suspend_and_resume(&mut terminal, &mut state);
            }
            if state.is_dirty() {
                // 一帧写在一个同步区里。synchronized update（DECSET 2026）只把缓冲差分包
                // 在里面 —— 读键盘没有任何理由待在它里面 —— 而终端不支持这一对时就当没
                // 看见。
                let mut frame_out = std::io::stdout();
                let _ = execute!(frame_out, BeginSynchronizedUpdate);
                let _ = terminal.draw(|frame| draw_frame(frame, &mut state));
                let _ = execute!(frame_out, EndSynchronizedUpdate);
                state.mark_clean();
                // 标题只在真变了的时候重写：算一次期望值、与上一版比对。状态来自
                // `running` / `pending` / `replay` / 目标名四处，逐个挂钩一定会漏，所以
                // 比对发生在绘制路径上（`.scratch/terminal-title/spec.md` §5）。
                if let Some(title) = state.sync_title() {
                    set_terminal_title(&title);
                }
            }
            if state.should_quit() {
                break;
            }
        }

        drop(modes);
        ratatui::restore();
    }
}

/// 写一条终端标题（`OSC 0`，crossterm 的 `SetTitle` 发的就是 `\x1b]0;…\x07`）。
///
/// 标题是**窗口属性**、不是屏幕内容，所以它不跟 alt screen 的进出绑在一起，也不进
/// 事件流（`.scratch/terminal-title/spec.md` §4、§6）。
fn set_terminal_title(title: &str) {
    let _ = execute!(std::io::stdout(), SetTitle(title));
}

/// 挂起：交还终端 → 用 SIGTSTP 停住 → `fg` 回来后重进终端并全量重绘
/// （`.scratch/suspend-gesture/spec.md` §3、§4）。
///
/// 顺序不能换：**先交还、再停**。反过来的话，停止期间终端还留在 raw 模式与 alt screen
/// 上，用户回到 shell 面对的就是一屏不属于自己的画面。恢复走 crossterm 的底层原语而不是
/// `ratatui::init()` —— 后者每次都会再包一层 panic hook，反复挂起会把 hook 叠起来。
fn suspend_and_resume(terminal: &mut ratatui::DefaultTerminal, state: &mut TuiState) {
    disable_terminal_modes();
    ratatui::restore();

    // 安全性：`SIGTSTP` 的处置是进程级属性，`signal` 只读它拿到的那个编号。
    //
    // 用 `raise` 而不是 `kill(0, …)`：`kill` 是**异步**的 —— 实测里它返回之后当前线程又
    // 往前跑了半条恢复路径（`enable_raw_mode` 与 `EnterAlternateScreen` 都发了出去），信号
    // 才被处理，于是「先交还、再停」在时间上并不成立。`raise` 把信号投给当前线程并等它处理
    // 完：这一行返回，就是 `fg` 回来了。停止信号停的是整个线程组（也就是这个进程），所以
    // 「只发给当前线程」不影响「整个进程停住」；glibc 手册那条「发给进程组」针对的是一个
    // 作业里有多个进程的情形，而 fs-agent 的组里只有它自己（`bash` 与动态工具刻意各自成组）。
    //
    // 处置先置回默认再发：`SIGTSTP` 可以被忽略，而被忽略的处置会跨 `execve` 继承，包装器
    // 可能把它留着 —— 那时信号被丢弃、进程不停，而终端已经交还了。发完还原，父进程若有意
    // 忽略它，那是父进程的意图，我们借一次就还（spec §5）。
    unsafe {
        let previous = libc::signal(libc::SIGTSTP, libc::SIG_DFL);
        libc::raise(libc::SIGTSTP);
        libc::signal(libc::SIGTSTP, previous);
    }

    // 回到前台：把进终端那套反向做一遍，然后清屏、下一帧全量重绘 —— 挂起期间用户可能
    // 已经改过窗口尺寸了。标题要**强制**重写：交还时它被 pop 回了用户原来那条。
    let _ = enable_raw_mode();
    let _ = execute!(std::io::stdout(), EnterAlternateScreen);
    let title = state.retitle();
    enable_terminal_modes(&title);
    let _ = terminal.clear();
    state.mark_dirty();
}

/// 鼠标上报、括号粘贴与终端标题：进来时管上，出去时还回去。
///
/// `ratatui::init` 只管 raw 模式与 alt screen —— 它的 `TerminalOptions` 里根本没有鼠标
/// 开关 —— 所以这几样得我们自己撤，正常路径与 panic 路径都一样。留着不管，fs-agent
/// 退出之后终端就没法选文字了。
struct TerminalModes;

impl TerminalModes {
    fn enter(title: &str) -> Self {
        enable_terminal_modes(title);
        // `init` 装了一个 hook 恢复 raw 模式与 alt screen；把它包一层，让 panic 在它跑
        // 之前也把鼠标与粘贴模式还回去。
        //
        // **只在启动时装这一次**：挂起的恢复走 [`enable_terminal_modes`]、不经过这里，
        // 否则每挂起一次就再叠一层 hook（`.scratch/suspend-gesture/spec.md` §4）。
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            disable_terminal_modes();
            previous(info);
        }));
        Self
    }
}

/// 进终端那套里**可以反复做**的那一半：请终端把原标题存起来（CSI 22 t）、写我们的第一版、
/// 开鼠标与括号粘贴；[`disable_terminal_modes`] 用 CSI 23 t 把标题换回来。
///
/// 启动时由 [`TerminalModes::enter`] 调一次，挂起恢复时由 [`suspend_and_resume`] 再调 ——
/// panic hook 因此不在这里（那个只能装一次）。不支持 push/pop 的终端上这两条是 no-op，
/// 标题会停在我们写的那条：那是一个**接受**的退化，不为它加 fallback —— 补发一条「清空
/// 标题」在那种终端上会把用户原本的标题抹掉，比不还原更糟
/// （`.scratch/terminal-title/spec.md` §4）。
fn enable_terminal_modes(title: &str) {
    let _ = execute!(std::io::stdout(), Print("\x1b[22;0t"));
    set_terminal_title(title);
    let _ = execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste);
}

impl Drop for TerminalModes {
    fn drop(&mut self) {
        disable_terminal_modes();
    }
}

fn disable_terminal_modes() {
    let _ = execute!(
        std::io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        // 还原进入时保存的那条标题。
        Print("\x1b[23;0t")
    );
}

#[async_trait]
impl Render for Tui {
    async fn consume(self: Box<Self>, receiver: broadcast::Receiver<RenderEvent>) {
        (*self).run(receiver).await;
    }
}

/// 举手槽位里举着的是哪一把（[`TuiState::exit_deadline`]）。
///
/// 两把手势的后果完全不同，屏幕上却共用那一行提示的位置，所以槽位只有一个、由这个标签说
/// 它是哪一把：举新的就等于把旧的那把作废（`.scratch/questionnaire-keys/spec.md` §5）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gesture {
    /// 退出手势：窗口内第二下退出（`.scratch/exit-gesture/spec.md` §1）。
    Exit,
    /// 问卷里的「退出这次询问」：窗口内第二下 drop 掉 sender，模型继续跑。
    DeclineQuestion,
}

/// 显示就绪的状态，与终端拆开，好让它不用终端也能测。
pub struct TuiState {
    /// 左栏与状态行显示什么，组装时注入（spec §8）。
    facts: SessionFacts,
    /// 会话的工作目录与用户的家目录：终端标题那一段路径的两半
    /// （`.scratch/terminal-title/spec.md` §1）。`home` 由 [`Tui::run`] 读一次传进来，
    /// 于是 [`TuiState::title`] 是纯函数、测试能钉死。
    cwd: std::path::PathBuf,
    home: Option<std::path::PathBuf>,
    /// 正在推进的目标名：从流上那条 `GoalSelected` 留下，`GoalStopped` / `GoalCompleted`
    /// 到了就清掉（spec §6）。目标名本来就被推到前端，只是转录层把它丢了。
    goal: Option<String>,
    /// 上一次写进终端的那条标题，用来在绘制路径上比对 —— 有它就不必枚举状态来源
    /// （spec §5）。
    last_title: Option<String>,
    /// 举手槽位：有值 = 正在举手，值是它的截止时刻
    /// （`.scratch/exit-gesture/spec.md` §1）。纯渲染器状态，不进事件流；`should_quit()`
    /// 只反映 [`TuiState::quit`]。
    ///
    /// 槽位只有一个，所以两把手势（退出、退出这次询问）**互斥**是结构性的
    /// （`.scratch/questionnaire-keys/spec.md` §5）。
    exit_deadline: Option<std::time::Instant>,
    /// 槽位里举着的是哪一把。
    exit_gesture: Gesture,
    /// 这一把举手是在**忙碌时**举起的吗（spec §1、§3）。
    ///
    /// 第一下 `Ctrl-C` 会取消当前回合，而那次取消可能在第二下之前就落地 —— 那时
    /// `busy()` 已经为假，但人的意图仍然是「打断这次运行、退出」，退出码不该退回 0。
    /// 举手的出身记在这里，第二下照它分派。
    exit_gesture_busy: bool,
    /// 挂起请求：`Ctrl-Z` 置位，[`Tui::run`] 取走并执行「交还终端 → 停住 → 恢复重绘」
    /// （`.scratch/suspend-gesture/spec.md` §2、§3）。纯渲染器状态，不进事件流，也不经过
    /// 任何一次性通道。
    suspend: bool,
    /// 会话所在的模式。用组装时的值（[`SessionFacts::mode`]）打底，此后只被那个手势挪动：
    /// 流上没有任何东西说一个会话处在什么模式，而 `Shift+Tab` 是唯一改变它的东西
    /// （`.scratch/todo-and-modes/spec.md` §1）。
    mode: Mode,
    /// plain 渲染器共用的那个「事件转块」的合并器。
    transcript: Transcript,
    /// 对话窗格：每个完成块的行按画出来的宽度折行，带自己的视口。转录归这个窗格所有，
    /// 而不是归终端的滚动回退。
    pane: Pane,
    /// 当前消息正在流的尾巴。
    live: String,
    /// 已经画进窗格的那些绘制记录，按到达顺序。
    ///
    /// 表格与代码块是按宽度排出来的，所以宽度一变，**源行本身**就得整批重排（spec §1）。
    /// 留着这份清单就是为了那时候按新宽度重放它们。代价是 `Tui` 多持一份块（与 `pane`
    /// 已经持有的源行同量级）—— 先按「全量重放」实现，简单可靠优先。
    painted: Vec<Painted>,
    /// 上面那份块是按多宽的转录内容排的。
    render_width: u16,
    /// 上一帧画完之后有没有什么东西变了。
    dirty: bool,
    /// 草稿与它的光标。
    editor: Input,
    /// 开头的 `/` 能变成哪些名字，按循环报上来的样子。在那份报告到达之前是空的，这
    /// 正是 `/` 菜单要等它到了才开的原因。
    catalog: Vec<CatalogEntry>,
    /// `/` 菜单的高亮，以及它属于哪个 token。匹配结果本身不保存：它们是草稿与目录的纯
    /// 函数，所以这里只留那个选择 —— 那是推不出来的。
    slash: MenuSelection,
    /// 左栏调用量页显示的那些数字，从流上数出来的。
    panel: Panel,
    /// 待办页与它的闩：当时生效的列表（从渲染器已经见过的调用读出），以及这个页签到底
    /// 给不给（`.scratch/todo-and-modes/spec.md` §4）。是渲染器状态，不是事件 —— 重新
    /// 打开的会话靠重放同样的调用把它重建出来。
    todo: crate::render::TodoPanel,
    /// 正在显示左栏的哪一页。是渲染器状态，不是事件：它的任何一部分都不该在流上，而且
    /// 它随进程一起死（spec §3）。
    tab: Tab,
    /// 用户想不想看见左栏（`Ctrl-O` 切换，`.scratch/sidebar-toggle/spec.md` §1）。
    ///
    /// 与选中的页签同级：只活在这一次进程里，重开回到显示，不落配置、不跨会话。它与宽度是
    /// 两层 —— 这一位说偏好，[`layout::sidebar_tier`] 说可行性 —— 两者相乘才是屏幕上真的有
    /// 的那一栏。
    sidebar_wanted: bool,
    /// 每个发言者的名字用什么颜色画（票 07）。放在这里而不是每行重算，因为会话中途第一
    /// 次出现的名字得保住已经发给它的那个槽位。
    colors: SpeakerColors,
    /// 正在被思考的那一段的推理增量。它是一条思考行两半之间的桥：这行在第一个增量上开
    /// 出来，等 trace 完成时用这里的内容重写，因为完整的 trace 只在 `MessageCompleted`
    /// 上才有（票 02 §1）。
    reasoning: String,
    /// 敞开的那条思考行属于哪个发言者，好让写完的那行用同一个名字。
    thinking_speaker: crate::events::SpeakerId,
    /// 此刻屏幕上有没有一条敞开的思考行 —— 窗格里唯一会变的那一行。
    thinking_open: bool,
    /// 这一回合的消息是否已经有它的思考行了。正文的第一个增量在 `MessageCompleted` 到达
    /// 之前很久就把这行定下来了，所以这条记录必须比 `thinking_open` 活得久，否则完成
    /// 事件会为同一个想法再加一行（票 02 §1）。
    thinking_done: bool,
    /// 回合条的单位与它们的分段头。
    turn_rail: TurnRail,
    /// 窗格每一条来源行背后的详情，与它平行，并且按窗格自己的上限一起裁剪，好让两者
    /// 永不脱节。不是任何入口的那些行是 `None`。
    links: std::collections::VecDeque<Option<Detail>>,
    /// 上一帧把每一个显示行画在了哪里，好把一次点击换回它落在的那条来源行。每帧重建，
    /// 与问题覆盖层的命中区域一样，因为只有真画出来的行才会回应指针（票 04 §1）。
    drawn_rows: Vec<Option<usize>>,
    /// 转录第一条被画出来的行所在的屏幕行，好把鼠标行（它是屏幕坐标）变成 `drawn_rows`
    /// 的下标。
    drawn_top: u16,
    /// 详情覆盖层，开着的时候。
    detail: Option<DetailView>,
    /// 上一帧把这个覆盖层画在哪里，好让框外的一次点击把它关掉 —— 与指示器遵循的是同一条
    /// 「记住读的人真看到了什么」的规矩（票 02 §4）。
    detail_rect: Option<Rect>,
    /// 上一帧把**中间的模态**画在哪里，好让滚轮知道指针是不是落在它上面（`tui-chrome` §5）。
    /// 与 [`TuiState::detail_rect`] 同一条规矩：这一帧真的画了什么就记什么，没画出来就是
    /// `None`，而那时滚轮落到转录上。
    modal_rect: Option<Rect>,
    /// 上一帧问卷占着的那块底部（输入区**与**提示行，连它们中间那条线一起）在哪里。问卷
    /// 没有边框，所以它「在哪儿」只能这样记；滚轮据此决定归谁（`tui-chrome` §5）。
    questionnaire_bottom: Option<Rect>,
    /// 上一帧把一个问题的可点部分画在了哪里。
    regions: Regions,
    /// 上一帧的整个终端区域。详情覆盖层的主体在打开时就排版好了，而那次排版需要的宽度是
    /// 终端尺寸的函数 —— 在覆盖层被画出来之前就知道，所以一次点击不必等一帧（票 04 §1）。
    area: Rect,
    /// 一次 `Prompt` 请求的答案往哪儿去。
    prompt_reply: Option<tokio::sync::oneshot::Sender<Option<String>>>,
    /// 循环是否说它正在一次运行里面。**由循环推过来**，绝不在这里推断：见
    /// [`TuiState::busy`]。
    running: bool,
    /// 忙碌脉冲的帧计数器，由 [`TuiState::tick`] 推进、由标记的画家读。没有运行进行中时
    /// 就是零 —— 推动它的是循环的定时器，所以空闲的会话把它原样留在上次运行留下的位置，
    /// 也就是零（`.scratch/tui-input-pulse/spec.md` §2）。
    pulse: u64,
    /// 一个等着按键的问题。
    pending: Option<Pending>,
    /// 要交回给循环的手势。
    events: Vec<FrontEndEvent>,
    /// 上一帧把「回到末尾」指示器画在哪里，好让一次点击跟用户真看见的东西对得上。
    indicator: Option<Rect>,
    /// 进行中的那次历史重放，有的话。`Some` 是一个一次性的启动状态：在它排空之前，键盘、
    /// 指针与循环的行为都不一样（`.scratch/tui-history-replay/spec.md` §2）。
    replay: Option<Replay>,
    /// 重放进行中时到达的实时渲染事件，按到达顺序。它们在重放结束之后（历史与其接缝
    /// 之后）才被应用，所以启动横幅不会被画进历史中间（spec §3）。
    live_buffer: Vec<RenderEvent>,
    quit: bool,
    /// 输入区是不是禁言了（`.scratch/goal-loop/spec.md` §5）：目标循环跑着的时候为真。
    ///
    /// 它是循环推过来的一个值（[`ConsoleRequest::Muted`]），不是从「忙」推出来的：一次普通
    /// 回合里打字照旧落进草稿。
    muted: bool,
}

/// 一个等着答案的问题。
///
/// 这里住着两种。循环的询问通过一条一次性通道把答案送回去（[`PermissionRequest`]）；
/// 渲染器自己的询问 —— 一次过大的粘贴、一份 Esc 会丢掉的草稿 —— 没有可答的对象，所以
/// 它们自己拿着「用户说 yes 之后要做的事」（spec §7）。
enum Pending {
    /// 循环在等一个答案。
    Loop {
        request: PermissionRequest,
        reply: tokio::sync::oneshot::Sender<Answer>,
    },
    /// 一次大到不能不问就收下的粘贴。
    Paste { text: String, chars: usize },
    /// 一份多行的草稿，`Esc` 会把它清掉。
    ClearDraft,
    /// 目标循环跑着的时候按 `Esc`：渲染器自己问一句「停下还是继续跑」
    /// （`.scratch/goal-loop/spec.md` §5）。
    ///
    /// 它是渲染器侧的问题，与一次过大的粘贴、一份会被 `Esc` 丢掉的草稿同一档 —— 没有可答的
    /// 对象，说「yes」就是推一条取消手势给循环。**默认停在「继续跑」**：只有明确选「停下」
    /// 才推那一条。
    GoalStop,
    /// 模型发起的问卷，它占着底部输入区（spec §7、§19）。
    ///
    /// 这是唯一**不**走 [`Pending::modal`] 的问题种类：中间覆盖层适合一行确认，而问卷是
    /// 多行、分页的，于是它改占输入区。
    Questionnaire(Questionnaire),
}

/// 问卷里键盘当前落在哪一区（[`Questionnaire`]）。
///
/// 它是**问卷级**的状态：每题共用，翻页时按新题的样子复位。区域决定键位怎么分派 ——
/// `j`/`k` 在选项区是移动、在输入区就是普通字符（`.scratch/questionnaire-keys/spec.md` §1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zone {
    /// 选项区：高亮落在选项上，`j`/`k`、`Ctrl-N`/`Ctrl-P`、`↑`/`↓` 在这里移动它。
    Options,
    /// 输入区：键盘让给自由文本，`j`/`k` 在这里就是字符，`Ctrl-N`/`Ctrl-P` 什么都不做。
    Input,
}

/// 模型发起的问卷，在它占着底部输入区的时候。
///
/// 状态住在这里而不是会话里，因为问卷是键盘状态，不是会话状态：它没有任何东西被记录，
/// 这场交换唯一持久的痕迹是那次工具调用的参数与它那一条结果（spec §7）。
struct Questionnaire {
    /// 那些问题，按模型发出来的顺序。
    questions: Vec<UserQuestion>,
    /// 问卷提交时答案往哪儿去。不发就丢掉，端口读作「没有答案」，也就是一次被取消的
    /// 运行的意思。
    reply: tokio::sync::oneshot::Sender<Result<UserAnswers, String>>,
    /// 每个问题一份草稿，下标与 `questions` 对齐。
    drafts: Vec<QuestionDraft>,
    /// 屏幕上是第几个问题。一次一个，页脚里写作 `2 / 3`。
    index: usize,
    /// 键盘现在在哪一区。翻页时按新题的样子复位（`.scratch/questionnaire-keys/spec.md` §1）。
    zone: Zone,
}

/// 到目前为止用户对一个问题做了什么。
#[derive(Default, Clone)]
struct QuestionDraft {
    /// 选中的选项标签，按选中的顺序。
    selected: Vec<String>,
    /// 打进去的自由文本。单选时自定义文本覆盖 `selected`；多选时它是对它的补充
    /// （spec §7）。
    custom: String,
    /// 高亮落在哪个选项上。`↑`/`↓` 移动它，`Enter`/`Space` 确认它。它是每份草稿各自的，
    /// 所以翻走再翻回来会发现高亮还留在离开时的位置。
    highlight: usize,
    /// 用户按了跳过键走掉了。这是一次刻意的「不答」，与一个从没走到的问题不同。
    skipped: bool,
}

impl QuestionDraft {
    /// 这个问题是被答了还是被明确跳过了。问卷能提交之前，每个问题都必须走到这个状态。
    fn handled(&self) -> bool {
        self.skipped || !self.selected.is_empty() || !self.custom.trim().is_empty()
    }
}

impl Questionnaire {
    /// 吃一个按键。问卷被提交时返回 `true`。
    ///
    /// 键位**按区域分派**（`.scratch/questionnaire-keys/spec.md` §2、§4）：选项区里
    /// `j`/`k`/`Ctrl-N`/`Ctrl-P`/`↑`/`↓` 是同一条移动，空格与 `Enter` 在高的那一项上**切换**，
    /// 而可打印字符与 `Backspace` 被吞掉 —— 键盘只在输入区让给文本。越过选项的两端就是进
    /// 输入区；输入区里 `↑`/`↓` 回来并移动高亮（两端环绕），`Enter` 则是「放下这题、去下一题」。
    ///
    /// **确认不翻页**：并存的意义就是「选项和文本可以一起给」，选完就跳走会把它取消一半，
    /// 所以翻页留给 `←`/`→` 与页脚那三个按钮。**提交**是唯一的例外：全部题都处理完之后
    /// `Enter` 提交整份问卷，而确认本身不会在同一次按键里提交（spec §4、§7）。
    fn press(&mut self, key: Key) -> bool {
        match key {
            // 提交优先于一切：它是键盘上唯一的提交路径，不该被「等价」或区域吃掉。
            Key::Enter => {
                if self.all_handled() {
                    return true;
                }
                match self.zone {
                    // 输入区里 `Enter` 是「放下这题、去下一题」。
                    Zone::Input => {
                        if self.drafts[self.index].handled() {
                            self.advance();
                        }
                    }
                    // 选项区里它与空格完全一致。
                    Zone::Options => {
                        if self.has_options() {
                            self.confirm_highlight();
                        }
                    }
                }
            }
            // `Space` 也确认，这样人能不用那个兼作提交的键来作答 —— 但只在**选项区**里：
            // 人已经在输入区打过字，这一下就是文本的一部分。中文答案里夹英文词（`llm wiki`
            // 这种）最容易撞上（票 33）。没有选项的题本来就没有可确认的东西，空格在那里
            // 也照旧是普通字符。
            Key::Char(' ') if self.has_options() && self.zone == Zone::Options => {
                self.confirm_highlight();
            }
            // `Tab` 是那个明确的「跳过这个，继续」。
            Key::Tab => {
                self.drafts[self.index].skipped = true;
                self.advance();
            }
            // 选项区里的移动：`j`/`k` 与 Emacs 的 `Ctrl-N`/`Ctrl-P` 走同一条路。
            Key::Up => self.step(-1),
            Key::Down => self.step(1),
            Key::Char('k') | Key::CtrlP if self.zone == Zone::Options => self.step(-1),
            Key::Char('j') | Key::CtrlN if self.zone == Zone::Options => self.step(1),
            Key::Left => self.back(),
            Key::Right => self.advance(),
            Key::Backspace if self.zone == Zone::Input => {
                self.drafts[self.index].custom.pop();
            }
            // 输入区里每个可打印字符都是自由文本，数字也一样。
            Key::Char(ch) if self.zone == Zone::Input => {
                self.type_custom(ch);
            }
            _ => {}
        }
        false
    }

    /// 屏幕上的这个问题有没有可以高亮的选项。
    fn has_options(&self) -> bool {
        !self.questions[self.index].options.is_empty()
    }

    /// 确认高亮的那个选项：**切换**它（spec §3）。
    ///
    /// 单选与多选在按键语义上是同一条：已选中就取消它，没选中就选中它。差别只剩单选的集合
    /// 至多一个 —— 确认另一个选项是**替换**，不是叠加（题面上写着「单选」，交回去的就该是
    /// 一个）。自定义文本两者都不碰：它现在与选择并存。
    fn confirm_highlight(&mut self) {
        let index = self.drafts[self.index].highlight;
        let Some(label) = self.questions[self.index]
            .options
            .get(index)
            .map(|choice| choice.label.clone())
        else {
            return;
        };
        let multi_select = self.questions[self.index].multi_select;
        let draft = &mut self.drafts[self.index];
        match draft.selected.iter().position(|picked| picked == &label) {
            Some(at) => {
                draft.selected.remove(at);
            }
            None if multi_select => draft.selected.push(label),
            None => draft.selected = vec![label],
        }
    }

    /// 沿选项走一格：`j`/`k`/`Ctrl-N`/`Ctrl-P`/`↑`/`↓` 都走这一条。
    ///
    /// 在选项区里，越过两端就是**进输入区**（末项再往下、首项再往上）；在输入区里，它则是
    /// **回到选项区**并把高亮挪一格，两端环绕 —— 于是「输入区」是这条循环里的一个位置，
    /// 只是不占 `highlight` 的下标。挪高亮就是「键盘回到选项上」，与点选项行
    /// （`select_option`）一致；没有选项可挪时什么都不做，**也就**不碰区域：那种题只有
    /// 输入区一个落点。
    fn step(&mut self, delta: isize) {
        let count = self.questions[self.index].options.len();
        if count == 0 {
            return;
        }
        let next = self.drafts[self.index].highlight as isize + delta;
        if self.zone == Zone::Input {
            self.drafts[self.index].highlight = next.rem_euclid(count as isize) as usize;
            self.zone = Zone::Options;
            return;
        }
        if next < 0 || next >= count as isize {
            self.zone = Zone::Input;
            return;
        }
        self.drafts[self.index].highlight = next as usize;
    }

    /// 添一个自由文本字符。
    ///
    /// 它**不动**选中的选项：单选与多选一个形状，文本与选择并存交回（spec §3）。想清掉文本
    /// 就自己按 `Backspace` 删。
    fn type_custom(&mut self, ch: char) {
        self.drafts[self.index].custom.push(ch);
    }

    fn advance(&mut self) {
        if self.index + 1 < self.questions.len() {
            self.index += 1;
            // 翻页了，所以键盘回到新题该在的区域（票 04 §5）。
            self.reset_zone();
        }
    }

    fn back(&mut self) {
        if self.index > 0 {
            self.index -= 1;
            self.reset_zone();
        }
    }

    /// 把区域放回这道题该有的地方：有选项就回选项区，而没有选项的题只有输入区一个落点。
    fn reset_zone(&mut self) {
        self.zone = if self.has_options() {
            Zone::Options
        } else {
            Zone::Input
        };
    }

    fn all_handled(&self) -> bool {
        self.drafts.iter().all(QuestionDraft::handled)
    }

    /// 确认当前问题里下标为 `index` 的选项，跟用键盘确认高亮的那一项一样。
    ///
    /// 单选与多选一样是**切换**，而且都不翻页：点击回答这个问题，翻页留给 `←`/`→` 与页脚
    /// 按钮（spec §3、§4）。点一个选项行也把键盘带回选项区。
    fn select_option(&mut self, index: usize) {
        if index >= self.questions[self.index].options.len() {
            return;
        }
        self.drafts[self.index].highlight = index;
        self.zone = Zone::Options;
        self.confirm_highlight();
    }

    /// 把键盘交给自由文本行。
    fn focus_custom(&mut self) {
        self.zone = Zone::Input;
    }

    /// 答案，也就是工具那一条结果（spec §7）：跳过的问题写作 `selected: []` 且没有
    /// `custom`；其余问题把**所选与自由文本一起**交回去，单选与多选一个形状（spec §3）。
    ///
    /// 跳过的检查放在最前面，因为跳过是对整个问题的决定：`Tab` 之前打的字被丢掉，否则
    /// `selected: []` 再带一个 `custom` 会被模型读成一次刻意的自定义作答，而不是「用户
    /// 选择不回答」（spec §7）。
    fn answers(&self) -> UserAnswers {
        UserAnswers {
            answers: self
                .questions
                .iter()
                .zip(&self.drafts)
                .map(|(question, draft)| {
                    if draft.skipped {
                        return UserAnswer {
                            id: question.id.clone(),
                            selected: Vec::new(),
                            custom: None,
                        };
                    }
                    let custom = draft.custom.trim();
                    let custom = (!custom.is_empty()).then(|| custom.to_owned());
                    UserAnswer {
                        id: question.id.clone(),
                        selected: draft.selected.clone(),
                        custom,
                    }
                })
                .collect(),
        }
    }
}

impl Pending {
    /// 覆盖层为这个问题显示的那些行，不画成覆盖层的问题返回 `None`。
    ///
    /// 循环的询问与渲染器自己的询问都画在转录之上，所以草稿留在用户放它的地方
    /// （spec §7、§9）。问卷不是：它是多行、分页的，所以它改为接管底部输入区
    /// （spec §19），而这里对它的回答是 `None`。
    fn modal(&self) -> Option<Modal> {
        let modal = match self {
            Pending::Loop { request, .. } => {
                // 发起者不是主会话时，第一行点名说话人：执行者与讨论者各问各的，而作答者
                // 是人（`.scratch/workspace-mode/spec.md` §7）。
                let call = wording::tool_call_line(&request.tool_name, &request.args);
                let description = match &request.speaker {
                    Some(speaker) => wording::permission_speaker_line(&call, speaker),
                    None => call,
                };
                Modal {
                    title: wording::permission_title().to_owned(),
                    // 然后是折叠转录行带着的那句一行描述 —— 再*然后*是它将要跑起来的样子，
                    // 因为批准的那一刻正是确切命令必须可读的时刻（2026-09-23）。
                    description: Some(description),
                    notes: request
                        .escalation
                        .as_ref()
                        .map(wording::permission_escalation_lines)
                        .unwrap_or_default(),
                    detail: Some(wording::permission_call(
                        &request.tool_name,
                        &summarize_args(&request.args),
                    )),
                    choices: &wording::PERMISSION_CHOICES,
                    actions: wording::PERMISSION_CHOICE_ANSWERS
                        .iter()
                        .map(|(_, answer)| HitAction::Answer(*answer))
                        .collect(),
                }
            }
            Pending::Paste { chars, .. } => Modal {
                title: wording::paste_title().to_owned(),
                description: None,
                notes: Vec::new(),
                detail: Some(wording::paste_body(*chars)),
                choices: &wording::PASTE_CHOICES,
                actions: vec![HitAction::Paste, HitAction::Dismiss],
            },
            Pending::ClearDraft => Modal {
                title: wording::clear_draft_title().to_owned(),
                description: None,
                notes: Vec::new(),
                detail: Some(wording::clear_draft_body().to_owned()),
                choices: &wording::CLEAR_CHOICES,
                actions: vec![HitAction::ClearDraft, HitAction::Dismiss],
            },
            Pending::GoalStop => Modal {
                title: wording::goal_stop_title().to_owned(),
                description: None,
                notes: Vec::new(),
                detail: Some(wording::goal_stop_body().to_owned()),
                choices: &wording::GOAL_STOP_CHOICES,
                actions: vec![HitAction::Dismiss, HitAction::StopGoal],
            },
            Pending::Questionnaire(_) => return None,
        };
        Some(modal)
    }
}

/// 一个问题被问出来时用的那些部件（spec §9）。
///
/// 拆开就是重点。问题曾经是一个折行的段落，于是一条长命令把它的键挤到右边之外，读的人
/// 得从一句话里把它们挑出来；这里标题说在问什么，摘要说这个动作*是*什么，详情给出具体
/// 的调用，而决定它的那些键独占一行、不被上面任何东西埋掉。
struct Modal {
    /// 标题行，位置更高、比其余更粗：`权限询问：bash`。
    title: String,
    /// 这次调用是*为了*什么，就用折叠转录行的那几个字（`调用 bash 查看 git status`），
    /// 这样问题与它所问的那一行读起来一致（2026-09-23，用户要求）。不是关于工具调用的
    /// 问题没有这一项。
    description: Option<String>,
    /// 描述与详情之间那几行：一次升级询问的「理由」与「要放开的路径」。它们排在确切命令
    /// 之前，而命令行保持最后一行（`.scratch/workspace-mode/spec.md` §7）。
    notes: Vec<String>,
    /// 这个问题所问的那一件具体的事：那次调用、那个路径、那个大小。
    detail: Option<String>,
    /// 回答它的那些键，画成一行按钮。
    choices: &'static [wording::Choice],
    /// 点这些按钮各自会做什么。空的意思是「用键回答」，循环的问题要的就是这个；渲染器
    /// 自己的确认会自我点名，因为它们没有可答的通道（票 04 §3）。
    actions: Vec<HitAction>,
}

/// 一个问题的某一块可点区域，在画出来的时候记下。
///
/// 矩形是**屏幕**坐标，而它的 `action` 就是点在那里意味着的全部，所以指针处理函数从来
/// 不必把它正在看的版式再推一遍（票 04 §1）。
#[derive(Clone)]
struct Region {
    rect: Rect,
    action: HitAction,
}

/// 点一下问题被画出来的部件会做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HitAction {
    /// 用这个选择回答循环的问题，跟按下它的键完全一样。
    Answer(Answer),
    /// 确认这次过大的粘贴。
    Paste,
    /// 确认清掉草稿。
    ClearDraft,
    /// 确认停下目标循环（§5）。
    StopGoal,
    /// 什么都不做就关掉问题：安全的那一个答案，`Esc` 也是它。
    Dismiss,
    /// 退回上一个问题。
    Previous,
    /// 前进到下一个问题。
    Next,
    /// 提交问卷。
    Submit,
    /// 显示左栏的这一页，跟点它的页签一样（spec §3）。
    SwitchTab(Tab),
    /// 跳到这个单位的开头，跟点它的回合条格子一样（spec §4）。
    TurnRailUnit(usize),
}

/// 一个问题占着指针时的一次指针手势。
enum QuestionClick {
    /// 滚轮一格：`true` 是向上。
    Wheel(bool),
    /// 一次左键点击，屏幕坐标。
    At(u16, u16),
}

/// 上一帧画出来的、问题的指针能作用上去的那些东西。
///
/// 这里的一切每帧重建、由下一次点击来读，与转录的 `indicator` 一直在用的机制相同 ——
/// 这也是一个滚出屏幕的选项、一个被裁掉的按钮不需要各自失效的原因（票 04 §1）。
#[derive(Default, Clone)]
struct Regions {
    /// 问题覆盖层每个按钮一块区域，按画出来的顺序。
    cells: Vec<Region>,
    /// 问卷可见的选项行：`(行, 该问题里的下标)`。
    options: Vec<(u16, usize)>,
    /// 问卷的自由文本行，画出来了的时候。
    custom: Option<u16>,
}

impl Regions {
    /// 忘掉一切：每帧调一次，在画任何东西之前。
    fn clear(&mut self) {
        self.cells.clear();
        self.options.clear();
        self.custom = None;
    }

    /// 点 `row` 落在了哪个选项下标上，如果那一行确实是选项。整行都是目标，所以列不会
    /// 把它收窄（票 04 §4）。
    fn option_at(&self, row: u16) -> Option<usize> {
        self.options
            .iter()
            .find(|(option_row, _)| *option_row == row)
            .map(|(_, index)| *index)
    }

    /// `row` 是不是自由文本行。
    fn custom_at(&self, row: u16) -> bool {
        self.custom == Some(row)
    }

    /// 一次点击落到的那个按钮的动作，落在按钮上的话。
    fn action_at(&self, column: u16, row: u16) -> Option<HitAction> {
        self.cells
            .iter()
            .find(|region| region.rect.contains((column, row).into()))
            .map(|region| region.action)
    }
}

/// `/` 菜单里需要被记住的那一半。
///
/// 菜单的其余部分全是推出来的：token 从草稿来，匹配从目录与那个 token 来。推不出来的是
/// 用户挑了哪一行，所以这里留的就是它 —— 以及它是在哪个前缀下挑的。
#[derive(Debug, Default)]
struct MenuSelection {
    /// 高亮是在哪个前缀下做出来的。前缀不同就意味着这是另一个菜单，于是高亮从头开始，
    /// 而那次 `Esc` 也不再作数。
    prefix: String,
    /// 高亮落在哪个匹配上，有的话。
    ///
    /// **裸 `/` 时是 `None`**，这就是那条安全规矩：在用户打出名字、或者用方向键走过列表
    /// 之前，什么都没被选中，于是只为看一眼菜单而按下的键不可能跑起一条没人要的命令
    /// （spec §7 对一个顺手按键的读法）。
    selected: Option<usize>,
    /// `Esc` 关掉了菜单。只要还在打的就是它当初被关掉时的那个前缀，它就保持关着。
    dismissed: bool,
}

/// 此刻的 `/` 菜单：它属于哪个 token、什么与它匹配、哪个匹配被高亮。
///
/// 一个值，由草稿加目录在每帧、每次按键时建出来。菜单永远不会是可以与屏幕上所见漂移开
/// 的状态。
#[derive(Debug, Clone, PartialEq, Eq)]
struct SlashMenu {
    /// 斜杠后面已经打进去的内容。
    prefix: String,
    /// 匹配上的那些条目，按目录顺序，形如 `(名字, 描述)`。
    entries: Vec<(String, String)>,
    /// 高亮落在哪个条目上，夹进范围；没有时是 `None`。
    selected: Option<usize>,
}

/// 对一个渲染器自己问出来的问题，某个键是否意味着 yes。
///
/// 只有 `y`。`Esc` 与 `Enter` 是**安全**的那个答案 —— 「不」—— 因为这两个都是伸手就按
/// 下去的键（spec §7，票 06 §1）。
fn agrees(key: Key) -> bool {
    matches!(key, Key::Char('y') | Key::Char('Y'))
}

/// 回合条的记账：每个完成的回合（讨论里是每一轮）一个单位，以及每个单位从哪里开始
/// （`.scratch/tui-sidebar/spec.md` §4）。
///
/// 两个索引加一个计数器，全部从流上推出来 —— 刻意没有记下来的选择，因为存下来的
/// 「当前单位」会在任何一个事件到达的瞬间与视口漂开。
#[derive(Default)]
struct TurnRail {
    /// 每条来源行一个条目，与 `links` 平行，并跟它一起裁剪。
    lines: std::collections::VecDeque<TurnRailLine>,
    /// 每个**已完成**单位的分段头来源行。
    heads: Vec<usize>,
    /// 上一个边界之后画出来的来源行数。它是数出来的、而不是记成一个索引，因为上限从前面
    /// 丢行。
    lines_in_unit: usize,
}

/// 回合条记住的一条被画出来的来源行的信息。
#[derive(Debug, Clone, Copy)]
struct TurnRailLine {
    /// 它属于哪个单位。上一个边界之后到达的行属于那个还没结束的单位。
    unit: usize,
    /// 它是不是用户自己的消息。一个单位的头是它里面第一条这样的行。
    user: bool,
}

impl TurnRail {
    /// 会话已经完成了多少个单位。
    fn units(&self) -> usize {
        self.heads.len()
    }

    /// 记下一条被画出来的来源行。
    fn push_line(&mut self, user_message: bool) {
        // 一行属于的那个单位就是正在建的那个：`units()` 是已经完成的个数，所以那就是这一行
        // 在它回合结束时将拿到的下标。
        self.lines.push_back(TurnRailLine {
            unit: self.heads.len(),
            user: user_message,
        });
        self.lines_in_unit += 1;
    }

    /// 当前单位结束了：记下它的分段从哪里开始，并开出下一个。
    ///
    /// 头是**单位里第一条用户消息**，那才是点一格该落到的地方：问题是谁提的，一个回合就
    /// 从那里开始。讨论的单位自己没有用户消息（讨论者回答的是会话本来就持有的那一个问题），
    /// 于是退回到单位自己的第一行 —— 也就是那一轮的开场叙述。
    fn close_unit(&mut self) {
        let start = self.lines.len().saturating_sub(self.lines_in_unit);
        let head = (start..self.lines.len())
            .find(|index| self.lines[*index].user)
            .unwrap_or(start);
        self.heads.push(head);
        self.lines_in_unit = 0;
    }

    /// 按转录的上限丢掉最老的来源行，并把指到它们之外的段头平移回来。整个跨度都被丢掉的
    /// 单位会塌到最老的那条幸存行上，那是还剩的、最接近的跳转目标。
    fn prune(&mut self, dropped: usize) {
        for _ in 0..dropped {
            self.lines.pop_front();
        }
        for head in &mut self.heads {
            *head = head.saturating_sub(dropped);
        }
        self.lines_in_unit = self.lines_in_unit.saturating_sub(dropped);
    }

    /// 丢掉全部条目，好让转录按新的宽度重放一遍（spec §1）。
    ///
    /// 单位会随着重放的块重新关出来，所以三个字段一起清。
    fn clear(&mut self) {
        self.lines.clear();
        self.heads.clear();
        self.lines_in_unit = 0;
    }

    /// 一条来源行属于哪个单位。
    fn unit_of(&self, source: usize) -> usize {
        self.lines
            .get(source)
            .map(|line| line.unit)
            .unwrap_or_else(|| self.units())
    }

    /// 一个单位的分段从哪里开始。
    fn head(&self, unit: usize) -> Option<usize> {
        self.heads.get(unit).copied()
    }
}

/// 一条已经画进窗格、且能在宽度变化时重放的记录。
///
/// 大多数行来自一个 [`Block`]；思考行是唯一的例外 —— 它是渲染器自己的状态（票 02 §1），
/// 由 `pane.push` / `pane.replace_last` 原地管。宽度一变两者都得回来，所以清单里两种都记
/// （`.scratch/markdown-render/spec.md` §1）。
enum Painted {
    /// 一个定稿的块。
    Block(Block),
    /// 还开着的思考行。
    Thinking { speaker: crate::events::SpeakerId },
    /// 定稿的思考行；`trace` 是记下来的整段推理，`None` 是合成器那种「没记下来」。
    Thought {
        speaker: crate::events::SpeakerId,
        trace: Option<String>,
    },
}

/// 一次进行中的历史重放：组装好的事件流、其中有多少已经铺进转录、以及那产出了多少条
/// 来源行。
///
/// 重放是渲染器的一个一次性状态 —— 它不是 [`TuiState::busy`]，那是循环对「一次运行」的
/// 说法：重放期间没有什么可取消的，所以 `Ctrl-C` 改为退出
/// （`.scratch/tui-history-replay/spec.md` §2、§5）。
struct Replay {
    /// 整条流，按 `seq` 顺序，与组装留下它时一模一样。
    events: Vec<Event>,
    /// 下一个要应用的事件。同时充当进度行显示的 `n`。
    next: usize,
    /// 重放到目前为止产出的来源行数。零意味着历史什么都没画 —— 一条空流，或者一副只有
    /// `SessionStarted` 的骨架 —— 于是没有接缝可标（spec §6）。
    lines: usize,
}

impl TuiState {
    pub fn new(
        facts: SessionFacts,
        cwd: std::path::PathBuf,
        home: Option<std::path::PathBuf>,
    ) -> Self {
        let colors = SpeakerColors::new(&facts.speaker_order);
        let mode = facts.mode;
        Self {
            facts,
            cwd,
            home,
            goal: None,
            last_title: None,
            exit_deadline: None,
            exit_gesture: Gesture::Exit,
            exit_gesture_busy: false,
            suspend: false,
            mode,
            transcript: Transcript::new(),
            pane: Pane::new(),
            live: String::new(),
            painted: Vec::new(),
            // 第一帧之前没有真正的宽度；先用共享渲染那个缺省把行排出来，首帧一画出来就会
            // 发现宽度不同并按真宽度重放（spec §1）。
            render_width: SHARED_RENDER_WIDTH,
            dirty: true,
            editor: Input::new(),
            catalog: Vec::new(),
            slash: MenuSelection::default(),
            panel: Panel::new(),
            todo: crate::render::TodoPanel::default(),
            tab: Tab::Usage,
            sidebar_wanted: true,
            colors,
            reasoning: String::new(),
            thinking_speaker: crate::events::SpeakerId::System,
            thinking_open: false,
            thinking_done: false,
            turn_rail: TurnRail::default(),
            links: std::collections::VecDeque::new(),
            drawn_rows: Vec::new(),
            drawn_top: 0,
            detail: None,
            detail_rect: None,
            modal_rect: None,
            questionnaire_bottom: None,
            regions: Regions::default(),
            area: Rect::default(),
            prompt_reply: None,
            // 空闲，直到循环另说：在它要第一行之前没有任何东西在跑，而键盘必须读起来就是
            // 这个样子（spec §6）。
            running: false,
            pulse: 0,
            pending: None,
            events: Vec::new(),
            indicator: None,
            replay: None,
            live_buffer: Vec::new(),
            quit: false,
            // 循环开始跑目标时才会把它置上（[`ConsoleRequest::Muted`]）。
            muted: false,
        }
    }

    /// 这一刻终端标题该是什么（`.scratch/terminal-title/spec.md` §1–§3）。
    pub fn title(&self) -> String {
        wording::terminal_title(
            &self.cwd,
            self.home.as_deref(),
            wording::title_state(self.replay.is_some(), self.pending.is_some(), self.busy()),
            self.goal.as_deref(),
        )
    }

    /// 举手：记下「这一刻起，窗口之内第二下算数」，并记下这是**哪一把**
    /// （`.scratch/exit-gesture/spec.md` §1、`.scratch/questionnaire-keys/spec.md` §5）。
    ///
    /// 槽位只有一个，所以举一把就等于把另一把作废 —— 两把手势的互斥是结构性的，不靠额外
    /// 规则维持。时间从参数进来，测试不必睡真实时间；`key()` 自己传 [`std::time::Instant::now`]。
    pub fn raise_gesture_at(&mut self, now: std::time::Instant, gesture: Gesture) {
        self.exit_deadline = Some(now + GESTURE_WINDOW);
        self.exit_gesture = gesture;
        // 出身由调用方在举起之后自己盖（退出手势按 `busy()` 分派退出码）；这里先归零，
        // 「退出这次询问」那一把永远用不到它。
        self.exit_gesture_busy = false;
        self.dirty = true;
    }

    /// 举手：空闲里举起的**退出手**。
    pub fn raise_exit_gesture_at(&mut self, now: std::time::Instant) {
        self.raise_gesture_at(now, Gesture::Exit);
    }

    /// 槽位里现在立着的是哪一把，没有就 `None`（超时也算没有）。
    pub fn raised_gesture(&self, now: std::time::Instant) -> Option<Gesture> {
        (self.exit_deadline.is_some() && !exit_gesture_due(self.exit_deadline, now))
            .then_some(self.exit_gesture)
    }

    /// 这把举手还立着吗：有 deadline，而且还没到点。
    pub fn exit_gesture_raised(&self, now: std::time::Instant) -> bool {
        self.raised_gesture(now).is_some()
    }

    /// 这把举手的截止时刻，有的话。
    ///
    /// 主循环用它武装那个按需的唤醒；字段本身只有 [`TuiState`] 自己改
    /// （`.scratch/exit-gesture/spec.md` §6）。
    pub fn exit_deadline(&self) -> Option<std::time::Instant> {
        self.exit_deadline
    }

    /// 让这把举手超时作废：清字段、置 `dirty`，提示行跟着恢复。
    ///
    /// 它是作废的**唯一**入口 —— 主循环到点也调它（票 02），而「别的键先清旧举手」也走
    /// 这里，于是超时与按键两条路不会各写一份。
    pub fn expire_exit_gesture(&mut self) {
        self.exit_deadline = None;
        self.exit_gesture_busy = false;
        self.dirty = true;
    }

    /// `Ctrl-C` / `Ctrl-D` 共用的那一把手势（`.scratch/exit-gesture/spec.md` §1）。
    ///
    /// 空闲时两键完全对等：第一下举手（什么也不做），窗口内第二下退出，混按也算第二下。
    /// 忙碌时只有 `Ctrl-C` 参与：第一下取消当前回合**并且**举手，第二下退出 —— 那一条由
    /// CLI 走有序收尾并以 130 收尾（票 03）；`Ctrl-D` 维持忽略，而且**不清**举手，因为它是
    /// 「被忽略」，不是「别的键」。
    fn exit_key(&mut self, key: Key) {
        let now = std::time::Instant::now();
        // 只认**退出**那一把：举着「退出这次询问」时按 `Ctrl-C` 不作数，走下面第一下的路
        // —— 两把手势互斥（`.scratch/questionnaire-keys/spec.md` §5）。
        let raised = self.raised_gesture(now) == Some(Gesture::Exit);
        // `Ctrl-D` 在忙碌、**或任何问题立着**时什么都不做（票 01）：它是「被忽略」，不是
        // 「别的键」，所以它也不清掉已经举起的那把手。
        if key == Key::CtrlD && (self.busy() || self.pending.is_some()) {
            return;
        }
        if raised {
            // 第二下：按**这一把手的出身**走（spec §3）—— 空闲里举的手不会因为回合中途开跑
            // 就变成 130，忙碌里举的手也不会因为回合已经收尾就变成 0。
            if self.exit_gesture_busy {
                self.events.push(FrontEndEvent::Quit);
            } else {
                self.quit = true;
            }
            return;
        }
        // 第一下：举手。忙碌时这一下同时是取消当前回合，而这一把的出身就是「忙碌」——
        // 取消可能马上就落地，第二下到达时 `busy()` 已经为假，但那次退出仍然是「忙碌中被打断
        // 而退」，得走 130 那条路（spec §3）。
        if self.busy() {
            self.events.push(FrontEndEvent::Cancel);
        }
        self.raise_gesture_at(now, Gesture::Exit);
        self.exit_gesture_busy = self.busy();
    }

    /// 算一次标题，与上一版比对；变了就记下新的并返回它，没变答 `None`。
    ///
    /// 绘制路径每帧问一次，于是「只在状态变化时更新」自动成立（spec §5）。
    pub fn sync_title(&mut self) -> Option<String> {
        let title = self.title();
        if self.last_title.as_deref() == Some(title.as_str()) {
            return None;
        }
        self.last_title = Some(title.clone());
        Some(title)
    }

    /// 挂起恢复后**强制**重写标题：交还终端时标题被 pop 回了用户原来那条，而 `last_title`
    /// 的记忆还停在挂起前那一版 —— 两者已经对不上（`.scratch/suspend-gesture/spec.md` §4）。
    /// 所以先让比对失效，再照 [`TuiState::sync_title`] 的规则算一次。
    pub fn retitle(&mut self) -> String {
        self.last_title = None;
        self.sync_title().unwrap_or_else(|| self.title())
    }

    /// 上一帧画完之后有没有什么东西变了。
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// 把脉冲推进一帧（`.scratch/tui-input-pulse/spec.md` §2b）。
    ///
    /// 循环在它于一次运行进行中时武装的那个定时器里调它，所以空闲的前端从来到不了这里；
    /// 这个守卫还是照写一遍，因为一个在它的写作者正打字时变了色的提示符，就是动画挡了路
    /// （票 09）。它推进的是脉冲的帧、别的什么都不是：**不是**一个通用的「重画点什么」的
    /// 钩子，任何需要一帧的东西都应该通过那个改变了它的事件说出来。
    pub fn tick(&mut self) {
        if self.busy() {
            self.pulse = self.pulse.wrapping_add(1);
            self.dirty = true;
        }
    }

    /// 括号粘贴是作为文本到达的，不是作为按键。
    ///
    /// 这里做三件 crossterm 留给我们的事（research §6.3）：行尾归一化、丢掉控制字符、以及
    /// 一次大到不能看一眼就收下的粘贴先问一句。**这些都不提交** —— 粘进来的换行就是换行
    /// （spec §7）。
    pub fn paste(&mut self, text: &str) {
        if self.pending.is_some() {
            // 问题在的时候它占着键盘：一次粘贴不许回答它，也不许落进用户看不见的草稿里
            // （spec §9）。
            return;
        }
        let text = editor::normalize_paste(text);
        if text.is_empty() {
            return;
        }
        self.dirty = true;
        let chars = text.chars().count();
        if chars > PASTE_CONFIRM_CHARS {
            self.pending = Some(Pending::Paste { text, chars });
        } else {
            self.editor.insert_str(&text);
            // 一次粘贴可以像任何别的按键一样是条 `/` 命令：粘进来的 `/ask-matt` 在它落地
            // 后的那一帧打开菜单。
            self.sync_menu();
        }
    }

    /// 喂进一个渲染事件，并回答它画了多少条转录来源行。
    ///
    /// 这个计数就是历史重放批次预算的量度：一帧的成本由它铺下去的文字定界，而不是只按它
    /// 吃掉多少事件来算（`.scratch/tui-history-replay/spec.md` §2）。实时调用方不看它。
    pub fn apply(&mut self, event: RenderEvent) -> usize {
        self.dirty = true;
        self.observe_goal(&event);
        let mut produced = 0usize;
        for block in self.transcript.push(event) {
            // 思考行的生命周期跑在块被画出来之前：一个推理增量开出它，正文的第一个增量把它
            // 就地冻住，而 `MessageCompleted` 把还开着的那条定下来（票 02 §1）。
            if let Block::Delta {
                speaker,
                kind,
                text,
            } = &block
            {
                match kind {
                    DeltaKind::Reasoning => {
                        produced += usize::from(self.open_thinking(speaker.clone()));
                        self.reasoning.push_str(text);
                    }
                    DeltaKind::Text => self.freeze_thinking(),
                }
            }
            if let Block::Message {
                speaker,
                role,
                reasoning,
                ..
            } = &block
            {
                if matches!(role, Role::Assistant) {
                    // 消息的整段 trace 把还开着的定下来。provider 一个增量都没发时，这行改在
                    // 这里开出来、且已经完成；已经开过并定过的那条（正文在流中途把它冻住了）
                    // 不再加新的，因为一段思考就是一行（票 02 §1）。trace 缺席时把开着的行按
                    // 「未记录」定下来 —— 那是合成器的形状，增量流过了，什么都没记下来。
                    match reasoning {
                        Some(text) => {
                            let text = text.clone();
                            if !self.thinking_open && !self.thinking_done {
                                produced += usize::from(self.open_thinking(speaker.clone()));
                            }
                            self.settle_thinking(Some(text));
                        }
                        None => {
                            if self.thinking_open {
                                self.settle_thinking(None);
                            }
                        }
                    }
                    self.thinking_done = true;
                }
            }
            // 新回合的思考是新的一段，所以「已经画过」这个闩随回合一起清掉（票 02 §1）。
            if matches!(&block, Block::TurnStarted { .. }) {
                self.thinking_done = false;
            }
            match &block {
                // 推理不是消息正文的一部分：它被折进自己那一行，所以永远不加入正文流过的那条
                // 活尾巴（票 02 §3）。
                Block::Delta {
                    kind: DeltaKind::Reasoning,
                    ..
                } => {}
                Block::Delta { text, .. } => {
                    self.live.push_str(text);
                    if self.live.len() > LIVE_BUFFER {
                        let cut = self.live.len() - LIVE_BUFFER;
                        // 按字符边界裁。
                        let cut = (cut..self.live.len())
                            .find(|index| self.live.is_char_boundary(*index))
                            .unwrap_or(self.live.len());
                        self.live.drain(..cut);
                    }
                }
                Block::Message { .. } => {
                    // 增量是那一版实时的视图；块才是永久的那一版，所以尾巴可以走了。
                    self.live.clear();
                }
                _ => {}
            }
            // 面板数的是这个块就会话说了什么；窗格显示的是它向读者说了什么。名字在进来的路上
            // 就上了色，所以发言者的第一行就是给注入名册没列到的名字定色的那一下（票 07）。
            self.panel.observe(&block);
            // `todo` 页也是同一种推法，来源是唯一带列表的那一种块：一次调用自己的参数。
            self.todo.observe(&block);
            produced += self.push_block(block, self.render_width);
        }
        produced
    }

    /// 把一个块排成行、推进窗格，并**记住它**，好在宽度变化时重放（spec §1）。
    fn push_block(&mut self, block: Block, width: u16) -> usize {
        let produced = self.emit_block(&block, width);
        if produced > 0 {
            // 不产生行的那些块（流式增量）不留：重放它们什么都不画，白占一份内存。
            self.painted.push(Painted::Block(block));
        }
        produced
    }

    /// 只把块排成行推进窗格，不记它 —— 到达时与重放时走的是同一条路。
    fn emit_block(&mut self, block: &Block, width: u16) -> usize {
        let lines = paint_block(block, &mut self.colors, width);
        let produced = lines.len();
        for rendered in lines {
            self.push_source(rendered.line, rendered.link, Some(is_user_message(block)));
        }
        // 回合的结束关掉一个单位；讨论里一轮的结束也是 —— 那里单位是**轮**，因为那才是
        // 讨论计数的东西（`CONTEXT.md` 把轮次与回合分开，spec §4）。
        if is_boundary(block, self.discussion()) {
            self.turn_rail.close_unit();
        }
        produced
    }

    /// 转录内容的宽度变了：清空窗格，按新宽度把绘制记录整批重放一遍（spec §1）。
    ///
    /// 宽度一变就不能只重新折行：表格的列宽与超宽代码行的折行是**渲染时**定下的，
    /// 那些源行本身已经依赖宽度了。
    fn rerender_if_width_changed(&mut self, width: u16) {
        if width == self.render_width {
            return;
        }
        self.render_width = width;
        if self.painted.is_empty() {
            return;
        }
        self.pane.clear();
        self.links.clear();
        self.turn_rail.clear();
        let painted = std::mem::take(&mut self.painted);
        for item in &painted {
            self.emit_painted(item, width);
        }
        self.painted = painted;
        self.dirty = true;
    }

    /// 重放一条绘制记录。
    fn emit_painted(&mut self, painted: &Painted, width: u16) {
        match painted {
            Painted::Block(block) => {
                self.emit_block(block, width);
            }
            Painted::Thinking { speaker } => {
                let line = self.thinking_in_progress_line(speaker);
                self.push_source(line, None, None);
            }
            Painted::Thought { speaker, trace } => {
                let (line, detail) = self.thinking_settled_line(speaker, trace.clone());
                self.push_source(line, Some(detail), None);
            }
        }
    }

    /// 推进一条来源行：窗格、它的链接入口、回合条的纹理，最后裁掉两边溢出的部分。
    ///
    /// `user` 说这条行要不要记进回合条，`None` 是不记 —— 思考行是唯一的这种行：它属于当前
    /// 单位，但它不是一次新的发言，也不改变单位的划分。
    fn push_source(&mut self, line: Line<'static>, link: Option<Detail>, user: Option<bool>) {
        self.pane.push(line);
        self.links.push_back(link);
        if let Some(user) = user {
            self.turn_rail.push_line(user);
        }
        self.prune_links();
    }

    /// 这个会话数的是**轮**而不是回合。
    ///
    /// 注入而不是推断：讨论就是一个有不止一个讨论者的会话，而这属于组装已经知道的东西
    /// （spec §4）。
    fn discussion(&self) -> bool {
        self.facts.speaker_order.len() > 1
    }

    /// 喂进一个**实时**渲染事件：渲染器跑着的时候到达的事件，相对于历史重放正在铺下的那种。
    ///
    /// 重放进行中的时候，事件按到达顺序压着不发，这样历史与这个会话加的行不会交错。**进流
    /// 的**事件根本不压：重放的快照就是组装好的那条流，所以重放期间到达的一条进流事件是
    /// 快照已经持有的 —— 缓冲它会把同一次工具调用或同一条消息画第二遍。只有那些永不进事件
    /// 流的事件 —— 横幅、诊断、流式增量 —— 才被缓冲到接缝之后（`spec` §3）。
    pub fn live_event(&mut self, event: RenderEvent) {
        if self.replay.is_some() {
            if !matches!(event, RenderEvent::Logged(_)) {
                self.live_buffer.push(event);
            }
            self.dirty = true;
        } else {
            self.apply(event);
        }
    }

    /// 历史重放还有没有事件要铺。
    pub fn replay_pending(&self) -> bool {
        self.replay.is_some()
    }

    /// 应用历史重放的一批事件。
    ///
    /// 批次由 [`REPLAY_BATCH_EVENTS`] 与 [`REPLAY_BATCH_LINES`] **两者**定界，谁先到算谁：
    /// 512 个事件的一片本身并不是一帧的界限，因为每一个都可能是巨型工具结果。吃掉最后
    /// 一个事件的那一批同时也关掉重放 —— 接缝、缓冲着的实时事件与回到末尾都发生在这里，
    /// 在下一帧被画出来之前。
    pub fn replay_batch(&mut self) {
        let Some(mut replay) = self.replay.take() else {
            return;
        };
        let mut applied = 0usize;
        let mut produced = 0usize;
        while replay.next < replay.events.len()
            && applied < REPLAY_BATCH_EVENTS
            && produced < REPLAY_BATCH_LINES
        {
            let event = replay.events[replay.next].clone();
            replay.next += 1;
            applied += 1;
            produced += self.apply(RenderEvent::Logged(event));
        }
        replay.lines += produced;
        // `apply` 自己会置这个标志，但进度行的 `n` 是只有这一趟说了话、帧才看得见的状态 ——
        // 而只改了计数、别的什么都没改的那一趟，恰恰就是本来永远不会被画出来的那趟。
        self.dirty = true;
        if replay.next >= replay.events.len() {
            self.finish_replay(replay);
        } else {
            self.replay = Some(replay);
        }
    }

    /// 关掉一次跑完的重放：标出接缝、放出缓冲着的实时事件、把视口送回末尾。
    ///
    /// 分隔线只在历史真的画出了东西时才插入，所以一条空流或一副只剩 `SessionStarted` 的
    /// 骨架不会拿到一条通向虚无的接缝。它是渲染层的一行，不是事件，所以它永不进事件流，
    /// 而下一次 `--continue` 会插入新的一条、而不是重放旧的那条。
    fn finish_replay(&mut self, replay: Replay) {
        if replay.lines > 0 {
            self.apply(RenderEvent::Notice(wording::history_divider().to_owned()));
        }
        for event in std::mem::take(&mut self.live_buffer) {
            self.apply(event);
        }
        // 读的人追上来了：转录就是历史，视口在它的末尾，而一个刚开出来的会话本来就该在那里。
        self.pane.to_bottom();
        self.dirty = true;
    }

    /// 历史重放进行中时处理一个按键。
    ///
    /// 重放**不是**一次运行，所以运行的那些键一个都不作运行解：没有什么可取消的，而
    /// `Ctrl-C` 是退出。编辑器照常工作 —— 等待的时间就是打字的时间 —— 但 `Enter` 不能
    /// 提交，转录的滚动键被忽略，因为下面的历史还在铺，视口钉在它的末尾（`spec` §3、§5）。
    fn replay_key(&mut self, key: Key) {
        if key == Key::CtrlC {
            // 重放里 `Ctrl-C` 也走双击：第一下举手、第二下退出。误按一下不该把一次重放
            // （以及它后面还没画出来的那些历史）直接扔掉（spec §1）。
            let now = std::time::Instant::now();
            if self.exit_gesture_raised(now) {
                self.quit = true;
            } else {
                self.raise_exit_gesture_at(now);
            }
            return;
        }
        // 别的键清掉旧举手（§1 的总则），然后照常编辑：编辑器认的那些键全都工作，而
        // `Ctrl-D`、`Esc`、`Enter`、滚动键与模式手势一律忽略。
        self.expire_exit_gesture();
        self.editor_key(key);
        self.sync_menu();
    }

    /// 施加那些编辑草稿的键，无论草稿活在哪儿 —— 常驻编辑器与重放共用它们，所以两者不会
    /// 漂开。
    fn editor_key(&mut self, key: Key) {
        match key {
            Key::Char(ch) => self.editor.insert_char(ch),
            // 唯一可靠的换行键：在没有 keyboard-enhancement 协议的终端上 Shift+Enter 到达时
            // 就是一个普通的 Enter，所以它会提交（spec §6）。
            Key::CtrlJ => self.editor.insert_char('\n'),
            Key::Backspace => self.editor.backspace(),
            Key::Delete => self.editor.delete_forward(),
            Key::Left => self.editor.left(),
            Key::Right => self.editor.right(),
            Key::Up => self.editor.up(),
            Key::Down => self.editor.down(),
            Key::Home | Key::CtrlA => self.editor.home(),
            Key::End | Key::CtrlE => self.editor.end(),
            Key::CtrlU => self.editor.kill_to_line_start(),
            Key::CtrlK => self.editor.kill_to_line_end(),
            Key::CtrlW => self.editor.kill_word(),
            // 历史只归 `Ctrl-P` / `Ctrl-N`；方向键属于光标。
            Key::CtrlP => self.editor.history_previous(),
            Key::CtrlN => self.editor.history_next(),
            _ => {}
        }
    }

    /// 开出思考行，除非已经有一条开着。它画出了一行时返回 `true`。
    ///
    /// 这一行是普通的转录行 —— 它照算窗格的上限，也跟别的一切一起滚 —— 而且刻意**还**
    /// 不可点：完整 trace 只在 `MessageCompleted` 上才有（票 02 §1）。
    fn open_thinking(&mut self, speaker: crate::events::SpeakerId) -> bool {
        if self.thinking_open {
            return false;
        }
        self.thinking_open = true;
        self.thinking_speaker = speaker.clone();
        self.reasoning.clear();
        // 名字是 `speaker_label`，所以它拿发言者的颜色 —— 每一条带名字的行都遵循同一条规矩
        // （票 07 §2）。
        let line = self.thinking_in_progress_line(&speaker);
        self.push_source(line, None, None);
        // 记进重放清单：宽度变化时它也要跟着回来（spec §1）。
        self.painted.push(Painted::Thinking { speaker });
        true
    }

    /// 思考开始那一行：名字加「正在思考」。重放时按同一份构造重建。
    fn thinking_in_progress_line(&mut self, speaker: &crate::events::SpeakerId) -> Line<'static> {
        let name = wording::speaker_label(speaker);
        let color = self.colors.of(speaker);
        Line::from(vec![
            Span::styled(format!("{name} "), Style::default().fg(color)),
            Span::styled(
                wording::thinking_in_progress(),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    }

    /// 思考落定那一行，以及它通向详情的入口。重放时按同一份构造重建。
    fn thinking_settled_line(
        &mut self,
        speaker: &crate::events::SpeakerId,
        trace: Option<String>,
    ) -> (Line<'static>, Detail) {
        let name = wording::speaker_label(speaker);
        let color = self.colors.of(speaker);
        // 名字后面那个 `▸` 说的是这行可以打开 —— 它跟在发言者后面，这样每一行仍然以
        // 「谁在说话」开头（票 03 §Answer，2026-09-23 修正）。
        let line = Line::from(vec![
            Span::styled(format!("{name} "), Style::default().fg(color)),
            Span::styled("▸ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                wording::thinking_finished(),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let detail = Detail {
            // 覆盖层的标题就是被点那一行自己的文字（票 02 §4）。
            title: line_text(&line),
            color,
            kind: DetailKind::Thinking { text: trace },
        };
        (line, detail)
    }

    /// 把敞开的那条思考行就地冻住：正文的第一个增量意味着模型已经不思考、开始作答了，
    /// 于是这行定下来（票 02 §1）。
    fn freeze_thinking(&mut self) {
        if !self.thinking_open {
            return;
        }
        let text = std::mem::take(&mut self.reasoning);
        let recorded = !text.is_empty();
        self.settle_thinking(recorded.then_some(text));
    }

    /// 把思考行定下来 —— 不管有没有记录下来的 trace —— 并把它变成通向它详情的入口。
    ///
    /// `Some` 是记录下来的 trace；`None` 是合成器的形状 —— 增量流过了，事件流里没有整段
    /// 文本 —— 它的详情会说出来。一个根本没有敞开思考行的回合什么都不加（票 02 §1）。
    fn settle_thinking(&mut self, text: Option<String>) {
        if !self.thinking_open {
            return;
        }
        self.reasoning.clear();
        self.thinking_open = false;
        self.thinking_done = true;
        // 就地写：一段思考就是一行，从 `正在思考` 到 `思考完成`（票 02 §1）。
        let speaker = self.thinking_speaker.clone();
        let (line, detail) = self.thinking_settled_line(&speaker, text.clone());
        self.pane.replace_last(line);
        if let Some(link) = self.links.back_mut() {
            *link = Some(detail);
        }
        // 重放清单里那一条也从「开着」换成「定稿」，连它的详情一起 —— 否则一次宽度变化
        // 会把这条行变回进行中，或者把它的 trace 丢掉（spec §1）。
        let settled = Painted::Thought {
            speaker: speaker.clone(),
            trace: text,
        };
        match self.painted.last_mut() {
            Some(slot @ Painted::Thinking { .. }) => *slot = settled,
            _ => self.painted.push(settled),
        }
    }

    /// 一直丢掉最老的链接，直到这个列表不比窗格的上限长，这是两者保持平行的唯一办法：
    /// 一条来源行在两边要么意思相同、要么两边都没有（票 04 §1）。
    ///
    /// 回合条的逐行索引也在同一口气里裁掉，理由相同：一条来源行属于哪个单位，是按窗格
    /// 交回来的下标去查的。
    fn prune_links(&mut self) {
        let before = self.links.len();
        while self.links.len() > pane::CAP {
            self.links.pop_front();
        }
        let dropped = before - self.links.len();
        if dropped > 0 {
            self.turn_rail.prune(dropped);
        }
    }

    /// 处理一个鼠标事件。
    ///
    /// 有五样东西可以回应指针，它们之间的顺序就是谁占着它：开着的详情覆盖层，然后是问题，
    /// 然后是回合条，然后是左栏的页签，最后是转录 —— 它的滚轮、指示器与折叠行都回应它。
    /// 没有一样认领的就忽略：终端自己的选择是用户的，而这里什么都不抢焦点（spec §4、§7）。
    pub fn mouse(&mut self, mouse: MouseEvent) {
        // 重放靠忽略来占着指针：历史还在一个钉在末尾的视口下面铺，所以滚轮一格与一次点击
        // 都不许挪动它、也不许打开一行还没到达完的行（`spec` §5）。
        if self.replay.is_some() {
            return;
        }
        // 五次分派，按谁占着指针排序。详情覆盖层直接占着它；否则是问题；否则是这一帧自己的
        // 那些部件，回合条与页签排在它们旁边的文字之前。这里从不滚动某个立着的东西背后的
        // 转录（票 04 §2，`tui-sidebar` spec §7）。
        self.dirty = true;
        // 1. 详情覆盖层直接占着指针。它的整个主体都滚，而再点一次它来自的那一行会关掉它
        // （票 02 §4）。
        if self.detail_open() {
            match mouse.kind {
                MouseEventKind::ScrollUp => self.detail_scroll(-1),
                MouseEventKind::ScrollDown => self.detail_scroll(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    // 框外的一次点击关掉它 —— 它来自的那一行、转录、页脚，什么都行
                    // （票 02 §4；2026-09-23 修正，原先只认「再点同一行」）。框内的点击是
                    // 覆盖层自己的、什么都不做，因为它没有自己的按钮。
                    let inside = self
                        .detail_rect
                        .is_some_and(|rect| rect.contains((mouse.column, mouse.row).into()));
                    if !inside {
                        self.close_detail();
                    }
                }
                _ => {}
            }
            return;
        }
        // 2. 接下来是问题占着指针。**点击**照旧在它可答的地方回答它（可答的是上一帧记成区域
        // 的那些，所以一个被裁掉或滚走的键干脆没有区域 —— spec §9，票 04 §2）；**滚轮**则按
        // 指针落在哪一块分派：落在覆盖层自己那块里就归它，落在转录上就滚转录
        // （`tui-chrome` §5，推翻票 04 §2 里「吃掉一切」的那半句）。
        if self.pending.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
                    let point = (mouse.column, mouse.row).into();
                    let questionnaire = self
                        .pending
                        .as_ref()
                        .is_some_and(|pending| matches!(pending, Pending::Questionnaire(_)));
                    // 问卷用排版给的底部块，中间的模态用它自己那个矩形（它带边框）。
                    let owned = if questionnaire {
                        self.questionnaire_bottom
                            .is_some_and(|rect| rect.contains(point))
                    } else {
                        self.modal_rect.is_some_and(|rect| rect.contains(point))
                    };
                    if owned {
                        // 模态自己没有可滚的内容，所以在自己那块里什么都不做；问卷挪高亮。
                        if questionnaire {
                            self.question_click(QuestionClick::Wheel(up));
                        }
                    } else {
                        self.pane.wheel(up);
                    }
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.question_click(QuestionClick::At(mouse.column, mouse.row))
                }
                _ => {}
            }
            return;
        }
        // 3. 否则就是这一帧自己的部件，按谁占着指针排序：左栏的页签，然后是转录 —— 它的
        // 滚轮、指示器与折叠行都回应它。页签是个控件，所以它在周围的文字之前被问
        // （spec §7）。
        match mouse.kind {
            MouseEventKind::ScrollUp => self.pane.wheel(true),
            MouseEventKind::ScrollDown => self.pane.wheel(false),
            MouseEventKind::Down(MouseButton::Left) => {
                match self.regions.action_at(mouse.column, mouse.row) {
                    Some(HitAction::SwitchTab(tab)) => self.tab = tab,
                    Some(HitAction::TurnRailUnit(unit)) => self.jump_to_unit(unit),
                    _ if self.indicator_hit(mouse.column, mouse.row) => self.pane.to_bottom(),
                    _ => {
                        let width =
                            layout::plan(self.area, 1, self.sidebar_wanted).detail_width() as usize;
                        if let Some(detail) = self.link_hit(&mouse) {
                            self.open_detail(detail, width);
                        }
                    }
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// 问题占着指针时，对一次点击或滚轮一格作出反应。
    fn question_click(&mut self, click: QuestionClick) {
        match self.pending.as_mut() {
            // 中间覆盖层：每一个 `[键] 标签` 区间就是一个按钮，点一下跑的就是那个键会给出的
            // 答案，分毫不差（票 04 §3）。
            Some(
                Pending::Loop { .. }
                | Pending::Paste { .. }
                | Pending::ClearDraft
                | Pending::GoalStop,
            ) => {
                let QuestionClick::At(column, row) = click else {
                    // 滚轮在一行的问题上什么都不做。
                    return;
                };
                let Some(action) = self.regions.action_at(column, row) else {
                    return;
                };
                let pending = self.pending.take().expect("有一个问题正等着作答");
                // 循环的问题发出按钮被建出来时带着的那个答案 —— 与它那个键发出的一样 ——
                // 而渲染器自己的确认是自己回答自己。
                match (pending, action) {
                    (Pending::Loop { reply, .. }, HitAction::Answer(choice)) => {
                        let _ = reply.send(choice);
                    }
                    (
                        pending @ (Pending::Paste { .. } | Pending::ClearDraft | Pending::GoalStop),
                        action,
                    ) => self.own_answer(pending, action),
                    // 一块不属于这个问题的区域。左栏的页签在同一张「这一帧画了什么」的表里，
                    // 所以点在它上面也会到这里来；问题占着指针，所以这次点击什么都不做 ——
                    // 尤其是，它绝不许关掉一个读的人还没回答的问题（spec §7、§9）。
                    (pending, _) => self.pending = Some(pending),
                }
            }
            // 问卷占着底部输入区：选项行、自定义行，以及页脚的翻页按钮（票 04 §4）。
            Some(Pending::Questionnaire(_)) => self.questionnaire_click(click),
            None => {}
        }
    }

    /// 回答渲染器自己的一条确认，用一次点击带着的动作。
    fn own_answer(&mut self, pending: Pending, action: HitAction) {
        match (pending, action) {
            (Pending::Paste { text, .. }, HitAction::Paste) => {
                self.editor.insert_str(&text);
                self.sync_menu();
            }
            (Pending::ClearDraft, HitAction::ClearDraft) => self.editor.clear(),
            // 点「停下」与按 `s` 是同一次动作：推一条取消手势，循环据此收尾并落一条「人主动
            // 停」的收尾事件。
            (Pending::GoalStop, HitAction::StopGoal) => self.events.push(FrontEndEvent::Cancel),
            // `Dismiss`，以及任何不可能出现的组合，都是安全的那一个答案：问题关掉、什么都
            // 不发生，`Esc` 干的就是这个。
            _ => {}
        }
    }

    /// 问卷占着输入区时，对一次点击或滚轮一格作出反应。
    fn questionnaire_click(&mut self, click: QuestionClick) {
        let regions = self.regions.clone();
        let mut submitted = false;
        let Some(Pending::Questionnaire(questionnaire)) = self.pending.as_mut() else {
            return;
        };
        match click {
            // 滚轮挪动选项窗口，而窗口跟着高亮走 —— 所以一格正好是一个选项的位移（票 04 §4）。
            // 它走的是与 `j`/`k` 同一条路，所以越过两端也会落到输入区；输入区里没有可滚动
            // 的位置，静默。
            QuestionClick::Wheel(up) => {
                if questionnaire.zone == Zone::Options {
                    questionnaire.step(if up { -1 } else { 1 });
                }
            }
            QuestionClick::At(column, row) => {
                // 选项行与自由文本行按屏幕行记录；页脚的按钮像别的按钮一样记成矩形，所以它们
                // 全都从同一张表里查（票 04 §1）。
                if let Some(option) = regions.option_at(row) {
                    questionnaire.select_option(option);
                    return;
                }
                if regions.custom_at(row) {
                    // 点自由文本行把光标交给它；否则键盘焦点留在上一个键放它的地方（票 04 §5）。
                    questionnaire.focus_custom();
                    return;
                }
                match regions.action_at(column, row) {
                    Some(HitAction::Previous) => {
                        questionnaire.back();
                        questionnaire.reset_zone();
                    }
                    Some(HitAction::Next) => {
                        questionnaire.advance();
                        questionnaire.reset_zone();
                    }
                    Some(HitAction::Submit) => {
                        // 这个按钮的提交与全部处理完之后 `Enter` 的提交一模一样，所以它走的
                        // 是同一条路：放掉接管、回答工具（票 04 §4）。
                        if questionnaire.all_handled() {
                            submitted = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        if submitted {
            self.questionnaire_key(Key::Enter);
        }
    }

    /// 视口最上面一行属于哪个单位 —— 回合条上亮的那一格。
    ///
    /// 刻意是一个**推出来**的量：视口才是状态，而存下来的「当前单位」会在任何一个事件
    /// 到达、或读的人滚动一下的瞬间就漂开。在末尾时它是最新的那个单位，也就是「我在跟着
    /// 这场对话走」的意思，哪怕整条转录一屏就装得下（spec §4）。
    fn focused_turn(&self) -> Option<usize> {
        let units = self.turn_rail.units();
        if units == 0 {
            return None;
        }
        if self.pane.following() {
            return Some(units - 1);
        }
        let source = self.pane.source_at(self.pane.top())?;
        Some(self.turn_rail.unit_of(source).min(units - 1))
    }

    /// 跳到某个单位的开头：点它那一格所做的事。
    ///
    /// 落点是**顶端对齐**的，所以每一次跳都落在眼睛期待的地方；最新那个单位改为夹到末尾，
    /// 这是同一条规矩在转录末端读到的样子，而不是一个特例（spec §4）。
    fn jump_to_unit(&mut self, unit: usize) {
        let Some(head) = self.turn_rail.head(unit) else {
            return;
        };
        self.pane.scroll_to_source(head);
    }

    /// 一次点击落到的那个可点链接，拷成它要打开的东西。
    ///
    /// 覆盖层将在哪个宽度上打开，来自上一帧，那是中间块几何唯一已知的地方（票 04 §1）。
    fn link_hit(&self, mouse: &MouseEvent) -> Option<Detail> {
        let offset = (mouse.row.checked_sub(self.drawn_top)?) as usize;
        let row = (*self.drawn_rows.get(offset)?)?;
        self.links.get(row)?.clone()
    }

    /// 一次点击是否落在了「回到末尾」指示器上。
    fn indicator_hit(&self, column: u16, row: u16) -> bool {
        self.indicator.is_some_and(|rect| {
            column >= rect.x
                && column < rect.x.saturating_add(rect.width)
                && row >= rect.y
                && row < rect.y.saturating_add(rect.height)
        })
    }

    /// 回答循环发来的一个请求。
    pub fn request(&mut self, request: ConsoleRequest) {
        self.dirty = true;
        match request {
            ConsoleRequest::Prompt { reply } => self.prompt_reply = Some(reply),
            // 循环自己对「它是不是正在跑东西」的说法。这个状态里没有别的什么可以替它说话。
            ConsoleRequest::Muted { muted } => {
                self.muted = muted;
                // 禁言开始的那一刻，草稿里还没有什么东西是「插话」；立着的问题也不受影响
                // （它是循环在等答案，与打字不是一回事）。
            }
            ConsoleRequest::RunState { running } => {
                self.running = running;
                // 脉冲属于某一次运行：运行结束时提示符回到它歇着的那个颜色，于是下一次从
                // 同一个地方开始 —— 而歇着的颜色是一个常量，不是上一次回合恰好停下来的地方
                // （`.scratch/tui-input-pulse/spec.md` §2b，票 09）。
                if !running {
                    self.pulse = 0;
                }
                // 问题属于把它提出来的那次运行，所以那次运行的结束就是它变馊的原因：循环
                // 不再等答案，它那次询问也随运行一起死了。把覆盖层留在屏幕上，会把下一个
                // 按键送进一个没人在等的问题 —— 一次静默的失败，读起来像死键（spec §6、§9）。
                // 丢掉发送端是对「没人在等」最诚实的读法：留着一个问题会被拒绝。
                //
                // 只有循环的问题随运行一起走。渲染器自己的那些 —— 一次过大的粘贴、一份
                // `Esc` 会清掉的草稿 —— 不是这次运行能撤回的，而且它们只能趁循环空闲时才
                // 立着。问卷也是循环的：它是模型提的，而一次被取消的运行让它既没人在等、
                // 也没答案可给。
                if !running
                    && matches!(
                        self.pending,
                        Some(Pending::Loop { .. } | Pending::Questionnaire(_))
                    )
                {
                    self.pending = None;
                }
            }
            ConsoleRequest::Ask(ask) => {
                // 问题不可以画在详情覆盖层之上：覆盖层不是一个 `pending`，所以没有别的东西
                // 会让它退下，而它底下那个模态会变得无法回答（票 02 §4）。
                self.close_detail();
                if self.pending.is_some() {
                    // 循环一次只问一个问题、并等那个答案，所以这不可能发生。万一发生了，
                    // 丢掉*新*的问题能让屏幕上那个仍然可答；丢掉它的发送端就是拒绝它，那是
                    // 对一个孤儿询问安全的读法。
                    return;
                }
                self.pending = Some(Pending::Loop {
                    request: ask.request,
                    reply: ask.reply,
                });
            }
            ConsoleRequest::Questionnaire(request) => {
                // 与 `Ask` 同一个理由：覆盖层为问题退下。
                self.close_detail();
                if self.pending.is_some() {
                    // 一次只有一个问题占着键盘，正如循环的询问那样：丢掉新的那个让屏幕上那个
                    // 仍然可答，而它被丢掉的发送端拒绝了那个孤儿询问。
                    return;
                }
                // 工具在到达端口之前就拒掉空问卷，所以这不可能是模型发来的。一个没有问题的
                // 问题会既没东西可画、也没东西可索引，所以它被拒掉，而不是放任它把渲染器
                // panic 掉。
                if request.questions.is_empty() {
                    let _ = request.reply.send(Err("一份问卷至少要有一道题".to_owned()));
                    return;
                }
                let drafts = request
                    .questions
                    .iter()
                    .map(|_| QuestionDraft::default())
                    .collect();
                let mut questionnaire = Questionnaire {
                    questions: request.questions,
                    reply: request.reply,
                    drafts,
                    index: 0,
                    zone: Zone::Options,
                };
                // 没有选项的题只有输入区一个落点，所以起始区域由第一题的样子定。
                questionnaire.reset_zone();
                self.pending = Some(Pending::Questionnaire(questionnaire));
            }
            // 循环能作用上去的那些名字。它们在组装之后到达一次 —— skills 来自会话 ——
            // 没有别的东西携带它们。
            ConsoleRequest::Catalog { entries } => self.catalog = entries,
            // 重新打开的会话组装时用的那段历史。空流不是一次重放：进入那个状态会为一次什么
            // 都不铺、也不标接缝的操作显示一条进度行（`spec` §2）。
            ConsoleRequest::Replay { events } => {
                if !events.is_empty() {
                    self.replay = Some(Replay {
                        events,
                        next: 0,
                        lines: 0,
                    });
                }
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<FrontEndEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// 取走挂起请求：`true` 表示 [`Tui::run`] 该执行一次「交还 → 停 → 恢复」
    /// （`.scratch/suspend-gesture/spec.md` §2、§3）。与 [`TuiState::take_events`] 同一种
    /// 形状：状态机只置位，循环来取。
    pub fn take_suspend_request(&mut self) -> bool {
        std::mem::take(&mut self.suspend)
    }

    /// 取一次 broadcast 接收。`true` 表示通道没了。
    fn take_render_event(
        &mut self,
        received: Result<RenderEvent, broadcast::error::RecvError>,
    ) -> bool {
        match received {
            Ok(event) => {
                self.live_event(event);
                false
            }
            // 丢掉的渲染器增量损害的是输出，从不是正确性，所以它像任何别的实时事件一样被
            // 叙述出来 —— 并且和横幅同一个理由，在重放期间被缓冲。
            Err(broadcast::error::RecvError::Lagged(dropped)) => {
                self.live_event(RenderEvent::Diagnostic(wording::renderer_dropped(dropped)));
                false
            }
            Err(broadcast::error::RecvError::Closed) => true,
        }
    }

    /// 取一个终端事件。
    fn terminal_event(&mut self, event: Option<std::io::Result<CtEvent>>) {
        match event {
            Some(Ok(CtEvent::Key(key))) => {
                if key.kind == KeyEventKind::Press {
                    if let Some(key) = map_key(key) {
                        self.key(key);
                    }
                }
            }
            Some(Ok(CtEvent::Paste(text))) => self.paste(&text),
            Some(Ok(CtEvent::Mouse(mouse))) => self.mouse(mouse),
            // resize 就是一次重画，而重放继续成批地跑：下一帧按新尺寸排版
            // （`.scratch/tui-history-replay/spec.md` §5）。
            Some(Ok(CtEvent::Resize(..))) => self.mark_dirty(),
            _ => {}
        }
    }

    /// 取一个来自循环的请求。`true` 表示端口没了。
    fn port_request(&mut self, request: Option<ConsoleRequest>) -> bool {
        match request {
            Some(request) => {
                self.request(request);
                false
            }
            None => true,
        }
    }

    /// 模型的问卷，在它占着底部输入区的时候。
    fn questionnaire(&self) -> Option<&Questionnaire> {
        match &self.pending {
            Some(Pending::Questionnaire(questionnaire)) => Some(questionnaire),
            _ => None,
        }
    }

    /// 问卷立着时那一下 `Esc`（`.scratch/questionnaire-keys/spec.md` §5）。
    ///
    /// 在输入区里它先回选项区（文本不清），**并且**举手 —— 于是任何区域连按两下 `Esc` 都是
    /// 「退出这次询问」。举手期间的第二下把它兑现：drop 掉 sender，工具读到「没作答」，
    /// **模型继续跑**，这一回合不中止。
    fn questionnaire_escape(&mut self) {
        let now = std::time::Instant::now();
        if self.raised_gesture(now) == Some(Gesture::DeclineQuestion) {
            if let Some(pending) = self.pending.take() {
                self.decline(pending);
            }
            self.expire_exit_gesture();
            return;
        }
        // 输入区那一下同时是「回选项区」：举手不改变那件事，文本也不清。没有选项的题只有
        // 输入区一个落点，所以它用 `reset_zone` 而不是硬置 `Options`
        // （`.scratch/questionnaire-keys/spec.md` §1、§5）。
        if let Some(Pending::Questionnaire(questionnaire)) = self.pending.as_mut() {
            questionnaire.reset_zone();
        }
        self.raise_gesture_at(now, Gesture::DeclineQuestion);
    }

    /// 给问卷喂一个按键。
    ///
    /// 只要问卷还有问题剩下，接管就立着；提交它的那一个按键把它丢掉，那也正是把底部输入区
    /// 交还给常驻编辑器的那一下。
    fn questionnaire_key(&mut self, key: Key) {
        let Some(Pending::Questionnaire(mut questionnaire)) = self.pending.take() else {
            return;
        };
        if questionnaire.press(key) {
            let answers = questionnaire.answers();
            let _ = questionnaire.reply.send(Ok(answers));
        } else {
            self.pending = Some(Pending::Questionnaire(questionnaire));
        }
    }

    /// 处理一个按键。答案与提交通过待答的一次性通道发出去；手势排队交给循环。
    pub fn key(&mut self, key: Key) {
        self.dirty = true;
        // 挂起排在**一切**之前：它是终端层手势，重放、详情覆盖层、问卷、举手都拦不住它
        // （`.scratch/suspend-gesture/spec.md` §2）。真正的终端动作由 [`Tui::run`] 做 ——
        // 这里只置位，跟 `quit` 是同一种「状态机请求、循环执行」的形状。
        if key == Key::CtrlZ {
            self.suspend = true;
            return;
        }
        // 重放在别的一切之前就占着键盘 —— 包括详情覆盖层，它在这个阶段不可能开着 ——
        // 因为它的边界是它自己的：`Ctrl-C` 退出，`Ctrl-D` 与 `Esc` 不起作用，而编辑器照常
        // 工作（`spec` §5）。
        if self.replay.is_some() {
            self.replay_key(key);
            return;
        }
        // 详情覆盖层是一个自成一体的视图模式：它立着的时候占着键盘，而它下面的转录冻在
        // 读的人离开的地方（票 02 §4）。
        if self.detail_open() {
            match key {
                // 「别的都被忽略」的唯一例外：`Ctrl-D` 关掉覆盖层，而不是退出、更不是发问
                // （票 06 §5）。`Ctrl-C` **不是**例外：它是被忽略的键之一（票 02 §4）。
                Key::Esc | Key::CtrlD => self.close_detail(),
                Key::Up => self.detail_scroll(-1),
                Key::Down => self.detail_scroll(1),
                Key::PageUp => self.detail_scroll(-(self.detail_page() as isize)),
                Key::PageDown => self.detail_scroll(self.detail_page() as isize),
                _ => {}
            }
            return;
        }
        // 左栏开关（`.scratch/sidebar-toggle/spec.md` §3）：排在两个「独占键盘的视图」之后
        // —— 详情覆盖层与历史重放各自拦得住它 —— 而在清举手之前：它是纯视图手势，不该让
        // 半分钟前那一下退出举手作废；问卷 / `/` 菜单立着时也照常生效。
        if key == Key::CtrlO {
            self.sidebar_wanted = !self.sidebar_wanted;
            return;
        }
        // 别的键先清掉旧的举手：半分钟前那一下不该莫名其妙地算数（spec §1）。`Ctrl-C` 与
        // `Ctrl-D` 自己不在这里清 —— 它们正是要摸这把举手的那两个键；问卷立着时的 `Esc`
        // 也不是「别的键」，它摸的是槽位里另一把（`.scratch/questionnaire-keys/spec.md` §5）。
        match key {
            Key::CtrlC | Key::CtrlD => {}
            Key::Esc if matches!(self.pending, Some(Pending::Questionnaire(_))) => {}
            _ => self.expire_exit_gesture(),
        }
        // 两键共用一把举手，按当前视图的意思是空闲还是忙碌分派；详情覆盖层与重放各有自己
        // 的分支，已经在上面提前返回（spec §1、§7）。
        if matches!(key, Key::CtrlC | Key::CtrlD) {
            self.exit_key(key);
            return;
        }
        if key == Key::Esc {
            // 问卷立着时 `Esc` 只管「退出这次询问」，不再按 busy / idle / 禁言分叉：那个手势
            // 根本不掐回合，所以禁言那一支要保护的东西在这里不存在
            // （`.scratch/questionnaire-keys/spec.md` §5）。
            if matches!(self.pending, Some(Pending::Questionnaire(_))) {
                self.questionnaire_escape();
                return;
            }
            if self.busy() {
                if !self.muted {
                    self.events.push(FrontEndEvent::Cancel);
                } else {
                    // 目标循环跑着：**默认停在「继续跑」** —— 误按一下不该掐掉一个已经跑了
                    // 两小时的目标（`.scratch/goal-loop/spec.md` §5）。
                    match self.pending {
                        // 已经问出来了：关掉它就是那个安全的答案。
                        Some(Pending::GoalStop) => self.pending = None,
                        // 别的框属于这次运行，所以 `Esc` 对它是取消手势，与别处一样。
                        Some(_) => self.events.push(FrontEndEvent::Cancel),
                        // 没有框：先问一句。
                        None => self.pending = Some(Pending::GoalStop),
                    }
                }
            } else if let Some(pending) = self.pending.take() {
                self.decline(pending);
            } else if self.slash_menu().is_some() {
                // `/` 菜单是屏幕上最小的东西，所以 `Esc` 先关掉它，然后才轮到手扔草稿
                // （spec §6）。
                self.slash.dismissed = true;
            } else if self.editor.has_multiple_lines() {
                // 在一份这么长的草稿上按 Esc 会丢掉真的工作，所以它先问一句 —— 而安全的
                // 答案是「不」（spec §7）。
                self.pending = Some(Pending::ClearDraft);
            } else {
                self.editor.clear();
            }
            return;
        }
        if self.pending.is_some() {
            // 问题占着键盘：它自己的键回答它，上面的 `Ctrl-C` 与 `Esc` 是出路，别的什么都进
            // 不来 —— 一个乱按的字符不行，模式手势也不行（spec §9）。
            //
            // 问卷认的键盘比那些单键问题宽 —— 方向键移动与翻页，`Tab` 跳过，`Enter`/`Space`
            // 确认 —— 所以它拿到每一个键并自己分发。别的种类保持那条窄规矩，正是它让一个
            // 乱按的字符无法批准一次写入。
            if matches!(self.pending, Some(Pending::Questionnaire(_))) {
                self.questionnaire_key(key);
            } else if matches!(key, Key::Char(_) | Key::Enter) {
                self.answer_key(key);
            }
            return;
        }
        // 输入区禁言（§5）：键位照旧响应 —— 方向键、翻页、`Shift+Tab` 都还做事 —— 只是编辑
        // 与提交进不来。这不是「暂停」，循环照常跑。
        if self.muted && matches!(key, Key::Char(_) | Key::Enter | Key::Tab) {
            return;
        }
        if key == Key::BackTab {
            // 手势与显示是同一次循环的一步：模式是循环持有的一个会话值，而前端是唯一显示它
            // 的东西 —— 所以它挪动自己那份副本，并请求同一次挪动。两者都对组装打底的那个值
            // 施加 [`Mode::next`]，这让它们不会互相矛盾
            // （`.scratch/todo-and-modes/spec.md` §1）。
            self.mode = self.mode.next();
            self.events.push(FrontEndEvent::CycleMode);
            return;
        }
        // `/` 菜单立着时，它占着那四个本来会编辑或提交的键：`↑`/`↓` 走过匹配，`Tab` 填进
        // 一个，`Enter` 填进一个并把它发出去。别的都落到编辑器，而编辑器正是用户继续打字时
        // 过滤匹配的地方。
        if self.slash_menu().is_some() {
            match key {
                Key::Down => {
                    self.menu_move(1);
                    return;
                }
                Key::Up => {
                    self.menu_move(-1);
                    return;
                }
                Key::Tab | Key::Enter => {
                    // `Tab` 填进高亮的那个名字就停在那里；`Enter` 填进去**并提交**，于是
                    // `/ask` + Enter 跑起菜单指着的那条 skill。裸 `/` 什么都没高亮：`Enter`
                    // 把它按打出来的样子发出去，循环回一串名字。
                    self.menu_accept();
                    if key == Key::Enter {
                        self.submit();
                    }
                    return;
                }
                _ => {}
            }
        }
        match key {
            Key::Enter => self.submit(),
            Key::PageUp => self.pane.page(true),
            Key::PageDown => self.pane.page(false),
            Key::CtrlG => self.pane.to_bottom(),
            // 编辑器认的每一个别的键；两条路径共用它们。
            _ => self.editor_key(key),
        }
        // 一个改了草稿（或只在 token 里挪了光标）的键，可能把菜单撑宽或收窄了。在这里一次
        // 收进来，而不是在每个分支里各收一次。
        self.sync_menu();
    }

    /// 从流上留一份「现在在推进哪个目标」，给终端标题用（spec §6）。
    ///
    /// 以**正在推进**为准：`events::current_goal` 只认最后一条 `GoalSelected`，目标停下
    /// 之后它仍会返回名字，所以停下与完成这两条都要清。重放走同一个 `apply`，于是
    /// `--continue` 的目标名自然重建，不需要第二套路径。
    fn observe_goal(&mut self, event: &RenderEvent) {
        let RenderEvent::Logged(event) = event else {
            return;
        };
        match &event.payload {
            EventPayload::GoalSelected { goal } => self.goal = Some(goal.clone()),
            EventPayload::GoalStopped { .. } | EventPayload::GoalCompleted { .. } => {
                self.goal = None;
            }
            _ => {}
        }
    }

    /// 循环是否**在一次运行里面**：一个回合，或者它在驱动的一场讨论。
    ///
    /// 这件事由循环通过 [`ConsoleRequest::RunState`] 说出来；这里什么都不推断它。两次推断
    /// 都失败了。从渲染流推：只有 `TurnEnded` 会清掉旧标志，而合成器那一次调用不结束任何
    /// 回合，于是一场讨论之后 TUI 以为自己永远在工作。从「没有未决的提示」推：那个谓词在
    /// 循环要它的*第一*行之前就是真的，于是组装期间空闲的键盘被读成工作中。两个错都把
    /// `Ctrl-C` 变成了一个空闲循环会丢掉的取消手势 —— 一块死键盘。
    fn busy(&self) -> bool {
        self.running
    }

    /// 把打好的草稿交给循环并记住它。
    ///
    /// 提交也把转录送回末尾：用户刚问了一件事，想看着答案，不管他当时在读什么（spec §4）。
    ///
    /// 空草稿作为**一个空行**发出去。通道对「stdin 关了」的哨兵是 `None`（见
    /// [`ConsoleRequest::Prompt`]），而敲 Enter 从来不是那个意思；退出是 `Ctrl-C`（下面那个
    /// 标志）或 `/quit`（一行普通输入）。
    ///
    /// 在**没有行被读**的时候 —— 一个回合进行中，或者一次性的 `discuss`，后者从不索要一行
    /// —— Enter 干脆什么都不做，而不是把草稿扔掉：循环在准备好接一行的时候才要一行
    /// （spec §6），在那之前那份草稿是用户打的东西的唯一副本。
    fn submit(&mut self) {
        // 重放不是一场对话：`Enter` 不许往历史中间打出一个回合。草稿原封不动留在那儿，而
        // 这个键就是不是它看起来的那个提交（`spec` §3）。
        if self.replay.is_some() {
            return;
        }
        let Some(reply) = self.prompt_reply.take() else {
            return;
        };
        self.pane.to_bottom();
        let line = self.editor.submitted();
        let _ = reply.send(Some(line));
    }

    /// 此刻草稿所要求的那个 `/` 菜单，没什么可提供时是 `None`。
    ///
    /// 推出来的，从不保存：草稿与循环的目录就是全部输入。有问题立着时什么都不显示，因为
    /// 问题占着键盘 —— 菜单会提供一些回答别的东西的键（spec §9）。
    fn slash_menu(&self) -> Option<SlashMenu> {
        if self.pending.is_some() || self.slash.dismissed {
            return None;
        }
        let token = self.editor.slash_token()?;
        // 按大小写过滤，但提供的是目录里的那个名字：`/Ask` 找得到 `ask-matt`，而 Tab 写出
        // 循环会认的那个拼法。
        let typed = token.prefix.to_lowercase();
        let entries: Vec<(String, String)> = self
            .catalog
            .iter()
            .filter(|entry| entry.name.to_lowercase().starts_with(&typed))
            .map(|entry| (entry.name.clone(), entry.description.clone()))
            .collect();
        if entries.is_empty() {
            return None;
        }
        Some(SlashMenu {
            prefix: token.prefix,
            selected: self.slash.selected.map(|at| at.min(entries.len() - 1)),
            entries,
        })
    }

    /// 把高亮移动 `delta`，两端回绕。
    fn menu_move(&mut self, delta: isize) {
        let Some(menu) = self.slash_menu() else {
            return;
        };
        let len = menu.entries.len() as isize;
        self.slash.selected = Some(match menu.selected {
            Some(at) => (at as isize + delta).rem_euclid(len) as usize,
            // 什么都没被高亮：`↓` 拿第一行、`↑` 拿最后一行，于是方向键按它被画出来的顺序走过
            // 列表。
            None if delta > 0 => 0,
            None => (len - 1) as usize,
        });
        self.slash.prefix = menu.prefix;
    }

    /// 把高亮的那个名字填进草稿。
    ///
    /// 什么都没高亮时什么都不做 —— 裸 `/` 是一份用来看的列表，不是已经做出的选择。
    fn menu_accept(&mut self) {
        let Some(menu) = self.slash_menu() else {
            return;
        };
        let Some(selected) = menu.selected else {
            return;
        };
        let name = menu.entries[selected].0.clone();
        if !self.editor.complete_slash(&name) {
            return;
        }
        // 记住的前缀随草稿一起走，否则下一次同步会把这次补全读成一次变化，并把刚刚关掉的
        // 东西重新打开。
        self.slash.prefix = self
            .editor
            .slash_token()
            .map(|token| token.prefix)
            .unwrap_or_default();
        self.slash.selected = None;
        self.slash.dismissed = true;
    }

    /// 把草稿当前的 token 收进菜单记住的那个选择里。
    ///
    /// 变了的前缀是另一个菜单：高亮从头开始 —— 打过名字之后落在第一个匹配上，而 token 只是
    /// 一个斜杠时落在什么都不高亮上 —— 而那次关掉旧菜单的 `Esc` 不再作数。
    fn sync_menu(&mut self) {
        let prefix = self
            .editor
            .slash_token()
            .map(|token| token.prefix)
            .unwrap_or_default();
        if prefix != self.slash.prefix {
            self.slash.selected = (!prefix.is_empty()).then_some(0);
            self.slash.prefix = prefix;
            self.slash.dismissed = false;
        }
    }

    /// 用一个按键回答一个问题。
    ///
    /// 循环的问题有各自的词汇表，而认不出来的键退回到那个不起作用的答案，所以一个乱按的
    /// 字符永远无法批准一次写入。渲染器自己的问题只认 `y`（或 Enter），别的都不认。
    fn answer_key(&mut self, key: Key) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.answer_pending(pending, key);
    }

    /// [`TuiState::answer_key`] 的主体，给已经拿着那个问题的调用方 —— 指针那条路，它在回答
    /// 之前得先看看它。
    fn answer_pending(&mut self, pending: Pending, key: Key) {
        match pending {
            Pending::Loop { reply, .. } => {
                let answer = match key {
                    Key::Char('y') => Answer::Allow,
                    Key::Char('a') => Answer::AlwaysAllow,
                    // 别的任何东西都是不起作用的那种读法：一个乱按的字符永远无法批准一次写入
                    // （spec §9）。
                    _ => Answer::Deny,
                };
                let _ = reply.send(answer);
            }
            Pending::Paste { text, .. } => {
                if agrees(key) {
                    self.editor.insert_str(&text);
                    self.sync_menu();
                }
            }
            Pending::ClearDraft => {
                if agrees(key) {
                    self.editor.clear();
                }
            }
            // **默认停在「继续跑」**：只有明确按 `s` 才停，`Enter` 与别的键都是继续。
            Pending::GoalStop => {
                if matches!(key, Key::Char('s') | Key::Char('S')) {
                    self.events.push(FrontEndEvent::Cancel);
                }
            }
            // 到不了：`key` 在到这之前就把问卷路由给 `questionnaire_key` 了，因为问卷认的
            // 键盘更宽。在这里丢掉它会拒掉那个工具，所以它只为了让 match 保持穷尽而留着。
            Pending::Questionnaire(_) => {}
        }
    }

    /// 对一个问题，`Esc` 是什么意思：不起作用的那个答案；问题是这个渲染器自己提的时，
    /// 什么都不做。
    fn decline(&mut self, pending: Pending) {
        match pending {
            Pending::Loop { reply, .. } => {
                let _ = reply.send(Answer::Deny);
            }
            // 问卷立着时的 `Esc` 现在是「退出这次询问」，它正是走这里：丢掉发送端就是诚实的
            // 「没有答案」，模型继续跑（`.scratch/questionnaire-keys/spec.md` §5）。退出确认是
            // 渲染器自己的，而 `Esc` 是它的安全答案：拒绝，留在里面。
            Pending::Questionnaire(_) | Pending::Paste { .. } | Pending::ClearDraft => {}
            // `Esc` 关掉它，意思就是**继续跑** —— 那个安全的答案。
            Pending::GoalStop => {}
        }
    }

    fn status_line(&self, width: u16) -> String {
        // 只有**退出**那一把会换提示行的出口段：举着「退出这次询问」时那句
        // 「再按一次 ctrl-c/ctrl-d 退出」是错的（问卷的举手由它自己的页脚说，见票 04）。
        let raised = self.raised_gesture(std::time::Instant::now()) == Some(Gesture::Exit);
        // 重放自己会说话：它临时用自己的进度替换掉提示集合。举手时那句催促压在进度行前面
        // ——「再按一次」比一个 `n/m` 更急，而进度行只是暂时让位（spec §2）。
        if let Some(replay) = &self.replay {
            if raised {
                return wording::EXIT_HINT_REPLAY_RAISED.to_owned();
            }
            return wording::history_progress_line(replay.next, replay.events.len(), width);
        }
        // 提示说的是键盘*现在*干什么。没有行被读的时候 —— 一个回合进行中，或者一次性的
        // `discuss` —— `enter 发送` 会是一个这个会话兑现不了的承诺（spec §6）。
        if self.prompt_reply.is_some() {
            wording::status_line(self.busy(), width, raised)
        } else {
            wording::viewer_status_line(self.busy(), width, raised)
        }
    }

    /// 底部块这一帧要多少内容行。
    ///
    /// 平常由常驻编辑器的草稿决定；问卷占着输入区时由问卷决定 —— 那正是底部块会长高到装下
    /// 问题与它的选项的原因（spec §19）。
    fn bottom_rows(&self, area: Rect) -> u16 {
        match self.questionnaire() {
            Some(questionnaire) => questionnaire_lines(
                &questionnaire.questions[questionnaire.index],
                &questionnaire.drafts[questionnaire.index],
                layout::content_width(area, self.sidebar_wanted) as usize,
                questionnaire.zone == Zone::Options,
            )
            .len() as u16,
            None => self
                .editor
                .height(layout::input_text_width(area, self.sidebar_wanted)),
        }
    }
}

/// 一个问题的那些行：它的表头、它的文字、它编号的选项，以及一行用来打答案的地方。
///
/// 它的长度正是 [`TuiState::bottom_rows`] 向排版要的东西，所以它是**完整**的那份列表；
/// 画家通过 [`questionnaire_window`] 裁剪并滚动它。选项的编号只是给读的 —— 定下来的键盘
/// 没有数字键 —— 而一个推荐选项会拿到一个展示用的小标记，它底下的标签（也就是答案携带的
/// 那个值）则原样不动（spec §7）。
fn questionnaire_lines(
    question: &UserQuestion,
    draft: &QuestionDraft,
    width: usize,
    options_focused: bool,
) -> Vec<Line<'static>> {
    let (mut rows, options, custom) = questionnaire_parts(question, draft, width, options_focused);
    rows.extend(options.into_iter().flatten());
    rows.extend(custom);
    rows
}

/// 一个问题在 `height` 行内装得下的那些行，滚动选项窗口好让高亮的选项始终可见（spec §7）。
///
/// 表头与问题文本是钉住的：它们说在问什么，把它们滚掉会让选项变得读不懂。打答案的那几行
/// 出于同样的理由钉在底部。中间那些选项就是窗口，它跟着高亮走：往下越过裁剪线时，滚出来
/// 的是尾部，而不是把高亮留在屏幕外。
///
/// 窗口的**单位是行、不是选项**：一个折成三行的长选项要整块看得见
/// （`.scratch/questionnaire-keys/spec.md` §7）。
fn questionnaire_window(
    question: &UserQuestion,
    draft: &QuestionDraft,
    width: usize,
    height: usize,
    options_focused: bool,
) -> Vec<Line<'static>> {
    let (prefix, options, custom) = questionnaire_parts(question, draft, width, options_focused);
    let geometry = option_window_geometry(
        &options,
        draft.highlight,
        height,
        prefix.len(),
        custom.len(),
    );
    // 装得下就全画，装不下就开窗 —— 两种形态最后都按 `height` 收口，于是输入区折出来的
    // 续行既不会被截成一行，也不会画到窗格外面去（`.scratch/questionnaire-keys/spec.md` §7）。
    let fits = prefix.len() + geometry.total + custom.len() <= height;
    let mut rows = prefix;
    if fits {
        rows.extend(options.into_iter().flatten());
    } else {
        rows.extend(
            options
                .into_iter()
                .skip(geometry.start)
                .flatten()
                .take(geometry.room),
        );
    }
    rows.extend(custom);
    rows.truncate(height);
    rows
}

/// 选项窗口的几何：每个选项占几行、一共几行、留给窗口几行、第一个该画的选项。
///
/// 画的那一遍与登记点击区域的那一遍各自要一次，所以它只算在这一处 —— 两边不会对「窗口从
/// 哪儿开始」有分歧（`.scratch/questionnaire-keys/spec.md` §7）。
struct OptionWindowGeometry {
    /// 所有选项一共占几行。
    total: usize,
    /// 留给窗口的行数。
    room: usize,
    /// 第一个该画的选项。
    start: usize,
}

fn option_window_geometry(
    options: &[Vec<Line<'static>>],
    highlight: usize,
    height: usize,
    prefix_rows: usize,
    custom_rows: usize,
) -> OptionWindowGeometry {
    let heights: Vec<usize> = options.iter().map(Vec::len).collect();
    let total: usize = heights.iter().sum();
    // 题面（前缀）与输入区那几行都是**钉住**的：它们先占，剩下的才是选项窗口。输入区自己
    // 折了几行就占几行（`.scratch/questionnaire-keys/spec.md` §7）。
    let room = height.saturating_sub(prefix_rows + custom_rows);
    let start = if prefix_rows + total + custom_rows <= height {
        0
    } else {
        option_window_start(&heights, highlight, room)
    };
    OptionWindowGeometry { total, room, start }
}

/// 为了让**高亮那一项**完整落在 `room` 行的窗口里，第一个该画的选项。
///
/// 列表不回绕：窗口跟着高亮走。做法是把高亮那一项的底部贴在窗口底部，再回退到包含那个
/// 行号的选项的起点；高亮项自己就超过整个窗口时不再往前退，从它的头部画起。
fn option_window_start(heights: &[usize], highlight: usize, room: usize) -> usize {
    if room == 0 || heights.is_empty() {
        return 0;
    }
    let total: usize = heights.iter().sum();
    if total <= room {
        return 0;
    }
    let highlight = highlight.min(heights.len() - 1);
    let end: usize = heights[..=highlight].iter().sum();
    let bottom = end.saturating_sub(room);
    let mut start = 0;
    let mut drawn = 0;
    while start < highlight && drawn + heights[start] <= bottom {
        drawn += heights[start];
        start += 1;
    }
    start
}

/// 把一个问题的钉住部分（表头与文本）、选项行、以及打答案的那一行拆开。
///
/// 拆开是为了那个滚动的窗口；每一行怎么组成只在这里写一次，这样完整列表与窗口不会对
/// 「一个选项读起来是什么样」有分歧。
fn questionnaire_parts(
    question: &UserQuestion,
    draft: &QuestionDraft,
    width: usize,
    options_focused: bool,
) -> (
    Vec<Line<'static>>,
    Vec<Vec<Line<'static>>>,
    Vec<Line<'static>>,
) {
    let mut prefix: Vec<Line<'static>> = Vec::new();
    if let Some(header) = question
        .header
        .as_deref()
        .map(str::trim)
        .filter(|header| !header.is_empty())
    {
        for mut row in pane::wrap_text(header, width) {
            row.style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            prefix.push(row);
        }
    }
    let mut title = question.question.clone();
    if question.multi_select {
        title.push_str(wording::questionnaire_multi_marker());
    }
    prefix.extend(pane::wrap_text(title.trim(), width));

    let mut options: Vec<Vec<Line<'static>>> = Vec::with_capacity(question.options.len());
    for (index, choice) in question.options.iter().enumerate() {
        let highlighted = index == draft.highlight;
        let picked = draft
            .selected
            .iter()
            .any(|selected| selected == &choice.label);
        let marker = match (question.multi_select, picked) {
            (true, true) => "[x]",
            (true, false) => "[ ]",
            (false, true) => "●",
            (false, false) => "○",
        };
        // 光标说的是 `Enter`/`Space` 会确认哪个选项；标记说的是哪些被选中了。这是两个不同的
        // 事实，可以不一致。
        let cursor = if highlighted { ">" } else { " " };
        let lead = format!("{cursor} {marker} ");
        let body =
            wording::questionnaire_option(index + 1, &choice.label, choice.description.as_deref());
        let mut style = if picked {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        if highlighted {
            // 反显说的是「键盘在这里」；输入区拿着键盘时它降暗，屏幕上于是只有一个焦点
            // （`.scratch/questionnaire-keys/spec.md` §8）。
            style = if options_focused {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style.add_modifier(Modifier::DIM)
            };
        }
        // 长选项**折行**，不截断（`.scratch/questionnaire-keys/spec.md` §7）。
        let mut lines = wrap_with_lead(&body, &lead, width);
        for line in &mut lines {
            line.style = style;
        }
        options.push(lines);
    }

    let label = if question.options.is_empty() {
        wording::questionnaire_answer_label()
    } else {
        wording::questionnaire_custom_label()
    };
    // 输入区拿着键盘时那一行提亮：光标就在那儿（`.scratch/questionnaire-keys/spec.md` §8）。
    let lead_style = if options_focused {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    };
    let mut custom = wrap_with_lead(&draft.custom, label, width);
    match custom.first_mut() {
        Some(line) => {
            if let Some(lead) = line.spans.first_mut() {
                lead.style = lead_style;
            }
        }
        // 空的自由文本仍然要占那一行：它是输入区，光标与点击都落在它上面。
        None => custom.push(Line::from(Span::styled(label, lead_style))),
    }
    (prefix, options, custom)
}

/// 把一段文本折成若干行：第一行带 `lead`（`> ○ ` 这种），续行用等宽的空格缩进。
///
/// 前缀也算进折行宽度，所以每一行都不超过 `width` 列；续行缩进让折行读起来仍在同一个选项里。
fn wrap_with_lead(body: &str, lead: &str, width: usize) -> Vec<Line<'static>> {
    let lead_columns = text_columns(lead);
    let body_width = width.saturating_sub(lead_columns).max(1);
    let indent = " ".repeat(lead_columns);
    pane::wrap_text(body, body_width)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            let pad = if index == 0 { lead } else { indent.as_str() };
            let mut spans = vec![Span::raw(pad.to_owned())];
            spans.extend(row.spans);
            Line::from(spans)
        })
        .collect()
}

/// 画一帧外壳。
///
/// 这是排版被测试所通过的接缝：一个状态进去，一帧固定尺寸的画出来，不涉及任何终端
/// （spec §2）。
pub fn draw_frame(frame: &mut ratatui::Frame, state: &mut TuiState) {
    let area = frame.area();
    // 指针是在两帧之间被回应的，而打开详情需要这一帧被画出来时的宽度。
    state.area = area;
    // 上一帧记下来的才是指针可能打中的；这一帧从什么都没有开始，只记它真画出来的东西
    // （票 04 §1）。
    state.regions.clear();
    // 覆盖层「在哪儿」与命中区域同一条纪律：这一帧画在哪儿，指针才可能落在哪儿
    // （`tui-chrome` §5）。两者都在下面各自画出来时被重新填上。
    state.modal_rect = None;
    state.questionnaire_bottom = None;
    if layout::below_minimum(area) {
        // 什么都不画，好让点击无处可落。
        state.indicator = None;
        state.detail_rect = None;
        draw_too_small(frame, area);
        return;
    }
    // 草稿自己的高度决定输入区占多少位置：它随文字长高、直到排版的上限，然后就地滚动
    // （spec §2）。问卷用自己的高度替换掉它，于是输入区长高到装下问题（spec §19）。
    let content_rows = state.bottom_rows(area);
    let panes = layout::plan(area, content_rows, state.sidebar_wanted);
    // 问卷没有边框：它占的就是排版给底部的那两块（输入区与提示行，连中间那条线一起）。
    // 没有问卷时它就是 `None`，滚轮于是落到转录上（`tui-chrome` §5）。
    if state.questionnaire().is_some() {
        state.questionnaire_bottom = Some(Rect::new(
            panes.input.x,
            panes.input.y,
            panes.input.width,
            panes.hints.bottom().saturating_sub(panes.input.y),
        ));
    }
    draw_shell(frame, &panes, state, area);
    draw_transcript(frame, &panes, state);
    draw_status(frame, &panes, state);
    let anchor = draw_bottom(frame, &panes, state);
    // `/` 菜单浮在主列之上、在它所属的那个光标下面 —— 也浮在问题之下，因为问题占着键盘、
    // 于是没有菜单可提供（spec §6、§9）。
    if let Some(anchor) = anchor {
        draw_menu(frame, &panes, state, anchor);
    }
    // 最后画，所以它在它所问的那条转录之上。
    draw_modal(frame, &panes, state);
    // 详情覆盖层盖在所有这一切之上。它不可能与一个问题同时立着 —— 打开它需要一个空闲的
    // 键盘 —— 所以两者之间的顺序只是形式（票 02 §4）。
    draw_detail(frame, &panes, state);
}

/// 外壳里那些不是自己一块区域的部件：分隔列、左栏，以及主列的两条分隔线。
///
/// **外框已经不在**（spec §1）：每条线都画在自己该在的地方，没有哪一格要留给边框，也没有
/// 交叉符要拼。这个顺序就是绘制顺序：先沿左栏右边缘往下画的分隔列，然后是左栏 —— 它的页签
/// 条在两者之上画 —— 最后是主列的分隔线（spec §1–§2）。
fn draw_shell(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
    area: Rect,
) {
    draw_divide(frame, panes, area);
    draw_sidebar(frame, panes, state);
    // 输入区与提示行各自上面那条分隔线 —— 状态行上方那条已经离开（spec §2），所以这里是
    // 两条而不是三条。它们从**分隔列右边一格**起画：分隔列那一格的 `┆` 留着，于是竖线从
    // 屏幕顶一直贯通到底，横线只是接在它旁边（2026-10-01 真机反馈：横线原先把竖线截断了）。
    // 没有左栏时就没有那条竖线，横线从屏幕左缘起。
    let left = panes.divide.map_or(area.x, |divide| divide + 1);
    for y in [panes.input.y - 1, panes.hints.y - 1] {
        paint_rule(frame, y, left, area.right());
    }
}

/// 一行横贯某个行、从 `left` 到 `right`（不含 `right`）的分隔线（spec §1、§3）。
///
/// 虚线，颜色是 [`CHROME_LINE`]：框架退到内容后面。主列的分隔线与页签条用的是同一笔；它们
/// 只在跨的列上不同。
fn paint_rule(frame: &mut ratatui::Frame, y: u16, left: u16, right: u16) {
    let style = Style::default().fg(CHROME_LINE);
    let buffer = frame.buffer_mut();
    for x in left..right {
        buffer[(x, y)].set_symbol("┄").set_style(style);
    }
}

/// 左栏与主列共用的那一列：一条从屏幕顶到屏幕底的竖虚线（spec §1、§3）。
///
/// 外框走了之后，这一列不再有「上边框 / 下边框」可以接，所以两端也就是同样的 `┆`。
fn draw_divide(frame: &mut ratatui::Frame, panes: &layout::Regions, area: Rect) {
    let Some(divide) = panes.divide else {
        return;
    };
    let style = Style::default().fg(CHROME_LINE);
    let buffer = frame.buffer_mut();
    for y in area.y..area.bottom() {
        buffer[(divide, y)].set_symbol("┆").set_style(style);
    }
}

/// 左栏，自上而下：身份、页签条，以及页签选中的那一页（spec §3）。每个部件一个函数，
/// 因为每个都有自己会变的理由 —— 那条阶梯、页签的行为，以及页面的内容。
///
/// 左栏在哪、它的各部件多高，是排版的事，绝不在这里重写一遍尺寸判断。
fn draw_sidebar(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let (Some(sidebar), Some(tabs)) = (panes.sidebar, panes.tabs) else {
        return;
    };
    draw_sidebar_identity(frame, panes, sidebar);
    draw_tab_bar(frame, panes, state, sidebar, tabs);
    draw_sidebar_page(frame, panes, state);
}

/// 左栏的身份：标记、文字身份，或者什么都没有（spec §3）。
///
/// 三者选哪一个是排版的决定 —— [`layout::SidebarKind`] —— 于是那条阶梯只有一个家。标记在
/// 宽档里居中，那档宽度就是标记自己的宽度加左右各一列留白。这里什么都不动：标记的下落
/// 短横与文字身份的那一半都在票 08 关掉了（`.scratch/tui-input-pulse/spec.md` §2），这个
/// 界面里全部的动画如今都活在提示符的颜色里。
fn draw_sidebar_identity(frame: &mut ratatui::Frame, panes: &layout::Regions, sidebar: Rect) {
    let dim = Style::default().fg(Color::DarkGray);
    match panes.sidebar_kind {
        layout::SidebarKind::Mark => {
            // 标记是 38 列，而宽档是 40 列，所以它居中时左右各留一列白；比标记还窄的档位
            // 根本不会要这几行（spec §2）。
            let offset = sidebar.width.saturating_sub(layout::LOGO_WIDTH) / 2;
            // `None`：标记不动（票 08）。下落短横还在这里 —— 它的帧就在它旁边有单元测试
            // —— 但维护者把它关了，左栏回到了这一切之前那个静止的标记。
            let lines: Vec<Line<'static>> = mark_lines(None)
                .into_iter()
                .map(|(text, color)| Line::from(Span::styled(text, Style::default().fg(color))))
                .collect();
            let rows = lines.len() as u16;
            frame.render_widget(
                Paragraph::new(lines),
                Rect::new(
                    sidebar.x + offset,
                    sidebar.y,
                    sidebar.width.saturating_sub(offset),
                    rows,
                ),
            );
        }
        layout::SidebarKind::Text => {
            // 静止的身份：窄档那一半下落短横随标记的一起退出了屏幕（票 08）。
            // `wording::identity_falling` 就留在它旁边，等那个想法回来。
            let identity = wording::identity();
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(identity, dim))),
                Rect::new(sidebar.x, sidebar.y, sidebar.width, 1),
            );
        }
        layout::SidebarKind::Hidden => {}
    }
}

/// 页签选中的那一页（spec §3）。
///
/// 这些行来自排版的高度阶梯，所以被压扁的左栏是从尾部丢字段，而不是把三个要紧的读数裁掉
/// （spec §2）。还没做出来的页面用一行说出来，而不是显示编出来的数据。
fn draw_sidebar_page(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &TuiState) {
    let Some(page) = panes.sidebar_page else {
        return;
    };
    let rows = match state.tab {
        Tab::Usage => state.panel.lines(&state.facts, page),
        Tab::Todo => state.todo.lines(page),
        Tab::Trace | Tab::Files => vec![Line::from(Span::styled(
            truncate_columns(wording::tab_placeholder(), page.width as usize),
            Style::default().fg(Color::DarkGray),
        ))],
    };
    frame.render_widget(Paragraph::new(rows), page);
}

/// 左栏的页签条：两条分隔线、标签夹在中间，选中的那个更亮（spec §3，
/// `.scratch/todo-and-modes/spec.md` §4）。
///
/// 两条线都从屏幕左缘开始、到分隔列结束，于是左栏读起来是一个隔间，而不是自成一体的
/// 一块。每个标签在画出来时记下一个命中矩形：指针只能打中真在那里的东西，而填满这一行
/// 其余部分的线不是页签。
///
/// 标签列表是建出来的而不是写死的，因为 `todo` 是有条件的 —— 会话有了列表它才在条上 ——
/// 别的都从它推出来：分隔符、填充与命中矩形都从同一份列表来，所以没被画出来的标签点不到。
fn draw_tab_bar(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
    sidebar: Rect,
    tabs: Rect,
) {
    let dim = Style::default().fg(Color::DarkGray);
    // 「线」与「字」在这一行上分开取色：未选中的标签是**文字**（仍旧 `DarkGray`，它得读得
    // 出来），而两条线与它们之间的分隔符是**框架**（`CHROME_LINE`，退到后面去）。
    let rule = Style::default().fg(CHROME_LINE);
    for y in [tabs.y - 1, tabs.y + 1] {
        paint_rule(
            frame,
            y,
            panes.screen.x,
            panes.divide.unwrap_or(sidebar.right()),
        );
    }
    // 标签、之间一个分隔符，这一行其余部分用线填满，于是这一行读起来是一根条，而不是几个
    // 落单的词。
    let mut entries = vec![(Tab::Usage, wording::TAB_USAGE)];
    if state.todo.visible() {
        entries.push((Tab::Todo, wording::TAB_TODO));
    }
    entries.push((Tab::Trace, wording::TAB_TRACE));
    entries.push((Tab::Files, wording::TAB_FILES));
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0u16;
    for (index, (tab, label)) in entries.iter().enumerate() {
        let selected = *tab == state.tab;
        let style = if selected {
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD)
        } else {
            dim
        };
        let width = text_columns(label) as u16;
        if used + width <= sidebar.width {
            state.regions.cells.push(Region {
                rect: Rect::new(tabs.x + used, tabs.y, width, 1),
                action: HitAction::SwitchTab(*tab),
            });
        }
        spans.push(Span::styled((*label).to_owned(), style));
        used += width;
        if index + 1 < entries.len() {
            spans.push(Span::styled("┆", rule));
            used += 1;
        }
    }
    spans.push(Span::styled(
        "┄".repeat(sidebar.width.saturating_sub(used) as usize),
        rule,
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), tabs);
}

/// 状态行：哪个模型、哪个模式，以及窗口有多满（spec §5）。
///
/// 三段是画出来的，不可点：这一行上没有任何东西是控件，所以这里什么都不记命中区域。宽度的
/// 阶梯住在 [`wording::status_row`] 里；这一行本身总是画出来的，连它最后一档都容不下的宽度
/// 会被截断而不是丢掉（spec §2）。
fn draw_status(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &TuiState) {
    let share = wording::context_share(state.panel.last_input(), state.facts.context_window);
    let width = panes.status.width as usize;
    let text = wording::status_row(
        &state.facts.model,
        &wording::mode_field(state.mode),
        &share,
        width,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_columns(&text, width),
            Style::default().fg(Color::DarkGray),
        ))),
        panes.status,
    );
}

/// 正在显示左栏的哪一页（spec §3）。
///
/// 点出来的，从不给键位：`Tab` 归 `/` 菜单、`Shift+Tab` 归模式循环，而这个仓库不启用
/// keyboard-enhancement 协议。还没做出来的页面显示 [`wording::tab_placeholder`]，而不是编
/// 出来的数据。
///
/// 接受它的代价是：在占位页上，会话的读数根本不在屏幕上，于是状态行的 `上下文 n%` 是唯一
/// 剩下的那个。这不是 bug —— 没有第二份数字可以退回 —— 而另一条路（用键盘走页签）在这里
/// 本来也不通。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    /// 会话的读数：旧信息面板装着的那些。
    Usage,
    /// agent 的待办列表，也是唯一不总在条上的页签：主会话第一次提交一次列表时它出现，此后
    /// 整个会话都在（`.scratch/todo-and-modes/spec.md` §4）。
    Todo,
    /// 调用轨迹。还没做。
    Trace,
    /// 这个会话碰过的文件。还没做。
    Files,
}

/// 问题被问出来时所在的覆盖层（spec §9）。
///
/// 它坐在主列中间，这样问题不会被新输出甩在后面，而且它**不是**转录的一部分：流上仍然带着
/// `PermissionAsked` 块，给回看的人。把它居中在主列而不是整个终端，能让左栏的读数在问题
/// 立着时仍然可见（spec §1）。它立着的时候占着指针，所以「回到末尾」那个矩形被丢掉。
fn draw_modal(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let Some(modal) = state.pending.as_ref().and_then(Pending::modal) else {
        return;
    };
    // 从这里开始问题就立着了，而覆盖层盖住指示器：它原来所在的位置上的一次点击不许起作用，
    // 哪怕覆盖层自己最后发现没有地方可画。
    state.indicator = None;
    let inner = panes.modal_width().saturating_sub(2) as usize;
    if inner == 0 {
        return;
    }
    // 那些行，按顺序。长的一律折到下一行，而不是丢掉键；覆盖层仍然把中间块自己的边框露出来。
    let rows_available = panes.main.height.saturating_sub(2) as usize;
    if rows_available == 0 {
        return;
    }
    let mut rows: Vec<Line<'static>> = pane::wrap_text(modal.title.trim(), inner);
    for row in &mut rows {
        row.style = Style::default().add_modifier(Modifier::BOLD);
    }
    // 这个动作是什么，然后是调用本身：先给出读的人能据以行动的那句话，再给出它下面确切的
    // 参数。
    // 这次调用是为了什么，然后是它将要跑起来的样子：先给方向，再给正在被批准的东西。
    if let Some(description) = modal.description.as_deref() {
        rows.extend(pane::wrap_text(description.trim(), inner));
    }
    for note in &modal.notes {
        rows.extend(pane::wrap_text(note.trim(), inner));
    }
    if let Some(detail) = modal.detail.as_deref() {
        rows.extend(pane::wrap_text(detail.trim(), inner));
    }
    // 按钮行的预算最先分：它是问题少不掉的那一行。把它隔开的那条空行也要花掉一行，但只在既有
    // 地方放它、上面又有东西可隔的时候。
    let separator = usize::from(rows_available >= 3 && !rows.is_empty());
    rows.truncate(rows_available.saturating_sub(1 + separator));
    if separator == 1 {
        rows.push(Line::default());
    }
    // 主体是按钮行上面的那些，而按钮拿走覆盖层会有的最后一行，所以两者都在要矩形之前就定下来
    // 了。
    let body_rows = rows.len() as u16;
    rows.push(Line::default());
    let buttons = modal.choices;
    let Some(area) = panes.modal(rows.len() as u16) else {
        return;
    };
    // 滚轮要问「指针是不是落在模态上」（`tui-chrome` §5）：与点击的命中区域同一条纪律，
    // 只记这一帧真画出来的那个矩形。
    state.modal_rect = Some(area);
    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    frame.render_widget(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .border_set(border::LIGHT_TRIPLE_DASHED)
            .border_style(Style::default().fg(Color::Yellow)),
        area,
    );
    // 主体与按钮行分开画，这样按钮的列能被准确记下来：问题占着它们，而一次点击必须落在它看
    // 起来落在的那个按钮上（票 04 §3）。
    let inner_area = layout::inner(area);
    let body = Rect::new(
        inner_area.x,
        inner_area.y,
        inner_area.width,
        body_rows.min(inner_area.height),
    );
    frame.render_widget(
        Paragraph::new(rows)
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center),
        body,
    );
    // 按钮行是按**它自己的宽度**居中的，不是按主体的：主体是居中的文本，而按钮是更短的一行，
    // 继承主体的内缩会让它们卡在左边（2026-09-23，用户报告）。
    let (line, regions) = buttons_row(buttons, &modal.actions);
    let buttons_width = regions.iter().map(|(start, width, _)| start + width).max();
    let buttons_area = Rect::new(
        inner_area.x + centred_inset(inner_area.width, buttons_width),
        inner_area.bottom().saturating_sub(1),
        inner_area.width,
        1,
    );
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(Color::Yellow)),
        buttons_area,
    );
    state
        .regions
        .cells
        .extend(regions.into_iter().filter_map(|(start, width, action)| {
            // 比覆盖层还宽的按钮，越过边框的部分点不到：没被画出来的那部分没有区域（票 04 §3）。
            let x = buttons_area.x + start as u16;
            let room = buttons_area.right().saturating_sub(x).min(width as u16);
            (room > 0).then_some(Region {
                rect: Rect::new(x, buttons_area.y, room, 1),
                action,
            })
        }));
}

/// 一个 `width` 列宽的东西在一个 `room` 列宽的区域里从哪一列开始，好让它在其中居中。
fn centred_inset(room: u16, width: Option<usize>) -> u16 {
    let width = width.unwrap_or(0).min(room as usize) as u16;
    room.saturating_sub(width) / 2
}

/// 回答一个问题的那些键，做成一行 `(偏移, 宽度, 动作)` 三元组：偏移是从那段文字开头算的
/// **列**数，宽度是那段文字的显示宽度。
///
/// 每个选择一个 `[y] 允许`，之间三个空格：覆盖层一直画的就是这一行，如今配上点它意味着什么，
/// 因为画它的人与给它做命中测试的人必须是同一个函数，否则两者会漂开（票 04 §7）。
fn button_regions(
    choices: &[wording::Choice],
    actions: &[HitAction],
) -> Vec<(usize, usize, HitAction)> {
    let mut regions = Vec::with_capacity(choices.len());
    let mut offset = 0usize;
    for (index, choice) in choices.iter().enumerate() {
        if index > 0 {
            offset += text_columns("   ");
        }
        let width = text_columns(&button_text(choice));
        // 动作列表与选项来自同一个地方，所以两者不会对「哪个按钮是什么意思」有分歧
        // （票 04 §7）。
        let action = actions.get(index).copied().unwrap_or(HitAction::Dismiss);
        regions.push((offset, width, action));
        offset += width;
    }
    regions
}

/// 一个按钮的文字，与覆盖层一直画的完全一样。
fn button_text(choice: &wording::Choice) -> String {
    format!("[{}] {}", choice.key, choice.label)
}

/// 按钮行作为一行带样式的文本，以及它那些可点的列。
fn buttons_row(
    choices: &[wording::Choice],
    actions: &[HitAction],
) -> (Line<'static>, Vec<(usize, usize, HitAction)>) {
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::Yellow);
    let mut spans = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(format!("[{}]", choice.key), key_style));
        spans.push(Span::styled(format!(" {}", choice.label), label_style));
    }
    (Line::from(spans), button_regions(choices, actions))
}

/// 把浮动框即将画上去的那个宽字形涂成空白。
///
/// 一个宽字形占两个单元格，而它后面那个单元格在把帧差分给终端时会被**跳过** —— 于是画在第二
/// 个单元格上的边框会被悄悄丢掉，框在任何非 ASCII 的东西上就少一个角。何况半个字形也画不
/// 出来：字形让位，边框保持完整。
fn blank_half_covered_glyphs(frame: &mut ratatui::Frame, area: Rect) {
    let buffer = frame.buffer_mut();
    let last = area.bottom().min(buffer.area.bottom());
    for y in area.y..last {
        if area.x > buffer.area.left() && buffer[(area.x - 1, y)].symbol().cell_width() > 1 {
            buffer[(area.x - 1, y)].set_symbol(" ");
        }
    }
}

/// 低于最小尺寸的终端会拿到的全部东西：一句话居中地说出来，而不是一副被压扁的外壳。
fn draw_too_small(frame: &mut ratatui::Frame, area: Rect) {
    let row = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
    frame.render_widget(
        Paragraph::new(wording::too_small(layout::MIN_WIDTH, layout::MIN_HEIGHT))
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center),
        row,
    );
}

/// 在一个脉冲帧上提示符的颜色（`.scratch/tui-input-pulse/spec.md` §2b）。
///
/// 维护者的脚本，翻译过来的样子：色相以每秒 0.3 圈走过色环 —— 每 3.3 秒一整圈 —— 同时饱和
/// 度在 0.55 上下呼吸，幅度 0.2、每 2.1 秒一次，而明度留在 0.85，好让这个字形永远不喊叫。
/// **它只在一次运行进行中的时候走**：帧来自循环的时钟，那时才武装、别时不武装，而运行结束时
/// 计数器复位，所以歇着的提示符永远穿帧 0 的颜色（票 09）。
/// 呼吸的意义正在于此：只变色相就是换了个颜色，而一个还会胀缩的颜色，读起来才像活的。
///
/// 它是 24 位色，这个界面里唯一不是 16 色 ANSI 码的地方 —— 提示符两边都坐在终端自己的背景
/// 上，而一个必须在十六个名字里挑一个的色相会看得见台阶。整个函数是帧的纯函数，所以测试
/// 不必有终端就能说出帧 0 长什么样。
fn prompt_colour(frame: u64) -> Color {
    let seconds = frame as f64 * PULSE_FRAME.as_secs_f64();
    let hue = (seconds * PROMPT_HUE_PER_SECOND) % 1.0;
    let saturation =
        PROMPT_SATURATION + PROMPT_SATURATION_BREATH * (seconds * PROMPT_BREATH_PER_SECOND).sin();
    let (red, green, blue) = hsv_to_rgb(hue, saturation, PROMPT_VALUE);
    Color::Rgb(red, green, blue)
}

/// 提示符的色相走色环有多快，单位是每秒圈数。维护者的脚本每 1/60 秒走 0.005，这三个常量就是
/// 那个：脚本的速率换成秒，这样换个帧长，样子还留得住。
const PROMPT_HUE_PER_SECOND: f64 = 0.3;

/// 提示符呼吸的中心饱和度、它摆多远，以及摆多快（每秒弧度：脚本里的每 1/60 秒 0.05）。
const PROMPT_SATURATION: f64 = 0.55;
const PROMPT_SATURATION_BREATH: f64 = 0.2;
const PROMPT_BREATH_PER_SECOND: f64 = 3.0;

/// 提示符保持的明度（亮度）：在暗色主题上够亮、读得清，在亮色主题上够暗、不刺眼。
const PROMPT_VALUE: f64 = 0.85;

/// HSV 转 RGB，按它来源那个脚本里 `colorsys.hsv_to_rgb` 的算法 —— 包括截到 8 位，这样同一帧
/// 给出的颜色与脚本当年给出的一样。
fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> (u8, u8, u8) {
    // 色环的每六分之一是一个色相升、下一个色相降；`sector` 是第几个六分之一，`offset` 是在
    // 里面走了多远。
    let scaled = (hue.fract() * 6.0).rem_euclid(6.0);
    let sector = scaled.floor();
    let offset = scaled - sector;
    let (rising, falling) = (
        value * (1.0 - saturation * (1.0 - offset)),
        value * (1.0 - saturation * offset),
    );
    let (red, green, blue) = match sector as u32 {
        0 => (value, rising, value * (1.0 - saturation)),
        1 => (falling, value, value * (1.0 - saturation)),
        2 => (value * (1.0 - saturation), value, rising),
        3 => (value * (1.0 - saturation), falling, value),
        4 => (rising, value * (1.0 - saturation), value),
        _ => (value, value * (1.0 - saturation), falling),
    };
    let byte = |channel: f64| (channel * 255.0).clamp(0.0, 255.0) as u8;
    (byte(red), byte(green), byte(blue))
}

/// 界面对「它在工作吗？」曾经给过的最忙的一个答案，做成一圈颜色 —— **留着，而且刻意不在屏幕上**
/// （`.scratch/tui-input-pulse/spec.md` §2，票 05）。
///
/// 它有两个版本在真终端上跑过，两个都被否了：12 帧亮/普通成对、100 ms 一帧，读起来像*闪*；
/// 6 个亮色相、400 ms，读起来像*生硬切换* —— 一圈颜色会把整个标记一次换掉，而眼睛在帧与帧
/// 之间没有东西可跟。今天在屏幕上的是 `fs-agent` 里下落的那根短横（见 [`mark_lines`]）；
/// 这圈颜色留在这里，因为用户要求把代码留着而不是删掉，也因为一旦有了让它动起来而不是跳过去
/// 的办法，一个颜色信号是合理得会再想要的东西。
///
/// **每一个条目都是亮色变体，这条性质是承重的**：一个颜色信号要读起来不像闪，就得只有一个
/// 亮度。一个测试钉住它，另一个钉住现在屏幕上没有任何东西穿这套调色板。
///
/// 六个色相，按顺序绕过色环；帧 0 是标记自己亮的那一端，这样一个颜色信号不会让空闲的标记挨
/// 一次跳。只用标准 ANSI 颜色：标记坐在用户已经有的任何主题上，而一个 24 位的值会是那个主题
/// 答不上来的颜色。
pub const PULSE_PALETTE: [Color; 6] = [
    Color::LightMagenta,
    Color::LightBlue,
    Color::LightCyan,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightRed,
];

/// 一个脉冲帧：大约每秒十六帧，这是一个走色环的颜色要读起来像在旋转、而不是一串跳跃所需要的
/// 速度（`.scratch/tui-input-pulse/spec.md` §2b：维护者自己的脚本跑在 60 fps，而这是同一个
/// 样子、步长更粗）。它是**每一个**脉冲驱动的动画的帧长，所以重新武装的下落短横也会以这个
/// 步频走。
///
/// **这个时钟只在一次运行进行中的时候武装**（票 09）。票 08 让它一直跑，理由是提示符 —— 它上色
/// 的那个东西 —— 在什么都没跑的时候也在屏幕上；维护者自己的回答是，一个在他们打字时手底下
/// 动来动去的颜色是噪声、不是生命。于是提示符在 agent 工作时呼吸，其余时间保持它歇着的颜色，
/// 而空闲的会话回到 `select!` 里的三个来源、一次唤醒都没有。
const PULSE_FRAME: std::time::Duration = std::time::Duration::from_millis(60);

/// 那个按需武装的 deadline 到点了没有（`None` = 永远不会到点）。
///
/// 抽成纯函数，好让「该不该到点」有一条单元断言 —— 主循环那根接线本身要真 pty 才跑得了
/// （`.scratch/exit-gesture/spec.md` §6）。
fn exit_gesture_due(deadline: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    deadline.is_some_and(|deadline| now >= deadline)
}

/// 退出手势的窗口：第一下举手之后，第二下要在这段时间内到达才算数
/// （`.scratch/exit-gesture/spec.md` §1、§6）。
///
/// 它同时是提示的寿命：超时作废、提示行恢复。写死、不做配置项 —— 这个手势只有「来得及
/// 收回那一下」一个用途，给它一个旋钮只会多一件要解释的事。
const GESTURE_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

/// 标记的连字符：它的单元格从哪一列开始，以及那个单元格多宽。
///
/// 标记用八个字形单元格拼出 `fs-agent`，每个四列、之间空一列（[`wording::logo_lines`] 里的
/// 一张标签；38 列网格里 `全空的列：4、9、14、……`）。短横是第三个单元格，所以它那四列是 10
/// 到 13，而除了中间那一行，它们在每一行都是空的 —— 那正是下落横条需要的空间：五行可以落，
/// 一行都没被占用。
const MARK_DASH_COLUMN: usize = 10;
const MARK_DASH_WIDTH: usize = 4;

/// 下落短横用什么字形画：与空闲标记携带的那根半块横条同一个。
///
/// 它永远不变 —— 变的是它落在哪一行（票 07）。这个动画的第一个版本把横条转过四个方向，第二个
/// 每个方向画一个不同的字形；两者都让标记自己那根短横在一次运行进行中时变成了另一个东西。一根
/// 横条、在单元格里往下走，是那个读起来像运动、却不会变成新字形的做法。
const DASH_BAR: char = '▀';

/// 标记的那些行与它们的颜色，它的短横落到 `frame` 指定的那一行上 —— 什么都没在跑时是
/// `None`，也就是标记一直画它的那一行。
///
/// 文字是 [`wording::logo_lines`] 的；让它读起来像字形的那条颜色坡道住在这里，与别的绘制在
/// 一起。行越靠上越亮，于是标记读起来像从上方照亮 —— **永远如此**，不管在不在工作：颜色信号
/// 在票 05 退休了（见 [`PULSE_PALETTE`]），所以这个标记里动的只有那根短横
/// （`.scratch/tui-input-pulse/spec.md` §2）。短横**保持它的形状、每帧往下一行**，从标记的
/// 最后一行回绕到第一行：那个单元格就是标记自己的五行，而空闲位置是中间那一行，所以一个静止
/// 的标记与这个动画存在之前逐字节相同。
///
/// 只设前景，而且是刻意不设背景：标记坐在用户主题已有的任何背景上，填掉那些半阴影行会在它能
/// 匹配的同样多的终端上与那个主题打架。
fn mark_lines(frame: Option<u64>) -> Vec<(String, Color)> {
    let rows = wording::logo_lines();
    debug_assert!(
        rows.iter()
            .all(|row| text_columns(row) == layout::LOGO_WIDTH as usize),
        "标记要么整个画出来，要么一个都不画，所以它的宽度是布局的契约"
    );
    let bar_row = match frame {
        Some(frame) => frame as usize % rows.len(),
        // 中间那一行：`logo_lines` 自己画短横的位置，因此也就是空闲标记的样子。它是下落的第三
        // 帧，所以一次运行的第一帧是最上面那一行 —— 横条在上方重新出现、再落一次（票 07）。
        None => rows.len() / 2,
    };
    let mut lines: Vec<(Vec<char>, Color)> = rows
        .iter()
        .enumerate()
        .map(|(row, text)| {
            let color = if row < rows.len() - 1 {
                Color::LightMagenta
            } else {
                Color::Magenta
            };
            let mut line: Vec<char> = text.chars().collect();
            // 短横自己的单元格先让出来：标记空闲时的连字符画在它里面，而除了平的那一个，每个
            // 方向都会往今天放着别的东西的行里放字形。
            for cell in line.iter_mut().skip(MARK_DASH_COLUMN).take(MARK_DASH_WIDTH) {
                *cell = ' ';
            }
            (line, color)
        })
        .collect();
    for cell in lines[bar_row]
        .0
        .iter_mut()
        .skip(MARK_DASH_COLUMN)
        .take(MARK_DASH_WIDTH)
    {
        *cell = DASH_BAR;
    }
    lines
        .into_iter()
        .map(|(line, color)| (line.into_iter().collect(), color))
        .collect()
}

/// 转录：它对着窗格滚动缓冲的那扇窗、它右边缘的滚动条与回合条，以及说出视口在哪里的指示器
/// （spec §1、§3、§4）。
fn draw_transcript(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let text_area = panes.transcript_text();
    // 宽度变了要按新宽度重排那些宽度敏感的块，而不只是重新折行（spec §1）。
    state.rerender_if_width_changed(text_area.width);
    let rows = state
        .pane
        .view(text_area.width, text_area.height, &state.live);
    // 一次点击能打中的就是这一帧真画出来的，逐行算。窗格回答每一条被画出来的显示行属于哪条
    // 来源行，而来源行正是这次点击的链接所按的键（票 04 §1）。
    state.drawn_top = text_area.y;
    state.drawn_rows = (0..rows.len())
        .map(|offset| state.pane.source_at(state.pane.top() + offset))
        .collect();
    frame.render_widget(Paragraph::new(rows), text_area);
    draw_scrollbar(frame, panes.scrollbar(), &state.pane);
    draw_turn_rail(frame, panes, state);
    draw_indicator(frame, text_area, state);
}

/// 回合条：每个回合一格（讨论里是每一轮），沿转录的右边缘往下（spec §4）。
///
/// 视口所在的那一格是亮的，而它是从窗格正在显示的东西**推出来**的 —— 从不保存 —— 所以它不会
/// 与读的人的位置漂开。每一格都记下自己被画在哪里，所以一次点击只能落在真在屏幕上的格子上。
fn draw_turn_rail(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let rows = panes.rail.height as usize;
    if rows == 0 || panes.rail.width == 0 {
        return;
    }
    let units = state.turn_rail.units();
    if units == 0 {
        // 空的会话有一条空列：没有格子，也没有一个 `⋮` 假装上面还有历史（spec §4）。
        return;
    }
    let focus = state.focused_turn().unwrap_or(units - 1);
    let style = Style::default().fg(Color::DarkGray);
    let focus_style = Style::default()
        .fg(Color::LightMagenta)
        .add_modifier(Modifier::BOLD);
    for (offset, slot) in turn_rail_rows(rows, units, focus).into_iter().enumerate() {
        let (symbol, style, unit) = match slot {
            TurnRailRow::Blank => continue,
            TurnRailRow::Cut => (wording::RAIL_TRUNCATED, style, None),
            TurnRailRow::Unit(unit) if unit == focus => {
                (wording::RAIL_FOCUS, focus_style, Some(unit))
            }
            TurnRailRow::Unit(unit) => (wording::RAIL_CELL, style, Some(unit)),
        };
        let y = panes.rail.y + offset as u16;
        let buffer = frame.buffer_mut();
        buffer[(panes.rail.x, y)]
            .set_symbol(symbol)
            .set_style(style);
        if let Some(unit) = unit {
            state.regions.cells.push(Region {
                rect: Rect::new(panes.rail.x, y, 1, 1),
                action: HitAction::TurnRailUnit(unit),
            });
        }
    }
}

/// 转录的滚动条：只在内容多于一屏时才画，画在排版一直为它留的那一列里。
fn draw_scrollbar(frame: &mut ratatui::Frame, track: Rect, pane: &Pane) {
    if track.width == 0 || pane.total() <= track.height as usize {
        return;
    }
    // 跟着末尾与在读历史看起来不一样，所以位置不用读数字就看得出来。
    let thumb = if pane.following() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    };
    let mut scrollbar = ScrollbarState::new(pane.total())
        .position(pane.top())
        .viewport_content_length(track.height as usize);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_style(Style::default().fg(Color::DarkGray))
            .thumb_style(thumb),
        track,
        &mut scrollbar,
    );
}

/// 窗格右下那个「来了什么、以及回去的路」的指示器。
///
/// 它的整块都是点击目标，所以这个矩形记在窗格上 —— 一次点击只能落在上一帧画出来的东西上。
fn draw_indicator(frame: &mut ratatui::Frame, area: Rect, state: &mut TuiState) {
    // 详情覆盖层占着转录的时候，回去的路是覆盖层自己的页脚：指示器的计数暂停了，它的点击不
    // 属于任何人（票 02 §4）。
    if state.pane.following() || state.detail_open() || area.width == 0 || area.height == 0 {
        state.indicator = None;
        return;
    }
    let fresh = state.pane.fresh();
    let text = if fresh == 0 {
        wording::back_to_bottom().to_owned()
    } else {
        wording::new_content(fresh)
    };
    // `area` 已经是文字区 —— 滚动条那一列不在里面 —— 所以右边缘的一个宽字符不可能把滚动条
    // 遮没。
    let width = (text_columns(&text) as u16).min(area.width);
    let rect = Rect::new(
        area.right().saturating_sub(width),
        area.bottom().saturating_sub(1),
        width,
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_columns(&text, width as usize),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))),
        rect,
    );
    state.indicator = Some(rect);
}

/// 主列的脚：输入行，以及它下面那行说明按键干什么的提示行。
///
/// 返回光标被放在了哪里，好让浮在主列之上的东西把自己锚在它上面 —— `/` 菜单跟着光标走
/// （spec §6）。有问题立着时是 `None`，因为那时候没有光标。
///
/// 问卷用自己替换掉输入行。这正是这一类问题的全部意义：中间覆盖层适合一行确认，而问卷是好
/// 几行、好几页，于是它拿走为打字建的那块区域（spec §19）。
fn draw_bottom(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
) -> Option<editor::Placed> {
    if state.questionnaire().is_some() {
        // 问卷被抬出来再放回去，好让画家把命中区域记在指针将来读它们的同一份状态里。它没有
        // clone：它那个回复通道是它身上唯一必须保持独一份的东西。
        let Some(Pending::Questionnaire(questionnaire)) = state.pending.take() else {
            return None;
        };
        let cursor = draw_questionnaire(frame, panes, &questionnaire, state);
        if let Some(cursor) = cursor {
            frame.set_cursor_position((
                (panes.input.x + cursor.column).min(panes.input.right().saturating_sub(1)),
                panes.input.y + cursor.row,
            ));
        }
        draw_questionnaire_footer(frame, panes, &questionnaire, state);
        state.pending = Some(Pending::Questionnaire(questionnaire));
        return None;
    }
    let (mut rows, cursor) = state.editor.view(
        layout::input_text_width(frame.area(), state.sidebar_wanted),
        panes.input.height,
    );
    // 提示符是自己的一个 span（见 `editor::Input::view`），正是它给了提示符一个草稿永远不会
    // 拿到的颜色。只有**就是**提示符的那个 span 上色：它下面那些行的缩进是同样宽的空格，而
    // 一份长到把提示符滚出顶端的草稿，屏幕上根本没有提示符可上色
    // （`.scratch/tui-input-pulse/spec.md` §2b）。
    let prompt_style = Style::default().fg(prompt_colour(state.pulse));
    for row in &mut rows {
        match row.spans.first_mut() {
            Some(lead) if lead.content.as_ref() == editor::PROMPT => lead.style = prompt_style,
            _ => {}
        }
    }
    frame.render_widget(
        Paragraph::new(rows).style(Style::default().add_modifier(Modifier::BOLD)),
        panes.input,
    );
    // 草稿在问题之下仍然可见 —— 那是用户正在写的东西 —— 但光标收起来：键盘正在回答，不是在
    // 编辑（spec §9）。光标是按刚画出来的那些行摆的，从不按帧与帧之间保存的状态摆，正是后者
    // 让内联视口的光标漂移（ADR 0002）。
    let anchor = state.pending.is_none().then_some(cursor);
    if anchor.is_some() {
        frame.set_cursor_position((
            (panes.input.x + cursor.column).min(panes.input.right().saturating_sub(1)),
            panes.input.y + cursor.row,
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            state.status_line(panes.hints.width),
            Style::default().fg(Color::DarkGray),
        ))),
        panes.hints,
    );
    anchor
}

/// 底部输入区里的问卷：一个问题的那些行，按排版给的地方滚动。
///
/// 排版给底部块的高度封了顶，所以行数多过能装下的问题是被开窗、而不是被裁掉：表头与问题待着
/// 不动，选项窗口跟着高亮走（spec §7、§19）。页脚仍然说是第几个问题，而那个顶让转录保持
/// 可见。
///
/// 这些行一次画一行，好记下每一行的屏幕行：选项行是可点的，而一行装着哪个选项取决于高亮当前
/// 驱动的那个窗口（票 04 §4、§6）。
fn draw_questionnaire(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    questionnaire: &Questionnaire,
    state: &mut TuiState,
) -> Option<editor::Placed> {
    let question = &questionnaire.questions[questionnaire.index];
    let draft = &questionnaire.drafts[questionnaire.index];
    let width = panes.input.width as usize;
    let options_focused = questionnaire.zone == Zone::Options;
    let (prefix, options, custom) = questionnaire_parts(question, draft, width, options_focused);
    let window = questionnaire_window(
        question,
        draft,
        width,
        panes.input.height as usize,
        options_focused,
    );
    let input_height = panes.input.height as usize;
    let geometry = option_window_geometry(
        &options,
        draft.highlight,
        input_height,
        prefix.len(),
        custom.len(),
    );
    // 自定义行是窗口画出的最后一行，不管它是因为裁剪被钉在那里，还是干脆结束了那个列表。
    let custom_row = window.len().saturating_sub(1);
    for (row, line) in window.iter().enumerate() {
        frame.render_widget(
            Paragraph::new(line.clone()),
            Rect::new(
                panes.input.x,
                panes.input.y + row as u16,
                panes.input.width,
                1,
            ),
        );
    }
    // 命中的是**选项的屏幕行**：折成几行就记几行，每一行都映射回同一个选项下标
    // （`.scratch/questionnaire-keys/spec.md` §7）。
    // 输入区那几行钉在底部，选项最多画到它们之前。
    let last_option_row = input_height.saturating_sub(custom.len());
    let mut row = prefix.len();
    for (index, lines) in options.iter().enumerate().skip(geometry.start) {
        for _ in lines {
            if row >= last_option_row {
                break;
            }
            state
                .regions
                .options
                .push((panes.input.y + row as u16, index));
            row += 1;
        }
        if row >= last_option_row {
            break;
        }
    }
    state.regions.custom = Some(panes.input.y + custom_row as u16);
    // 输入区聚焦时那行显示光标，跟常驻编辑器一样：它是唯一能往里打字的行（票 04 §5）。
    if questionnaire.zone == Zone::Input {
        // 光标跟在输入区**最后一个视觉行**的末尾 —— 折行之后它就是那一行的宽度。
        let column = window.last().map(|line| line.width()).unwrap_or(0);
        let column = column.min(panes.input.width.saturating_sub(1) as usize) as u16;
        Some(editor::Placed {
            row: custom_row as u16,
            column,
        })
    } else {
        None
    }
}

/// 问卷的页脚：这是第几个问题，然后只列真正可用的那些按钮。
///
/// 不可用的不画出来，所以它们没有点击区域 —— 指针与眼睛看到的是同一套（票 04 §4）。页脚那三个
/// 标签是每题一份的文案，而区域是从画它们的同一份排版里记下来的。
fn draw_questionnaire_footer(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    questionnaire: &Questionnaire,
    state: &mut TuiState,
) {
    let total = questionnaire.questions.len();
    // 进度计数器与第一个按钮前那三个字宽的间隔是同一个 span，所以第一个按钮起始的列就是这个
    // span 自己的宽度 —— 而不是对它的第二次猜测（票 04 §7）。
    let mut cursor = text_columns(&wording::questionnaire_progress(questionnaire.index, total)) + 3;
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        format!(
            "{}   ",
            wording::questionnaire_progress(questionnaire.index, total)
        ),
        Style::default().fg(Color::DarkGray),
    )];
    let items = [
        (
            questionnaire.index > 0,
            wording::questionnaire_previous(),
            HitAction::Previous,
        ),
        (
            questionnaire.index + 1 < total,
            wording::questionnaire_next(),
            HitAction::Next,
        ),
        (
            questionnaire.all_handled(),
            wording::questionnaire_submit(),
            HitAction::Submit,
        ),
    ];
    let mut drawn = false;
    for (available, label, action) in items {
        // 不可用的按钮不画、也没有区域，于是下一个按钮把这段间隔收掉：页脚读起来是一份真正在
        // 提供什么的列表。
        if !available {
            continue;
        }
        // 间隔只出现在**画出来的按钮之间**，而且它是**画出来**的、不是只在心里记个数：由一段
        // 不在屏幕上的间隔推出来的区域，是一个指着按钮旁边三个列开外的区域（票 04 §7）。
        if drawn {
            spans.push(Span::raw("   "));
            cursor += 3;
        }
        let width = text_columns(label);
        if let Some(rect) = hint_region(panes.hints, cursor, width) {
            state.regions.cells.push(Region { rect, action });
        }
        spans.push(Span::styled(
            label.to_owned(),
            Style::default().fg(Color::DarkGray),
        ));
        cursor += width;
        drawn = true;
    }
    // 最后那一段：举手时是回执 —— 任何宽度下都保，因为它回答的是「刚才那下生效了没有」；
    // 否则是键位提示，按剩下的列数降级（`.scratch/questionnaire-keys/spec.md` §6）。
    let tail = match state.raised_gesture(std::time::Instant::now()) {
        Some(Gesture::Exit) => wording::questionnaire_exit_raised(),
        Some(Gesture::DeclineQuestion) => wording::questionnaire_decline_raised(),
        None => {
            wording::questionnaire_hint((panes.hints.width as usize).saturating_sub(cursor + 3))
        }
    };
    if !tail.is_empty() {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            tail.to_owned(),
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), panes.hints);
}

/// 从 `row` 往里偏移 `offset` 列、长 `columns` 列的那一段的屏幕矩形，落到终端外面时是 `None`。
fn hint_region(row: Rect, offset: usize, columns: usize) -> Option<Rect> {
    let offset = u16::try_from(offset).ok()?;
    let columns = u16::try_from(columns).ok()?;
    let x = row.x.checked_add(offset)?;
    if x.checked_add(columns)? > row.right() {
        return None;
    }
    Some(Rect::new(x, row.y, columns, 1))
}

/// `/` 菜单：开头的 `/` 能变成哪些名字 —— 循环处理的内建命令，加上这个会话发现的 skills
/// —— 浮在光标处，并按斜杠后面打了什么过滤（spec §6）。
///
/// 它是一个**提示**，不是一个问题：它从不会把某个键从草稿那里拿走，而它确实占着的键（`↑`、
/// `↓`、`Tab`、`Enter`）只在它立着时才有。
fn draw_menu(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
    anchor: editor::Placed,
) {
    let Some(menu) = state.slash_menu() else {
        return;
    };
    // 要画的匹配那一窗：高亮始终可见，而它上面的行在它往下走时先被让出去。
    let room = panes
        .menu_room(anchor)
        .min(layout::MENU_MAX_ROWS)
        .min(menu.entries.len() as u16) as usize;
    if room == 0 {
        return;
    }
    let highlighted = menu.selected.unwrap_or(0);
    let first = if highlighted >= room {
        highlighted + 1 - room
    } else {
        0
    };
    let visible = &menu.entries[first..first + room];

    // 左右各一列内边距，名字列与最宽的名字一样宽，然后两个空格，然后是放得下的描述。
    let name_width = visible
        .iter()
        .map(|(name, _)| text_columns(name) + 1)
        .max()
        .unwrap_or(0);
    let widest = visible
        .iter()
        .map(|(_, description)| 2 + name_width + 2 + text_columns(description))
        .max()
        .unwrap_or(0);
    let width = (widest as u16).min(layout::MENU_MAX_WIDTH);
    let inner = width.saturating_sub(2) as usize;
    let Some(area) = panes.menu(anchor, width, room as u16) else {
        return;
    };

    let rows: Vec<Line<'static>> = visible
        .iter()
        .enumerate()
        .map(|(offset, (name, description))| {
            let selected = menu.selected == Some(first + offset);
            menu_row(name, description, inner, name_width, selected)
        })
        .collect();
    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    frame.render_widget(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .border_set(border::LIGHT_TRIPLE_DASHED)
            .border_style(Style::default().fg(CHROME_LINE)),
        area,
    );
    frame.render_widget(Paragraph::new(rows), layout::inner(area));
}

/// 一行菜单：`/<名字>`，补齐到名字列宽，然后是描述。
///
/// 高亮的那一行反色画，好让它读起来是 `Enter` 会按下的那个按钮，而不是又多了一行文字。
fn menu_row(
    name: &str,
    description: &str,
    inner: usize,
    name_width: usize,
    selected: bool,
) -> Line<'static> {
    let label = format!("/{name}");
    let mut text = label.clone();
    // 描述列，在有地方放一个描述、也有它要占的那一行的时候。太窄就让名字独占这一行，那仍然是
    // 一个完整的提示。
    let gap = name_width.saturating_sub(text_columns(&label)) + 2;
    if !description.is_empty() && text_columns(&label) + gap + 2 <= inner {
        text.push_str(&" ".repeat(gap));
        text.push_str(description);
    }
    // 开头一列内边距，然后是这一行，然后是剩下的部分 —— 于是文字永远不碰边框，而高亮盖住整行。
    let body = truncate_columns(&text, inner.saturating_sub(1));
    let padding = inner.saturating_sub(1 + text_columns(&body));
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow)
    };
    Line::from(vec![
        Span::styled(format!(" {body}"), style),
        Span::styled(" ".repeat(padding), style),
    ])
}

/// 把 assistant 的答案归到它的发言者名下：`[name] ` 只引领**第一行**，其余行顶格
/// （spec §5）。
///
/// 回答是一份文档，结构由 Markdown 自己给（标题、列表、代码块），那 11 格前导会把它整体
/// 推右、又吃掉主列约七分之一。零前导就是零前导 —— 不是一串空 span。
///
/// `indent` 是那个前缀占的列数，也是渲染器给**需要左边界对齐的块**（表格、代码块）铺的
/// 前导：第一行若已经以那 `indent` 个空格开头，就把它**换成**前缀，于是表头与数据行、
/// 语言名与代码行落在同一个左边界上，而两边的总列数一模一样。
fn attribute_document(
    speaker: &crate::events::SpeakerId,
    rows: Vec<Line<'static>>,
    colors: &mut SpeakerColors,
    indent: u16,
) -> Vec<Line<'static>> {
    let prefix = format!("{} ", speaker_label(speaker));
    let name_style = name_style(speaker, colors);
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let mut spans = row.spans;
            if index == 0 {
                if indent > 0 {
                    strip_leading_spaces(&mut spans, indent as usize);
                }
                spans.insert(0, Span::styled(prefix.clone(), name_style));
            }
            Line {
                spans,
                style: row.style,
                alignment: row.alignment,
            }
        })
        .collect()
}

/// 从一片行首拿掉 `count` 个空格：那是渲染器给结构化块铺的前导，前缀要换成它。
///
/// 前导不足 `count` 列（比如第一行是一段普通正文）时什么都不动 —— 那时前缀是**插**在
/// 前面，而不是替换。
fn strip_leading_spaces(spans: &mut Vec<Span<'static>>, count: usize) {
    let Some(first) = spans.first_mut() else {
        return;
    };
    let leading = first.content.chars().take_while(|ch| *ch == ' ').count();
    if leading < count {
        return;
    }
    let rest: String = first.content.chars().skip(count).collect();
    first.content = rest.into();
    if first.content.is_empty() {
        spans.remove(0);
    }
}

/// 把一次**发言**归到它的发言者名下：标签引领第一行，其余行按 `[name] ` 的显示宽度缩进，
/// 于是一条折行的或多行的消息读起来是**一次**发言（spec §3）。
///
/// 用户自己的输入与非 assistant 的系统行走这条路：它们的正文里没有任何结构可依赖，
/// 缩进就是那点结构（spec §5）。名字拿发言者自己的颜色，正文保持这一行的 —— 这个分工
/// 就是全部的上色规矩：名字做标识，正文的意思由它的严重度说了算（票 07 §2）。
///
/// 骨架与 [`attribute_document`] 相似，但前导规则不同。合并成一个带开关的函数会让这两条路
/// 看起来只差一个布尔值，而它们差的是一件别的事：一份文档有没有自己的结构。
fn attribute_speech(
    speaker: &crate::events::SpeakerId,
    rows: Vec<Line<'static>>,
    colors: &mut SpeakerColors,
) -> Vec<Line<'static>> {
    let prefix = format!("{} ", speaker_label(speaker));
    let indent = " ".repeat(prefix.as_str().cell_width() as usize);
    let name_style = name_style(speaker, colors);
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let lead = if index == 0 {
                prefix.clone()
            } else {
                indent.clone()
            };
            let mut spans = vec![Span::styled(lead, name_style)];
            spans.extend(row.spans);
            Line {
                spans,
                style: row.style,
                alignment: row.alignment,
            }
        })
        .collect()
}

/// 一个发言者的 `[name] ` 前缀在终端上占多少列。
///
/// 量的是**列**，中文名字按两列算 —— 与前缀本身的显示宽度同一把尺子。
fn prefix_columns(speaker: &crate::events::SpeakerId) -> u16 {
    format!("{} ", speaker_label(speaker)).as_str().cell_width()
}

/// 一个发言者的 `[name]` 前缀用什么样式画。
///
/// 没有调色板时标签保持它一直有的那个叙述灰，这正是共享渲染里素的那一半要的：只有 TUI 会给
/// 名字上色，而 `plain` 从不传调色板（票 07 §4）。
fn name_style(speaker: &crate::events::SpeakerId, colors: &mut SpeakerColors) -> Style {
    Style::default().fg(colors.of(speaker))
}

/// 一条叙述行，文字以某个发言者的 `[name]` 前缀开头：名字拿发言者的颜色，其余用调用方的样式
/// （票 07 §2）。
fn speaker_line(
    speaker: &crate::events::SpeakerId,
    text: String,
    body: Style,
    colors: &mut SpeakerColors,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(speaker_label(speaker), name_style(speaker, colors)),
        Span::styled(format!(" {text}"), body),
    ])
}

/// 一条被画出来的来源行，以及点它通向哪儿。
///
/// 链接可选，因为大多数行不是通向任何地方的入口。是入口的行带着整份详情，在这行被画出来的那
/// 一刻读出来，所以一次点击从不必回头去事件流里够它（票 02 §4，票 04 §1）。
pub struct RenderedLine {
    pub line: Line<'static>,
    pub link: Option<Detail>,
}

impl RenderedLine {
    /// 一整行都打开 `detail` 的一条行。
    fn linked(line: Line<'static>, detail: Detail) -> Self {
        Self {
            line,
            link: Some(detail),
        }
    }
}

impl From<Line<'static>> for RenderedLine {
    fn from(line: Line<'static>) -> Self {
        Self { line, link: None }
    }
}

/// 共享渲染（[`render_block`]）没有窗格宽度可依时的排版宽度。
///
/// TUI 自己把转录内容的宽度传给 [`paint_block`]；这个缺省只服务那些没有窗格的调用方 ——
/// 只关心文字的测试与任何别处的共享渲染。宽度敏感的块（表格、代码块）因此按这个宽度排版。
const SHARED_RENDER_WIDTH: u16 = 80;

/// 把一个定稿的块变成带样式的终端行。
///
/// 这是共享呈现层的 TUI 那一半：块已经被 [`Transcript`] 决定过一次，这里只发生绘制。
///
/// `colors` 是转录的名字调色板。手上没有名册的调用方 —— `plain` 的那一半渲染，以及只关心文字
/// 的测试 —— 通过 [`render_block_uncoloured`] 传一个空的进来，于是每个名字都画成叙述灰。
pub fn render_block(block: &Block, colors: &mut SpeakerColors) -> Vec<Line<'static>> {
    paint_block(block, colors, SHARED_RENDER_WIDTH)
        .into_iter()
        .map(|rendered| rendered.line)
        .collect()
}

/// 不带名册地画一个块：每个发言者的名字都是叙述灰。这是名字有颜色之前共享渲染的样子，留给那些
/// 没有名册可据以画的调用方。
pub fn render_block_uncoloured(block: &Block) -> Vec<Line<'static>> {
    render_block(block, &mut SpeakerColors::new(&[]))
}

/// 画一个块，保留每一行的链接。
///
/// `width` 是转录内容的可用列数：Markdown 里的表格与超宽代码行按它排版
/// （`.scratch/markdown-render/spec.md` §1）。
fn paint_block(block: &Block, colors: &mut SpeakerColors, width: u16) -> Vec<RenderedLine> {
    match block {
        Block::Message {
            speaker,
            role: Role::Assistant,
            text,
            // 推理是由状态机画的，不是从这个块画的：它已经成了一条思考行，在这里再画一遍会把
            // 同一个想法显示两次（票 02 §1）。
            reasoning: _,
        } => {
            if text.is_empty() {
                return Vec::new();
            }
            // 回答按 Markdown 以全亮度渲染；只有发言者标签上色。答案是一份文档，所以续行
            // 顶格（spec §5）。
            //
            // 前缀占的列交给渲染器：需要左边界对齐的块（表格、代码块）整块从那一列起，
            // 于是它们的左边界与第一行的 `[name] ` 对齐，而第一行前缀正好替换掉那段前导。
            let indent = prefix_columns(speaker);
            attribute_document(
                speaker,
                super::markdown::to_lines_indented(text, width, indent),
                colors,
                indent,
            )
            .into_iter()
            .map(RenderedLine::from)
            .collect()
        }
        // 用户自己的输入 —— 以及非 assistant 的系统行 —— 按写下来的样子显示：每一行都在，
        // 什么都不略去，也不上 Markdown，因为这不是一份文档。续行与第一行正文对齐（spec §3）。
        Block::Message { speaker, text, .. } => attribute_speech(
            speaker,
            text.split('\n')
                .map(|raw| Line::from(raw.to_owned()))
                .collect(),
            colors,
        )
        .into_iter()
        .map(RenderedLine::from)
        .collect(),
        Block::Delta { .. } => Vec::new(),
        Block::RoundStarted { round, mode } => vec![Line::from(Span::styled(
            wording::round_section(*round, *mode),
            Style::default()
                .fg(ratatui::style::Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .into()],
        Block::RoundEnded { round, reason } => {
            vec![severity_line(*reason, wording::round_ended(*round, *reason)).into()]
        }
        Block::Divergence { topic, positions } => {
            let mut lines: Vec<RenderedLine> = vec![Line::from(Span::styled(
                format!("!! {}", wording::divergence(topic)),
                Style::default()
                    .fg(ratatui::style::Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ))
            .into()];
            for position in positions {
                lines.push(Line::from(format!("  - {position}")).into());
            }
            lines
        }
        Block::Tool(tool) => tool_block_lines(tool, colors),
        // 后置 hook 的反馈，关于刚画出来的那次调用：一行普通的缩进行，黄色，因为说话的是策略
        // 而不是工具。
        Block::ToolFeedback { outcome, .. } => vec![Line::from(Span::styled(
            format!("  {}", wording::hook_feedback(outcome)),
            Style::default().fg(Color::Yellow),
        ))
        .into()],
        Block::TurnStarted { speaker, iteration } => vec![speaker_line(
            speaker,
            wording::turn_started(*iteration),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::TurnEnded { speaker, reason } => {
            vec![
                severity_speaker_line(speaker, *reason, wording::turn_ended(*reason), colors)
                    .into(),
            ]
        }
        Block::PermissionAsked {
            speaker,
            tool_name,
            args,
        } => vec![speaker_line(
            speaker,
            wording::permission_asked(tool_name.as_deref(), &summarize_args(args)),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::PermissionDecided {
            speaker,
            decision,
            source,
            reason,
        } => vec![speaker_line(
            speaker,
            wording::permission_decided(*decision, *source, reason.as_deref()),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::Hook {
            speaker,
            point,
            outcome,
        } => vec![speaker_line(
            speaker,
            wording::hook(point, outcome),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::ExecutorSpawned {
            speaker,
            executor_id,
        } => vec![speaker_line(
            speaker,
            wording::executor_spawned(executor_id.as_str()),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::ExecutorFinished {
            executor_id,
            reason,
            summary,
        } => vec![severity_line(
            *reason,
            wording::executor_finished(executor_id.as_str(), *reason, summary),
        )
        .into()],
        Block::Usage { speaker, usage } => vec![speaker_line(
            speaker,
            wording::usage_summary(usage),
            Style::default().fg(ratatui::style::Color::DarkGray),
            colors,
        )
        .into()],
        Block::AgentError { speaker, message } => vec![severity_speaker_line(
            speaker,
            StopReason::Error,
            wording::agent_error(message),
            colors,
        )
        .into()],
        Block::SessionError { code, detail } => {
            vec![severity_line(StopReason::Error, wording::session_error(code, detail)).into()]
        }
        Block::SessionEnded { reason } => {
            vec![severity_line(*reason, wording::session_ended(*reason)).into()]
        }
        Block::ContextInjected { source, content } => {
            // 这条记录**点得开**（票 19）：转录上只有一行来源名，加载数据本身在详情里。
            let text = wording::context_injected(source.clone());
            let line = narration(text.clone());
            let detail = Detail {
                title: text,
                color: Color::DarkGray,
                kind: DetailKind::Context {
                    source: source.clone(),
                    content: content.clone(),
                },
            };
            vec![RenderedLine::linked(line, detail)]
        }
        Block::Sandbox {
            mode,
            unavailable_reason,
        } => {
            vec![narration(wording::sandbox(mode, unavailable_reason.as_deref())).into()]
        }
        Block::History { reason, summary } => {
            vec![narration(wording::history(*reason, summary.as_deref())).into()]
        }
        Block::Diagnostic(message) => vec![Line::from(Span::styled(
            wording::diagnostic(message),
            Style::default().fg(ratatui::style::Color::Yellow),
        ))
        .into()],
        Block::Notice(message) => vec![narration(message.clone()).into()],
    }
}

/// 一条**中间**叙述行：用暗色，好让模型那个以全亮度渲染的回答成为显眼的东西。带严重度的行
/// 改为保持自己的颜色（见 [`severity_line`]）。
fn narration(text: String) -> Line<'static> {
    Line::from(Span::styled(
        text,
        Style::default().fg(ratatui::style::Color::DarkGray),
    ))
}

fn severity_line(reason: StopReason, text: String) -> Line<'static> {
    Line::from(Span::styled(text, severity_style(reason)))
}

/// 一条点名了发言者的严重度行：名字保持发言者的颜色，这一行的其余部分保持严重度的颜色
/// （票 07 §2）。这样一个错误仍然读起来像个错误，而读的人不会丢掉它是谁弄出来的。
fn severity_speaker_line(
    speaker: &crate::events::SpeakerId,
    reason: StopReason,
    text: String,
    colors: &mut SpeakerColors,
) -> Line<'static> {
    speaker_line(speaker, text, severity_style(reason), colors)
}

/// 一个停止点把它那一行画成什么颜色。
fn severity_style(reason: StopReason) -> Style {
    match Severity::of(reason) {
        Severity::Good => Style::default().fg(ratatui::style::Color::Green),
        Severity::Note => Style::default().fg(ratatui::style::Color::Cyan),
        Severity::Warn => Style::default().fg(ratatui::style::Color::Yellow),
        Severity::Bad => Style::default()
            .fg(ratatui::style::Color::Red)
            .add_modifier(Modifier::BOLD),
    }
}

/// 一次已完成的工具调用，折成一行：读的人能打开的**那次调用**，整份输出在它后面（票 02 §3）。
///
/// 失败是同一行在**末尾**多一个 `失败` —— 不是第二行 —— 而错误正文移进详情。后置 hook 的反馈
/// 是自己的一个块、留在屏幕上：它是策略的反馈，不是工具输出，所以不点也必须是可读的
/// （票 02 §3）。
fn tool_block_lines(tool: &ToolBlock, colors: &mut SpeakerColors) -> Vec<RenderedLine> {
    let failed = matches!(&tool.outcome, Some(outcome) if !outcome.ok);
    let color = colors.of(&tool.speaker);
    let mut call = vec![
        // 名字打头，于是每条转录行都以谁在说话开头；它后面那个标记说的是这行可以打开。它是
        // 绘制、不是文案，所以不是那句话的一部分（票 03 §Answer）。
        Span::styled(
            format!("{} ", speaker_label(&tool.speaker)),
            Style::default().fg(color),
        ),
        Span::styled("▸ ", Style::default().fg(Color::DarkGray)),
        // 这次调用是*为了*什么，用思考行穿的那个叙述灰 —— 参数本身离一次点击之遥
        // （票 02 §2，2026-09-23 修正）。
        Span::styled(
            wording::tool_call_line(&tool.tool, &tool.args),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if failed {
        call.push(Span::styled(
            format!(" {}", wording::tool_failed()),
            Style::default().fg(Color::Red),
        ));
    }
    let detail = Detail {
        title: line_text(&Line::from(call.clone())),
        color,
        kind: DetailKind::Tool {
            tool_call_id: tool.tool_call_id.clone(),
            output: tool
                .outcome
                .as_ref()
                .and_then(|outcome| outcome.output.clone()),
            error: tool
                .outcome
                .as_ref()
                .and_then(|outcome| outcome.error.clone()),
            args: tool.args.clone(),
            no_result: tool.outcome.is_none(),
        },
    };
    vec![RenderedLine::linked(Line::from(call), detail)]
}

/// 一条来源行是不是用户自己的消息，那正是回合条一格跳转所瞄准的（spec §4）。
fn is_user_message(block: &Block) -> bool {
    matches!(
        block,
        Block::Message {
            speaker: crate::events::SpeakerId::User,
            ..
        }
    )
}

/// 这个块是否结束回合条计数的那个单位。
///
/// 讨论数的是它的**轮**，交互会话数的是它的回合；两个边界都存在于一场讨论的流里，所以哪一个
/// 算数是会话的性质、而不是块的性质（spec §4）。
fn is_boundary(block: &Block, discussion: bool) -> bool {
    if discussion {
        matches!(block, Block::RoundEnded { .. })
    } else {
        matches!(block, Block::TurnEnded { .. })
    }
}

/// 回合条那一列的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnRailRow {
    /// 这个下标对应的单位，从最老的那个数起。
    Unit(usize),
    /// 这一端有单位被略掉了：`⋮`。
    Cut,
    /// 这里什么都不画。
    Blank,
}

/// 一条 `rows` 行高、`units` 个单位长、视口在 `focus` 上的转录，回合条该有哪些格。
///
/// 两条性质，两条都是 spec 的（§4）：
///
/// * 聚焦那一格永远在其中 —— 这就是这一列的整个意义；
/// * 哪一端有单位被裁掉，哪一端就用一个 `⋮` 说出来。
///
/// 窗口能贴底就贴底，只在焦点需要时往上滑：贴着底部的视口显示最新的单位，停在中间的显示它
/// 周围的那些。只保留最新 N 个 —— 这是它最初写成的样子 —— 会让一个停在一个老单位上的视口
/// **一个亮格都没有**，那正是 `prototype/frames/120x24-rail-30-units-focus-12-gap.txt` 记
/// 下来的那一帧。
fn turn_rail_rows(rows: usize, units: usize, focus: usize) -> Vec<TurnRailRow> {
    let mut out = vec![TurnRailRow::Blank; rows];
    if rows == 0 || units == 0 {
        return out;
    }
    let focus = focus.min(units - 1);
    if units <= rows {
        // 每个单位都装得下：最新的在底部，上面留白。
        for index in 0..units {
            out[rows - units + index] = TurnRailRow::Unit(index);
        }
        return out;
    }
    // 单位比行还多。一格给焦点，每一端有单位被裁掉的再花掉一格；一个短到连这些都不放不下的
    // 终端保住聚焦那格、把标记让出去。
    let above = focus;
    let below = units - 1 - focus;
    let mut budget = rows - 1;
    let mut top_cut = above > 0;
    let mut bottom_cut = below > 0;
    if top_cut {
        if budget > 0 {
            budget -= 1;
        } else {
            top_cut = false;
        }
    }
    if bottom_cut {
        if budget > 0 {
            budget -= 1;
        } else {
            bottom_cut = false;
        }
    }
    // 把剩下的在两侧分掉，每边不超过它有的，余下的还给更老的那一侧：贴底的视口拿走它上面
    // 全部，居中的那些大致居中。
    let mut above_taken = above.min(budget / 2);
    let below_taken = below.min(budget - above_taken);
    above_taken += (above - above_taken).min(budget - above_taken - below_taken);

    let mut cells: Vec<TurnRailRow> = Vec::with_capacity(rows);
    if top_cut {
        cells.push(TurnRailRow::Cut);
    }
    let first = focus - above_taken;
    for index in first..first + above_taken + 1 + below_taken {
        cells.push(TurnRailRow::Unit(index));
    }
    if bottom_cut {
        cells.push(TurnRailRow::Cut);
    }
    out[rows - cells.len()..].copy_from_slice(&cells);
    out
}

/// 一条被画出来的行的文字，用来做标题。
fn line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

/// 一条可点的转录行打开什么：一个被冻住的想法，或一次工具调用的参数与完整输出（票 02 §4）。
///
/// 主体是**在这行被画出来时**读的，所以覆盖层显示的东西不会与读的人点击时屏幕上的东西有
/// 分歧。
#[derive(Clone)]
pub struct Detail {
    /// 被点那一行自己的文字，用作覆盖层的标题。
    title: String,
    /// 这条详情所属那一行发言者的颜色：覆盖层的边框穿它，于是这个框在你读它一个字之前就说清
    /// 了你在读谁的行（2026-09-23）。
    color: Color,
    kind: DetailKind,
}

/// 一个详情视图可以关于的两件事。
#[derive(Clone)]
enum DetailKind {
    /// 一段已完成的思考。流记录了 trace 时 `text` 是整段 trace，而 `None` 是合成器的情形
    /// —— 增量到了、事件流里没有文本 —— 详情会把这一点说出来（票 02 §1）。
    Thinking { text: Option<String> },
    /// 一条上下文注入的正文（票 19）。转录那一行只说来源，这里摊开内容。
    Context {
        source: ContextSource,
        content: String,
    },
    /// 一次工具调用：它的参数，以及这次调用产出了什么。
    Tool {
        /// 给落盘输出文件命名的那个 id，`outputs/<id>.txt`。
        tool_call_id: ToolCallId,
        output: Option<String>,
        error: Option<String>,
        args: serde_json::Value,
        no_result: bool,
    },
}

/// 详情覆盖层的打开状态（票 02 §4）。
///
/// 它是一个**视图模式，不是一个待答的问题**：转录冻在原处，键盘与滚轮在它被关掉之前归主体
/// 所有，而且没有置任何 `pending` —— 这正是让问题守卫不会把瞄准覆盖层的滚轮吞掉的原因。
struct DetailView {
    /// 正在显示什么。
    detail: Detail,
    /// 主体，按它被打开时的宽度排版。
    body: Vec<Line<'static>>,
    /// 屏幕上主体的第一行。
    top: usize,
    /// 覆盖层一次能显示多少主体行。
    height: usize,
}

/// 详情覆盖层在边框与文字之间留的那一列空气。
const DETAIL_PADDING: u16 = 1;

/// 覆盖层自己的文字在有内边距之前需要的行数：一行标题、两行主体，加上页脚。
const DETAIL_MIN_TEXT_ROWS: u16 = 4;

/// 一个详情主体会从落盘的工具输出里读的最多字符数。
///
/// 一条工具结果在进事件流之前就被截过，但落盘的那个文件没有：这是读的人自己的上限，超过它
/// 主体以 [`wording::detail_truncated`] 结尾（票 02 §4）。
const DETAIL_MAX_CHARS: usize = 200_000;

impl TuiState {
    /// 为读的人点的那一行打开详情覆盖层。
    ///
    /// 主体在这里、在打开的那一刻读，并按覆盖层将被画出来的宽度排版，于是此后滚动是纯算术。
    fn open_detail(&mut self, detail: Detail, width: usize) {
        let body = detail_body(&detail, &self.facts.session_dir, width);
        self.detail = Some(DetailView {
            detail,
            body,
            top: 0,
            height: 0,
        });
    }

    /// 关掉它，无论它是从哪里打开的。
    ///
    /// 什么都没开时是空操作，这一点要紧，因为请求处理函数是无条件调它的：放开一次从没被拿走
    /// 的冻结，会把一个已经往上滚的读的人拽回底部（票 02 §4）。
    fn close_detail(&mut self) {
        if self.detail.take().is_some() {
            // 阅读位置是覆盖层的；放开它就是把转录送回底部，而计数也从那里重新起算。
            self.pane.set_holding(false);
            self.pane.set_following(true);
        }
    }

    fn detail_open(&self) -> bool {
        self.detail.is_some()
    }

    /// 把打开着的详情主体滚动 `rows` 个显示行；负数是往上。
    fn detail_scroll(&mut self, rows: isize) {
        let Some(view) = self.detail.as_mut() else {
            return;
        };
        let max_top = view.body.len().saturating_sub(view.height);
        view.top = (view.top as isize + rows).clamp(0, max_top as isize) as usize;
    }

    /// 详情主体的一页：它自己的高度减去一行重叠，好让读的人在跳跃之间不断线。
    fn detail_page(&self) -> usize {
        self.detail
            .as_ref()
            .map(|view| view.height.saturating_sub(1).max(1))
            .unwrap_or(1)
    }
}

/// 一个详情视图的主体，折到 `width`：那些小节，按它们被决定的顺序，每个在一道分隔线下面
/// （票 03 §Answer）。
///
/// 主体缺席不是错误：每个都有一句话说出来，因为一次点开一个空框的点击，比一次根本没打开的
/// 点击更糟。
fn detail_body(detail: &Detail, session_dir: &str, width: usize) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
    match &detail.kind {
        DetailKind::Thinking { text } => {
            rows.push(section_header(wording::detail_thinking_section()));
            match text {
                Some(text) if !text.trim().is_empty() => {
                    rows.extend(pane::wrap_text(text.trim_end(), width));
                }
                // 没有记录下来的 trace —— 合成器的形状 —— 所以主体把它说出来，而不是开成空白
                // （票 02 §1）。
                _ => rows.push(Line::from(Span::styled(
                    wording::detail_reasoning_unrecorded(),
                    Style::default().fg(Color::DarkGray),
                ))),
            }
        }
        DetailKind::Context { source, content } => {
            rows.push(section_header(&wording::context_source(source)));
            rows.extend(pane::wrap_text(content, width));
        }
        DetailKind::Tool {
            tool_call_id,
            output,
            error,
            args,
            no_result,
        } => {
            rows.push(section_header(wording::detail_args_section()));
            let args = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
            rows.extend(pane::wrap_text(&args, width));
            rows.push(section_header(wording::detail_output_section()));
            if *no_result {
                rows.push(Line::from(Span::styled(
                    wording::no_tool_result(),
                    Style::default().fg(Color::DarkGray),
                )));
            } else if let Some(error) = error {
                rows.extend(pane::wrap_text(error, width));
            } else if let Some(output) = output {
                let (body, truncated) = read_tool_body(tool_call_id, output, session_dir);
                rows.extend(pane::wrap_text(&body, width));
                if truncated {
                    rows.push(Line::from(Span::styled(
                        wording::detail_truncated(),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            } else {
                rows.push(Line::from(Span::styled(
                    wording::detail_output_unavailable(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    rows
}

/// 一个小节标题，画成一道分隔线：文字嵌在一串 `─` 里。
fn section_header(name: &str) -> Line<'static> {
    Line::from(Span::styled(
        wording::detail_section(name),
        Style::default().fg(Color::DarkGray),
    ))
}

/// 落盘文件读得出来时，一条工具结果的整个主体。
///
/// 事件只带首/尾的**预览**；完整文本是落盘到 `outputs/<tool_call_id>.txt` 的那份，而给那个
/// 文件命名的是调用 id —— 从来不是预览自己的叙述（票 02 §4）。文件缺失是文档写明的降级：
/// 预览，加一句说完整文本不可用。文件**为空**是同一种降级：没有完整文本可显示，而预览加那一
/// 句话是诚实的答案，而不是一个空主体（票 08 §8）。
fn read_tool_body(tool_call_id: &ToolCallId, preview: &str, session_dir: &str) -> (String, bool) {
    // 没被裁过的结果没有落盘文件可找，而它的预览就是整个主体：原样显示。只有流不得不*裁*过的
    // 结果在磁盘上才有文件，所以只有那一种可能缺文件（spec §11；2026-09-23，用户报告短输出的
    // 详情不该写着「全文不可用」）。
    if !preview.contains(crate::context::TRUNCATED_MARKER) {
        return (preview.to_owned(), false);
    }
    // `SessionFacts.session_dir` 装的是**会话目录**，所以 outputs 目录只差一次 join —— 与
    // harness 做的是同一道算术（票 01 事实 56）。
    let path = std::path::Path::new(session_dir)
        .join(crate::session::store::OUTPUTS_DIR)
        .join(format!("{tool_call_id}.txt"));
    let unavailable = || {
        (
            format!("{preview}\n{}", wording::detail_output_unavailable()),
            false,
        )
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return unavailable();
    };
    if text.is_empty() {
        return unavailable();
    }
    if text.chars().count() <= DETAIL_MAX_CHARS {
        return (text, false);
    }
    let cut: String = text.chars().take(DETAIL_MAX_CHARS).collect();
    (cut, true)
}

/// 把详情覆盖层画在主列之上。
///
/// 它立着的时候占着键盘与滚轮，而转录冻在原处 —— 一个阅读位置，不是一个会动的位置
/// （票 02 §4）。
fn draw_detail(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    if state.detail.is_none() {
        state.detail_rect = None;
        return;
    }
    let Some(area) = panes.detail() else {
        // 没地方画它：让它开着会把键盘扣在一个没人看得见的视图上。
        state.detail = None;
        state.detail_rect = None;
        return;
    };
    // 转录冻在原处：读的人正在看一行，一波输出不许把它拽走 —— 也不许让「N 行新内容」的计数在
    // 他们正读的覆盖层底下往上爬（票 02 §4）。
    state.pane.set_following(false);
    state.pane.set_holding(true);
    // 框外的一次点击能打中什么，只有记下来之后才存在。
    state.detail_rect = Some(area);
    let Some(view) = state.detail.as_ref() else {
        return;
    };
    let inner = layout::inner(area);
    // 边框里面一列空气，好让文字不碰外框。它是被**让出来**的，而不是吃掉主体：在低于「主体
    // 两行加标题与页脚」的那个高度时，内边距会藏起读的人正是为了它才打开覆盖层的内容
    // （2026-09-23）。
    let pad_x = u16::from(inner.width > DETAIL_PADDING * 3);
    let pad_y = u16::from(inner.height >= DETAIL_PADDING * 2 + DETAIL_MIN_TEXT_ROWS);
    let text = Rect::new(
        inner.x + DETAIL_PADDING * pad_x,
        inner.y + DETAIL_PADDING * pad_y,
        inner.width.saturating_sub(DETAIL_PADDING * 2 * pad_x),
        inner.height.saturating_sub(DETAIL_PADDING * 2 * pad_y),
    );
    // 下面的一切都按**带内边距**的矩形定尺寸：一个比显示它的框高一行的主体窗口，会把末尾几行
    // 裁掉，内边距当初就是这样吃掉了它正在为之腾地方的那份主体的一行。
    let body_rows = text.height.saturating_sub(2) as usize;
    let max_top = view.body.len().saturating_sub(body_rows);
    let top = view.top.min(max_top);
    let rows: Vec<Line<'static>> = view
        .body
        .iter()
        .skip(top)
        .take(body_rows)
        .cloned()
        .collect();
    // 页脚数的是**屏幕上最后一行**，不是第一行：一个已经滚到底的读的人就在底部，不管窗口恰好
    // 从哪一行开始（2026-09-23，用户报告：最后一行可见时它显示 `94/154`）。
    let footer = wording::detail_footer(
        (top + body_rows).min(view.body.len()).max(1),
        view.body.len().max(1),
    );
    let title = view.detail.title.clone();

    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    // 边框穿发言者的颜色：这个框属于一行，而它属于谁的行，应该在你读它一个字之前就看得出来
    // （2026-09-23）。
    frame.render_widget(
        WidgetBlock::default()
            .borders(Borders::ALL)
            .border_set(border::LIGHT_TRIPLE_DASHED)
            .border_style(Style::default().fg(view.detail.color)),
        area,
    );
    // 标题行是被点那一行自己的文字，这样读的人知道他们打开的是哪一行。
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_columns(&title, text.width as usize),
            Style::default().add_modifier(Modifier::BOLD),
        ))),
        Rect::new(text.x, text.y, text.width, 1),
    );
    // 主体拿走标题与页脚之间的一切；页脚钉在覆盖层最后一行文字上，所以两者不可能重叠
    // （票 03 §Answer）。
    let body = Rect::new(
        text.x,
        text.y + 1,
        text.width,
        text.height.saturating_sub(2),
    );
    frame.render_widget(Paragraph::new(rows), body);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(Color::DarkGray),
        ))),
        Rect::new(text.x, text.y + text.height - 1, text.width, 1),
    );
    let view = state.detail.as_mut().expect("刚刚查过它不是 None");
    view.height = body_rows;
    view.top = top;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一条上下文注入在转录里是一行，在详情里是它的正文（票 19）。
    #[test]
    fn a_context_injection_row_opens_its_loaded_data() {
        let block = Block::ContextInjected {
            source: ContextSource::McpCatalog,
            content: "[注入] MCP 加载\n\n- `fake`（stdio）：已连接\n".to_owned(),
        };
        let mut colors = SpeakerColors::new(&[]);
        let lines = paint_block(&block, &mut colors, 80);
        assert_eq!(lines.len(), 1, "注入只占一行");
        let detail = lines[0].link.clone().expect("这一行该点得开");
        assert_eq!(detail.title, "[上下文注入：MCP 加载]");

        let body = detail_body(&detail, "/tmp", 80);
        let text: String = body.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(text.contains("MCP 加载"), "{text}");
        assert!(text.contains("`fake`（stdio）：已连接"), "{text}");
    }

    /// 退出手势的窗口写死在 500 毫秒：它同时是提示的寿命，不做配置项
    /// （`.scratch/exit-gesture/spec.md` §1、§6）。
    #[test]
    fn the_exit_gesture_window_is_half_a_second() {
        assert_eq!(GESTURE_WINDOW, std::time::Duration::from_millis(500));
    }

    /// 「该不该到点」是一条纯判定，所以它有一条单元断言；主循环那根接线本身要真 pty
    /// （`.scratch/exit-gesture/spec.md` §6）。
    #[test]
    fn a_deadline_is_due_only_once_it_has_passed() {
        let now = std::time::Instant::now();
        assert!(!exit_gesture_due(None, now), "没有 deadline 就永远不到点");
        assert!(!exit_gesture_due(
            Some(now + std::time::Duration::from_millis(1)),
            now
        ));
        assert!(exit_gesture_due(Some(now), now), "正好到点就算到点");
        assert!(exit_gesture_due(
            Some(now - std::time::Duration::from_millis(1)),
            now
        ));
    }

    /// 提示符的颜色是维护者的脚本翻进这个渲染器的结果，所以这些数字是对着脚本自己的输出钉的。
    ///
    /// 脚本每 **1/60 秒**走 0.005 色相、0.05 呼吸，所以两者在*某个时刻*上一致，而**不是**在
    /// 某个下标上：一个脉冲帧是 60 ms，也就是三个半脚本帧，我们这里的帧 5 是脚本的帧 18。
    /// 这些三元组就是 `colorsys.hsv_to_rgb` 为脚本的帧打出来的
    /// （`.scratch/tui-input-pulse/spec.md` §2b）。
    #[test]
    fn the_prompt_colour_is_the_script_at_the_same_moment() {
        assert_eq!(PULSE_FRAME.as_millis(), 60);
        for (frame, expected) in [
            (0u64, Color::Rgb(216, 97, 97)),
            (5, Color::Rgb(216, 146, 63)),
            (10, Color::Rgb(203, 216, 55)),
            (30, Color::Rgb(131, 196, 216)),
        ] {
            assert_eq!(
                prompt_colour(frame),
                expected,
                "帧 {frame}：脚本在第 {} 秒打出来的就是这个颜色",
                frame as f64 * PULSE_FRAME.as_secs_f64()
            );
        }
    }

    // --- 目标循环跑着时的键盘（`.scratch/goal-loop/spec.md` §5） -------------

    /// 一个能收键盘的 TUI 状态：够跑那几条键的规矩。
    fn state() -> TuiState {
        TuiState::new(
            SessionFacts {
                session_id: "s-1".to_owned(),
                session_dir: "/tmp/s-1".to_owned(),
                model: "fake-model".to_owned(),
                context_window: 100_000,
                mode: crate::permissions::Mode::Workspace,
                budget_limit: None,
                number_style: crate::render::wording::NumberStyle::Cn,
                speaker_order: vec!["kimi".to_owned()],
            },
            std::path::PathBuf::from("/x/fs-agent"),
            None,
        )
    }

    #[test]
    fn a_muted_input_area_responds_to_keys_but_does_not_take_typing() {
        let mut state = state();
        state.running = true;

        // 没禁言时打字照旧落进草稿。
        state.key(Key::Char('x'));
        assert_eq!(state.editor.text(), "x");
        state.editor.clear();

        // 禁言之后不落字 —— 键位照旧响应，但编辑与提交都进不来。
        state.muted = true;
        for key in [Key::Char('x'), Key::Enter, Key::Tab] {
            state.key(key);
        }
        assert_eq!(state.editor.text(), "", "禁言时不落字");
        assert!(state.events.is_empty(), "也没有手势被误发出去");
    }

    #[test]
    fn esc_while_a_goal_runs_asks_before_it_stops() {
        let mut state = state();
        state.running = true;
        state.muted = true;

        // 误按 Esc：不是取消，而是问一句。
        state.key(Key::Esc);
        assert!(
            state.events.is_empty(),
            "问出来的时候还没停：{:?}",
            state.events
        );
        let modal = state
            .pending
            .as_ref()
            .and_then(Pending::modal)
            .expect("屏幕上立着那个确认框");
        assert_eq!(modal.title, wording::goal_stop_title());
        assert_eq!(modal.choices.len(), 2, "两个答案：继续跑与停下");
        assert_eq!(modal.choices[0].label, "继续跑", "安全的那个答案排在前面");

        // **默认停在「继续跑」**：Enter 关掉框，什么都不发生。
        state.key(Key::Enter);
        assert!(state.events.is_empty(), "Enter 是继续跑");
        assert!(state.pending.is_none(), "框关掉了");

        // 再误按一次 Esc，框又立起来；关掉它就是继续跑。
        state.key(Key::Esc);
        assert!(state.pending.is_some(), "框又立起来了");
        state.key(Key::Esc);
        assert!(state.events.is_empty(), "Esc 也是继续跑");
        assert!(state.pending.is_none(), "而框关掉了");
    }

    #[test]
    fn only_an_explicit_stop_choice_stops_the_goal_loop() {
        let mut state = state();
        state.running = true;
        state.muted = true;
        state.key(Key::Esc);
        // 明确选「停下」：推一条取消手势，循环据此收尾并落一条「人主动停」的事件。
        state.key(Key::Char('s'));
        assert_eq!(state.events, vec![FrontEndEvent::Cancel]);
        assert!(state.pending.is_none());
    }

    #[test]
    fn without_a_goal_loop_esc_is_still_the_plain_cancel_gesture() {
        let mut state = state();
        state.running = true;
        // 一次普通回合：Esc 照旧就是取消，不多问一句。
        state.key(Key::Esc);
        assert_eq!(state.events, vec![FrontEndEvent::Cancel]);
        assert!(state.pending.is_none());
    }

    /// 色环，钉在每个实现都同意的那六个点上 —— 分区算术里一个舍入错误最先显形的那几个角。
    #[test]
    fn hsv_to_rgb_matches_the_shortcut_table() {
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), (255, 255, 255));
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0));
        assert_eq!(hsv_to_rgb(1.0 / 3.0, 1.0, 1.0), (0, 255, 0));
        assert_eq!(hsv_to_rgb(2.0 / 3.0, 1.0, 1.0), (0, 0, 255));
        // 色环闭合：色相 1 就是色相 0，而越过它的色相会回绕而不是 panic。
        assert_eq!(hsv_to_rgb(1.0, 0.4, 0.8), hsv_to_rgb(0.0, 0.4, 0.8));
        assert_eq!(hsv_to_rgb(2.25, 0.4, 0.8), hsv_to_rgb(0.25, 0.4, 0.8));
    }

    /// 下落短横**在这里**做单元测试，因为它已经不在屏幕上了（票 08）：`draw_sidebar_identity`
    /// 传的是 `None`，所以没有集成测试能把它那些帧从一份渲染出来的缓冲里走一遍。这段代码留给
    /// 下一个关于标记该怎么动的想法，而这就是在此期间让它保持诚实的东西 —— 一个悄悄停摆了的
    /// 保留动画，比根本没有动画更糟。
    #[test]
    fn the_falling_dash_steps_down_one_row_per_frame_and_wraps() {
        let rows = wording::logo_lines();
        for frame in 0..10u64 {
            let expected_row = (frame as usize) % rows.len();
            let lines = mark_lines(Some(frame));
            for (index, (line, _)) in lines.iter().enumerate() {
                let cell: String = line
                    .chars()
                    .skip(MARK_DASH_COLUMN)
                    .take(MARK_DASH_WIDTH)
                    .collect();
                if index == expected_row {
                    assert_eq!(cell, "▀▀▀▀", "帧 {frame}：横条落在第 {expected_row} 行");
                } else {
                    assert_eq!(
                        cell.trim(),
                        "",
                        "帧 {frame}：第 {index} 行是空的，所以横条从不分开"
                    );
                }
            }
        }
        // 而静止的标记就是 `logo_lines` 画它那根短横的那一行 —— 中间那一行。
        assert_eq!(
            mark_lines(None)[rows.len() / 2].0,
            rows[rows.len() / 2],
            "空闲的标记保持标记一直画的那一行"
        );
    }

    /// `map_key` 是 crossterm 的词汇表变成这个渲染器词汇表的唯一地方，而这里漏掉一个键就是一个
    /// 悄无声息的死键 —— 那是任何渲染测试都看不见的，因为它们全都从 [`Key`] 开始。
    #[test]
    fn the_keys_the_pane_answers_to_map_from_crossterm() {
        let plain = |code| map_key(KeyEvent::new(code, KeyModifiers::empty()));
        assert_eq!(plain(KeyCode::PageUp), Some(Key::PageUp));
        assert_eq!(plain(KeyCode::PageDown), Some(Key::PageDown));
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)),
            Some(Key::CtrlG)
        );
        // 换行键：整个多行编辑器就挂在这一个键上。
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
            Some(Key::CtrlJ)
        );
        // 挂起：raw 模式把终端的 SIGTSTP 吃成了按键，所以这一个必须被认出来
        // （`.scratch/suspend-gesture/spec.md` §2）。
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL)),
            Some(Key::CtrlZ)
        );
        // 一个光秃秃的 `g` 是文字，不是手势。
        assert_eq!(plain(KeyCode::Char('g')), Some(Key::Char('g')));
    }
}
