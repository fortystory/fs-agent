//! ratatui 界面（`.scratch/tui-sidebar/spec.md` §1–§2，ADR 0002）。
//!
//! 有三条性质是结构性的，不是风格问题：
//!
//! * **alt screen，一帧。** TUI 画一条左栏与一条主列，中间是一条竖虚线；四周没有
//!   外框，终端自己就是边界（`.scratch/tui-chrome/spec.md` §1）。左栏放标记与会话的
//!   读数，主列自上而下堆着转录（右边缘带滚动条与回合条）、状态行、输入区与提示行。
//!   转录住在自己的缓冲里，而不是终端的滚动回退里 —— 内联视口那个漂移的光标也正是
//!   这么消掉的：全屏下窗格原点永远是 `(0, 0)`。
//! * **渲染器占着键盘。** 它是唯一读终端事件的 task，并且通过注入的 console 通道
//!   回答循环的请求（[`ConsoleRequest`]）。输入与输出不打架，靠的就是这一条。
//! * **`select!` 管 broadcast 与按键。** 渲染事件、循环的请求与键盘输入是三个互相
//!   独立的来源；`select!` 是它们合流的方式，不用再引入一条顺序未定义的第二通道。
//!   已经排进队列的在画帧之前先排空，所以一个爆发输出的 provider 花掉的是帧而不是
//!   事件。循环里有**两个**定时器：提示符与状态行字形循环共用的脉冲
//!   （**一直在走**，`.scratch/tui-visual-language/spec.md` §32 —— 空闲不再零唤醒），
//!   以及退出手势的 deadline（只在第一下举手之后，`.scratch/exit-gesture/spec.md` §6）。

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use ratatui::buffer::CellWidth;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as CtEvent, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{
    BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, SetTitle, enable_raw_mode,
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

use crate::events::{ContextSource, Event, EventPayload, Role, StopReason, ToolCallId, Usage};
use crate::permissions::{Answer, Mode, PermissionRequest};
use crate::provider::capability::caps_for;
use crate::questions::{UserAnswer, UserAnswers, UserQuestion};

use super::changes;
use super::editor::{self, Input};
use super::file_index::{self, FileIndex};
use super::files;
use super::highlight;
use super::input::{CatalogEntry, ConsolePort, ConsoleRequest, FrontEndEvent, PickerKind};
use super::layout;
use super::links;
use super::opener;
use super::palette;
use super::pane::{self, Pane};
use super::panel::Panel;
use super::selection;
use super::severity::Severity;
use super::token;
use super::transcript::{Block, BlockId, ToolBlock, ToolOutcome, Transcript, summarize_args};
use super::width::{char_columns, ellipsize_line, text_columns, truncate_columns};
use super::wording::{self, speaker_label};
use super::{DeltaKind, Render, RenderEvent};
use crate::render::viewer::{self, Viewer};

/// 流式文本保留多少才裁成一条尾巴。转录不需要整条消息都活着：完成的 `Message` 块会
/// 把它整段重画一遍。
const LIVE_BUFFER: usize = 4_000;

/// 大于这个值的粘贴要先问一句才收下（spec §7）。
const PASTE_CONFIRM_CHARS: usize = 100_000;

/// 一帧吸收多少个排队中的渲染事件。有界的排空让突发输出不至于把键盘饿掉整整一帧的
/// 工作量。
const DRAIN_LIMIT: usize = 4_096;

/// `@` 菜单一次最多收多少条候选。
///
/// 菜单是**一扇窗口**（`layout::MENU_MAX_ROWS` 行，高亮始终可见），所以它要的候选比窗口
/// 多，好让人用 `↓` 走下去；但也不能把几千条路径整批塞进状态里。前缀匹配通常已经把范围
/// 收得很小。
const TOKEN_MENU_CANDIDATES: usize = 200;

/// 草稿里两条能兑现的记号的颜色。定义住在色板里 —— 颜色值集中一处；判据（「能兑现」与
/// chip 是同一条）仍归 `.scratch/input-tokens/spec.md` §4，这里只是给那个模块的读者留个名字。
pub use super::palette::{TOKEN_COMMAND, TOKEN_REFERENCE};

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
    /// **换行**：`Ctrl-J`，以及支持键盘增强协议的终端上的 `Shift+Enter`
    /// （`.scratch/tui-feedback/spec.md` §9）。
    Newline,
    /// `Ctrl-Z`：**挂起** —— 交还终端、停到后台，`fg` 回来重进并重绘
    /// （`.scratch/suspend-gesture/spec.md` §2）。它不归任何视图管，所以
    /// [`TuiState::key`] 在一切守卫之前就把它接走。
    CtrlZ,
    /// `Ctrl-O`：左栏的开关 —— 收起与叫回（`.scratch/sidebar-toggle/spec.md` §3）。它是纯
    /// 视图手势，所以忙闲都生效，也不清举手；只有两个独占键盘的视图（详情覆盖层、历史重放）
    /// 拦得住它。
    CtrlO,
    /// `Ctrl-T`：打开模型/档位的选择器（`.scratch/model-switching/spec.md` §7）。
    ///
    /// 它不归任何视图管 —— 与 `Ctrl-Z` 同一档 —— 但**选择器立着时归选择器**（先判它）。
    ///
    /// 为什么不取 `Ctrl-M`：它在终端里就是回车（`\r`），crossterm 报成 `KeyCode::Enter`，
    /// 与「提交」正面撞车 —— 与 `Ctrl-J` 是同一件事（`.scratch/tui-feedback/spec.md` §9）。
    CtrlT,
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
                'j' => Some(Key::Newline),
                'z' => Some(Key::CtrlZ),
                'o' => Some(Key::CtrlO),
                // `Ctrl-M` 不在这儿：它在终端里就是回车（`\r`），落到下面那个 `Enter` 分支
                // 里去 —— 与「提交」撞车（spec §7）。
                't' => Some(Key::CtrlT),
                _ => None,
            };
        }
    }
    // `Shift+Enter` 与 `Ctrl-J` 是同一个动作。它只在支持键盘增强协议的终端里到得了这里 ——
    // 别的终端发来的是不带修饰的 `Enter`（提交），与从前一样
    // （`.scratch/tui-feedback/spec.md` §9）。
    if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT) {
        return Some(Key::Newline);
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

impl SpeakerColors {
    /// 一份名册对应的调色板：按槽位发出去的顺序列出讨论者。没有名册的调用方传一个空的
    /// 进来，于是每个名字都是灰的。
    pub fn new(roster: &[String]) -> Self {
        let roster = roster.to_vec();
        // 名册按位置认领调色板槽位：第 `n` 个讨论者拿槽位 `n`。讨论者比颜色多时多出来的
        // 槽位会绕回来，所以这里的认领是 `slot < len` 而不是整个名册。
        let claimed: Vec<usize> = (0..roster.len().min(palette::DEBATERS.len())).collect();
        let free_slots = (0..palette::DEBATERS.len())
            .filter(|slot| !claimed.contains(slot))
            .collect();
        Self {
            uncoloured: roster.is_empty(),
            roster,
            free_slots,
            extra: Vec::new(),
        }
    }

    /// 名册在会话中途换了（换 provider 就换了发言者的名字，spec §4）时的更新。
    ///
    /// **已经分配过槽位的名字一个都不动** —— 颜色对读的人必须稳定，而新名字走「没人认领的
    /// 槽位」那条路，与 `/discuss` 中途加入的讨论者同一条。于是同一场会话里先后两个名字各有
    /// 各的颜色，而转录里已经画过的行不改名（事件流只追加）：那正是那一段注释写明的意图。
    ///
    /// 注意 [`Self::roster`] **不**被换掉：它是**组装时**那份名册，也就是槽位按位置分配
    /// 的依据；换掉它会让同一个名字在换一次之后拿到另一个颜色。当前名册是
    /// [`SessionFacts::speaker_order`]，它只管显示。
    pub fn adopt_roster(&mut self, roster: &[String]) {
        if roster.is_empty() {
            return;
        }
        self.uncoloured = false;
        for name in roster {
            let known = self.roster.contains(name) || self.extra.contains(name);
            if !known {
                self.extra.push(name.clone());
            }
        }
    }

    /// 一个发言者名字的颜色。
    ///
    /// 调色板在组装时就定下，所以一个会话的名字到颜色的映射只要它在跑就是稳定的 ——
    /// 包括会话中途才第一次出现的名字（票 07 §1）。
    fn of(&mut self, speaker: &crate::events::SpeakerId) -> Color {
        use crate::events::SpeakerId;
        if self.uncoloured {
            // 「没有名字册」与「系统」用**同一种灰**（`.scratch/tui-visual-language/spec.md`
            // 用户故事 5）：同一个语义不该在屏幕上出现两种样子。
            return palette::SYSTEM;
        }
        match speaker {
            SpeakerId::Debater(id) => self.debater(id.as_str()),
            SpeakerId::Executor(_) => palette::EXECUTOR,
            SpeakerId::User => palette::USER,
            SpeakerId::System => palette::SYSTEM,
        }
    }

    fn debater(&mut self, id: &str) -> Color {
        if let Some(slot) = self.roster.iter().position(|name| name == id) {
            return palette::DEBATERS[slot % palette::DEBATERS.len()];
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
            Some(slot) => palette::DEBATERS[*slot],
            None => palette::SYSTEM,
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
    /// 点开一个工作区文件时用哪个查看器、它多宽（`[ui] file_viewer` 与 `file_viewer_width`）。
    ///
    /// 与 `number_style` 同一条：配置里定下来的值，组装时注入一次，会话中途不变
    /// （`.scratch/nvim-file-viewer/spec.md` §2）。
    pub file_viewer: crate::config::FileViewerSettings,
    /// 看一份 diff 时交给哪个外部命令（`[ui] diff_viewer` 与 `diff_viewer_args`）。
    ///
    /// 与 `file_viewer` 同一条：配置里定下来的值，组装时注入一次，会话中途不变
    /// （`.scratch/diff-page/spec.md` §8）。
    pub diff_viewer: crate::config::DiffViewerSettings,
    /// 这个会话的讨论者，按抽出来的顺序。单 agent 会话列出它那一个档案；讨论列出名册
    /// 产出的那一对。它就是发言者颜色的来源，所以它和别的事实一样在组装时注入：
    /// 名册不在流上（票 07 §1）。
    pub speaker_order: Vec<String>,
    /// 状态行那两格能不能点、能不能真的换（spec §12）。
    ///
    /// 单 agent 会话是 `true`；**讨论 CLI 那条路径**是 `false`，因为那里的
    /// [`model`](Self::model) 是两个模型的拼法 —— 那是名册里的配置事实，不是一场活会话
    /// 能中途改的值（`.scratch/model-switching/spec.md` §12）。组装时定下来：讨论那些参与者
    /// 是同时组装的，而换档要换 provider 与请求参数，与模式手势不同形。
    pub switchable: bool,
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
        // 「文件查看器有新画面」的通知：读线程每收到字节就叫一声，循环据此重绘。60 ms 的
        // 脉冲太慢 —— 打字会钝（`.scratch/nvim-file-viewer/spec.md` §5）。
        let (viewer_tx, mut viewer_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        state.set_viewer_wake(viewer_tx);
        // 文件索引的结果从这条通道回到循环。遍历跑在 `spawn_blocking` 里，于是一次大
        // 工作区的遍历永远不占着这个 task，也从不进任何一次按键的处理路径
        // （`.scratch/input-tokens/spec.md` §1）。
        let (files_tx, mut files_rx) =
            tokio::sync::mpsc::unbounded_channel::<Vec<std::path::PathBuf>>();

        // 改动页的读数从这条通道回来。它是渲染器自己在**宿主侧**起的一次只读 git：不进事件
        // 流、不进 `messages`、不过权限门与沙箱，而它必须 `tokio::spawn` 出去 —— 一帧的
        // `select!` 只等通道与两个定时器，任何一支里都不许 await
        // （`.scratch/diff-page/spec.md` §3）。
        let (changes_tx, mut changes_rx) =
            tokio::sync::mpsc::unbounded_channel::<changes::Outcome>();

        // 弹窗里那一份 diff 也是宿主侧跑出来的，走同一条形状：**打开那一刻**置一次请求，
        // 循环起子进程，结果经这条通道回来填进还开着的那一个弹窗
        // （`.scratch/diff-page/spec.md` §6）。
        let (diff_tx, mut diff_rx) =
            tokio::sync::mpsc::unbounded_channel::<(u64, changes::Body, Option<String>)>();

        // 重新打开的会话在画任何东西之前先等这次重放。循环把它作为**第一条** console 请求
        // 推过来，紧接组装之后、横幅之前；与此同时组装已经在渲染通道上发出了那些恢复
        // 结果，而它们已经在那份重放快照里了。在这里等，正是为了不让它们被画在历史之上、
        // 然后再被重放画一遍。端口没了就放弃等待。
        if reopened {
            if let Some(request) = port.recv().await {
                state.request(request);
            }
        }

        // 两个定时器（`.scratch/tui-input-pulse/spec.md` §2b，`.scratch/exit-gesture/spec.md` §6）：
        //
        // - `pulse`（60 ms）**一直在走**。用 `interval` 而不是每一趟新建一个 `sleep`：一个
        //   突发一千条增量的 provider 会让每轮迭代都重置一次 sleep，于是动画恰恰会在会话最忙
        //   的时候停住。`Delay` 让积压的漏帧不会在循环从某个长帧里回来时被一次性花掉。
        //   **它曾经只在一次运行进行中的时候武装**；`.scratch/tui-visual-language/spec.md` §32
        //   把它放宽到始终 —— 空闲时状态行那个字形循环也在动，只是慢下来。代价明码标价：
        //   空闲不再是零唤醒。
        // - 退出手势的 deadline（500 ms）只在举着手的那些帧里武装，到点就把举手作废。
        let mut pulse = tokio::time::interval(PULSE_FRAME);
        pulse.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            let mut closed = false;
            // 预热与「提交后重扫」在这里真去起遍历：状态机置位（进 TUI 一次、每次提交再
            // 一次），循环发活。已经在跑的时候 `take_file_scan` 不会被置位 —— 两次遍历
            // 的结果不会互相盖掉。
            if state.take_file_scan() {
                let root = state.cwd().to_path_buf();
                let tx = files_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let _ = tx.send(file_index::scan(&root));
                });
            }
            // 改动页那一次取数走 `tokio::spawn`（它要 await `tokio::process` 与超时，不是
            // blocking 的活）。同一时刻只有一次在飞 —— `take_changes` 里那个 `loading` 位。
            if state.take_changes() {
                let root = state.cwd().to_path_buf();
                let tx = changes_tx.clone();
                tokio::spawn(async move {
                    let _ = tx.send(changes::status(&root).await);
                });
            }
            // 弹窗里那一份 diff：一次 `git diff HEAD -- <path>`（或者对未跟踪文件的一次读盘
            // —— 那一档在打开时就同步做完了，不会走到这里）。配了 `[ui] diff_viewer` 时，
            // 这一趟还会把那份 diff 从 stdin 喂给它（`changes::body` 里分派）。
            if let Some((serial, file, columns)) = state.take_diff_read() {
                let root = state.cwd().to_path_buf();
                let viewer = state.diff_viewer().clone();
                let tx = diff_tx.clone();
                tokio::spawn(async move {
                    let (body, note) = changes::body(&root, &file, &viewer, columns).await;
                    let _ = tx.send((serial, body, note));
                });
            }
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
                // 四个来源，加上两个定时器。曾经住在这里的重绘 tick 随它存在的那个时钟一起
                // 走了 —— 待答的问题从 console 端口来，事件从渲染通道来，而键就是键，所以
                // 没有*别的*东西在等着被注意到（票 05 §1）。两个定时器都只是时间的函数：
                // 一直在走的脉冲（提示符的色相与状态行的字形循环），与举着手的那个 deadline
                // （`.scratch/tui-visual-language/spec.md` §32、`.scratch/exit-gesture/spec.md`
                // §6）。
                //
                // deadline 在 `select!` 之前取成值：`Instant` 是 `Copy`，取完借用就结束，
                // 分支里才借得到 `&mut state`。没举手时给它一个占位时刻 —— 关掉 poll 的是
                // 分支上那个 `if`，与 `pulse` 同构。
                let deadline = state.exit_deadline();
                tokio::select! {
                    received = receiver.recv() => closed = state.take_render_event(received),
                    maybe_event = keys.next() => state.terminal_event(maybe_event),
                    request = port.recv() => closed = state.port_request(request),
                    Some(paths) = files_rx.recv() => state.files_loaded(paths),
                    Some(outcome) = changes_rx.recv() => state.changes_loaded(outcome),
                    Some((serial, body, note)) = diff_rx.recv() => state.diff_loaded(serial, body, note),
                    Some(()) = viewer_rx.recv() => state.mark_dirty(),
                    _ = pulse.tick() => state.tick(),
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
                        state.live_event(RenderEvent::diagnostic(wording::renderer_dropped(
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
            if let Some(payload) = state.take_clipboard() {
                // OSC 52 —— 终端自己的剪贴板通道。这条通道是单向的，没有回话的地方，所以写不
                // 进去也不报错、更不探测；shift 原生选择照旧可用
                // （`.scratch/tui-feedback/spec.md` §6）。
                let mut clipboard_out = std::io::stdout();
                let _ = execute!(clipboard_out, Print(payload));
            }
            if let Some(target) = state.take_open_request() {
                // 交给系统默认程序（`.scratch/clickable-links/spec.md` §3、§5）：宿主侧、
                // argv 直传、不经过 `bash` 工具、不过权限门、不进沙箱。成没成由这里写回一句
                // 回执 —— 状态机只交出一个字符串，它不知道进程这一层。
                let receipt = match opener::open(&target) {
                    Ok(()) => wording::opened(&target),
                    Err(error) => wording::open_failed(&error.to_string()),
                };
                state.note_open_receipt(receipt);
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
                paint_frame(&mut terminal, &mut state);
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
    // 作业里有多个进程的情形，而 heng 的组里只有它自己（`bash` 与动态工具刻意各自成组）。
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
/// 开关 —— 所以这几样得我们自己撤，正常路径与 panic 路径都一样。留着不管，heng
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
    // 键盘增强协议（Kitty keyboard protocol）：请终端把 `Shift+Enter` 这类带修饰的键报成
    // `CSI 13;2u`，而不是一个与裸 `Enter` 分不开的字节 —— 没有它，「Shift+Enter 换行」在
    // 协议层就是一句空话（`.scratch/tui-feedback/spec.md` §9）。不支持的终端把这一串当没
    // 看见，于是那些终端里的 `Shift+Enter` 仍旧提交；挂起恢复会成对地弹了再推。
    let _ = execute!(
        std::io::stdout(),
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
}

impl Drop for TerminalModes {
    fn drop(&mut self) {
        disable_terminal_modes();
    }
}

fn disable_terminal_modes() {
    let _ = execute!(
        std::io::stdout(),
        PopKeyboardEnhancementFlags,
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
    /// 对话视图的窗格：主列那些行按画出来的宽度折行，带自己的视口（`.scratch/trace-tab/spec.md` §1）。
    conversation: Pane,
    /// 轨迹视图的窗格：主列 `轨迹` 页画它。与对话视图共用同一份共享源（`painted`），但各持
    /// 折行缓存、宽度、视口与「新行」计数。
    ///
    /// 它**常驻**：轨迹视图住在主列里，而主列永远在（`.scratch/trace-in-main/spec.md` §3）。
    trace: Pane,
    /// 当前消息正在流的尾巴。
    live: String,
    /// 已经画进窗格的那些绘制记录，按到达顺序。
    ///
    /// 表格与代码块是按宽度排出来的，所以宽度一变，**源行本身**就得整批重排（spec §1）。
    /// 留着这份清单就是为了那时候按新宽度重放它们。代价是 `Tui` 多持一份块（与 `pane`
    /// 已经持有的源行同量级）—— 先按「全量重放」实现，简单可靠优先。
    painted: Vec<Painted>,
    /// 现在在不在一次模型调用里（`TurnStarted` 之后、`TurnEnded` 之前）。合成器那次调用不属于
    /// 任何回合，所以它的用量走独立行那条老路（§4）。
    in_call: bool,
    /// 这条发言里模型被调用了几次，以及那几次的读数之和 —— 收尾那条合计的原料（§5）。
    turn_calls: usize,
    turn_usage: Usage,
    /// 对话视图的源行是按多宽排的。
    ///
    /// 表格与代码块是按宽度排出来的，所以宽度一变，**源行本身**就得整批重排（spec §1）：
    /// 它是重放判据的一半，另一半是 [`TuiState::trace_width`]。
    conversation_width: u16,
    /// 轨迹视图的源行是按多宽排的 —— 主列的内容宽（两个视图同源，也都常驻）。
    trace_width: u16,
    /// 对话视图的跨块排版状态（上一个发言者、上一条是不是消息）：换人空一行、同一人连发的
    /// 多段正文只留一个名字，都靠它（`.scratch/tui-visual-language/spec.md` §23）。窗格清空
    /// 重放时它跟着一起清。
    conversation_flow: Flow,
    /// 轨迹视图那一份：它只按发言者空行，消息行每条都带名字。
    trace_flow: Flow,
    /// 轨迹页**自己的**宽度档：前缀带不带方括号看它，不看正文列数。
    ///
    /// 判据量的是**主列页宽**，而主列最小 40 列（终端下界就是它），所以它恒为宽档 —— 那个去
    /// 括号的分支还留在 [`prefix_style`] 里（它与共享渲染那条路径共用签名），只是到不了屏幕
    /// （`.scratch/trace-in-main/spec.md` §3）。
    trace_tier_width: u16,
    /// 上一帧画完之后有没有什么东西变了。
    dirty: bool,
    /// 草稿与它的光标。
    editor: Input,
    /// 工作区里的文件与目录：`@` 的候选，也是「能兑现」那条判据的出处
    /// （`.scratch/input-tokens/spec.md` §1）。
    ///
    /// 会话级的一个值：进 TUI 预热一次、每次提交一条消息之后再扫一次。遍历本身跑在
    /// `spawn_blocking` 里，索引从不挡住键盘。
    files: FileIndex,
    /// 该发一次遍历了吗。预热与提交各置一次，渲染循环取走并真去起那个 blocking 任务 ——
    /// 于是「什么时候扫」是状态机的事，「怎么扫」是循环的事，测试不必起任务。
    file_scan_wanted: bool,
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
    /// 文件页那一页自己的状态（`.scratch/files-page/spec.md` §1、§5）。
    ///
    /// 与 [`Panel`]、[`TodoPanel`] 同一族：一页的状态收在一个值里，而它整个只活在进程内 ——
    /// 不进事件流、不落盘。
    files_page: FilesPage,
    /// 改动页那一页自己的状态（`.scratch/diff-page/spec.md` §2、§4）。
    ///
    /// 数据来自渲染器自己在宿主侧跑的 `git status`：不进事件流、不进 `messages`、不过权限门
    /// 与沙箱（只读、不写盘）。
    changes: ChangesPage,
    /// 该发一次 git 取数了吗。与 `file_scan_wanted` **分开一位** —— 两件事的节奏与失败模式
    /// 不同，共用一位会让一边的失败拖住另一边（§4）。
    changes_wanted: bool,
    /// 上一帧左栏页签条的标签行画在哪 —— 「点左栏」把页签条也算进去
    /// （`.scratch/files-page/spec.md` §5）。
    tabs_rect: Option<Rect>,
    /// 键盘交给左栏了吗。
    ///
    /// 它与既有的两个「独占键盘的视图」（详情覆盖层、历史重放）同一族，只是不占满屏：立着
    /// 的时候走树的那几个键归文件页，`Esc` 把键盘还回输入区
    /// （`.scratch/files-page/spec.md` §5）。
    sidebar_keyboard: bool,
    /// 正在显示主列的哪一页：`对话` 还是 `轨迹`（`.scratch/trace-in-main/spec.md` §2）。
    /// 与 [`TuiState::tab`] 同级：只活在这一次进程里，不落配置，重开回到对话。
    main_tab: MainTab,
    /// 用户想不想看见左栏（`Ctrl-O` 切换，`.scratch/sidebar-toggle/spec.md` §1）。
    ///
    /// 与选中的页签同级：只活在这一次进程里，重开回到显示，不落配置、不跨会话。它与宽度是
    /// 两层 —— 这一位说偏好，[`layout::sidebar_tier`] 说可行性 —— 两者相乘才是屏幕上真的有
    /// 的那一栏。
    sidebar_wanted: bool,
    /// 每个发言者的名字用什么颜色画（票 07）。放在这里而不是每行重算，因为会话中途第一
    /// 次出现的名字得保住已经发给它的那个槽位。
    colors: SpeakerColors,
    /// 状态行那一格显示的推理档位（spec §11）。它是**渲染器状态**，与 `mode` 同一格：
    /// 循环推一条 `SessionUpdate` 过来，前端并进它自己的显示，不进事件流 ——
    /// 换档是会话属性，不是这一场会话发生过的一件事。
    effort: Option<crate::config::ReasoningEffort>,
    /// 会话中途换模型/档位的选择器浮层（spec §10）。
    ///
    /// 它**不进** `questionnaire()` 那条路径：问卷是模型发起的，答完就是那条工具调用的结果；
    /// 选择器是人发起的会话属性，答完要回循环去改这一场会话
    /// （`.scratch/model-switching/spec.md` §10）。键盘归属按「谁立着谁拿」判。
    picker: Option<Picker>,
    /// 提示行上那一句短的说明（spec §12：讨论会话里点状态行那两格）。
    ///
    /// 与复制/打开链接那两句回执同一个机制与同一段寿命：提示行一次只留一句最新的。
    notice: Option<(std::time::Instant, String)>,
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
    /// 轨迹页每条来源行背后的详情，与那个窗格平行、按它的上限一起裁剪，好让两者永不脱节。
    /// 不是入口的那些行是 `None`。
    ///
    /// **只有轨迹页有一份**：那是它独有的入口 —— 对话页画的是全文，点它不打开任何东西
    /// （`.scratch/tui-feedback/spec.md` §9）。
    trace_links: std::collections::VecDeque<Option<Detail>>,
    /// 轨迹页每条来源行**属于哪个块** —— 与 [`Self::trace_links`] 平行、按同一个丢弃数裁，
    /// 所以两者与那个窗格永远同长（ADR 0021）。
    ///
    /// 「源行 → 块」这一向是行选择、折叠、命中导航与详情跳转的共同落点，而**块 → 位置**是这张
    /// 表上的一次线性查找（[`Self::first_source_of`]）。不另立一个以块为键的映射：那张表每次
    /// 裁剪都要全体下标左移，以块为键的映射就得跟着重算。
    ///
    /// `None` = 那一行不属于任何块（单位之间的分隔线、折叠出来的折行将来各自另有身份）。
    trace_block_ids: std::collections::VecDeque<Option<BlockId>>,
    /// 轨迹页两级分组的当前进度：一个正在开着的一级组与一个正在开着的二级组。
    ///
    /// 它是**跨块排版状态**的一部分，与 [`Self::trace_flow`] 同进同出（宽度重放时一起清）——
    /// 因为组的边界也是由块序列推出来的，重放要把整本账按同样的顺序再推一遍。
    trace_groups: TraceGroups,
    /// 渲染层自己造的那条记录流（推理增量开出的思考行、诊断、通知、身份注入）已经发到第几件
    /// —— 它们没有信封，于是 `index` 由它补。它**不随重放重置**：重放走的是这份清单而不重新
    /// 发号（重推用的就是清单里那一份身份）。
    next_local_id: u32,
    /// 上一帧轨迹页把每一个显示行画在了哪里，好把一次点击换回它落在的那条来源行。每帧重建，
    /// 与问题覆盖层的命中区域一样，因为只有真画出来的行才会回应指针（票 04 §1）。
    trace_drawn: Drawn,
    /// 上一帧把轨迹页画在哪里，好让滚轮与点击按指针落在哪个视口分派。没画轨迹页就是
    /// `None`（与 [`TuiState::detail_rect`] 同一条「记住读的人真看到了什么」的纪律）。
    ///
    /// 判据刻意是**轨迹页这一帧画出来了没有**，而不是「指针在不在左栏页矩形里」：轨迹
    /// pane 没被画出来时它的取景高度是陈旧的，滚它会算出错的落点（`.scratch/trace-tab/spec.md`
    /// §5 那句「指针在左栏页矩形里就滚轨迹 pane」说的正是轨迹页在屏幕上的那些帧）。
    trace_rect: Option<Rect>,
    /// 详情覆盖层，开着的时候。
    detail: Option<DetailView>,
    /// 文件查看器那一档的浮层（`[ui] file_viewer = "nvim"`）。
    ///
    /// 它与 `detail` **互斥**：同一时刻屏幕上只有一块浮层。状态分开，是因为它管的是一块
    /// **外来**的屏幕（一个进程、一条 pty，键盘与鼠标全给它），而不是我们排版出来的行
    /// （`.scratch/nvim-file-viewer/spec.md` §1）。
    file_viewer: Option<FileViewerPane>,
    /// 上一次真画出来的查看器浮层矩形 —— 鼠标靠它判框内框外，与 `detail_rect` 同一条
    /// 「指针只回应真看见的东西」的纪律。
    file_viewer_rect: Option<Rect>,
    /// 「查看器有新画面」的通知口。由 [`Tui::run`] 注入；测试里是 `None`（测试不跑那个循环，
    /// 也就不必被叫醒）。
    viewer_wake: Option<tokio::sync::mpsc::UnboundedSender<()>>,
    /// 打开这个覆盖层的那个视图，以及它打开前的滚动状态；关掉时还原给它
    /// （票 13、`.scratch/files-page/spec.md` §5）。
    detail_opener: Option<DetailOpener>,
    /// 上一帧把这个覆盖层画在哪里，好让框外的一次点击把它关掉 —— 与指示器遵循的是同一条
    /// 「记住读的人真看到了什么」的规矩（票 02 §4）。
    detail_rect: Option<Rect>,
    /// 正在读的那一份 diff：读什么，以及它属于哪一次打开
    /// （`.scratch/diff-page/spec.md` §6）。
    ///
    /// 弹窗**先立起来**、正文后到：一次 `git diff` 是一次子进程，而处理一个按键的路径上不许
    /// 等它（与那三条「一帧里不 await」同一条纪律）。
    diff_reading: Option<DiffReading>,
    /// 打开过多少次详情弹窗。给在飞的那次取数对号：结果回来时弹窗已经换成别的文件、或者
    /// 干脆关掉了，那一次结果就直接丢掉。
    detail_serial: u64,
    /// 上一帧把**中间的模态**画在哪里，好让滚轮知道指针是不是落在它上面（`tui-chrome` §5）。
    /// 与 [`TuiState::detail_rect`] 同一条规矩：这一帧真的画了什么就记什么，没画出来就是
    /// `None`，而那时滚轮落到转录上。
    modal_rect: Option<Rect>,
    /// 上一帧问卷占着的那块底部（输入区**与**提示行，连它们中间那条线一起）在哪里。问卷
    /// 没有边框，所以它「在哪儿」只能这样记；滚轮据此决定归谁（`tui-chrome` §5）。
    questionnaire_bottom: Option<Rect>,
    /// 上一帧把一个问题的可点部分画在了哪里。
    regions: Regions,
    /// 上一帧每块区域画了哪些显示行 —— 拖选取文本用的那一层
    /// （`.scratch/tui-feedback/spec.md` §5）。与 [`TuiState::regions`] 同一套纪律：每帧重建。
    screen_text: selection::ScreenText,
    /// 按下到抬起之间的一次拖选，没有就是 `None`。
    drag: Option<selection::Drag>,
    /// 最近一次复制：`(时刻, 字数, 行数)`。提示行那句回执从这里来，寿命由
    /// [`RECEIPT_WINDOW`] 定（`.scratch/tui-feedback/spec.md` §6）。
    copied: Option<(std::time::Instant, usize, usize)>,
    /// 一次复制要写出去的那串字节（OSC 52），等着运行期取走。
    ///
    /// 与 `quit` / `suspend` / 标题同一种形状：**状态机算、运行期写** —— `TuiState` 因此仍然
    /// 「不接终端也能测」，跑一次拖选不会真的往测试进程的 stdout 吐一个剪贴板序列。
    clipboard: Option<String>,
    /// 最近一次打开的回执：`(时刻, 那句话)`，成功与失败都走这里
    /// （`.scratch/clickable-links/spec.md` §4）。它由**运行期**写回来 —— 点了链接之后
    /// `xdg-open` 成不成，只有真去 spawn 的那一侧知道，而状态机不该自己起进程。
    opened: Option<(std::time::Instant, String)>,
    /// 一次点击命中的那个目标，等着运行期取走并打开。
    ///
    /// 与 [`TuiState::clipboard`] 同一个形状，理由也一样：点击只**解析**（`Target::resolve`，
    /// 一次 `canonicalize`），把「起一个进程」留给唯一那处运行期代码
    /// （`.scratch/clickable-links/spec.md` §3）。测试因此能逐字断言一次点击要打开什么，
    /// 而它一个浏览器都不会开。
    open_request: Option<String>,
    /// 对话视图这一帧画出来的转录矩形，没有就是这一页没画。
    ///
    /// 与 `trace_rect` 对称、同样只服务点击：落在它里面的一次点击才可能命中**链接热区** ——
    /// 别处的可点文字各有各的语义（`.scratch/clickable-links/spec.md` §1）。
    conversation_rect: Option<Rect>,
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
    /// 正在流的正文属于哪个发言者 —— 等待块与流式正文那一行名字用的就是它（正文一完成，
    /// 名字改由那条源行自己带）。还没有增量时是 `None`，那时用名册的第一个。
    live_speaker: Option<crate::events::SpeakerId>,
    /// 此刻**正在跑**的那个工具，以及它的参数 —— 对话视图末尾那句「正在做什么」用的就是
    /// 它（2026-10-05 维护者的优化）。由 `ToolCallStarted` 置上、`ToolCallCompleted`
    /// 或一次收尾清掉。
    running_tool: Option<(String, serde_json::Value)>,
    /// 一个等着按键的问题。
    pending: Option<Pending>,
    /// 要交回给循环的手势。
    events: Vec<FrontEndEvent>,
    /// 上一帧把主列的「回到末尾」指示器画在哪里，好让一次点击跟用户真看见的东西对得上。
    indicator: Option<Rect>,
    /// 轨迹页的同一份矩形（票 09）。
    trace_indicator: Option<Rect>,
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
    /// 键位**按区域分派**（`.scratch/questionnaire-keys/spec.md` §2、§4），而**回车不在这条
    /// 分派里**（§11 推翻了 §4 的「选项区里回车与空格完全一致」）：它在两个区域同一条规则
    /// ——「处置这一题、往前走」：当前题还没作答就记跳过，然后每题都有着落就提交、否则前进
    /// 一题。空格仍是唯一的选中键，`Tab` 仍是那个明确的跳过。
    ///
    /// **确认不翻页**：并存的意义就是「选项和文本可以一起给」，选完就跳走会把它取消一半，
    /// 所以翻页留给 `←`/`→` 与页脚那三个按钮。
    fn press(&mut self, key: Key) -> bool {
        match key {
            // 回车：两个区域同一条规则（§11）。「末题回车 = 提交」是它的推论 —— 往前走的每
            // 一步都会给没作答的题记上跳过，所以走到末题时前面必然都有着落。
            Key::Enter => {
                self.skip_if_unanswered();
                if self.all_handled() {
                    return true;
                }
                self.advance();
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
            // 选项区里的移动：`j`/`k`、Emacs 的 `Ctrl-N`/`Ctrl-P` 与**四个方向键**走
            // 同一条路 —— 所以它们在输入区里什么都不做（2026-10-06 推翻，see §2 那张表：
            // 输入区那三格的旧值是「回选项区并移动高亮」与「同左」）。人在输入区打自由
            // 文本，方向键的每一次挪动都是一次没被要求的状态变化；想离开那里有 `Esc`、
            // `Tab`、`Enter` 三条路。
            Key::Up if self.zone == Zone::Options => self.step(-1),
            Key::Down if self.zone == Zone::Options => self.step(1),
            Key::Char('k') | Key::CtrlP if self.zone == Zone::Options => self.step(-1),
            Key::Char('j') | Key::CtrlN if self.zone == Zone::Options => self.step(1),
            // `←` 只移动，不记任何东西：往回走是「我还没决定」，往前才是「这题我不要了」。
            Key::Left if self.zone == Zone::Options => self.back(),
            // `→` 与回车同一条「往前走」的规则，但它**不提交**：末题上什么都不做（不记、不
            // 前进）—— 提交不可逆，不该由一个移动键承担。
            Key::Right if self.zone == Zone::Options => {
                if self.index + 1 < self.questions.len() {
                    self.skip_if_unanswered();
                    self.advance();
                }
            }
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

    /// 「往前走」之前那一步：当前题还没作答就把它记成**跳过**（spec §11）。
    ///
    /// 只动没作答的题 —— 答过的题往前走时不该被改成「跳过」，否则 `answers()` 里那条「跳过
    /// 无条件优先」会把用户真答的那一份吞掉。
    fn skip_if_unanswered(&mut self) {
        let draft = &mut self.drafts[self.index];
        if !draft.handled() {
            draft.skipped = true;
        }
    }

    /// 回车这一下会不会**提交**：把当前题（若还没作答）记成跳过之后，是不是每题都有着落。
    ///
    /// 页脚据此说「回车 提交」还是「回车 下一题」—— 提示段跟着它实际会做什么变（§11）。
    fn enter_submits(&self) -> bool {
        self.drafts
            .iter()
            .enumerate()
            .all(|(at, draft)| draft.handled() || at == self.index)
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
        // 作答即**撤销跳过**（只清不回滚）：回头改过的答案不该被 `answers()` 里那条「跳过
        // 无条件优先」吞掉（spec §11）。
        draft.skipped = false;
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
        // 「从输入区回来时高亮挪一格、两端环绕」那一支随 §1 那条一起作废（2026-10-06）：
        // 能走到这里的只有选项区里的 `j`/`k`/`Ctrl-N`/`Ctrl-P`/`↑`/`↓` 与滚轮，而它们
        // 在输入区里都静默了 —— 回选项区只有 `Esc`，那条路不经过这里（`reset_zone`）。
        if next < 0 || next >= count as isize {
            self.zone = Zone::Input;
            return;
        }
        self.drafts[self.index].highlight = next as usize;
    }

    /// 添一个自由文本字符。
    ///
    /// 它**不动**选中的选项：单选与多选一个形状，文本与选择并存交回（spec §3）。想清掉文本
    /// 就自己按 `Backspace` 删。它**撤销跳过**（只清不回滚）—— 打字就是作答（spec §11）。
    fn type_custom(&mut self, ch: char) {
        let draft = &mut self.drafts[self.index];
        draft.custom.push(ch);
        draft.skipped = false;
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
    /// 显示主列的这一页，跟点它的页签一样（`.scratch/trace-in-main/spec.md` §2）。
    SwitchMainTab(MainTab),
    /// 跳到这个单位的开头，跟点它的回合条格子一样（spec §4）。
    TurnRailUnit(usize),
    /// 点状态行的模型那一格：打开模型清单（spec §7）。
    SwitchModel,
    /// 点状态行的档位那一格：打开思考强度清单（spec §7）。
    SwitchEffort,
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
    /// 记下一块可点的区域。
    fn push(&mut self, rect: Rect, action: HitAction) {
        self.cells.push(Region { rect, action });
    }

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

/// 菜单记账的键：那一次选择挂在**哪一个**记号上。
///
/// 三个字段缺一不可：前缀字符区分 `/` 与 `@`（两者的候选来源完全不同，同一个 query 在两者
/// 下是两个菜单）；**位置**区分同一行里两个名字相同的记号（`@a @a` —— `Esc` 关掉一个不该
/// 牵连另一个）；query 让「同一个记号里改字」重开菜单（`.scratch/input-tokens/spec.md`
/// §2 那条「`Esc` 按 token 记账」）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuKey {
    sigil: char,
    start: usize,
    query: String,
}

/// 记号菜单里需要被记住的那一半。
///
/// 菜单的其余部分全是推出来的：token 从草稿来，匹配从候选来源与那个 token 来。推不出来的是
/// 用户挑了哪一行，所以这里留的就是它 —— 以及它是在哪个记号上挑的。
#[derive(Debug, Default)]
struct MenuSelection {
    /// 高亮是在哪个记号上做出来的。`None` 表示此刻草稿里没有记号。
    token: Option<MenuKey>,
    /// 高亮落在哪个匹配上，有的话。
    ///
    /// **裸 `/` 与裸 `@` 时是 `None`**，这就是那条安全规矩：在用户打出名字、或者用方向键
    /// 走过列表之前，什么都没被选中，于是只为看一眼菜单而按下的键不可能跑起一条没人要的
    /// 命令（spec §7 对一个顺手按键的读法）。
    selected: Option<usize>,
    /// `Esc` 关掉了菜单。只要还在打的就是它当初被关掉时的**那一个记号**，它就保持关着。
    dismissed: bool,
}

/// 菜单里的一行：名字、描述，以及它**来自哪一类**。
///
/// 类别只决定名字的颜色（`.scratch/tui-feedback/spec.md` §11）：`/` 的三类来源共用一个菜单，
/// 而人得一眼看出哪几个是程序自带的命令、哪几个是装进来的技能。
#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuEntry {
    name: String,
    description: String,
    kind: MenuKind,
}

/// 一行的来处。`@` 的候选不是任何一类名字，它只是路径 —— 于是归正文档色，与从前一样。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    Command,
    Skill,
    Template,
    Path,
}

impl From<crate::render::CatalogKind> for MenuKind {
    fn from(kind: crate::render::CatalogKind) -> Self {
        use crate::render::CatalogKind;
        match kind {
            CatalogKind::Command => Self::Command,
            CatalogKind::Skill => Self::Skill,
            CatalogKind::Template => Self::Template,
        }
    }
}

impl MenuEntry {
    /// 这一行名字的颜色。三类来源各一色，路径归正文档。
    fn colour(&self) -> Color {
        match self.kind {
            MenuKind::Command => palette::TOKEN_COMMAND,
            MenuKind::Skill => palette::MENU_SKILL,
            MenuKind::Template => palette::MENU_TEMPLATE,
            MenuKind::Path => palette::PLAIN,
        }
    }
}

/// 此刻的记号菜单：它属于哪个前缀、什么与它匹配、哪个匹配被高亮。
///
/// 一个值，由草稿加候选来源在每帧、每次按键时建出来。菜单永远不会是可以与屏幕上所见漂移
/// 开的状态。
#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenMenu {
    /// 开这个菜单的那个前缀字符。
    sigil: char,
    /// 前缀后面已经打进去的内容。
    query: String,
    /// 匹配上的那些条目，按来源的顺序。
    entries: Vec<MenuEntry>,
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

/// 一块转录可以去的两个视图（`.scratch/trace-tab/spec.md` §1）。
///
/// 两个视口共用同一份共享源（[`Painted`]），但各持折行缓存、宽度、视口与「新行」计数 ——
/// 凡是「哪个窗格」「哪张平行表」「哪个指示器」的取数都按它分派。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Viewport {
    /// 主列页签条的第一页：对话视图。
    Conversation,
    /// 主列页签条的第二页：轨迹视图。
    Trace,
}

/// 这一次要把块喂给哪些视图。
///
/// 两个视图都常驻，所以这一趟**两个目标都在**的时候是常态；之所以还留着这张表，是因为宽度
/// 变化时的重放只喂**真的清了**的那个 pane —— 对没清空过的 pane 重放会把它整份推第二遍
/// （票 09、`.scratch/trace-in-main/spec.md` §3）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Targets {
    conversation: bool,
    trace: bool,
}

/// 上一帧一个视图把每个显示行画在了哪里，好把一次点击换回它落在的那条来源行。
///
/// 每帧重建，**每个视图一套**：两个视口的窗口不必相同，共用一份映射会让点击指错行。
#[derive(Default)]
struct Drawn {
    /// 那个显示行对应的源行下标；不是任何入口的那些行是 `None`。
    rows: Vec<Option<usize>>,
    /// 这个视图第一条被画出来的行的屏幕 y。
    top: u16,
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
    /// 它是不是一条**分段用的空行**（§23 插进来的那种）。单位没有用户消息时，头退到它里面
    /// 第一条**非空**行 —— 点一格该落到正文上，不是落到它上面那条空隙里。
    blank: bool,
}

impl TurnRail {
    /// 会话已经完成了多少个单位。
    fn units(&self) -> usize {
        self.heads.len()
    }

    /// 记下一条被画出来的来源行。
    fn push_line(&mut self, user_message: bool, blank: bool) {
        // 一行属于的那个单位就是正在建的那个：`units()` 是已经完成的个数，所以那就是这一行
        // 在它回合结束时将拿到的下标。
        self.lines.push_back(TurnRailLine {
            unit: self.heads.len(),
            user: user_message,
            blank,
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
            .or_else(|| (start..self.lines.len()).find(|index| !self.lines[*index].blank))
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

/// 轨迹页里长在一条行尾的那一段：一笔用量，或一次发言的合计。
///
/// 它不是块，所以既不带来自己的时刻戳，也不开详情入口 —— `▸` 的判据是「谁把内容折起来谁
/// 就有」，而它什么都没折（ADR 0016、`.scratch/trace-usage-tail/spec.md` §1、§5）。
#[derive(Debug, Clone, Copy)]
enum Tail {
    /// 产生它的那次调用的读数。
    Usage(Usage),
    /// 一次发言里各次调用之和 —— 只在跨 ≥2 次调用时出现。
    Total(Usage),
}

impl Tail {
    /// 它在屏幕上占的那几个字。
    fn span(self) -> Span<'static> {
        let text = match self {
            Tail::Usage(usage) => wording::usage_tail(&usage),
            Tail::Total(usage) => wording::total_tail(&usage),
        };
        Span::styled(format!(" {text}"), Style::default().fg(palette::MUTED))
    }
}

/// 一条已经画进窗格、且能在宽度变化时重放的记录。
///
/// 大多数行来自一个 [`Block`]；思考行是唯一的例外 —— 它是渲染器自己的状态（票 02 §1），
/// 由 `pane.push` / `pane.replace_last` 原地管。宽度一变两者都得回来，所以清单里两种都记
/// （`.scratch/markdown-render/spec.md` §1）。
///
/// 每一条还带一段可选的**尾巴**：一笔用量长在它所归属的那次调用的行尾，合计长在发言收尾那
/// 一行上。它记在这里而不是当场写屏，是为了让宽度变化与 `--continue` 的重放跟实时同源
/// （ADR 0016、`.scratch/trace-usage-tail/spec.md` §2）。
enum Painted {
    /// 一个定稿的块，以及产生它的那一刻（`.scratch/trace-in-main/spec.md` §5）。轨迹页把这个
    /// 时刻画在行的开头，而重放要把它一起带回来。
    ///
    /// `id` 是它的身份（ADR 0021）：行选择、折叠、命中导航与详情跳转的共同落点，而它们都要活过
    /// 上限裁剪与宽度重放 —— 位置下标两次都活不过。
    Block {
        block: Block,
        at: DateTime<Utc>,
        id: BlockId,
        tail: Option<Tail>,
    },
    /// 还开着的思考行，以及**思考开始那一刻** —— 第一条推理增量到达的时候
    /// （`.scratch/trace-thought-stamp/spec.md` §1，推翻 `trace-in-main` 用户故事 21 的后半句）。
    /// 带 `at` 是为了宽度变化重放出来的那一行仍有同一个戳。
    Thinking {
        speaker: crate::events::SpeakerId,
        at: DateTime<Utc>,
        id: BlockId,
        tail: Option<Tail>,
    },
    /// 定稿的思考行。
    Thought(SettledThinking),
    /// 一个**无主段落**的小标题：开场与压缩这两种段落不属于任何一级组，所以在账本上要有
    /// 一个自己的标题（`.scratch/trace-ledger/spec.md` §5）。
    SectionHeader(SectionHeader),
    /// 轨迹页两级分组的一个**组头**：它不是块，所以不来自转录，而是由组的边界推出来的
    /// （`.scratch/trace-ledger/spec.md` §5）。
    ///
    /// 它住在重放清单里是因为**它自己会变**：直方图每来一条长一格、跨度跟着长，而宽度变化
    /// 要把整本账按新宽度重画一遍 —— 重画时用的是**定稿后**那一份，不是「此刻重算」。
    GroupHeader(GroupHeader),
}

/// 组头那一级的两个档：一级是**单位**（回合 / 轮次），二级是**迭代**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderLevel {
    Unit,
    Iteration,
}

/// 无主段落小标题的两种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionKind {
    /// 一个单位还没开出来之前的那一段：开场的人机两方都还没说话。
    Preamble,
    /// 上下文压缩把前面若干轮压成了摘要。
    Compaction,
}

/// 一个无主段落小标题的那份数据。
#[derive(Debug, Clone)]
struct SectionHeader {
    /// 它在账本上那一个落点（小标题也占一条来源行）。
    id: BlockId,
    kind: SectionKind,
    at: DateTime<Utc>,
    /// 开场段里**数到几条注入**了 —— 每来一条就地改写那一行，读者不用等这段读完
    /// 才知道开场有多大。
    injections: u32,
}

/// 一个组头的那份数据 —— 它本身就是那一行的全部内容。
#[derive(Debug, Clone)]
struct GroupHeader {
    /// 它在账本上那一个落点（组头也占一条来源行）。
    id: BlockId,
    level: HeaderLevel,
    /// 第几个：回合 / 轮次的序号，或迭代的序号。
    ordinal: u32,
    /// 这一组的**起点**那一刻 —— 行首那九列画的也是它，于是所有行的时刻列竖向对齐不变。
    at: DateTime<Utc>,
    /// 收尾那一刻的跨度。**还没收尾就是 `None`**，而「此刻的跨度」是画的时候用墙钟现算的
    /// —— 所以一个正在跑的组在实时与重放里读出来一致。
    span: Option<std::time::Duration>,
    /// 工具直方图，同类归并、**按首次出现排序**。
    tools: Vec<(String, u64)>,
}

/// 一段**定稿**的思考：谁说的、记下来没有、到此为止那一刻，以及它的身份与行尾。
///
/// 收成一个结构体是因为这五样是一件事：定稿时构造一次，重画时读一次
/// （`.scratch/trace-ledger/spec.md` §1）。`trace` 是记下来的整段推理，`None` 是合成器那种
/// 「没记下来」。
#[derive(Debug, Clone)]
struct SettledThinking {
    speaker: crate::events::SpeakerId,
    trace: Option<String>,
    at: DateTime<Utc>,
    /// **与它开着的时候同一个** —— 定稿是同一行的两个阶段，不是一条新记录（ADR 0021）。
    /// 所以选中那一行在定稿那一刻不跳。
    id: BlockId,
    tail: Option<Tail>,
}

impl Painted {
    /// 这一条的身份。
    fn id(&self) -> BlockId {
        match self {
            Painted::Block { id, .. }
            | Painted::Thinking { id, .. }
            | Painted::Thought(SettledThinking { id, .. })
            | Painted::GroupHeader(GroupHeader { id, .. })
            | Painted::SectionHeader(SectionHeader { id, .. }) => *id,
        }
    }

    /// 这条记录尾上挂着的那一段，有的话。
    fn tail(&self) -> Option<Tail> {
        match self {
            Painted::Block { tail, .. }
            | Painted::Thinking { tail, .. }
            | Painted::Thought(SettledThinking { tail, .. }) => *tail,
            // 组头与小标题没有尾巴：它们不是一条调用，不挂用量。
            Painted::GroupHeader(_) | Painted::SectionHeader(_) => None,
        }
    }

    /// 把尾巴挂上去 —— 该次调用收尾时那一下。
    fn set_tail(&mut self, tail: Tail) {
        match self {
            Painted::Block { tail: slot, .. }
            | Painted::Thinking { tail: slot, .. }
            | Painted::Thought(SettledThinking { tail: slot, .. }) => *slot = Some(tail),
            // 组头与小标题没有尾巴可挂。
            Painted::GroupHeader(_) | Painted::SectionHeader(_) => {}
        }
    }
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
            conversation: Pane::new(),
            trace: Pane::new(),
            live: String::new(),
            painted: Vec::new(),
            in_call: false,
            turn_calls: 0,
            turn_usage: Usage::default(),
            // 第一帧之前没有真正的宽度；先用共享渲染那个缺省把行排出来，首帧一画出来就会
            // 发现宽度不同并按真宽度重放（spec §1）。轨迹视图那一份同理。
            conversation_width: SHARED_RENDER_WIDTH,
            trace_width: SHARED_RENDER_WIDTH,
            conversation_flow: Flow::default(),
            trace_flow: Flow::default(),
            trace_tier_width: 0,
            dirty: true,
            editor: Input::new(),
            files: FileIndex::new(),
            // 进 TUI 就预热一次：人真打 `@` 的时候它通常已经好了
            // （`.scratch/input-tokens/spec.md` §1）。
            file_scan_wanted: true,
            catalog: Vec::new(),
            slash: MenuSelection::default(),
            panel: Panel::new(),
            todo: crate::render::TodoPanel::default(),
            tab: Tab::Usage,
            files_page: FilesPage::default(),
            // 进 TUI 就取一次数：打开就有东西看（§4）。
            changes: ChangesPage::default(),
            changes_wanted: true,
            tabs_rect: None,
            sidebar_keyboard: false,
            main_tab: MainTab::Conversation,
            sidebar_wanted: true,
            colors,
            effort: None,
            picker: None,
            notice: None,
            reasoning: String::new(),
            thinking_speaker: crate::events::SpeakerId::System,
            thinking_open: false,
            thinking_done: false,
            turn_rail: TurnRail::default(),
            trace_links: std::collections::VecDeque::new(),
            trace_block_ids: std::collections::VecDeque::new(),
            trace_groups: TraceGroups::default(),
            next_local_id: 0,
            trace_drawn: Drawn::default(),
            trace_rect: None,
            detail: None,
            file_viewer: None,
            file_viewer_rect: None,
            viewer_wake: None,
            detail_opener: None,
            detail_rect: None,
            diff_reading: None,
            detail_serial: 0,
            modal_rect: None,
            questionnaire_bottom: None,
            regions: Regions::default(),
            screen_text: selection::ScreenText::default(),
            drag: None,
            copied: None,
            clipboard: None,
            opened: None,
            open_request: None,
            conversation_rect: None,
            area: Rect::default(),
            prompt_reply: None,
            // 空闲，直到循环另说：在它要第一行之前没有任何东西在跑，而键盘必须读起来就是
            // 这个样子（spec §6）。
            running: false,
            pulse: 0,
            live_speaker: None,
            running_tool: None,
            pending: None,
            events: Vec::new(),
            indicator: None,
            trace_indicator: None,
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
    /// **空闲也走**：挂在这条时钟上的东西空闲时也活着 —— 光标闪烁，以及那几条与运行无关的
    /// 回执寿命（复制、打开、通知行）。票 09 曾让空闲停掉时钟，`.scratch/tui-visual-language/spec.md`
    /// §32 推翻了它；2026-10-08 状态字形在就绪时也定住了（`.scratch/ui-trim/spec.md`），但这条
    /// 时钟的其余住户还在，所以「空闲不再零唤醒」这笔代价**没有**跟着收回。
    ///
    /// 它推进的是脉冲的帧、别的什么都不是：**不是**一个通用的「重画点什么」的钩子，任何需要
    /// 一帧的东西都应该通过那个改变了它的事件说出来。
    pub fn tick(&mut self) {
        self.pulse = self.pulse.wrapping_add(1);
        self.dirty = true;
        // 复制回执到点就作废：判据与提示行读它时是**同一条**（`.scratch/tui-feedback/spec.md`
        // §6）—— 脉冲一直在走，所以这里总会走到。打开的回执（成功或失败）共用这一条寿命
        // （`.scratch/clickable-links/spec.md` §4）。
        let now = std::time::Instant::now();
        if copy_receipt(self.copied, now).is_none() {
            self.copied = None;
        }
        if open_receipt(self.opened.as_ref(), now).is_none() {
            self.opened = None;
        }
        if notice_line(self.notice.as_ref(), now).is_none() {
            self.notice = None;
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
            // 后的那一帧打开菜单。它也按同一条推导成为 chip —— 给粘贴单开豁免等于让同一段
            // 文本有两种身份（spec §4）。
            self.sync_tokens();
            self.sync_menu();
        }
    }

    /// 喂进一个渲染事件，并回答它画了多少条转录来源行。
    ///
    /// 这个计数就是历史重放批次预算的量度：一帧的成本由它铺下去的文字定界，而不是只按它
    /// 吃掉多少事件来算（`.scratch/tui-history-replay/spec.md` §2）。实时调用方不看它。
    pub fn apply(&mut self, event: RenderEvent) -> usize {
        // 「工作区变了」是一条静默信号：它不画任何东西，只请一次重扫 —— 索引落地时那一帧
        // 自己会脏（`.scratch/files-page/spec.md` §2）。
        //
        // 它同时请改动页重取一次（`.scratch/diff-page/spec.md` §4）：判据是那条既有的
        // `Effect`，所以执行者与讨论者的调用**也走得通**（它们落在同一条父流、同一个渲染器
        // 上）。两位分开置，因为两件事的节奏与失败模式不同。
        if matches!(event, RenderEvent::WorkspaceChanged) {
            self.file_scan_wanted = true;
            self.changes_wanted = true;
            return 0;
        }
        self.dirty = true;
        // 这一刻属于这条事件：轨迹页把它的时刻画在块的开头，重放时从同一处取
        // （`.scratch/trace-in-main/spec.md` §5）。
        let at = event_at(&event);
        // 身份的一半：流上的事件带信封，所以它的行号就是这一批块的来源（ADR 0021）。
        // 渲染层自己造的那几条（增量 / 诊断 / 通知 / 身份注入）没有信封，于是是 `None`。
        let seq = match &event {
            RenderEvent::Logged(event) => Some(event.seq),
            _ => None,
        };
        self.observe_goal(&event);
        self.observe_running_tool(&event);
        // 这一整趟的收件人（两个视图都常驻，所以通常两个都在）。
        let targets = self.targets();
        let mut produced = 0usize;
        for (offset, block) in self.transcript.push(event).into_iter().enumerate() {
            let id = self.next_block_id(seq, offset as u32);
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
                        // `at` 就是收到这条增量的时候：推理增量没有信封，所以「思考开始那一刻」
                        // 只能是它（`.scratch/trace-thought-stamp/spec.md` §1）。
                        produced += usize::from(self.open_thinking(speaker.clone(), at, id));
                        self.reasoning.push_str(text);
                    }
                    DeltaKind::Text => self.freeze_thinking(at),
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
                                produced +=
                                    usize::from(self.open_thinking(speaker.clone(), at, id));
                            }
                            self.settle_thinking(Some(text), at);
                        }
                        None => {
                            if self.thinking_open {
                                self.settle_thinking(None, at);
                            }
                        }
                    }
                    self.thinking_done = true;
                }
            }
            // 新回合的思考是新的一段，所以「已经画过」这个闩随回合一起清掉（票 02 §1）。
            if matches!(&block, Block::TurnStarted { .. }) {
                self.thinking_done = false;
                // 每一次迭代 = 一次模型调用，也就是一笔用量的归属单位（§5）。
                self.turn_calls += 1;
                self.in_call = true;
            }
            match &block {
                // 推理不是消息正文的一部分：它被折进自己那一行，所以永远不加入正文流过的那条
                // 活尾巴（票 02 §3）。
                Block::Delta {
                    kind: DeltaKind::Reasoning,
                    ..
                } => {}
                Block::Delta { speaker, text, .. } => {
                    self.live_speaker = Some(speaker.clone());
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
                    self.live_speaker = None;
                }
                _ => {}
            }
            // 面板数的是这个块就会话说了什么；窗格显示的是它向读者说了什么。名字在进来的路上
            // 就上了色，所以发言者的第一行就是给注入名册没列到的名字定色的那一下（票 07）。
            self.panel.observe(&block);
            // `todo` 页也是同一种推法，来源是唯一带列表的那一种块：一次调用自己的参数。
            self.todo.observe(&block);
            produced += self.trace_block(block, at, targets, id);
        }
        produced
    }

    /// 这一刻该把块喂给哪些视图。
    ///
    /// 实时事件到达时**不问「现在显示哪一页」**：两个视图都常驻，切页与 `Ctrl-O` 都不会漏
    /// 内容，发言者配色槽位也不随可见性变（票 09、`.scratch/trace-in-main/spec.md` §3）。
    fn targets(&self) -> Targets {
        Targets {
            conversation: true,
            // 轨迹视图与对话视图一样常驻：它住在主列里，而主列永远在
            // （`.scratch/trace-in-main/spec.md` §3）。
            trace: true,
        }
    }

    /// 画一个块，并处理它那两种尾巴：一笔用量不再是自己的一条，而是攒着等那次调用收尾；
    /// `TurnEnded` 在跨 ≥2 次调用时带上合计
    /// （ADR 0016、`.scratch/trace-usage-tail/spec.md` §1、§5）。
    fn trace_block(
        &mut self,
        block: Block,
        at: DateTime<Utc>,
        targets: Targets,
        id: BlockId,
    ) -> usize {
        // 新到达的块：它要开组、也要让当前那组长一格。
        self.trace_block_inner(block, at, targets, id, false)
    }

    fn trace_block_inner(
        &mut self,
        block: Block,
        at: DateTime<Utc>,
        targets: Targets,
        id: BlockId,
        replaying: bool,
    ) -> usize {
        // 用量属于产生它的那次调用：贴到**它到达这一刻**那次调用最后画出的那条过程行上。
        //
        // 不推迟到那次调用收尾再贴 —— 工具行排在用量之后，消息行也排在它之后，等下去只会
        // 等到一条内容行，而最常见的纯对话调用就再也贴不上（ADR 0016 的「被否决的替代
        // 方案」）。
        if let Block::Usage { usage, .. } = &block {
            if self.in_call {
                self.turn_usage.accumulate(*usage);
                if self.usage_host_exists() {
                    let tail = Tail::Usage(*usage);
                    if let Some(slot) = self.painted.last_mut() {
                        slot.set_tail(tail);
                    }
                    // 那一行已经画出去了，所以这一段是补在它尾巴上，不是另起一行 —— 它的
                    // 时刻戳、名字配色与详情入口都留在原地。
                    self.trace.append_to_last(tail.span());
                    return 0;
                }
            }
            return self.push_block(block, at, targets, None, id, replaying);
        }
        let tail = if matches!(&block, Block::TurnEnded { .. }) {
            self.in_call = false;
            let total = (self.turn_calls >= 2).then_some(Tail::Total(self.turn_usage));
            self.turn_calls = 0;
            self.turn_usage = Usage::default();
            total
        } else {
            None
        };
        self.push_block(block, at, targets, tail, id, replaying)
    }

    /// 窗格最后那条来源行能不能承载一笔用量。
    ///
    /// 用量到达这一刻，那次调用最后画出来的那条正是它 —— 有推理时是思考行，没推理时是回合
    /// 开始那行。消息行与一条独立的用量行都不承载：前者在轨迹页是「首行 + `…`」的内容入口，
    /// 把数字挂上去读不出归属（§1、§4）。
    fn usage_host_exists(&self) -> bool {
        match self.painted.last() {
            Some(Painted::Thinking { .. } | Painted::Thought { .. }) => true,
            Some(Painted::Block { block, .. }) => {
                !matches!(block, Block::Message { .. } | Block::Usage { .. })
            }
            // 组头与小标题是一次调用的**摘要**或一个段落标题，而用量属于产生它的那次调用
            // —— 挂到它们行尾会把数字摆在一个读不出归属的位置上（ADR 0016）。
            Some(Painted::GroupHeader(_)) | Some(Painted::SectionHeader(_)) => false,
            None => false,
        }
    }

    /// 把一个块排成行、推进窗格，并**记住它**（连产生它的时刻与尾巴），好在宽度变化时重放
    /// （spec §1；尾巴见 ADR 0016）。
    fn push_block(
        &mut self,
        block: Block,
        at: DateTime<Utc>,
        targets: Targets,
        tail: Option<Tail>,
        id: BlockId,
        replaying: bool,
    ) -> usize {
        let produced = self.emit_block(&block, at, targets, tail, id, replaying);
        if produced > 0 {
            // 不产生行的那些块（流式增量）不留：重放它们什么都不画，白占一份内存。判据是
            // **任一**目标产出了行 —— 只在一个视图里出行的块，不记就再也回不来了。
            self.painted.push(Painted::Block {
                block,
                at,
                id,
                tail,
            });
        }
        produced
    }

    /// 只把块排成行推进窗格，不记它 —— 到达时与重放时走的是同一条路。
    ///
    /// 对每个被选中的目标各排版一次：分派点必须在这里，因为只有这里同时知道「画给谁」与
    /// 「按多宽画」（票 09）。发言者配色的分配是幂等的，所以同一块画两遍不会分叉。
    fn emit_block(
        &mut self,
        block: &Block,
        at: DateTime<Utc>,
        targets: Targets,
        tail: Option<Tail>,
        id: BlockId,
        replaying: bool,
    ) -> usize {
        let mut produced = 0;
        // 对话视图只收保留清单；左栏不在时**也不退回全量**（2026-10-05 维护者推翻 §6）——
        // 收起左栏就是「过程行暂时看不到」，规则只有一个。
        if targets.conversation && selects(Viewport::Conversation, block) {
            let width = self.conversation_width;
            let style = prefix_style(Viewport::Conversation, width);
            let speaker = block_speaker(block);
            let is_message = matches!(block, Block::Message { .. });
            // 同一个人连着说的几段正文只在**第一段**画名字 —— 后面几段由它们前面那个空行
            // 分段，名字重复出现在屏幕上只是噪音（2026-10-06 维护者，§23）。
            let carry_name = !is_message
                || speaker.is_none()
                || self.conversation_flow.speaker.as_ref() != speaker
                || !self.conversation_flow.message;
            let mut lines = paint_block(
                block,
                &mut self.colors,
                width,
                style,
                Viewport::Conversation,
                carry_name,
            );
            // 例外一：用户自己的话在**对话视图**里排成一个气泡（`bubble`）。它只在**这里**
            // 发生：轨迹视图仍左对齐、`plain` 也仍顶格，因为两边都不走这个窗格。
            if is_user_message(block) {
                bubble(&mut lines, width, carry_name);
            }
            produced = produced.max(lines.len());
            self.push_view_lines(
                Viewport::Conversation,
                speaker,
                is_message,
                lines,
                Some(is_user_message(block)),
                id,
            );
        }
        if targets.trace && selects(Viewport::Trace, block) {
            // 组头**长在组的最前面** —— 它先于成员画出去，读的人先看到这一段的界。
            // 重画时跳过：它已经在清单里，那一条会被 `emit_painted` 画出来。
            if !replaying {
                self.note_section_header(block, at, targets, false);
                self.open_group_headers(block, at, targets, self.discussion());
            }
            // 行首那几列归时间戳，所以轨迹内容的排版宽度是主列内容宽减掉它们
            // （`.scratch/trace-in-main/spec.md` §5）。
            let width = self.trace_width.saturating_sub(layout::STAMP_COLUMNS);
            let style = prefix_style(Viewport::Trace, self.trace_tier_width);
            // 轨迹页的名字在**每一行**上（它的行是紧凑的单行），所以这里不参与去重。
            let mut lines =
                paint_block(block, &mut self.colors, width, style, Viewport::Trace, true);
            // 行尾那几段账目：**这次调用自己花了多久**（只有工具块有这一笔），然后是产生它的
            // 那次调用的用量、或一次发言的合计。放不下时先丢耗时、保用量 —— 行尾是用量的家，
            // 而耗时是后来者（ADR 0016、票 17 第 1、2 条）。
            let duration = tool_duration(block);
            attach_row_tail(lines.last_mut(), duration, tail, width as usize);
            produced = produced.max(lines.len());
            self.push_view_lines(
                Viewport::Trace,
                block_speaker(block),
                matches!(block, Block::Message { .. }),
                stamp_lines(lines, at),
                Some(is_user_message(block)),
                id,
            );
        }
        // 这一块让当前开着的那一组发生了什么：工具算进直方图，收尾给跨度盖棺。两者都只
        // **改写组头那一行**（`.scratch/trace-ledger/spec.md` §5）。
        if targets.trace && !replaying {
            self.note_group_progress(block, at, targets);
        }
        // 回合的结束关掉一个单位；讨论里一轮的结束也是 —— 那里单位是**轮**，因为那才是
        // 讨论计数的东西（`CONTEXT.md` 把轮次与回合分开，spec §4）。回合条只与对话 pane
        // 平行，所以这条记账只看对话目标在不在，**不看那个块有没有真的画出来**：边界块在
        // 对话视图里可能一行都不留，而一格照旧要长出来。
        if targets.conversation && is_boundary(block, self.discussion()) {
            self.turn_rail.close_unit();
        }
        produced
    }

    /// 某个视图的内容宽度变了：清空它，按新宽度把绘制记录整批重放一遍（spec §1）。
    ///
    /// 宽度一变就不能只重新折行：表格的列宽与超宽代码行的折行是**渲染时**定下的，
    /// 那些源行本身已经依赖宽度了。
    ///
    /// **只清、只重放宽度真变了的那个视口**（票 09）：`emit_painted` 是追加，对没清空过的
    /// pane 再放一遍会把它整份推第二遍。两个视图都常驻，所以这里不再有「宽度为零 = 不物化」
    /// 那一支（`.scratch/trace-in-main/spec.md` §3）。
    fn rerender_if_width_changed(&mut self, conversation_width: u16, trace_width: u16) {
        let conversation = conversation_width != self.conversation_width;
        let trace = trace_width != self.trace_width;
        if !conversation && !trace {
            return;
        }
        self.conversation_width = conversation_width;
        self.trace_width = trace_width;
        if conversation {
            self.conversation.clear();
            // 跨块排版的记账与窗格平行，所以它也跟着一起清。
            self.conversation_flow = Flow::default();
            // 回合条的源行下标与对话 pane 平行，所以它跟着对话 pane 一起重建。
            self.turn_rail.clear();
        }
        if trace {
            self.trace.clear();
            self.trace_links.clear();
            self.trace_block_ids.clear();
            // 组的边界也是由块序列推出来的，所以重放要按同样的顺序再推一遍。
            self.trace_groups = TraceGroups::default();
            self.trace_flow = Flow::default();
        }
        if self.painted.is_empty() {
            self.dirty = true;
            return;
        }
        let painted = std::mem::take(&mut self.painted);
        let replay = Targets {
            conversation,
            trace,
        };
        for item in &painted {
            // `true` = 这是**重画**：组头已经在那份清单里，重画它而不是再开一个
            // （`.scratch/trace-ledger/spec.md` §5）。新到达的块才走开组那半边。
            self.emit_painted(item, replay, true);
        }
        self.painted = painted;
        self.dirty = true;
    }

    /// 重放一条绘制记录。`targets` 说这一趟要把记录喂给谁 —— 宽度没变的那个视口不在里面，
    /// 它原样留着自己那份内容。
    fn emit_painted(&mut self, painted: &Painted, targets: Targets, replaying: bool) {
        match painted {
            Painted::Block {
                block,
                at,
                id,
                tail,
            } => {
                self.emit_block(block, *at, targets, *tail, *id, replaying);
            }
            Painted::Thinking {
                speaker, at, id, ..
            } => {
                self.paint_thinking_line(speaker, *at, targets, *id);
            }
            Painted::Thought(settled) => {
                // 重放是**追加**：窗格刚被清空，定稿的那一条要重新画出来（实时路径才是
                // 就地重写那条「正在思考」）。
                self.paint_settled_thinking(settled, targets, false);
            }
            Painted::GroupHeader(header) => {
                // 重放同样重画整条组头 —— 用的是这份记录里的**定稿值**，于是它与实时路径
                // 画出来的是同一行（`.scratch/trace-ledger/spec.md` §5）。
                self.paint_group_header(header, targets, false);
            }
            Painted::SectionHeader(header) => {
                self.paint_section_header(header, targets, false);
            }
        }
    }

    /// 把一条来源行推进它那个视图的窗格：折行缓存、链接入口、回合条的纹理，最后按窗格报回来
    /// 的丢弃数裁掉两边溢出的部分。
    ///
    /// `user` 说这条行要不要记进回合条，`None` 是不记 —— 思考行是唯一的这种行：它属于当前
    /// 单位，但它不是一次新的发言，也不改变单位的划分。回合条只与对话 pane 平行，所以只有
    /// 那个视口会喂它。
    fn push_line(
        &mut self,
        view: Viewport,
        line: Line<'static>,
        link: Option<Detail>,
        user: Option<bool>,
        id: Option<BlockId>,
    ) {
        // 空行只有分段那一个用途，所以它按内容判：没有字就是空行。它在 `line` 被移进窗格
        // 之前取出来。
        let blank = line.spans.iter().all(|span| span.content.trim().is_empty());
        let dropped = match view {
            Viewport::Conversation => self.conversation.push(line),
            Viewport::Trace => {
                let dropped = self.trace.push(line);
                // 链接表跟着窗格交回来的丢弃数裁，不自己数 `CAP`：一条来源行在两边要么意思
                // 相同、要么两边都没有（票 04 §1、票 07）。
                self.trace_links.push_back(link);
                // 身份表与它同进同出 —— 「这张表与窗格永远同长」是同一个理由（ADR 0021）。
                self.trace_block_ids.push_back(id);
                for _ in 0..dropped {
                    self.trace_links.pop_front();
                    self.trace_block_ids.pop_front();
                }
                dropped
            }
        };
        if view == Viewport::Conversation {
            if let Some(user) = user {
                self.turn_rail.push_line(user, blank);
            }
            if dropped > 0 {
                self.turn_rail.prune(dropped);
            }
        }
    }

    /// 把一个视图要的那些行推进窗格，并在**换发言者**时先空一行
    /// （`.scratch/tui-visual-language/spec.md` §23）。
    ///
    /// 插入点是**块序列生成期**、不是绘制期：两个视图各记各的 [`Flow`]（重放时跟着窗格一起清），
    /// 于是各自的空行也各归各的，而且滚动时不会重排。
    ///
    /// 空行的两条规矩：**换发言者**要空（把两个人的话分开）；对话视图里**两条消息之间**也要空
    /// —— 一条消息是一段（2026-10-06 维护者：同一个人连说几段时，只留一个名字，段与段之间
    /// 空一行）。同一人连发的**工具行**不空：一组动作读起来是一组。
    fn push_view_lines(
        &mut self,
        view: Viewport,
        speaker: Option<&crate::events::SpeakerId>,
        is_message: bool,
        lines: Vec<RenderedLine>,
        user: Option<bool>,
        id: BlockId,
    ) {
        if lines.is_empty() {
            return;
        }
        let blank = {
            let flow = match view {
                Viewport::Conversation => &self.conversation_flow,
                Viewport::Trace => &self.trace_flow,
            };
            let changes_speaker = speaker
                .is_some_and(|speaker| flow.speaker.as_ref().is_some_and(|last| last != speaker));
            // 两者都成立时也只空一行。
            changes_speaker || (view == Viewport::Conversation && is_message && flow.message)
        };
        if blank {
            // 空行也走 `push_line`：回合条的平行表按来源行下标记账，跳过它会把格子指到
            // 隔壁去。它不是一个用户消息，所以那一格记 `false`。
            self.push_line(view, Line::default(), None, Some(false), Some(id));
        }
        {
            let flow = match view {
                Viewport::Conversation => &mut self.conversation_flow,
                Viewport::Trace => &mut self.trace_flow,
            };
            if let Some(speaker) = speaker {
                flow.speaker = Some(speaker.clone());
            }
            // 叙述、诊断这些块不带发言者，也要断开「同一人连续几段」这条线：下一段消息
            // 因此重新带上名字。
            flow.message = is_message;
        }
        for rendered in lines {
            self.push_line(view, rendered.line, rendered.link, user, Some(id));
        }
    }

    /// 记下「现在在跑哪个工具」—— 对话视图末尾那句「正在做什么」用的就是它。
    fn observe_running_tool(&mut self, event: &RenderEvent) {
        let RenderEvent::Logged(event) = event else {
            return;
        };
        match &event.payload {
            EventPayload::ToolCallStarted {
                tool_name, args, ..
            } => self.running_tool = Some((tool_name.clone(), args.clone())),
            // 一次调用结束、一个回合结束、一场会话收尾：都说明没有工具在跑了。
            EventPayload::ToolCallCompleted { .. }
            | EventPayload::TurnEnded { .. }
            | EventPayload::SessionEnded { .. } => self.running_tool = None,
            _ => {}
        }
    }

    /// 一段正在流的文字折行之前的样子：每个逻辑行一条。
    ///
    /// 折行归窗格（[`Pane::view`] 收的就是这些行）。
    fn live_rows(text: &str) -> Vec<Line<'static>> {
        text.split('\n')
            .map(|raw| Line::from(raw.to_owned()))
            .collect()
    }

    /// 对话视图末尾那条正在流的东西：正文尾巴，或者**等待提示** —— 两种情形下都**先有
    /// 一行名字**。
    ///
    /// 名字行在等待与流式两个阶段都留在原地，所以正文一开始流、以及 markdown 完成那一刻
    /// 排版换过来的瞬间，`[名字]` 都不会闪掉（2026-10-05 维护者报告的观感问题）。
    /// **上一段就是同一个人说的话时反过来**：不重复名字、而且先空一行 —— 与定稿之后
    /// [`emit_block`] 画出来的那一段逐字一致，否则名字会在完成那一刻消失
    /// （`.scratch/tui-visual-language/spec.md` §23）。
    ///
    /// 等待阶段（正文尾巴还空着）：名字下面那行是它**在做什么** —— 有工具在跑就说那个工具，
    /// 否则说它在想。流式阶段：名字下面是正在流的那几行正文。
    fn conversation_live(&mut self) -> Vec<Line<'static>> {
        if self.live.is_empty() && !self.running {
            return Vec::new();
        }
        // 谁在答：正在流的那条正文的发言者；还没有增量时退回名册的第一个（讨论里几轮之间会
        // 换人，而这一刻流上还没有归属）。
        let speaker = self.live_speaker.clone().unwrap_or_else(|| {
            self.facts
                .speaker_order
                .first()
                .map(|name| crate::events::SpeakerId::Debater(name.as_str().into()))
                .unwrap_or(crate::events::SpeakerId::System)
        });
        let continues = self.conversation_flow.message
            && self.conversation_flow.speaker.as_ref() == Some(&speaker);
        let mut rows = Vec::new();
        if continues {
            rows.push(Line::default());
        } else {
            let colour = self.colors.of(&speaker);
            rows.push(Line::from(Span::styled(
                wording::speaker_label(&speaker),
                Style::default().fg(colour),
            )));
        }
        if self.live.is_empty() {
            let text = match &self.running_tool {
                Some((tool, args)) => wording::working(tool, args),
                None => wording::waiting(self.pulse),
            };
            rows.push(Line::from(Span::styled(
                text,
                Style::default().fg(palette::MUTED),
            )));
        } else {
            rows.extend(Self::live_rows(&self.live));
        }
        rows
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
            self.apply(RenderEvent::notice(wording::history_divider()));
        }
        for event in std::mem::take(&mut self.live_buffer) {
            self.apply(event);
        }
        // 读的人追上来了：转录就是历史，视口在它的末尾，而一个刚开出来的会话本来就该在那里。
        // 两个视口都回到末尾：历史重放长出来的内容两边都有（票 09）。
        self.conversation.to_bottom();
        self.trace.to_bottom();
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
        self.sync_tokens();
        self.editor_key(key);
        self.sync_menu();
    }

    /// 施加那些编辑草稿的键，无论草稿活在哪儿 —— 常驻编辑器与重放共用它们，所以两者不会
    /// 漂开。
    fn editor_key(&mut self, key: Key) {
        match key {
            Key::Char(ch) => self.editor.insert_char(ch),
            // 换行有两个键：`Ctrl-J` 到处都到得了，`Shift+Enter` 只在支持键盘增强协议的终端上
            // 到得了（那些终端由 [`enable_terminal_modes`] 推了标志）。别的终端里 `Shift+Enter`
            // 到达时就是一个普通的 `Enter`，于是它提交 —— 提示行因此只写 `ctrl-j`
            // （`.scratch/tui-feedback/spec.md` §9、spec §6）。
            Key::Newline => self.editor.insert_char('\n'),
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
        // 这一下把草稿改了（或只是挪了光标），记号区间要跟着重算 —— 它既是颜色的出处，
        // 也是下一按键吸附与整块删的输入。
        self.sync_tokens();
    }

    /// 开出思考行，除非已经有一条开着。它画出了一行时返回 `true`。
    ///
    /// 这一行是普通的转录行 —— 它照算窗格的上限，也跟别的一切一起滚 —— 而且刻意**还**
    /// 不可点：完整 trace 只在 `MessageCompleted` 上才有（票 02 §1）。
    fn open_thinking(
        &mut self,
        speaker: crate::events::SpeakerId,
        at: DateTime<Utc>,
        id: BlockId,
    ) -> bool {
        if self.thinking_open {
            return false;
        }
        self.thinking_open = true;
        self.thinking_speaker = speaker.clone();
        self.reasoning.clear();
        // 名字打头，所以它拿发言者的颜色 —— 每一条带名字的行都遵循同一条规矩
        // （票 07 §2）。两个视口各画一遍：它们的前缀分档可能不同（票 09）。
        self.paint_thinking_line(&speaker, at, self.targets(), id);
        // 记进重放清单：宽度变化时它也要跟着回来，连它的时刻一起（spec §1、§3）。
        self.painted.push(Painted::Thinking {
            speaker,
            at,
            id,
            tail: None,
        });
        true
    }

    /// 把「正在思考」那一行推进轨迹视图。
    ///
    /// 思考是一条**过程行**，只归轨迹（冻结项 2、8）：对话视图里它除了打断阅读什么都不做。
    /// 左栏不在时它也不回对话视图（2026-10-05 维护者推翻 §6）。
    fn paint_thinking_line(
        &mut self,
        speaker: &crate::events::SpeakerId,
        at: DateTime<Utc>,
        targets: Targets,
        id: BlockId,
    ) {
        if targets.trace {
            let line = self.thinking_in_progress_line(speaker, Viewport::Trace);
            // 开着的行也有时刻：思考**开始**那一刻（`.scratch/trace-thought-stamp/spec.md`
            // §1）。它会在定稿时就地重写成完成那一刻（§2）。
            self.push_line(
                Viewport::Trace,
                stamped_line(line, at),
                None,
                None,
                Some(id),
            );
        }
    }

    /// 把定稿的思考行喂给每条选中的视口，并把它变成通向详情的入口。
    ///
    /// `in_place` 说它是**就地重写**还是**追加**：实时路径上那条「正在思考」已经在窗格里，
    /// 冻住它就是重写最后一行；重放路径上窗格刚被清空，定稿那条要重新画出来
    /// （票 02 §1、票 09）。只喂轨迹视图 —— 思考行是过程行（票 10）。
    fn paint_settled_thinking(
        &mut self,
        settled: &SettledThinking,
        targets: Targets,
        in_place: bool,
    ) {
        if targets.trace {
            let (line, detail) = self.thinking_settled_line(
                &settled.speaker,
                settled.trace.clone(),
                Viewport::Trace,
            );
            let mut line = line;
            // 尾巴跟着这条行走：定稿是整行重写，丢在这里就等于把那笔用量吞掉。
            if let Some(tail) = settled.tail {
                line.spans.push(tail.span());
            }
            let line = stamped_line(line, settled.at);
            if in_place {
                self.trace.replace_last(line);
                if let Some(link) = self.trace_links.back_mut() {
                    *link = Some(detail);
                }
            } else {
                self.push_line(Viewport::Trace, line, Some(detail), None, Some(settled.id));
            }
        }
    }

    /// 这一批块里要不要**开一个新的一级组 / 一个新迭代**，以及开的那一个要画成什么样。
    ///
    /// 组头**长在组的最前面**：它在成员被推出去之前画，所以读的人先看到这一段的界，再看到
    /// 里面有什么（`.scratch/trace-ledger/spec.md` §5）。
    fn open_group_headers(
        &mut self,
        block: &Block,
        at: DateTime<Utc>,
        targets: Targets,
        discussion: bool,
    ) -> Option<HeaderLevel> {
        let level = if self.trace_groups.opens_unit(block, discussion) {
            Some(HeaderLevel::Unit)
        } else if self.trace_groups.opens_iteration(block, discussion) {
            Some(HeaderLevel::Iteration)
        } else {
            None
        };
        let level = level?;
        // 上一个迭代到此为止 —— 它的耗时到这一刻（`.scratch/trace-ledger/spec.md` §5：
        // 一次迭代没有自己的行，二级头就是它的行）。
        if level == HeaderLevel::Iteration
            && let Some(previous) = self.trace_groups.iteration.as_ref().map(|open| open.header)
        {
            self.close_group(previous, at);
            self.redraw_group_header(previous, targets);
        }
        let ordinal = match level {
            HeaderLevel::Unit => unit_ordinal(&self.trace_groups),
            HeaderLevel::Iteration => match block {
                Block::TurnStarted { iteration, .. } => *iteration,
                _ => 1,
            },
        };
        let id = self.next_block_id(None, 0);
        let header = GroupHeader {
            id,
            level,
            ordinal,
            at,
            span: None,
            tools: Vec::new(),
        };
        // 画它，然后记住它画在哪一行 —— 直方图每来一条长一格，靠的是这个下标。
        self.paint_group_header(&header, targets, false);
        self.painted.push(Painted::GroupHeader(header));
        let open = OpenGroup { header: id };
        match level {
            HeaderLevel::Unit => {
                self.trace_groups.units += 1;
                self.trace_groups.unit = Some(open);
            }
            HeaderLevel::Iteration => self.trace_groups.iteration = Some(open),
        }
        Some(level)
    }

    /// 一个块让**当前开着的那一组**发生了什么：工具算进直方图，而收尾事件给跨度盖棺。
    ///
    /// 两者都只**改写组头那一行**（`Pane::replace_at`），不重放整本账 —— 于是读的人看到的是
    /// 「正在跑的这个回合在干什么」，而它在屏上仍然只占一行。
    fn note_group_progress(&mut self, block: &Block, at: DateTime<Utc>, targets: Targets) {
        // 工具直方图：一级组收，同类归并。
        if let Block::Tool(tool) = block {
            let name = tool.tool.clone();
            let touched = self.trace_groups.unit.as_ref().map(|unit| unit.header);
            if let Some(touched) = touched
                && self.bump_tool_count(touched, &name)
            {
                self.redraw_group_header(touched, targets);
            }
        }
        // 收尾：给开着的那几组各记下它到此为止的跨度，于是重放时画的是同一行。
        // 一级与二级**都**收 —— 迭代的耗时住在它自己的二级头上。
        if is_boundary(block, self.discussion()) {
            let closing: Vec<BlockId> = [
                self.trace_groups.unit.as_ref().map(|open| open.header),
                self.trace_groups.iteration.as_ref().map(|open| open.header),
            ]
            .into_iter()
            .flatten()
            .collect();
            for id in closing {
                self.close_group(id, at);
                self.redraw_group_header(id, targets);
            }
        }
    }

    /// 直方图上那个工具名 +1；返回「要不要重画那一行」（第一次见到这个名字要）。
    fn bump_tool_count(&mut self, id: BlockId, name: &str) -> bool {
        let Some(header) = self.group_header_mut(id) else {
            return false;
        };
        match header.tools.iter_mut().find(|(known, _)| known == name) {
            Some(entry) => {
                entry.1 += 1;
                false
            }
            None => {
                header.tools.push((name.to_owned(), 1));
                true
            }
        }
    }

    /// 那一组在 `at` 那一刻收尾 —— 跨度从此是定稿值。
    fn close_group(&mut self, id: BlockId, at: DateTime<Utc>) {
        if let Some(header) = self.group_header_mut(id) {
            header.span = at.signed_duration_since(header.at).to_std().ok();
        }
    }

    /// 重放清单里那个组头记录的可变引用。
    fn group_header_mut(&mut self, id: BlockId) -> Option<&mut GroupHeader> {
        self.painted.iter_mut().find_map(|painted| match painted {
            Painted::GroupHeader(header) if header.id == id => Some(header),
            _ => None,
        })
    }

    /// 把组头那一行按它现在的数据重画一遍（就地改写，不动其他行）。
    fn redraw_group_header(&mut self, id: BlockId, targets: Targets) {
        let Some(header) = self.painted.iter().find_map(|painted| match painted {
            Painted::GroupHeader(header) if header.id == id => Some(header.clone()),
            _ => None,
        }) else {
            return;
        };
        self.paint_group_header(&header, targets, true);
    }

    /// 把一个无主段落的小标题画进轨迹窗格 —— 与组头同一套形状（`in_place` 是就地改写）。
    fn paint_section_header(&mut self, header: &SectionHeader, targets: Targets, in_place: bool) {
        if !targets.trace {
            return;
        }
        let line = section_header_line(header);
        if in_place {
            if let Some(source) = self.first_source_of(header.id) {
                self.trace.replace_at(source, line);
            }
        } else {
            self.push_line(Viewport::Trace, line, None, None, Some(header.id));
        }
    }

    /// 一个块是不是**开场段**里的一块。
    ///
    /// 开场是第一个单位之前那一段：身份注入、技能注入、命令、诊断与回执。
    ///
    /// 判据是**列出那几类**，而不是「不在组边界事件里」—— 后者会把一条落在组外的工具行
    /// 也算成开场，而工具行永远属于某次调用（真实流里它总在一个 `TurnStarted` 之后）。
    fn is_preamble(block: &Block) -> bool {
        matches!(
            block,
            Block::ContextInjected { .. }
                | Block::CommandRun { .. }
                | Block::Notice(_)
                | Block::Diagnostic(_)
        )
    }

    /// 开场与压缩这两段**不属于任何一级组**，所以它们各有自己的小标题。
    ///
    /// 开场那条**随注入数增长而就地改写**（用票 12 那个窗格入口），所以读者不用等开场读完
    /// 才知道它有多大（`.scratch/trace-ledger/spec.md` §5）。
    ///
    /// 「`/clear` 之后的那一段」**判不做**：`SessionStarted` 在转录层不产块，于是新会话的第一条
    /// 可见块与本会话的第一条在块层面**同形** —— 拿不到判据就不画一条猜出来的界。
    fn note_section_header(
        &mut self,
        block: &Block,
        at: DateTime<Utc>,
        targets: Targets,
        replaying: bool,
    ) {
        if replaying {
            return;
        }
        if matches!(block, Block::History { .. }) {
            let id = self.next_block_id(None, 0);
            let header = SectionHeader {
                id,
                kind: SectionKind::Compaction,
                at,
                injections: 0,
            };
            self.paint_section_header(&header, targets, false);
            self.painted.push(Painted::SectionHeader(header));
            return;
        }
        // 开场段：一个单位还没开出来之前的那些块。压缩已经在上面处理掉了。
        if self.trace_groups.unit.is_some() || !Self::is_preamble(block) {
            return;
        }
        let injection = matches!(block, Block::ContextInjected { .. });
        match self.trace_groups.preamble {
            Some(id) => {
                if !injection {
                    return;
                }
                if self.bump_injections(id) {
                    self.redraw_section_header(id, targets);
                }
            }
            None => {
                let id = self.next_block_id(None, 0);
                let header = SectionHeader {
                    id,
                    kind: SectionKind::Preamble,
                    at,
                    injections: u32::from(injection),
                };
                self.paint_section_header(&header, targets, false);
                self.painted.push(Painted::SectionHeader(header));
                self.trace_groups.preamble = Some(id);
            }
        }
    }

    /// 开场那一行的注入计数 +1；返回「要不要重画」。
    fn bump_injections(&mut self, id: BlockId) -> bool {
        let Some(header) = self.painted.iter_mut().find_map(|painted| match painted {
            Painted::SectionHeader(header) if header.id == id => Some(header),
            _ => None,
        }) else {
            return false;
        };
        header.injections += 1;
        true
    }

    /// 把小标题那一行按它现在的数据重画一遍。
    fn redraw_section_header(&mut self, id: BlockId, targets: Targets) {
        let Some(header) = self.painted.iter().find_map(|painted| match painted {
            Painted::SectionHeader(header) if header.id == id => Some(header.clone()),
            _ => None,
        }) else {
            return;
        };
        self.paint_section_header(&header, targets, true);
    }

    /// 把一个组头画进轨迹窗格：`in_place` 说它是**就地改写**（直方图长了一格、跨度变了长）
    /// 还是**追加**（宽度重放时窗格刚被清空）。
    ///
    /// 两种形状共用同一个函数，所以实时与重放画出来的那一行逐字相同 —— 而宽度变化不能把一个
    /// 正在跑的回合的组头「按此刻重算」一遍（`.scratch/trace-ledger/spec.md` §5）。
    fn paint_group_header(&mut self, header: &GroupHeader, targets: Targets, in_place: bool) {
        if !targets.trace {
            return;
        }
        let line = group_header_line(header, self.trace_width, self.discussion());
        if in_place {
            if let Some(source) = self.first_source_of(header.id) {
                self.trace.replace_at(source, line);
            }
        } else {
            self.push_line(Viewport::Trace, line, None, None, Some(header.id));
        }
    }

    /// 这个视口里发言者前缀怎么写。
    fn prefix_style_of(&self, view: Viewport) -> PrefixStyle {
        let width = match view {
            Viewport::Conversation => self.conversation_width,
            Viewport::Trace => self.trace_tier_width,
        };
        prefix_style(view, width)
    }

    /// 思考开始那一行：名字加「正在思考」。重放时按同一份构造重建。
    fn thinking_in_progress_line(
        &mut self,
        speaker: &crate::events::SpeakerId,
        view: Viewport,
    ) -> Line<'static> {
        let style = self.prefix_style_of(view);
        let color = self.colors.of(speaker);
        Line::from(vec![
            Span::styled(style.prefix(speaker), Style::default().fg(color)),
            Span::styled(
                wording::thinking_in_progress(),
                Style::default().fg(palette::MUTED),
            ),
        ])
    }

    /// 思考落定那一行，以及它通向详情的入口。重放时按同一份构造重建。
    fn thinking_settled_line(
        &mut self,
        speaker: &crate::events::SpeakerId,
        trace: Option<String>,
        view: Viewport,
    ) -> (Line<'static>, Detail) {
        let style = self.prefix_style_of(view);
        let color = self.colors.of(speaker);
        // 名字后面那个 `▸` 说的是这行可以打开 —— 它跟在发言者后面，这样每一行仍然以
        // 「谁在说话」开头（票 03 §Answer，2026-09-23 修正）。
        let line = Line::from(vec![
            Span::styled(style.prefix(speaker), Style::default().fg(color)),
            Span::styled(
                format!("{} ", wording::FOLDABLE),
                Style::default().fg(palette::MUTED),
            ),
            Span::styled(
                wording::thinking_finished(),
                Style::default().fg(palette::MUTED),
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
    fn freeze_thinking(&mut self, at: DateTime<Utc>) {
        if !self.thinking_open {
            return;
        }
        let text = std::mem::take(&mut self.reasoning);
        let recorded = !text.is_empty();
        self.settle_thinking(recorded.then_some(text), at);
    }

    /// 把思考行定下来 —— 不管有没有记录下来的 trace —— 并把它变成通向它详情的入口。
    ///
    /// `Some` 是记录下来的 trace；`None` 是合成器的形状 —— 增量流过了，事件流里没有整段
    /// 文本 —— 它的详情会说出来。一个根本没有敞开思考行的回合什么都不加（票 02 §1）。
    ///
    /// `at` 是这段思考**到此为止**的那一刻：实时路径上它是正文第一个增量到达的时候，重放
    /// 路径上它是那条 `MessageCompleted` 的时刻（`.scratch/trace-in-main/spec.md` §5）。
    fn settle_thinking(&mut self, text: Option<String>, at: DateTime<Utc>) {
        if !self.thinking_open {
            return;
        }
        self.reasoning.clear();
        self.thinking_open = false;
        self.thinking_done = true;
        // 就地写：一段思考就是一行，从 `正在思考` 到 `思考完成`（票 02 §1）。
        let speaker = self.thinking_speaker.clone();
        // 尾巴若已经挂在还开着的那一条上，定稿是整行重写，得跟着搬过去。
        let tail = self.painted.last().and_then(Painted::tail);
        // **身份也搬**：定稿不是一条新记录，是同一行从「正在思考」到「思考完成」的两个阶段
        // （ADR 0021）—— 所以选中那一行在定稿那一刻不跳。
        let id = self
            .painted
            .last()
            .map(Painted::id)
            .unwrap_or_else(|| self.next_block_id(None, 0));
        let settled = SettledThinking {
            speaker,
            trace: text,
            at,
            id,
            tail,
        };
        self.paint_settled_thinking(&settled, self.targets(), true);
        // 重放清单里那一条也从「开着」换成「定稿」，连它的详情与时刻一起 —— 否则一次宽度变化
        // 会把这条行变回进行中，或者把它的 trace 与时刻丢掉（spec §1）。
        match self.painted.last_mut() {
            Some(slot @ Painted::Thinking { .. }) => *slot = Painted::Thought(settled),
            _ => self.painted.push(Painted::Thought(settled)),
        }
    }

    /// 处理一个鼠标事件。
    ///
    /// 有五样东西可以回应指针，它们之间的顺序就是谁占着它：开着的详情覆盖层，然后是问题，
    /// 然后是回合条，然后是左栏的页签，最后是转录 —— 它的滚轮、指示器与折叠行都回应它。
    /// 没有一样认领的就忽略：终端自己的选择是用户的，而这里什么都不抢焦点（spec §4、§7）。
    /// 滚轮一格：滚的是**当前显示的那一页**
    /// （`.scratch/trace-in-main/spec.md` §4）。
    fn wheel_current(&mut self, up: bool) {
        match self.main_tab {
            MainTab::Conversation => self.conversation.wheel(up),
            MainTab::Trace => self.trace.wheel(up),
        }
    }

    /// 翻页两键：翻的是**当前显示的那一页**（§4）。
    fn page_current(&mut self, up: bool) {
        match self.main_tab {
            MainTab::Conversation => self.conversation.page(up),
            MainTab::Trace => self.trace.page(up),
        }
    }

    /// 回底键：回的是**当前显示的那一页**的底（§4）。
    fn current_page_to_bottom(&mut self) {
        match self.main_tab {
            MainTab::Conversation => self.conversation.to_bottom(),
            MainTab::Trace => self.trace.to_bottom(),
        }
    }

    /// 指针事件的入口：按**手势**分派，再按谁占着指针分派。
    ///
    /// 左键是**三段**的（`.scratch/tui-feedback/spec.md` §5）：按下只记起点，拖动越过门槛就成为
    /// 一次拖选，抬起时要么复制（§6）、要么把这一次按下的**点击**交给原来那套分派。点击动作
    /// 从 `Down` 挪到 `Up` 正是为了让拖选拦得住它 —— 否则一次「按下就开始选」的拖动会先切页签。
    pub fn mouse(&mut self, mouse: MouseEvent) {
        // 重放靠忽略来占着指针：历史还在一个钉在末尾的视口下面铺，所以滚轮一格与一次点击
        // 都不许挪动它、也不许打开一行还没到达完的行（`spec` §5）。
        if self.replay.is_some() {
            return;
        }
        self.dirty = true;
        // 查看器立着时指针全归它（含滚轮与拖动）：框外关掉、框内转发。排在别的分派之前，
        // 与「覆盖层立着时滚轮按指针位置分派」同一条来路。
        if self.file_viewer.is_some() {
            self.viewer_mouse(mouse);
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.wheel_at(mouse.column, mouse.row, true),
            MouseEventKind::ScrollDown => self.wheel_at(mouse.column, mouse.row, false),
            MouseEventKind::Down(MouseButton::Left) => self.press_at(mouse.column, mouse.row),
            MouseEventKind::Drag(MouseButton::Left) => self.drag_to(mouse.column, mouse.row),
            MouseEventKind::Up(MouseButton::Left) => self.release_at(mouse.column, mouse.row),
            _ => {}
        }
    }

    /// 滚轮一格：详情覆盖层直接占着它；否则是问题那一块（问卷挪高亮，中间的模态什么都不做）；
    /// 否则是左栏的文件页（指针落在它的页区里时）；否则滚**当前显示的那一页**
    /// （票 04 §2、`tui-chrome` §5、`.scratch/trace-in-main/spec.md` §4、
    /// `.scratch/files-page/spec.md` §4）。
    fn wheel_at(&mut self, column: u16, row: u16, up: bool) {
        if self.detail_open() {
            self.detail_scroll(if up { -1 } else { 1 });
            return;
        }
        if self.pending.is_some() {
            let point = (column, row).into();
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
                if questionnaire {
                    self.question_click(QuestionClick::Wheel(up));
                }
                return;
            }
        }
        // 指针落在左栏页区里时这一格归那一页（`.scratch/files-page/spec.md` §4、
        // `.scratch/diff-page/spec.md` §7）：与覆盖层、问卷分派同一条「指针在哪就管哪」的
        // 规矩。别的页照旧滚主列当前那一页。
        if self
            .sidebar_page_rect()
            .is_some_and(|page| page.contains((column, row).into()))
        {
            // 改动页**没有滚动机制**（那一份 spec 明确不做）：滚轮落在它上面时这一格归它，
            // 但什么都不动 —— 不穿透去滚主列背后那一页。
            if self.tab == Tab::Files {
                self.files_wheel(up);
            }
            return;
        }
        self.wheel_current(up);
    }

    /// 文件页滚一格：一个可见行。
    fn files_wheel(&mut self, up: bool) {
        let window = self.files_page.rect.map_or(0, |page| page.height as usize);
        let max_top = self.files_page.rows.len().saturating_sub(window);
        self.files_page.scroll = if up {
            self.files_page.scroll.saturating_sub(1)
        } else {
            (self.files_page.scroll + 1).min(max_top)
        };
        self.dirty = true;
    }

    /// 左键按下：记下起点与它落进的那块**文本区域**，别的什么都不做 —— 那一次点击属于抬起。
    ///
    /// 落进哪一块由上一帧的 [`selection::ScreenText`] 说了算：指针只能选中真画出来的东西
    /// （与命中区域同一条纪律）。
    fn press_at(&mut self, column: u16, row: u16) {
        let block = self.screen_text.block_at(column, row);
        self.drag = Some(selection::Drag::press((column, row), block));
    }

    /// 按住移动：越过门槛就进入选择态，头的落点夹在所属区域里。
    ///
    /// 没落进任何文本块（状态行、页签条、空白）时这次按下不产生选区，抬起照旧按一次点击处理。
    fn drag_to(&mut self, column: u16, row: u16) {
        let Some(rect) = self
            .drag
            .and_then(|drag| drag.block(&self.screen_text))
            .map(|block| block.rect)
        else {
            return;
        };
        if let Some(drag) = self.drag.as_mut() {
            drag.moved((column, row), rect);
        }
    }

    /// 左键抬起：这是一次拖选就复制，否则把这一次点击交给 [`TuiState::click_at`]。
    fn release_at(&mut self, column: u16, row: u16) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.selecting {
            self.copy(&drag);
            return;
        }
        self.click_at(column, row);
    }

    /// 把这次拖选取到的文本交给剪贴板，并在提示行留一句回执
    /// （`.scratch/tui-feedback/spec.md` §6）。
    ///
    /// 这里只**算出**要写的字节（OSC 52）与那句回执：写出去是运行期的事，与标题同一个形状。
    /// 选区取出来是空的时候什么都不做，也不留回执。
    fn copy(&mut self, drag: &selection::Drag) {
        let text = selection::text(&self.screen_text, drag);
        if text.is_empty() {
            return;
        }
        self.clipboard = Some(selection::osc52(&text));
        self.copied = Some((
            std::time::Instant::now(),
            text.chars().count(),
            text.lines().count(),
        ));
    }

    /// 取走这次复制要写出的字节，没有就是 `None`。运行期把它原样交给终端。
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    /// 取走这次点击要打开的目标，没有就是 `None`。运行期把它交给系统默认程序
    /// （`.scratch/clickable-links/spec.md` §3）。
    pub fn take_open_request(&mut self) -> Option<String> {
        self.open_request.take()
    }

    /// 运行期把一次打开的结果写回来：这就是提示行那句回执的**唯一**来源。
    ///
    /// 措辞由运行期算（它才知道成没成），这里只管记下来并请一帧 —— 于是「打开」这件事在
    /// 状态机里只剩一个字符串，测试不必起进程就能钉住点击要打开什么。
    pub fn note_open_receipt(&mut self, text: String) {
        self.opened = Some((std::time::Instant::now(), text));
        self.mark_dirty();
    }

    /// 一次点击落到哪儿：五次分派，按谁占着指针排序。详情覆盖层直接占着它；否则是问题；否则
    /// 是这一帧自己的那些部件，回合条与页签排在它们旁边的文字之前。这里从不滚动某个立着的东西
    /// 背后的转录（票 04 §2，`tui-sidebar` spec §7）。
    fn click_at(&mut self, column: u16, row: u16) {
        // 选择器**不独占指针**（spec §10）：框外点击关掉它（与详情覆盖层同一条），框内点一行
        // 等于选中那行，其余点击照旧分派 —— 立着的时候输入区仍然可以被点，键盘归属才不变。
        if self.picker.is_some() && self.picker_click(column, row) {
            return;
        }
        if self.detail_open() {
            // 框外的一次点击关掉它 —— 它来自的那一行、转录、页脚，什么都行
            // （票 02 §4；2026-09-23 修正，原先只认「再点同一行」）。框内的点击是覆盖层自己的、
            // 什么都不做，因为它没有自己的按钮（拖选它的正文是另外一条路）。
            let inside = self
                .detail_rect
                .is_some_and(|rect| rect.contains((column, row).into()));
            if !inside {
                self.close_detail();
                // 关掉它的这一下如果落在输入区上，那同时就是「我要打字」：键盘也一起还回去。
                self.take_input_keyboard(column, row);
            }
            return;
        }
        // 问卷占着的是**底部输入区**，不是整个指针：它只吃自己那块区域里的点击 —— 主列上方的
        // 页签条、转录与左栏都还在屏幕上，点它们照旧归它们本来管的人。滚轮早就是这条判据
        // （`wheel_at` 的 `owned`），点击跟上它（`.scratch/questionnaire-keys/spec.md` §7 的
        // 补记）。
        //
        // 而**键盘**是另一条判据：没有浮层立着时它归问卷（`j`/`k` 移动高亮、回车提交、`Esc`
        // 退出询问），一旦**浮层**立着就归浮层 —— `Esc` 与 `Ctrl-D` 把它关掉，问卷的作答原样
        // 留在那里等你回来答（`.scratch/questionnaire-reading/spec.md`，取代原先「切页不打开
        // 详情」那半句）。
        let questioning = self.questionnaire().is_some();
        if questioning {
            let owned = self
                .questionnaire_bottom
                .is_some_and(|rect| rect.contains((column, row).into()));
            if owned {
                self.question_click(QuestionClick::At(column, row));
                return;
            }
        } else if self.pending.is_some() {
            // 中间那三种确认是**覆盖层**，它们占着**指针**：一次落在别处的点击什么都不做 ——
            // 尤其是，它绝不许关掉一个读的人还没回答的问题（spec §7、§9）。
            self.question_click(QuestionClick::At(column, row));
            return;
        }
        // 点回输入区 = 「我要打字」：键盘还给输入区。它与「点左栏任意处把键盘交给这一页」
        // 是同一条规矩的两半（`.scratch/files-page/spec.md` §5 的「归还」）—— 少了它，点开过
        // 一个文件的人想接着打字，只能先按一下 `Esc`。输入区没有别的点击语义，所以到头了。
        if !questioning && self.take_input_keyboard(column, row) {
            return;
        }
        // 点左栏任意处把键盘交给这一页 —— 页签条与页区都算，而这一下同时仍然是它本来
        // 那件事（切页、展开、开弹窗）（`.scratch/files-page/spec.md` §5）。
        if !questioning {
            self.take_sidebar_keyboard(column, row);
        }
        match self.regions.action_at(column, row) {
            Some(HitAction::SwitchTab(tab)) => {
                self.tab = tab;
                // 点页签条也是一次「点左栏」：切到能走键盘的那两页时键盘跟着交给它们，并先给
                // 第一行焦点，于是 `↓` 立刻走得动（`.scratch/files-page/spec.md` §5）。
                // 切到另外两页则把键盘还回去 —— 它们没有能用方向键走的东西。
                if !questioning {
                    self.sidebar_keyboard = matches!(tab, Tab::Files | Tab::Changes);
                    if tab == Tab::Files && self.files_page.focus.is_none() {
                        self.files_page.focus = Some(0);
                    }
                    if tab == Tab::Changes && self.changes.focus.is_none() {
                        self.changes.rows = changes::rows(&self.changes.files);
                        self.changes_move_focus(1);
                    }
                }
            }
            Some(HitAction::SwitchMainTab(tab)) => self.main_tab = tab,
            // 点状态行那两格（spec §7、§12）。讨论会话不上行 —— 那一格的模型是**两个模型的
            // 拼法**，是名册里的配置事实，所以它只给一句说明。
            Some(HitAction::SwitchModel) => self.ask_picker(PickerKind::Model),
            Some(HitAction::SwitchEffort) => self.ask_picker(PickerKind::Effort),
            Some(HitAction::TurnRailUnit(unit)) => self.jump_to_unit(unit),
            _ if self.indicator_hit(Viewport::Trace, column, row) => self.trace.to_bottom(),
            _ if self.indicator_hit(Viewport::Conversation, column, row) => {
                self.conversation.to_bottom()
            }
            _ => {
                // 文件页上的点击先于转录：左栏页区里的一行是这一页自己的东西
                // （`.scratch/files-page/spec.md` §4）。问卷立着时它照旧接这一下 —— 它开出来
                // 的浮层要走的正是上面那条键盘分派（`.scratch/questionnaire-reading/spec.md`）。
                if self.files_click(column, row) {
                    return;
                }
                // 改动页同理：页区里的一个**文件行**是这一页自己的东西（点一行就把焦点放到
                // 那一行上，并在详情覆盖层里开那份 diff）。
                if self.changes_click(column, row) {
                    return;
                }
                // `todo` 页同理：整页任何一格都是「看全那份清单」
                // （`.scratch/todo-page/spec.md` §5）。
                if self.todo_click(column, row) {
                    return;
                }
                // 对话视图里点到一段可点的文本（链接热区）：解析出目标就交给运行期去打开
                // （`.scratch/clickable-links/spec.md` §1、§3）。它排在轨迹页之前 —— 两页
                // 共用同一块矩形，但各自的矩形只在真画了那一页时才有值，所以不会互相抢。
                if self.follow_link(column, row) {
                    return;
                }
                // 指针落在轨迹页上吗？`trace_rect` 只在上一帧真的画了轨迹页时才有值，所以
                // 「记住读的人真看到了什么」这条纪律也管着视口的选择
                // （`.scratch/trace-in-main/spec.md` §4）。
                let over_trace = self
                    .trace_rect
                    .is_some_and(|rect| rect.contains((column, row).into()));
                // 落点不在轨迹页的内容区里就什么都不点：左栏、分隔列、状态行、输入区、对话页
                // 里**没点中链接的那几列**都没有可点开的行
                // （`.scratch/tui-feedback/spec.md` §9）。行号是**屏幕**行号，拿它去取别处的
                // 详情会点到同一横行的别的行上。
                if !over_trace {
                    return;
                }
                let panes = layout::plan(self.area, 1, self.sidebar_wanted);
                if let Some(detail) = self.trace_link_at(row) {
                    // 打开方先算好：`open_detail` 借 `&mut self`。
                    let opener = DetailOpener::Trace {
                        top: self.trace.top(),
                        follow: self.trace.following(),
                    };
                    self.open_detail(detail, panes.detail_text_width() as usize, opener);
                }
            }
        }
    }

    /// 点左栏的 `todo` 页：页区里**任何一格**都打开那份完整清单
    /// （`.scratch/todo-page/spec.md` §5）。
    ///
    /// 这一页没有焦点行，所以这里**不需要**「屏幕行 → 行下标」那套换算：落在这块矩形里就是
    /// 落在这一页上。列表空着时开不出一个空弹窗，于是这一次归 `false`，指针继续往下走。
    fn todo_click(&mut self, column: u16, row: u16) -> bool {
        if self.tab != Tab::Todo || !self.todo.page_contains((column, row)) {
            return false;
        }
        if self.todo.is_empty() {
            return false;
        }
        self.open_todo_detail();
        true
    }

    /// 把左栏 `todo` 页那份完整清单打开在详情覆盖层里
    /// （`.scratch/todo-page/spec.md` §6）。
    ///
    /// 正文是**打开那一刻**的那份列表的一份拷贝 —— 此后模型提交新列表只改面板，不动开着的
    /// 这个。与文件页那个弹窗同一纪律：只给人看，不进事件流、不进模型上下文。
    fn open_todo_detail(&mut self) {
        self.close_file_viewer();
        let detail = Detail {
            // 标题说清这一份**是谁提交的** —— 讨论会话里各方各有一份（§1）。
            title: wording::detail_todo_title(
                self.todo.speaker().map(wording::speaker_name).as_deref(),
            ),
            // 一份计划不是谁说的话：标题用正文档那一档，与文件页那个内容弹窗同一个颜色。
            color: palette::PLAIN,
            kind: DetailKind::Todo {
                items: self.todo.all().to_vec(),
            },
        };
        let width = layout::plan(self.area, 1, self.sidebar_wanted).detail_text_width() as usize;
        self.open_detail(detail, width, DetailOpener::Todo);
    }

    /// 一次点击落在文件页上吗：落在哪一行就动那一行 —— 目录展开或收起，
    /// 而文件行的一次点击由内容弹窗那一张票接上（`.scratch/files-page/spec.md` §4）。
    ///
    /// 回答 `true` 表示这一下归这一页，调用方因此不再按转录那一套分派。落在页区之外、
    /// 或者这一帧根本没画文件页时回答 `false`。
    fn files_click(&mut self, column: u16, row: u16) -> bool {
        if self.tab != Tab::Files || !self.files_page_contains(column, row) {
            return false;
        }
        let Some(index) = self.files_index_at(row) else {
            return false;
        };
        self.files_page.focus = Some(index);
        match (self.files_dir_at(index), self.file_path_at(index)) {
            (Some(dir), _) => self.toggle_directory(&dir),
            // 文件行：点一下打开内容弹窗（`.scratch/files-page/spec.md` §4），至于是内置
            // 预览还是嵌一个 nvim，由配置挑（`.scratch/nvim-file-viewer/spec.md` §2）。
            (None, Some(path)) => self.open_file(&path),
            (None, None) => {}
        }
        true
    }

    /// 这一格是不是落在文件页的页区里。
    fn files_page_contains(&self, column: u16, row: u16) -> bool {
        self.files_page
            .rect
            .is_some_and(|page| page.contains((column, row).into()))
    }

    /// 屏幕行 `row` 对应的可见行下标；落在页区之外或超出那一份可见行时是 `None`。
    ///
    /// `files_rows` 是上一帧画出来的那一份，而它从滚动位置起 —— 与「只有真画出来的行才回应
    /// 指针」是同一条纪律。
    fn files_index_at(&self, row: u16) -> Option<usize> {
        let page = self.files_page.rect?;
        if row < page.y {
            return None;
        }
        let index = self.files_page.scroll + (row - page.y) as usize;
        (index < self.files_page.rows.len()).then_some(index)
    }

    /// 那一行是个目录时它的路径（索引里的拼法，带尾斜杠）。
    fn files_dir_at(&self, index: usize) -> Option<String> {
        let row = self.files_page.rows.get(index)?;
        row.dir.then(|| row.path.clone())
    }

    /// 那一行的路径（索引里的拼法）。
    fn file_path_at(&self, index: usize) -> Option<String> {
        self.files_page.rows.get(index).map(|row| row.path.clone())
    }

    /// 注入「查看器有新画面」的通知口（[`Tui::run`] 调；测试不调）。
    pub fn set_viewer_wake(&mut self, wake: tokio::sync::mpsc::UnboundedSender<()>) {
        self.viewer_wake = Some(wake);
    }

    /// 点开一个工作区文件：按配置挑哪一档查看器
    /// （`.scratch/nvim-file-viewer/spec.md` §2）。
    ///
    /// 这是那一个岔口 —— 鼠标单击与 `→` 都从这里进，所以「配了就用 nvim」在两个入口上
    /// 不可能走岔。
    fn open_file(&mut self, path: &str) {
        use crate::config::FileViewer;
        match self.facts.file_viewer.kind {
            FileViewer::Builtin => self.open_file_detail(path),
            FileViewer::Nvim => self.open_file_viewer(path),
        }
    }

    /// 把一屏 nvim 打开在浮层里（`[ui] file_viewer = "nvim"`）。
    ///
    /// 起不来就**回退到内置预览**：配置里写着 `nvim` 而 PATH 上没有它是能发生的事，那时该
    /// 看见文件的内容，而不是一块空白（`wording::viewer_unavailable` 在提示行说一句）。
    ///
    /// 它是只读的、不折行、鼠标给 nvim —— 三条都写在
    /// [`viewer::NvimViewer::spawn`] 里，因为它们是同一件事的三个面：这一档进来是**看**文件。
    fn open_file_viewer(&mut self, path: &str) {
        if path.is_empty() {
            return;
        }
        // 两块浮层互斥：开着详情时点开文件查看器，先把详情收掉。
        self.close_detail();
        let panes = layout::plan(self.area, 1, self.sidebar_wanted);
        let Some(area) = panes.overlay_area(panes.viewer_width(self.facts.file_viewer.width))
        else {
            // 没地方画：与详情覆盖层给同一个诚实答案 —— 不开，而不是开成两行。
            return;
        };
        let grid = layout::inner(area);
        match viewer::NvimViewer::spawn(
            &self.cwd,
            path,
            grid.width,
            grid.height,
            self.viewer_wake.clone(),
        ) {
            Ok(spawned) => {
                self.file_viewer = Some(FileViewerPane {
                    viewer: Box::new(spawned),
                    grid,
                });
            }
            // 起不来（PATH 上没有 nvim、或 pty 开不出来）：**回退到内置预览**。读的人
            // 照样看得见文件的内容 —— 这一档唯一的降级，也是唯一一处不声张的地方：
            // 打开文件失败而不给内容，比给内容少一样东西更让人摸不着头脑。
            Err(_) => self.open_file_detail(path),
        }
    }

    /// 收掉查看器那一块。没开着时是空操作。
    ///
    /// 顺手请一次重扫：那一档是只读的（改不动文件），但这条契约不该压在「它只读」上 ——
    /// nvim 里跑的任何东西都可能碰过工作区，而重扫是一条静默信号，宽一点没有代价。
    fn close_file_viewer(&mut self) {
        if let Some(mut pane) = self.file_viewer.take() {
            pane.viewer.kill();
            self.file_scan_wanted = true;
        }
        self.file_viewer_rect = None;
    }

    /// 查看器里的一个按键：归它回答 `true`。
    ///
    /// **`Ctrl-C` 关掉浮层** —— 这个手势在整个 heng 里就是「退出当前这件事」（举手退出、
    /// 取消回合）。`Esc` 与 `Ctrl-D` **不**留给前端：它们在 nvim 里是退出插入模式与向下翻
    /// 半屏，抢走就等于把编辑器弄坏了。这是与详情覆盖层**有意不同**的一处（那边 `Esc` 与
    /// `Ctrl-D` 是关），也是这一档唯一两处「覆盖层关法不一样」之一。
    ///
    /// `Ctrl-Z` 返回 `false`：它是终端层手势，`TuiState::key` 在一切守卫之前就把它接走了
    /// （`.scratch/suspend-gesture/spec.md` §1），这里只是不替它做主。
    fn viewer_key(&mut self, key: Key) -> bool {
        match key {
            Key::CtrlC => {
                self.close_file_viewer();
                true
            }
            Key::CtrlZ => false,
            other => {
                if let Some(bytes) = viewer::key_bytes(&other) {
                    if let Some(pane) = &mut self.file_viewer {
                        pane.viewer.feed(&bytes);
                    }
                }
                true
            }
        }
    }

    /// 查看器里的一次鼠标事件。
    ///
    /// 两条都照详情覆盖层的老规矩：**框外一次左键**关掉它、且那一下**不再穿透**；框内但
    /// 落在留白上的那几格算覆盖层自己的，什么都不做。别的（拖动、滚轮）原样转给 nvim。
    fn viewer_mouse(&mut self, mouse: MouseEvent) {
        let Some(pane) = self.file_viewer.as_ref() else {
            return;
        };
        let area = self.file_viewer_rect.unwrap_or(pane.grid);
        let grid = pane.grid;
        let inside = area.contains((mouse.column, mouse.row).into());
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) && !inside {
            self.close_file_viewer();
            return;
        }
        if let Some(bytes) = viewer::mouse_bytes(mouse, grid) {
            if let Some(pane) = &mut self.file_viewer {
                pane.viewer.feed(&bytes);
            }
        }
    }

    /// 把一个工作区文件的内容打开在详情覆盖层里
    /// （`.scratch/files-page/spec.md` §6）。
    ///
    /// 正文在**这里**、在打开的那一刻读盘，并按正文文本区宽排版 —— 与轨迹页的详情同一个形状。
    /// 打开方记成 [`DetailOrigin::Files`]：覆盖层立着时文件页既不冻也不还原，所以关掉它之后
    /// 这一页停在原处。
    ///
    /// 这个弹窗不进事件流、不进模型上下文：渲染器不 emit 任何东西，权限门约束的也不是人在
    /// 自己的终端里点开自己的文件。
    fn open_file_detail(&mut self, path: &str) {
        if path.is_empty() {
            return;
        }
        self.close_file_viewer();
        let body = files::read(&self.cwd, path);
        let detail = Detail {
            title: path.to_owned(),
            // 文件不是谁说的话：标题用正文档那一档，与树里那一行同一个颜色。
            color: palette::PLAIN,
            kind: DetailKind::File { body },
        };
        let width = layout::plan(self.area, 1, self.sidebar_wanted).detail_text_width() as usize;
        self.open_detail(detail, width, DetailOpener::Files);
    }

    /// 焦点所在的那一行。
    fn focused_file_row(&self) -> Option<files::Row> {
        if self.tab != Tab::Files {
            return None;
        }
        self.files_page.rows.get(self.files_page.focus?).cloned()
    }

    /// 点左栏的**文件页**或**改动页**：键盘交给这一页，焦点行落在点的那一行上。
    ///
    /// 只认得**真画出来**的那两块矩形（页签条与页区），与指针分派同一条纪律；也只认能走键盘
    /// 的那两页 —— `调用量` 与 `todo` 没有能用方向键走的东西，键盘扣在它们上面只会让输入区
    /// 静默失灵。落在页签条上、或者落在改动页的分组标题上时，焦点行保持原样；还没有焦点时
    /// 先给第一个文件行，于是 `↓` 立刻走得动。
    fn take_sidebar_keyboard(&mut self, column: u16, row: u16) {
        let Some(page) = self.sidebar_page_rect() else {
            return;
        };
        let point = (column, row).into();
        let in_page = page.contains(point);
        let in_tabs = self.tabs_rect.is_some_and(|tabs| tabs.contains(point));
        if !in_page && !in_tabs {
            return;
        }
        self.sidebar_keyboard = true;
        if in_page {
            match self.tab {
                Tab::Files => {
                    if let Some(index) = self.files_index_at(row) {
                        self.files_page.focus = Some(index);
                        return;
                    }
                }
                Tab::Changes => {
                    if let Some(index) = self.changes_index_at(row) {
                        self.changes.focus = Some(index);
                        return;
                    }
                }
                _ => {}
            }
        }
        match self.tab {
            Tab::Files => {
                if self.files_page.focus.is_none() && !self.files_page.rows.is_empty() {
                    self.files_page.focus = Some(0);
                }
            }
            Tab::Changes => {
                if self.changes.focus.is_none() {
                    self.changes_move_focus(1);
                }
            }
            _ => {}
        }
    }

    /// 把键盘还给输入区。切页签与收起左栏都走它 —— 那一页没有键盘语义，而收起来的那一栏更
    /// 不该扣着键盘（与「没地方画覆盖层就关掉它」同一条纪律）。
    fn release_sidebar_keyboard(&mut self) {
        self.sidebar_keyboard = false;
    }

    /// 点在输入区上：把键盘还给输入区，回答 `true`。
    ///
    /// 它与「点左栏任意处把键盘交给这一页」是同一条规矩的两半
    /// （`.scratch/files-page/spec.md` §5 的「归还」）—— 点在哪儿，键盘就归哪儿。输入区没有
    /// 别的点击语义（那是行编辑，点在哪儿都还是同一份草稿），所以这一下到头了。
    fn take_input_keyboard(&mut self, column: u16, row: u16) -> bool {
        let input = layout::plan(self.area, 1, self.sidebar_wanted).input;
        if !input.contains((column, row).into()) {
            return false;
        }
        self.release_sidebar_keyboard();
        true
    }

    /// 「有人要开这份清单」：能换就上行一个手势，换不了就给一句说明。
    ///
    /// 忙闲不在这里判 —— 渲染器不知道什么在跑，所以只上行；循环那一侧答一句「这一回合跑完
    /// 再切」（spec §6）。
    fn ask_picker(&mut self, kind: PickerKind) {
        if !self.facts.switchable {
            self.say(wording::heng(wording::switch_not_switchable()));
            return;
        }
        // 没有档位旋钮的模型：开出来只会是一个**空**浮层（spec §8 的那一句正是为了这个），
        // 所以这里给一句说明，而不是让读的人看见一块没有候选的盒子。
        if kind == PickerKind::Effort && self.fixed_effort() {
            self.say(wording::effort_fixed_detail(&self.facts.model));
            return;
        }
        self.events.push(FrontEndEvent::OpenPicker(kind));
    }

    /// 指针落在选择器上时的那一下（spec §10）。
    ///
    /// 框内点一行等于选中它（禁用行不响应），框外关掉它。回答 `true` 表示这一下已经被它吃掉。
    fn picker_click(&mut self, column: u16, row: u16) -> bool {
        let Some(picker) = self.picker.as_ref() else {
            return false;
        };
        let inside = picker
            .rect
            .is_some_and(|rect| rect.contains((column, row).into()));
        if !inside {
            let mut picker = self.picker.take().expect("刚判过它在");
            picker.close(None);
            return true;
        }
        let hit = picker
            .rows
            .iter()
            .find(|(line, from, cell_width, _)| {
                *line == row && *from <= column && column < from.saturating_add(*cell_width)
            })
            .map(|(_, _, _, index)| *index);
        match hit {
            // 只有**画出来**的行才点得到（与问卷的选项区同一条纪律）。
            Some(index) => {
                let enabled = picker
                    .options
                    .get(index)
                    .is_some_and(|option| option.enabled);
                if enabled {
                    let mut picker = self.picker.take().expect("刚判过它在");
                    picker.close(Some(index));
                }
            }
            None => return false,
        }
        true
    }

    /// 选择器立着时的一个按键（spec §10）。
    ///
    /// `Ctrl-T` 在这里**不**上行 —— 它归选择器自己（这一格被它占着），而别的手势照旧穿透：
    /// 退出、挂起、左栏开关、模式循环都不该因为开着一份清单而失灵（与
    /// `sidebar_key` 同一判据）。
    fn picker_key(&mut self, key: Key) {
        match key {
            // 一行一个模型，所以横向与纵向是**同一件事**：下一行/上一行。
            Key::Char('j') | Key::Down | Key::Right | Key::Tab => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.move_highlight(1);
                }
            }
            Key::Char('k') | Key::Up | Key::Left | Key::BackTab => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.move_highlight(-1);
                }
            }
            Key::Enter => {
                // 禁用行不响应：回车在那里什么都不发生，浮层还立着（spec §8）。
                if self
                    .picker
                    .as_ref()
                    .is_some_and(Picker::highlighted_is_enabled)
                {
                    let mut picker = self.picker.take().expect("刚判过它在");
                    let answer = picker.highlight;
                    picker.close(Some(answer));
                }
            }
            // 取消不是一次切换，也不给回执（spec §7）。
            Key::Esc => {
                if let Some(mut picker) = self.picker.take() {
                    picker.close(None);
                }
            }
            _ => {}
        }
    }

    /// 键盘在左栏时的一个按键：归这一页回答 `true`。
    ///
    /// 认的只有**能走键盘的那两页**的键；别的键照旧落到它们本来去的地方 —— 点一下左栏不该把
    /// 打字的手感弄丢。`Esc` 在两页上都是「把键盘还回去」
    /// （`.scratch/files-page/spec.md` §5、`.scratch/diff-page/spec.md` §7）。
    fn sidebar_key(&mut self, key: Key) -> bool {
        match self.tab {
            Tab::Files => self.files_key(key),
            Tab::Changes => self.changes_key(key),
            // 另外两页没有能用方向键走的东西：键盘本来也不该扣在它们上面。
            _ => return false,
        }
    }

    /// 文件页的键位。
    fn files_key(&mut self, key: Key) -> bool {
        match key {
            // 一次手势一层：这一下只把键盘还回去。要取消回合，等键盘回去之后再按一下 ——
            // 那一下仍按既有的忙碌 / 空闲分叉走。
            Key::Esc => self.sidebar_keyboard = false,
            Key::Up => self.files_move_focus(-1),
            Key::Down => self.files_move_focus(1),
            Key::Left => self.files_collapse_focus(),
            Key::Right => self.files_open_focus(),
            Key::Enter => self.files_insert_focus(),
            // 打字不落进草稿：键盘确实在左栏。控制键（`Ctrl-*`）、`Tab` / `Shift+Tab` 与
            // 翻页键照常穿透 —— 退出、挂起、左栏开关、模式循环、翻页都不该因为点了一下左栏
            // 而失灵（`.scratch/files-page/spec.md` §5）。
            Key::Char(_) => {}
            _ => return false,
        }
        true
    }

    /// 改动页的键位：`↑` `↓` `Enter` `Esc` `r`（`.scratch/diff-page/spec.md` §7）。
    ///
    /// `@路径` 插入**不搬过来** —— 那是文件页的语义，两页各自回答一个问题。
    fn changes_key(&mut self, key: Key) -> bool {
        match key {
            // 一次手势一层：这一下只把键盘还回去，**不**取消正在跑的回合（§7）。
            Key::Esc => self.sidebar_keyboard = false,
            Key::Up => self.changes_move_focus(-1),
            Key::Down => self.changes_move_focus(1),
            Key::Enter => self.open_changes_focus(),
            // 手动重取（§4）：在**外部 shell** 里做的 `git add` 不会触发任何信号，所以这一页
            // 要有一把手动键。它落在改动页的键盘归属上，不是全局键 —— `r` 是普通字符，做成
            // 全局键会与输入区抢键。
            Key::Char('r') => {
                self.changes_wanted = true;
                self.say(wording::changes_refreshing().to_owned());
            }
            // 与文件页同一条：打字不落进草稿。
            Key::Char(_) => {}
            _ => return false,
        }
        true
    }

    /// `↑` / `↓`：焦点行挪一格，两端不越界；它挪出窗口时视口跟着走。
    fn files_move_focus(&mut self, delta: isize) {
        if self.tab != Tab::Files || self.files_page.rows.is_empty() {
            return;
        }
        let last = self.files_page.rows.len() - 1;
        let at = match self.files_page.focus {
            Some(at) => (at as isize + delta).clamp(0, last as isize) as usize,
            None if delta > 0 => 0,
            None => last,
        };
        self.files_page.focus = Some(at);
        self.files_scroll_to_focus();
        self.dirty = true;
    }

    /// 焦点行滚进窗口里：键盘走到窗口之外时，视口跟着它走。
    fn files_scroll_to_focus(&mut self) {
        let window = self.files_page.rect.map_or(0, |page| page.height as usize);
        let Some(at) = self.files_page.focus else {
            return;
        };
        if window == 0 {
            return;
        }
        if at < self.files_page.scroll {
            self.files_page.scroll = at;
        } else if at >= self.files_page.scroll + window {
            self.files_page.scroll = at + 1 - window;
        }
    }

    /// `→`：目录摊开，文件打开内容弹窗（`.scratch/files-page/spec.md` §5）。
    fn files_open_focus(&mut self) {
        let Some(row) = self.focused_file_row() else {
            return;
        };
        if row.dir {
            if !row.expanded {
                self.toggle_directory(&row.path);
            }
        } else {
            self.open_file(&row.path);
        }
    }

    /// `←`：目录收起。
    fn files_collapse_focus(&mut self) {
        let Some(row) = self.focused_file_row() else {
            return;
        };
        if row.dir && row.expanded {
            self.toggle_directory(&row.path);
        }
    }

    /// `Enter`：把焦点行的路径作为 `@路径` 插进草稿，然后把键盘还给输入区 ——
    /// 插完接着就要打字（`.scratch/files-page/spec.md` §5）。
    fn files_insert_focus(&mut self) {
        let Some(row) = self.focused_file_row() else {
            return;
        };
        // 记号以空白结束：草稿里已经有别的内容时先隔一个空格，否则两个记号会粘成一个。
        let lead = match self.editor.text().chars().last() {
            None => "",
            Some(ch) if ch.is_whitespace() => "",
            Some(_) => " ",
        };
        self.editor.insert_str(&format!("{lead}@{}", row.path));
        self.sync_tokens();
        self.sidebar_keyboard = false;
        self.dirty = true;
    }

    /// 目录的展开与收起：同一个目录再点一下（或再按一下 `←` / `→`）就是收起。
    ///
    /// 收起会连带藏起它下面那些行，所以可见行**立刻**重算一次：焦点行与滚动位置不能指着
    /// 一份已经不存在的清单（下一帧还会再算一遍，两处用的是同一个纯函数）。
    fn toggle_directory(&mut self, path: &str) {
        if !self.files_page.expanded.remove(path) {
            self.files_page.expanded.insert(path.to_owned());
        }
        self.refresh_files_rows();
        self.dirty = true;
    }

    /// 按当前的索引与展开状态重算可见行，并把焦点与滚动位置收回范围内。
    fn refresh_files_rows(&mut self) {
        self.files_page.rows = files_tree(self).unwrap_or_default();
        let last = self.files_page.rows.len().saturating_sub(1);
        if let Some(at) = self.files_page.focus {
            self.files_page.focus = Some(at.min(last));
        }
        self.files_page.scroll = self.files_page.scroll.min(last);
        self.files_scroll_to_focus();
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
                        // 页脚这个按钮与 `→` 键是**同一个手势**：先给还没作答的当前题记一次
                        // 跳过，再前进（spec §11）。按钮只在「后面还有题」时才画得出来，
                        // 所以末题那条守卫在这里不必再写一遍。
                        questionnaire.skip_if_unanswered();
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
        if self.conversation.following() {
            return Some(units - 1);
        }
        let source = self.conversation.source_at(self.conversation.top())?;
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
        self.conversation.scroll_to_source(head);
    }

    /// 一次点击落在**对话视图的一段可点文本**上吗：落在上面就把它解析掉，交给运行期去打开
    /// （`.scratch/clickable-links/spec.md` §1、§3）。
    ///
    /// 判据按顺序，一条不成立就当没点：落在对话转录区里、落在这一帧真画出来的那一块上、
    /// 那一行的列（扣掉 `lead`）落在某个候选里、候选解析得出目标。解析不出来（不存在、区外、
    /// 不可解析）就**什么都不发生、也不留回执** —— 点一段普通文字本来就该没有反应。
    ///
    /// 只有一个副作用：把目标放进 `open_request`。起进程是运行期的事，状态机因此仍然不必
    /// 碰终端，也不必起浏览器。
    fn follow_link(&mut self, column: u16, row: u16) -> bool {
        let Some(area) = self.conversation_rect else {
            return false;
        };
        if !area.contains((column, row).into()) {
            return false;
        }
        let Some(index) = self.screen_text.block_at(column, row) else {
            return false;
        };
        let Some(block) = self.screen_text.block(index) else {
            return false;
        };
        // 盖在转录上的东西（详情覆盖层、菜单）是最上层那一块 —— 点到的是它们，不是转录。
        if block.rect != area {
            return false;
        }
        let Some(line) = block.rows.get(usize::from(row - area.y)) else {
            return false;
        };
        // `lead` 是这一段自己的留白（气泡与它上面那行名字），候选的列从文本第 0 列起。
        let at = usize::from(column - area.x).saturating_sub(usize::from(line.lead));
        let Some(hot) = line.hotspots.iter().find(|hot| hot.covers(at)) else {
            return false;
        };
        let Some(target) = hot.target.resolve(&self.cwd) else {
            return false;
        };
        self.open_request = Some(target);
        self.mark_dirty();
        true
    }

    /// 轨迹页里一次点击落到的那个可点链接，拷成它要打开的东西。
    ///
    /// 覆盖层将在哪个宽度上打开，来自上一帧，那是中间块几何唯一已知的地方（票 04 §1）。行号是
    /// **屏幕**行号：一次点击按它换回那条来源行。**只有轨迹页有入口** —— 对话页画的是全文，
    /// 点它不打开任何东西（`.scratch/tui-feedback/spec.md` §9）。
    fn trace_link_at(&self, row: u16) -> Option<Detail> {
        let offset = (row.checked_sub(self.trace_drawn.top)?) as usize;
        let source = (*self.trace_drawn.rows.get(offset)?)?;
        self.trace_links.get(source)?.clone()
    }

    /// 一条记录的身份：流上的东西取信封的行号，渲染层自己造的没有信封，于是 `index` 由
    /// 渲染层那条单调计数补（[ADR 0021](../../docs/adr/0021-line-identity-comes-from-the-event-envelope.md)）。
    ///
    /// `offset` 是**这一批块里的第几个**（一条事件可以产出零个到多个）。流上的那批用批次内的
    /// 序号 —— 它跟着事件走，所以重放时哪怕块的产出顺序变了，身份仍然指向同一个块。
    fn next_block_id(&mut self, seq: Option<u64>, offset: u32) -> BlockId {
        let index = match seq {
            Some(_) => offset,
            None => {
                let index = self.next_local_id;
                self.next_local_id += 1;
                index
            }
        };
        BlockId { seq, index }
    }

    /// 那一块的**第一条**来源行；已经被上限裁掉、或者还没有画出来，就 `None`。
    ///
    /// 这是「块 → 位置」那一步，而窗格只按来源行下标寻址 —— 于是它是
    /// [`Self::trace_block_ids`] 上的一次线性查找。那张表与窗格**同长同进同出**（`push`
    /// 报的丢弃数是唯一权威），所以表在就是窗格在，而查到的那一行就是读者会看到的那一行。
    ///
    /// **不做以块为键的反向索引**：那张表每次裁剪都要全体左移一张映射，而查找只在读者按键
    /// 或绘制选中高亮时发生 —— 那是每帧至多一次、长度有上限的线性扫描。
    ///
    /// 它是**公共**的，因为「选中那一块」这件事的验收在集成测试里（屏上那一行亮着），
    /// 而那是后面几轮账本与检索的接缝（`.scratch/trace-ledger/spec.md` §2）。
    pub fn first_source_of(&self, id: BlockId) -> Option<usize> {
        self.trace_block_ids.iter().position(|it| *it == Some(id))
    }

    /// 一次点击是否落在了「回到末尾」指示器上。每个视口各有一个（票 09）。
    fn indicator_hit(&self, view: Viewport, column: u16, row: u16) -> bool {
        let slot = match view {
            Viewport::Conversation => self.indicator,
            Viewport::Trace => self.trace_indicator,
        };
        slot.is_some_and(|rect| {
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
                // 时钟现在**一直**在走（§32），所以相位要在这里对齐：一次运行总是从帧 0 开始
                // 呼吸，空闲时提示符歇在帧 0 的颜色上 —— 输入区因此仍然完全静止，动的是状态行
                // 那个字形循环（`.scratch/tui-visual-language/spec.md` §30–§33）。
                self.pulse = 0;
                if !running {
                    // 一次运行结束：「正在做什么」没有主语了。
                    self.running_tool = None;
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
            // 换完模型/档位之后循环推回来的新事实（spec §3）。它是**通知**：没有答案要等，
            // 前端把它并进自己那份注入事实，然后请一帧。
            ConsoleRequest::SessionUpdate {
                model,
                effort,
                context_window,
                speakers,
            } => {
                self.facts.model = model;
                self.facts.context_window = context_window;
                self.facts.speaker_order = speakers;
                // 档位是渲染器状态（与 `mode` 同一格）：它不进 `facts`，因为它是**会话中途**
                // 才有的值，而注入的 facts 是组装时的快照。
                self.effort = effort;
                // 名册换了，于是每个新名字去拿一个没人认领的颜色槽位 ——
                // `SpeakerColors` 已经为这件事备好了路（spec §4）。
                self.colors.adopt_roster(&self.facts.speaker_order);
            }
            // 一份「从这份清单里挑一个」的问话（spec §9）。
            ConsoleRequest::Picker(request) => {
                // 覆盖层与问题为它退下：一个还没回答的问题不该被一块浮层压住，而一份清单
                // 也只可能在这一刻没有别的东西占着键盘。
                self.close_detail();
                if matches!(
                    self.pending,
                    Some(Pending::Loop { .. } | Pending::Questionnaire(_))
                ) {
                    // 循环一次只问一个问题，所以这不可能发生。丢掉*新*的那份让屏幕上那个
                    // 仍然可答 —— 与 `Ask` 同一个读法。
                    let _ = request.reply.send(None);
                    return;
                }
                self.picker = Some(Picker::new(request));
            }
            // 循环能作用上去的那些名字。它们在组装之后到达一次 —— skills 来自会话 ——
            // 没有别的东西携带它们。
            ConsoleRequest::Catalog { entries } => {
                self.catalog = entries;
                // 命令表是判据的一半，它一到，草稿里的记号就该重新判一次。
                self.sync_tokens();
            }
            // 重新打开的会话组装时用的那段历史。空流不是一次重放：进入那个状态会为一次什么
            // 都不铺、也不标接缝的操作显示一条进度行（`spec` §2）。
            ConsoleRequest::Replay { events } => {
                if !events.is_empty() {
                    // 重放占着指针（`mouse()` 在重放期间整个早退），所以一次进行中的拖选到
                    // 这里必须作废：它的区域编号指的还是重放前那一帧的屏幕文本
                    // （`.scratch/tui-feedback/spec.md` §5）。
                    self.drag = None;
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
                self.live_event(RenderEvent::diagnostic(wording::renderer_dropped(dropped)));
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
        // 记号区间先于这一下按键同步：吸附与整块删读的就是它，而它得反映此刻的草稿
        // （`.scratch/input-tokens/spec.md` §4）。
        self.sync_tokens();
        // 挂起排在**一切**之前：它是终端层手势，重放、详情覆盖层、问卷、举手都拦不住它
        // （`.scratch/suspend-gesture/spec.md` §2）。真正的终端动作由 [`Tui::run`] 做 ——
        // 这里只置位，跟 `quit` 是同一种「状态机请求、循环执行」的形状。
        if key == Key::CtrlZ {
            self.suspend = true;
            return;
        }
        // 一次拖选最先被 `Esc` 取消：它是一次还没落地的指针手势，而 `Esc` 在任何视图里都是
        // 「撤掉手上这一手」（`.scratch/tui-feedback/spec.md` §5）。
        if key == Key::Esc && self.drag.is_some() {
            self.drag = None;
            return;
        }
        // 重放在别的一切之前就占着键盘 —— 包括详情覆盖层，它在这个阶段不可能开着 ——
        // 因为它的边界是它自己的：`Ctrl-C` 退出，`Ctrl-D` 与 `Esc` 不起作用，而编辑器照常
        // 工作（`spec` §5）。
        if self.replay.is_some() {
            self.replay_key(key);
            return;
        }
        // 文件查看器立着时**独占键盘**（`.scratch/nvim-file-viewer/spec.md` §4）：它是一块
        // 外来的屏幕，除了 `Ctrl-C`，别的键都该原样进去 —— 不然在浮层里按 `j` 会落到草稿上。
        // 它排在重放之后、详情之前：三者互斥，顺序与它们的来路一致。
        if self.file_viewer.is_some() && self.viewer_key(key) {
            return;
        }
        // 选择器立着时**键盘归它**（spec §10）：`j/k` 与上下键移动高亮、回车选中、`Esc`
        // 取消，其余可打印字符不落进草稿。关掉之后键盘原样还回去 —— 与问卷立着时输入区禁言
        // 是同一条纪律。
        if self.picker.is_some() {
            self.picker_key(key);
            return;
        }
        // 详情覆盖层是一个自成一体的视图模式：它立着的时候占着键盘，而它下面的转录冻在
        // 读的人离开的地方（票 02 §4）。问卷立着时也一样 —— 它排在问卷那一支**前面**，
        // 所以 `Esc` / `Ctrl-D` 关掉的是这一层，而下面那道题的作答原样留着
        // （`.scratch/questionnaire-reading/spec.md`）。
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
        // 键盘在左栏时，走树的那几个键归文件页（`.scratch/files-page/spec.md` §5）。它排在
        // 详情覆盖层之后 —— 覆盖层立着时它上面的左栏读不到键盘，`Esc` 先关覆盖层、再还键盘。
        if self.sidebar_keyboard && self.sidebar_key(key) {
            return;
        }
        // 左栏开关（`.scratch/sidebar-toggle/spec.md` §3）：排在两个「独占键盘的视图」之后
        // —— 详情覆盖层与历史重放各自拦得住它 —— 而在清举手之前：它是纯视图手势，不该让
        // 半分钟前那一下退出举手作废；问卷 / `/` 菜单立着时也照常生效。
        // `Ctrl-T`：打开模型/档位的选择器（spec §7）。它排在左栏开关旁边 —— 同样不归任何
        // 视图管，同样忙闲都生效 —— 而选择器立着时它归选择器（上面那一支已经提前返回）。
        //
        // `Ctrl-M` 不能用：它在终端里就是回车（`\r`），crossterm 报成 `KeyCode::Enter`，
        // 与「提交」正面撞车（`map_key` 那里记着同一件事）。
        if key == Key::CtrlT {
            // 讨论会话里的模型是名册里的配置事实，不换：给一句说明。
            self.ask_picker(PickerKind::Model);
            return;
        }
        if key == Key::CtrlO {
            self.sidebar_wanted = !self.sidebar_wanted;
            // 收起来的那一栏不该扣着键盘：下一次打字要落进输入区。
            if !self.sidebar_wanted {
                self.release_sidebar_keyboard();
            }
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
            } else if self.token_menu().is_some() {
                // 记号菜单是屏幕上最小的东西，所以 `Esc` 先关掉它，然后才轮到手扔草稿
                // （spec §6）。它按记号记账：同一个记号里改字会重开（`sync_menu`），同一行
                // 里另一个记号不受牵连。
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
        // 记号菜单立着时，它占着那四个本来会编辑或提交的键：`↑`/`↓` 走过匹配，`Tab` 填进
        // 一个，`Enter` 填进一个并把它发出去（`/`）或只填进去（`@`）。别的都落到编辑器，而
        // 编辑器正是用户继续打字时过滤匹配的地方。
        if let Some(menu) = self.token_menu() {
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
                    // `Tab` 填进高亮的那个候选就停在那里；`Enter` 在 `/` 菜单里填进去**并
                    // 提交**，于是 `/ask` + Enter 跑起菜单指着的那条 skill。裸 `/` 什么都没
                    // 高亮：`Enter` 把它按打出来的样子发出去，循环回一串名字。
                    //
                    // `@` 菜单里 `Enter` **只能接受**：插进去的路径只是句子的一部分，发送
                    // 仍旧是再按一次（spec §2 那处刻意分叉）。
                    let sigil = menu.sigil;
                    self.menu_accept();
                    if key == Key::Enter && sigil == '/' {
                        self.submit();
                    }
                    return;
                }
                _ => {}
            }
        }
        match key {
            Key::Enter => self.submit(),
            // 键盘三键归**当前显示的那一页**（`.scratch/trace-in-main/spec.md` §4）：轨迹视图
            // 搬进主列之后不再是「只吃滚轮」那个例外（推翻 `trace-tab` 用户故事 32）。
            Key::PageUp => self.page_current(true),
            Key::PageDown => self.page_current(false),
            Key::CtrlG => self.current_page_to_bottom(),
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
        self.conversation.to_bottom();
        let line = self.editor.submitted();
        // 「改完再 `@` 它」是索引的主要用法，所以每提交一条消息就在后台重扫一次
        // （`.scratch/input-tokens/spec.md` §1）。位在这里置，遍历由循环去发。
        self.file_scan_wanted = true;
        // 改动页同理：一次提交本身就可能改变 HEAD 附近的读数（§4）。
        self.changes_wanted = true;
        let _ = reply.send(Some(line));
    }

    /// 该发一次遍历了吗：取走那个位。
    ///
    /// 渲染循环每轮问一次；回答 `true` 时它自己起一个 `spawn_blocking` 的遍历，结果再从
    /// [`TuiState::files_loaded`] 回来。
    ///
    /// **一次遍历还在飞就先不发**：`FileIndex` 的 `Loading` 就是那个守卫，于是两次遍历的
    /// 结果不会互相盖掉。提交那一下想重扫、而预热还没回来时，位**留着** —— 等结果落地之后
    /// 的下一轮，这一次重扫才发得出去。
    pub fn take_file_scan(&mut self) -> bool {
        if !self.file_scan_wanted || !self.files.begin() {
            return false;
        }
        self.file_scan_wanted = false;
        true
    }

    /// 一次遍历的结果回来了：换上新的索引，并请一帧 —— 候选列表与「能兑现」的判据都跟着
    /// 它变。
    pub fn files_loaded(&mut self, paths: Vec<std::path::PathBuf>) {
        self.files.loaded(paths);
        // 一次重扫之后，不在索引里的路径**直接丢掉**：留在展开集合里的每一个目录都还在索引
        // 里，于是它下次回到工作区时是收起的，而不是带着上一次的展开状态复活
        // （`.scratch/files-page/spec.md` §1）。
        if let Some(paths) = self.files.paths() {
            let known: std::collections::HashSet<String> = paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            self.files_page.expanded.retain(|path| known.contains(path));
        }
        self.sync_tokens();
        self.dirty = true;
    }

    /// 该发一次 git 取数了吗：取走那个位。
    ///
    /// 与 [`TuiState::take_file_scan`] 同一条分工：状态机只置位，渲染循环真去起子进程，结果
    /// 从 [`TuiState::changes_loaded`] 回来 —— 于是测试不必起任何进程就能走完整条路。
    ///
    /// **一次取数还在飞就先不发**：`loading` 就是那个守卫（形状照 `FileIndex::Loading`），
    /// 位留着，等结果落地之后的下一轮补发 —— 连着几次触发因此合并成一次，而且不丢
    /// （`.scratch/diff-page/spec.md` §4）。
    pub fn take_changes(&mut self) -> bool {
        if !self.changes_wanted || self.changes.loading {
            return false;
        }
        self.changes.loading = true;
        self.changes_wanted = false;
        true
    }

    /// 一次取数的结果回来了。
    ///
    /// 三条分支各说一件事：成功换上读数；**不可用态**在没有上一次读数时才写进页里（有读数
    /// 就保留它，只在提示行说一句）；其它失败一律保留读数 + 提示行回执。
    pub fn changes_loaded(&mut self, outcome: changes::Outcome) {
        self.changes.loading = false;
        match outcome {
            changes::Outcome::Changed(files) => {
                self.changes.files = files;
                self.changes.state = ChangesState::Ready;
                // 新读数可能少了几行：焦点行不能指着一份已经不存在的清单。
                self.changes.rows = changes::rows(&self.changes.files);
                self.changes_clamp_focus();
            }
            changes::Outcome::NotARepo => {
                self.changes_unavailable(ChangesState::NotARepo, wording::changes_not_a_repo());
            }
            changes::Outcome::NoGit => {
                self.changes_unavailable(ChangesState::NoGit, wording::changes_no_git());
            }
            changes::Outcome::Failed => {
                // 有读数就保留它，只在提示行说一句；**一次都还没取到过**时页里那句得跟着
                // 变成「读不出来」—— 一句永远挂着的「正在读取改动…」是骗人。
                if self.changes.state == ChangesState::Ready {
                    self.say(wording::changes_read_failed().to_owned());
                } else {
                    self.changes.state = ChangesState::Failed;
                }
            }
        }
        self.dirty = true;
    }

    /// 这一页此刻只有一句话时的那句；有列表可画时是 `None`（§5 的四句文案）。
    fn changes_note(&self) -> Option<&'static str> {
        match self.changes.state {
            ChangesState::NotAsked => Some(wording::changes_loading()),
            ChangesState::Ready if self.changes.files.is_empty() => Some(wording::changes_empty()),
            ChangesState::Ready => None,
            ChangesState::NotARepo => Some(wording::changes_not_a_repo()),
            ChangesState::NoGit => Some(wording::changes_no_git()),
            ChangesState::Failed => Some(wording::changes_failed()),
        }
    }

    /// 「这里不可用」：**有上一次读数就保留它**，只在提示行说一句 —— 一个正看着 diff 列表的
    /// 人不该因为一次失败的取数看到那一页被换成一句错误（§4）。
    fn changes_unavailable(&mut self, state: ChangesState, note: &str) {
        if self.changes.state == ChangesState::Ready {
            self.say(note.to_owned());
        } else {
            self.changes.state = state;
        }
    }

    /// 把焦点行收回范围内，并保证它落在一个**文件行**上（分组标题不是可点的东西）。
    fn changes_clamp_focus(&mut self) {
        let Some(at) = self.changes.focus else {
            return;
        };
        self.changes.focus = nearest_file_row(&self.changes.rows, at);
    }

    /// `↑` / `↓`：焦点行移到**下一个文件行**（分组标题跳过），两端停住。
    fn changes_move_focus(&mut self, delta: isize) {
        let files: Vec<usize> = self
            .changes
            .rows
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, changes::Item::File(_)))
            .map(|(index, _)| index)
            .collect();
        if files.is_empty() {
            return;
        }
        let next = match self
            .changes
            .focus
            .and_then(|at| files.iter().position(|index| *index == at))
        {
            Some(position) => {
                let moved = (position as isize + delta).clamp(0, files.len() as isize - 1);
                files[moved as usize]
            }
            // 还没有焦点：往下走给第一个文件行，往上走给最后一个（与文件页同一条）。
            None if delta > 0 => files[0],
            None => *files.last().expect("刚判过它非空"),
        };
        self.changes.focus = Some(next);
        self.dirty = true;
    }

    /// `Enter` 或点一行：打开焦点行那份 diff（`.scratch/diff-page/spec.md` §6）。
    ///
    /// 焦点行**只落在文件行上**（`changes_move_focus` 跳过分组标题），所以这里不必再判一次。
    fn open_changes_focus(&mut self) {
        let Some(index) = self.changes.focus else {
            return;
        };
        let Some(changes::Item::File(file)) = self.changes.rows.get(index).cloned() else {
            return;
        };
        self.open_changes_detail(file);
    }

    /// 把一份 diff 打开在详情覆盖层里。
    ///
    /// 正文在**打开那一刻**取，两条路：未跟踪的新文件没有 diff 可比（`git diff HEAD` 对它
    /// 是空输出），当场读盘给全文；已跟踪的要跑一次 `git diff HEAD -- <path>` —— 那是子进程，
    /// 所以弹窗先带着一句「正在读」立起来，结果从渲染循环那条通道回来时填进去。
    ///
    /// 打开方记成 [`DetailOpener::Changes`]：这一页不冻也不还原 —— 它的位置由它自己那份列表
    /// 与焦点拿着。
    fn open_changes_detail(&mut self, file: changes::ChangedFile) {
        // 两块浮层互斥：开着文件查看器时点开一份 diff，先把查看器收掉。
        self.close_file_viewer();
        self.detail_serial += 1;
        let serial = self.detail_serial;
        let untracked = file.kind == changes::Kind::Untracked;
        let title = if untracked {
            // 未跟踪的文件没有 diff 可比，标题因此说清这一点（正文是全文）。
            wording::changes_new_file().to_owned()
        } else {
            file.path.clone()
        };
        // 两条路都走**同一趟取数**：未跟踪那一档在里面直接读盘（快），已跟踪那一档跑一次
        // `git diff`。于是配了 `[ui] diff_viewer` 时两种文件都会交给那个命令 —— 票 06 的口径
        // 是「未跟踪也一视同仁」（`.scratch/diff-page/issues/06-grilling-external-viewer.md`
        // 的边界那一条）。
        let width = layout::plan(self.area, 1, self.sidebar_wanted).detail_text_width() as usize;
        self.diff_reading = Some(DiffReading {
            serial,
            file: file.clone(),
            columns: width,
            sent: false,
        });
        let detail = Detail {
            title,
            // 一份 diff 不是谁说的话：标题用正文档那一档（与树里那一行同一个颜色）。
            color: palette::PLAIN,
            kind: DetailKind::Diff {
                path: file.path,
                body: changes::Body::Pending,
            },
        };
        self.open_detail(detail, width, DetailOpener::Changes);
    }

    /// 看一份 diff 时用哪个外部命令（`[ui] diff_viewer`）。渲染循环拿它去起子进程
    /// （`.scratch/diff-page/spec.md` §8）。
    pub fn diff_viewer(&self) -> &crate::config::DiffViewerSettings {
        &self.facts.diff_viewer
    }

    /// 该发一次 `git diff` 了吗：取走那一次请求（同一次不重复发）。
    ///
    /// 与 [`TuiState::take_changes`] 同一条分工：状态机只记「要读什么」，渲染循环真去起子
    /// 进程，结果从 [`TuiState::diff_loaded`] 回来。第二个返回值是**正文宽** —— 外部工具那一
    /// 档要把它当 `COLUMNS` 传出去（§8）。
    pub fn take_diff_read(&mut self) -> Option<(u64, changes::ChangedFile, usize)> {
        let reading = self.diff_reading.as_mut()?;
        if reading.sent {
            return None;
        }
        reading.sent = true;
        Some((reading.serial, reading.file.clone(), reading.columns))
    }

    /// 那份 diff 拿回来了：填进**还开着的那一个**弹窗。
    ///
    /// 序号对不上（这一份属于上一次打开、或者弹窗已经关了）就丢掉 —— 一个慢回答不许盖掉读的
    /// 人后来打开的东西。`note` 是外部工具没跑成时提示行那一句回执（那时正文已经回退成内置
    /// 那一档）。
    pub fn diff_loaded(&mut self, serial: u64, body: changes::Body, note: Option<String>) {
        if self.diff_reading.as_ref().map(|reading| reading.serial) != Some(serial) {
            return;
        }
        self.diff_reading = None;
        if let Some(note) = note {
            self.say(note);
        }
        let session_dir = self.facts.session_dir.clone();
        let Some(view) = self.detail.as_mut() else {
            return;
        };
        if let DetailKind::Diff { body: slot, .. } = &mut view.detail.kind {
            // 标题右端标出**谁画的**（内置那一档不标）：同一份 diff 在两台机器上长得不一样
            // 时，读的人知道为什么（§8）。底子是打开那一刻算好的那个（未跟踪的是「新文件」）
            // —— 结果只在这一步追加，而一次打开只会回填一次。
            if let changes::Body::External { tool, .. } = &body {
                let base = view.detail.title.clone();
                view.detail.title = format!("{base} · {tool}");
            }
            *slot = body;
        }
        // 正文换了就得按**同一个**宽度重排：弹窗的排版在打开那一刻就完成了。
        view.body = detail_body(&view.detail, &session_dir, view.width);
        self.dirty = true;
    }

    /// 一次点击落在改动页上吗：落在**文件行**上就把焦点放到那一行、并打开那份 diff，落在分组
    /// 标题上什么都不做。回答 `true` 表示这一下归这一页。
    fn changes_click(&mut self, column: u16, row: u16) -> bool {
        if self.tab != Tab::Changes || !self.changes_page_contains(column, row) {
            return false;
        }
        if let Some(index) = self.changes_index_at(row) {
            self.changes.focus = Some(index);
            self.open_changes_focus();
        }
        true
    }

    /// 这一格是不是落在改动页的页区里。
    fn changes_page_contains(&self, column: u16, row: u16) -> bool {
        self.changes
            .rect
            .is_some_and(|page| page.contains((column, row).into()))
    }

    /// 屏幕行 `row` 对应的列表行下标。
    ///
    /// 这一页**没有滚动**，所以它就是「离页区顶端几行」。落在页区之外、或者那一行是分组标题
    /// 时是 `None` —— 标题没有可点的东西。
    fn changes_index_at(&self, row: u16) -> Option<usize> {
        let page = self.changes.rect?;
        if row < page.y {
            return None;
        }
        let index = (row - page.y) as usize;
        matches!(self.changes.rows.get(index), Some(changes::Item::File(_))).then_some(index)
    }

    /// 左栏当前那一页的页区；没画、或者这一页不是能走键盘的那两页时是 `None`。
    fn sidebar_page_rect(&self) -> Option<Rect> {
        match self.tab {
            Tab::Files => self.files_page.rect,
            Tab::Changes => self.changes.rect,
            _ => None,
        }
    }

    /// 把草稿里**能兑现**的记号区间算好同步进编辑器（`.scratch/input-tokens/spec.md` §4）。
    ///
    /// 判据只有一条，上色与 chip 共用它：`/` 的记要在命令表里命中、`@` 的记要在索引里
    /// 命中。于是打字过程中一块记号会忽隐忽现，补全成功那一刻才凝固 —— 而 `Input` 继续
    /// 不认识命令表与文件系统，它只拿到区间与样式。
    fn sync_tokens(&mut self) {
        let spans = token::tokens(self.editor.text())
            .into_iter()
            .filter_map(|token| {
                let style = match token.prefix {
                    '/' if self.catalog.iter().any(|entry| entry.name == token.query) => {
                        Style::default().fg(TOKEN_COMMAND)
                    }
                    '@' if self.files.contains(&token.query) => {
                        Style::default().fg(TOKEN_REFERENCE)
                    }
                    _ => return None,
                };
                Some(editor::TokenSpan {
                    start: token.start,
                    end: token.end,
                    style,
                })
            })
            .collect();
        self.editor.set_token_spans(spans);
    }

    /// 这个会话的工作目录 —— 索引以它为根，索引里的路径都相对它。
    pub fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    /// 此刻草稿所要求的那个记号菜单，没什么可提供时是 `None`。
    ///
    /// 推出来的，从不保存：草稿与候选来源就是全部输入。有问题立着时什么都不显示，因为
    /// 问题占着键盘 —— 菜单会提供一些回答别的东西的键（spec §9）。
    ///
    /// 候选来源**按前缀挑**：`/` 是循环报上来的命令表，`@` 是会话级的文件索引
    /// （`.scratch/input-tokens/spec.md` §2、§3）。一套浮层、一套键位，只有来源不同。
    fn token_menu(&self) -> Option<TokenMenu> {
        if self.pending.is_some() || self.slash.dismissed {
            return None;
        }
        let token = self.editor.token()?;
        let entries: Vec<MenuEntry> = match token.prefix {
            '/' => {
                // 按大小写过滤，但提供的是目录里的那个名字：`/Ask` 找得到 `ask-matt`，
                // 而 Tab 写出循环会认的那个拼法。
                let typed = token.query.to_lowercase();
                self.catalog
                    .iter()
                    .filter(|entry| entry.name.to_lowercase().starts_with(&typed))
                    .map(|entry| MenuEntry {
                        name: entry.name.clone(),
                        description: entry.description.clone(),
                        kind: entry.kind.into(),
                    })
                    .collect()
            }
            '@' => {
                // **裸 `@` 不列候选**：命令表只有几十条，「裸 `/` 是一份用来看的列表」成立，
                // 而文件有几千个，「看全部」没有意义。索引未就绪时同样什么都不显示 ——
                // 预热让它几乎不可能被看见。
                if token.query.is_empty() {
                    return None;
                }
                self.files
                    .candidates(&token.query, TOKEN_MENU_CANDIDATES)
                    .into_iter()
                    // 目录下钻之后不必再列它自己（`@src/` 的第一行不再是 `@src/`）；文件
                    // 候选即使名字正好等于已经打完的那一段也照列 —— 那时菜单本来就要没了，
                    // 藏起来只会让「打全即消失」显得像出错。
                    .filter(|path| !(path.ends_with('/') && path == &token.query))
                    .map(|name| MenuEntry {
                        name,
                        description: String::new(),
                        kind: MenuKind::Path,
                    })
                    .collect()
            }
            _ => return None,
        };
        if entries.is_empty() {
            return None;
        }
        Some(TokenMenu {
            sigil: token.prefix,
            query: token.query,
            selected: self.slash.selected.map(|at| at.min(entries.len() - 1)),
            entries,
        })
    }

    /// 把高亮移动 `delta`，两端回绕。
    fn menu_move(&mut self, delta: isize) {
        let Some(menu) = self.token_menu() else {
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
        self.slash.token = self.menu_key();
    }

    /// 把高亮的那个候选填进草稿。
    ///
    /// 什么都没高亮时什么都不做 —— 裸 `/` 是一份用来看的列表，不是已经做出的选择。
    ///
    /// **接受的是不是目录**在这里分叉：文件与命令插完就关菜单，而目录插完**保持开着**，
    /// query 换成新的前缀、接着过滤那一层（spec §3）。判据就是那个尾随斜杠 —— 索引里的
    /// 目录候选正是这么写的。
    fn menu_accept(&mut self) {
        let Some(menu) = self.token_menu() else {
            return;
        };
        let Some(selected) = menu.selected else {
            return;
        };
        let name = menu.entries[selected].name.clone();
        if !self.editor.complete_token(menu.sigil, &name) {
            return;
        }
        // 命令与**文件**补完之后再补一个空格：记号到此为止，接着写下一样东西（这一段任务、
        // 下一句话），不必自己记得敲那个分隔。**目录不补** —— 补全它要的是「钻进去」，而
        // 一个空格会把记号当场结束在目录上，下一层就过滤不出来了。后面已经是一个空白时也
        // 不补：那会写出两个连着的空格。
        if !name.ends_with('/') && !self.editor.next_char().is_some_and(char::is_whitespace) {
            self.editor.insert_char(' ');
        }
        // 记住的记号随草稿一起走，否则下一次同步会把这次补全读成一次变化，并把刚刚关掉的
        // 东西重新打开。补过空格的记号光标在它**后面**，记账因此落到 `None`，正合「补完了」。
        self.slash.token = self.menu_key();
        if name.ends_with('/') {
            // 目录：菜单接着列这一层，所以它不关，高亮回到第一个候选。
            self.slash.selected = Some(0);
            self.slash.dismissed = false;
        } else {
            self.slash.selected = None;
            self.slash.dismissed = true;
        }
        // 补全成功那一刻记号就凝固：整段换成能兑现的那一个，区间与颜色当场刷新。
        self.sync_tokens();
    }

    /// 此刻光标所在的那个记号的记账键；草稿里没有记号时是 `None`。
    fn menu_key(&self) -> Option<MenuKey> {
        self.editor.token().map(|token| MenuKey {
            sigil: token.prefix,
            start: token.start,
            query: token.query,
        })
    }

    /// 把草稿当前的记号收进菜单记住的那个选择里。
    ///
    /// 换了**记号**（前缀、位置或 query 任一变）就是另一个菜单：高亮从头开始 —— 打过名字
    /// 之后落在第一个匹配上，而记号只是一个前缀字符时落在什么都不高亮上 —— 而那次关掉旧
    /// 菜单的 `Esc` 不再作数。位置也在键里，所以同一行里两个名字相同的记号各记各的账。
    fn sync_menu(&mut self) {
        let key = self.menu_key();
        if key == self.slash.token {
            return;
        }
        self.slash.selected = key
            .as_ref()
            .is_some_and(|key| !key.query.is_empty())
            .then_some(0);
        self.slash.dismissed = false;
        self.slash.token = key;
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
                    self.sync_tokens();
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

    /// 提示行的文字：键位提示与出口。**状态词不在这里** —— 它在状态行的最后一段
    /// （`.scratch/tui-visual-language/spec.md` §18）。
    fn hint_line(&self, width: u16) -> String {
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
        // `discuss` —— `enter 发送` 会是一个这个会话兑现不了的承诺（spec §6）。复制的回执
        // 排在提示集合**之前**（`.scratch/tui-feedback/spec.md` §6），而出口那一段由
        // `wording::hint_row` 保住 —— 回执不许把它挤掉。打开的回执排在同一位、更靠前一点：
        // 两者同时新鲜是几乎不可能的事，真撞上了就让更重的那件事说话
        // （`.scratch/clickable-links/spec.md` §4）。
        let receipt = self
            .notice_line()
            .or_else(|| self.open_receipt())
            .or_else(|| self.copy_receipt());
        if self.prompt_reply.is_some() {
            wording::status_line_with(receipt.as_deref(), self.busy(), width, raised)
        } else {
            wording::viewer_status_line_with(receipt.as_deref(), self.busy(), width, raised)
        }
    }

    /// 最近一次复制的那句回执，还在寿命内的话。
    ///
    /// 寿命由既有的脉冲 tick 走着 —— 不为它另起一个时钟（`.scratch/tui-feedback/spec.md` §6）。
    fn copy_receipt(&self) -> Option<String> {
        copy_receipt(self.copied, std::time::Instant::now())
    }

    /// 最近一次打开的那句回执（成功或失败），还在寿命内的话。
    fn open_receipt(&self) -> Option<String> {
        open_receipt(self.opened.as_ref(), std::time::Instant::now())
    }

    /// 状态行那一格显示的档位：表为空时是 `固定`，而「默认」那**一档**是 `None`
    /// （spec §5、§8）。
    fn effort_label(&self) -> String {
        if self.fixed_effort() {
            return wording::effort_fixed().to_owned();
        }
        self.effort
            .map(|effort| effort.as_str().to_owned())
            .unwrap_or_else(|| wording::effort_default().to_owned())
    }

    /// 当前模型有没有档位可选。没有就是「固定」：思考常开、模型不收那个参数。
    fn fixed_effort(&self) -> bool {
        caps_for(&self.facts.model).is_ok_and(|caps| caps.reasoning_efforts.is_empty())
    }

    /// 在提示行上留一句短说明。
    ///
    /// 与复制/打开链接那两句回执同一种形状与同一段寿命：提示行一次只留一句最新的，而过期
    /// 的那句在 [`TuiState::tick`] 里被清掉（不用另起一个时钟）。
    fn say(&mut self, text: String) {
        self.notice = Some((std::time::Instant::now(), text));
    }

    /// 提示行上那一句说明，还在寿命内的话。
    fn notice_line(&self) -> Option<String> {
        notice_line(self.notice.as_ref(), std::time::Instant::now())
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
                .height(layout::content_width(area, self.sidebar_wanted)),
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
/// 表头与问题文本尽量钉住：它们说在问什么，把它们滚掉会让选项变得读不懂。打答案的那几行
/// 出于同样的理由钉在底部。中间那些选项就是窗口，它跟着高亮走：往下越过裁剪线时，滚出来
/// 的是尾部，而不是把高亮留在屏幕外。
///
/// 三条一起放不下时，**让位的是题面**（[`QUESTION_FLOOR_ROWS`]）：一份二十行的题面不该把
/// 选项与打答案的那一行挤得一行不剩。题面被削时它自己会用一行说明削掉了多少。
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
    // 题面按几何留下的行数收口，然后选项窗口取它那一段，最后是打答案的那几行 —— 三种形态都
    // 按 `height` 收口，于是输入区折出来的续行既不会被截成一行，也不会画到窗格外面去
    // （`.scratch/questionnaire-keys/spec.md` §7）。
    let mut rows = clip_question(prefix, geometry.prefix_rows);
    rows.extend(
        options
            .into_iter()
            .skip(geometry.start)
            .flatten()
            .take(geometry.room),
    );
    rows.extend(custom);
    rows.truncate(height);
    rows
}

/// 题面留下 `kept` 行；被削掉的那些换成一行说明。
///
/// 说明行自己占掉留下的最后一格，所以「还有几行」是剩下没画出来的全部行数 —— 一句话说完，
/// 而不是让读者自己数一道被切断的句子。只剩一行可留时留的是**题面自己那一行**：那种尺寸下
/// 一行题目比一行「还有几行没显示」有用。
fn clip_question(prefix: Vec<Line<'static>>, kept: usize) -> Vec<Line<'static>> {
    if prefix.len() <= kept {
        return prefix;
    }
    if kept <= 1 {
        return prefix.into_iter().take(kept).collect();
    }
    let mut rows = prefix;
    let hidden = rows.len() - (kept - 1);
    rows.truncate(kept - 1);
    rows.push(Line::from(Span::styled(
        wording::questionnaire_question_clipped(hidden),
        Style::default().fg(palette::MUTED),
    )));
    rows
}

/// 选项窗口的几何：题面留下几行、留给窗口几行、第一个该画的选项。
///
/// 画的那一遍与登记点击区域的那一遍各自要一次，所以它只算在这一处 —— 两边不会对「窗口从
/// 哪儿开始」有分歧（`.scratch/questionnaire-keys/spec.md` §7）。
struct OptionWindowGeometry {
    /// 题面（表头与题目正文）留下几行 —— 装得下时就是它的全部行数。
    prefix_rows: usize,
    /// 留给窗口的行数。
    room: usize,
    /// 第一个该画的选项。
    start: usize,
}

fn option_window_geometry(
    options: &[Vec<Line<'static>>],
    highlight: usize,
    height: usize,
    question_rows: usize,
    custom_rows: usize,
) -> OptionWindowGeometry {
    let heights: Vec<usize> = options.iter().map(Vec::len).collect();
    let total: usize = heights.iter().sum();
    // 三样的优先级是：打答案的那几行 > 选项窗口的地板 > 题面。前面的都是「没有就问不出
    // 答案」，而题面少读几行仍然答得了 —— 所以一起放不下时先削它
    // （`.scratch/questionnaire-keys/spec.md` §7 的补记）。
    let floor = total.min(OPTION_FLOOR_ROWS);
    let wanted = question_rows + custom_rows + floor;
    let mut prefix_rows = question_rows;
    if wanted > height {
        let give = (wanted - height).min(question_rows.saturating_sub(QUESTION_FLOOR_ROWS));
        prefix_rows = question_rows - give;
    }
    let room = height.saturating_sub(prefix_rows + custom_rows);
    let start = if prefix_rows + total + custom_rows <= height {
        0
    } else {
        option_window_start(&heights, highlight, room)
    };
    OptionWindowGeometry {
        prefix_rows,
        room,
        start,
    }
}

/// 选项窗口在底部区里至少占的行数：题面再长也要给它留下这几行，否则读者对着一份没有选项的
/// 问卷，只能看见页脚在教他怎么选（`.scratch/questionnaire-keys/spec.md` §7 的补记）。
const OPTION_FLOOR_ROWS: usize = 3;

/// 题面被削时至少留下的行数：一行题面也比一行都没有强 —— 至少要知道在问什么。
const QUESTION_FLOOR_ROWS: usize = 1;

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
            // 表头归正文档 + `BOLD`（§29）。
            row.style = Style::default()
                .fg(palette::PLAIN)
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
            (true, true) => wording::CHOICE_CHECKED,
            (true, false) => wording::CHOICE_UNCHECKED,
            (false, true) => wording::CHOICE_PICKED,
            (false, false) => wording::CHOICE_UNPICKED,
        };
        // 光标说的是 `Enter`/`Space` 会确认哪个选项；标记说的是哪些被选中了。这是两个不同的
        // 事实，可以不一致。
        let cursor = if highlighted { ">" } else { " " };
        let lead = format!("{cursor} {marker} ");
        let body =
            wording::questionnaire_option(index + 1, &choice.label, choice.description.as_deref());
        let mut style = if picked {
            // 已选不用颜色：`BOLD` 与标记（`[x]` / `●`）已经说了这件事（§29）。
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        if highlighted && options_focused {
            // 反显说的只有一件事：键盘在这里。全屏的 `REVERSED` 永远只允许有一个（§29）。
            style = style.add_modifier(Modifier::REVERSED);
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
    // 键盘在输入区时反显的就是这一行：于是「键盘在哪」永远只有一个答案，`DIM` 退场
    // （§29）。
    let lead_style = if options_focused {
        Style::default().fg(palette::MUTED)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
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
    // 屏幕文本跟着一起重建：拖选要知道这一帧每块区域画了什么，而上一帧的内容不该接得住指针
    // （`.scratch/tui-feedback/spec.md` §5）。
    state.screen_text.clear();
    // 覆盖层「在哪儿」与命中区域同一条纪律：这一帧画在哪儿，指针才可能落在哪儿
    // （`tui-chrome` §5）。两者都在下面各自画出来时被重新填上。
    state.modal_rect = None;
    state.questionnaire_bottom = None;
    // 两个视图的矩形与指示器同理：这一帧真画了才有值，指针只回应真看见的东西（票 09）。
    // 主列页签让同一时刻只有一页在屏幕上，所以没被画出来的那一页必须失去它的命中区域 ——
    // 否则点击会落在上一帧留下的位置上（`.scratch/trace-in-main/spec.md` §4）。
    state.trace_rect = None;
    state.conversation_rect = None;
    state.trace_indicator = None;
    state.indicator = None;
    if layout::below_minimum(area) {
        // 什么都不画，好让点击无处可落。
        state.indicator = None;
        state.detail_rect = None;
        // 查看器是一块**进程**，看不见它时留着没有意义（详情覆盖层不一样：它不占别的资源，
        // 收起来反而会丢掉读的人翻到的那一页）—— 收掉，别把键盘扣在看不见的东西上。
        state.close_file_viewer();
        draw_too_small(frame, area);
        return;
    }
    // 草稿自己的高度决定输入区占多少位置：它随文字长高、直到排版的上限，然后就地滚动
    // （spec §2）。问卷用自己的高度替换掉它，于是输入区长高到装下问题（spec §19）—— 而那条
    // 上限也是问卷自己的：一份题面与选项要一起放下来的问卷，不该被草稿的十行夹住。
    let content_rows = state.bottom_rows(area);
    let panes = if state.questionnaire().is_some() {
        layout::plan_questionnaire(area, content_rows, state.sidebar_wanted)
    } else {
        layout::plan(area, content_rows, state.sidebar_wanted)
    };
    // 两个视口各自的源行宽度，在排版之后、画任何东西之前定下来：两个视图都画在主列那块
    // 内容区里，所以宽度同源，而且都**常驻** —— 与现在显示哪一页无关，切页才不会漏内容
    // （`.scratch/trace-in-main/spec.md` §3）。
    let conversation_width = panes.transcript_text().width;
    let trace_width = conversation_width;
    // 前缀分档量的是主列**页**的宽度：主列最小 40 列（终端下界就是它），所以轨迹视图恒为
    // 宽档、恒带方括号（`.scratch/trace-in-main/spec.md` §3）。
    state.trace_tier_width = panes.main.width;
    state.rerender_if_width_changed(conversation_width, trace_width);
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
    // 选择器浮层：与详情覆盖层一样盖在上面，而它与详情**不可能**同时立着（收到清单时详情
    // 已经关掉，见 `TuiState::request`）。
    if let Some(picker) = state.picker.as_mut() {
        draw_picker(frame, &panes, picker);
    }
    // 详情覆盖层盖在所有这一切之上。它**可以**与一个问题同时立着 —— 问卷立着时点一行就
    // 开（`.scratch/questionnaire-reading/spec.md`），而它的底边被 [`layout::Regions::overlay_floor`]
    // 挡在问卷上面，所以那道题始终看得见。
    draw_detail(frame, &panes, state);
    draw_file_viewer(frame, &panes, state);
    // 拖选的这一层反白画在最后：它只碰缓冲，所以它盖在所有东西之上，而没有任何绘制函数
    // 知道它存在（`.scratch/tui-feedback/spec.md` §5）。
    selection::paint(frame, &state.screen_text, state.drag.as_ref());
}

/// 把一帧落到终端上，并收好光标的**可见性**。
///
/// 位置在 [`draw_frame`] 里、可见性在这里，分成两处是因为 ratatui 的 `Frame` 只有「光标在
/// 哪」这一个旋钮：给了位置就一定 `Show`。于是「暗」的那一半不能靠不设位置来实现 —— 不设
/// 位置就是 `Hide`，而 `Hide` 不移动光标，终端会把它留在最后写入的那一格（空闲时那是状态
/// 行的月相），输入法的预编辑于是长在 `🌑` 与「就绪」之间。`draw_frame` 把位置钉在输入区
/// 那个光标上，这里只负责把不该露面的那一半收起来。
pub fn paint_frame<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    state: &mut TuiState,
) {
    let _ = terminal.draw(|frame| draw_frame(frame, state));
    if keyboard_in_the_input(state) && !blink_on(state.pulse) {
        let _ = terminal.hide_cursor();
    }
}

/// 外壳里那些不是自己一块区域的部件：分隔列、左栏，以及主列的两条分隔线。
///
/// **外框已经不在**（spec §1）：每条线都画在自己该在的地方，没有哪一格要留给边框，也没有
/// 交叉符要拼。这个顺序就是绘制顺序：先沿左栏右边缘往下画的分隔列，然后是左栏 —— 它的页签
/// 条在两者之上画 —— 再是主列的页签条（两者可能落在同一个屏幕行上，但跨的列不相交），最后是
/// 主列的分隔线（spec §1–§2；`.scratch/trace-in-main/spec.md` §1）。
fn draw_shell(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
    area: Rect,
) {
    draw_divide(frame, panes, area);
    draw_sidebar(frame, panes, state);
    draw_main_tab_bar(frame, panes, state);
    // 输入区与提示行各自上面那条分隔线 —— 状态行上方那条已经离开（spec §2），所以这里是
    // 两条而不是三条。两条都**只画在主列里**：提示行回到主列之后，左栏与它们无关了
    // （`.scratch/tui-feedback/spec.md` §2）。左栏不存在时主列就是整屏，与改动前逐字相同。
    let (left, right) = (panes.main.x, panes.main.right());
    for y in [panes.input.y - 1, panes.hints.y - 1] {
        paint_rule(frame, y, left, right);
    }
}

/// 一行横贯某个行、从 `left` 到 `right`（不含 `right`）的分隔线（spec §1、§3）。
///
/// 虚线，颜色是 [`palette::CHROME`]：框架退到内容后面。主列的分隔线与页签条用的是同一笔；它们
/// 只在跨的列上不同。
fn paint_rule(frame: &mut ratatui::Frame, y: u16, left: u16, right: u16) {
    let style = Style::default().fg(palette::CHROME);
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
    let style = Style::default().fg(palette::CHROME);
    let buffer = frame.buffer_mut();
    // 竖虚线跟左栏同高：画到屏幕最后一行（`.scratch/tui-feedback/spec.md` §2 —— 提示行回到
    // 主列之后，左栏不再为它让出底下那一行）。
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
        // 整栏没画出来：那两块命中矩形也跟着失效，否则一次点击会落到一条不在屏幕上的栏上。
        state.tabs_rect = None;
        state.files_page.rect = None;
        return;
    };
    draw_sidebar_identity(frame, panes, state, sidebar);
    draw_tab_bar(frame, panes, state, sidebar, tabs);
    draw_sidebar_page(frame, panes, state);
}

/// 左栏的身份：两版标记之一、文字身份，或者什么都没有（spec §3）。
///
/// 选哪一个是排版的决定 —— [`layout::SidebarKind`] —— 于是那条阶梯只有一个家。标记在
/// 宽档里居中，那档宽度就是标记自己的宽度加左右各一列留白。标记有大小两版：块字放得下就画
/// 块字，放不下退回收起来的那一版（`wording::logo_lines_compact`），这个选择同样只由排版做。
///
/// 标记是**静止的，除非有一次运行在进行中**：那时每帧给它一束从右下扫到左上的反光
/// （`.scratch/mark-sweep/spec.md` §2）。`sweep` 由 [`TuiState::busy`] 与脉冲那一个计数器
/// 拼出来 —— 循环只在一次运行里说 `busy`，而 `RunState` 的两个边沿都把计数器归零，所以一次
/// 运行总是从光带在右下角进场那一刻开始扫。歇着的时候这里是 `None`：一块颜色都不动。
fn draw_sidebar_identity(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &TuiState,
    sidebar: Rect,
) {
    let dim = Style::default().fg(palette::MUTED);
    match panes.sidebar_kind {
        kind @ (layout::SidebarKind::Mark | layout::SidebarKind::MarkCompact) => {
            // 两版标记都是 38 列，而宽档是 40 列，所以它居中时左右各留一列白；比标记还窄的档位
            // 根本不会要这几行（spec §2）。
            let offset = sidebar.width.saturating_sub(layout::LOGO_WIDTH) / 2;
            let sweep = state.busy().then_some(state.pulse);
            let lines: Vec<Line<'static>> = mark_lines(kind, sweep)
                .into_iter()
                .map(|spans| {
                    Line::from(
                        spans
                            .into_iter()
                            .map(|(text, color)| Span::styled(text, Style::default().fg(color)))
                            .collect::<Vec<_>>(),
                    )
                })
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
            // 静止的身份：窄档那一半下落短横随标记的一起退出了屏幕（票 08），而它的代码
            // 也随本 effort 的死代码收口一起删掉（`.scratch/tui-visual-language/spec.md` §34）。
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
///
/// 轨迹不在这里：它搬进了主列，成了那条页签条上的第二页
/// （`.scratch/trace-in-main/spec.md` §2）。
fn draw_sidebar_page(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let Some(page) = panes.sidebar_page else {
        return;
    };
    let rows = match state.tab {
        Tab::Usage => state.panel.lines(&state.facts, page),
        // `todo` 自己画：它要记下自己的矩形（整页可点，`.scratch/todo-page/spec.md` §5），
        // 也要把**折出来的续行**标给屏幕文本层 —— 否则一条折行的待办复制出来会多几个换行（§8）。
        Tab::Todo => {
            draw_todo_page(frame, page, state);
            return;
        }
        Tab::Files => {
            draw_files_page(frame, page, state);
            return;
        }
        // 改动页同理：一行的语义（哪一行是个文件、哪一行是分组标题）只在画的时候才算得出来。
        Tab::Changes => {
            draw_changes_page(frame, page, state);
            return;
        }
    };
    state.files_page.rect = None;
    // 这一帧没画改动页：指针于是不该再落在一页已经不在屏幕上的东西上（与文件页同一条
    // 「只认真画出来的东西」）。
    state.changes.rect = None;
    // 同理是这一页：停在 `调用量` 上时，点左栏不该开出一份清单。
    state.todo.clear_rect();
    // `调用量` 那一块也进屏幕文本层：它是「一页已经排好的行」，没有软折可言
    // （`.scratch/tui-feedback/spec.md` §5）。
    note_rows(state, page, &rows, &[], &[]);
    frame.render_widget(Paragraph::new(rows), page);
}

/// 左栏的 `todo` 页：页顶一条进度行（末行右端留着那个按钮）、下面是全部未完成的项
/// （`.scratch/todo-page/spec.md` §2–§5）。
fn draw_todo_page(frame: &mut ratatui::Frame, page: Rect, state: &mut TuiState) {
    state.todo.set_rect(page);
    state.files_page.rect = None;
    state.changes.rect = None;
    let (rows, folded) = state.todo.lines_with_folds(page);
    note_rows(state, page, &rows, &folded, &[]);
    frame.render_widget(Paragraph::new(rows), page);
}

/// 文件页：一棵从会话级索引排出来的工作区文件树
/// （`.scratch/files-page/spec.md` §1、§3）。
///
/// 这一页**不给色**：目录与文件只靠结构区分 —— 缩进、折叠字形、尾斜杠 —— 所以每一行都是
/// 正文档那一个默认样式。索引还没就绪与工作区真的是空的各说各的，都不画一块空白。
fn draw_files_page(frame: &mut ratatui::Frame, page: Rect, state: &mut TuiState) {
    state.files_page.rect = Some(page);
    // 这一帧画的是文件页：改动页那一块不再是在屏幕上的东西。
    state.changes.rect = None;
    let Some(tree) = files_tree(state) else {
        return draw_sidebar_note(frame, page, wording::files_loading(), state);
    };
    if tree.is_empty() {
        return draw_sidebar_note(frame, page, wording::files_empty(), state);
    }
    let window = page.height as usize;
    let top = state
        .files_page
        .scroll
        .min(tree.len().saturating_sub(window));
    state.files_page.scroll = top;
    let lines: Vec<Line<'static>> = tree
        .iter()
        .enumerate()
        .skip(top)
        .take(window)
        .map(|(index, row)| {
            files_line(
                row,
                page.width as usize,
                state.files_page.focus == Some(index),
            )
        })
        .collect();
    // 记的是**全部**可见行，不只是窗口里那几行：一次点击是拿屏幕行换成行下标，
    // 而键盘（下一张票）还要能在窗口之外走动。
    state.files_page.rows = tree;
    note_rows(state, page, &lines, &[], &[]);
    frame.render_widget(Paragraph::new(lines), page);
}

/// 这一帧的树；索引还没就绪（或一次遍历正在飞）时是 `None`。
///
/// 展开状态活在 `TuiState` 里，而 `files::rows` 拿它当输入 —— 于是「哪些行看得见」是一次
/// 纯计算，没有第二份状态要同步。
fn files_tree(state: &TuiState) -> Option<Vec<files::Row>> {
    let paths = state.files.paths()?;
    Some(files::rows(paths, &state.files_page.expanded))
}

/// 树的一行：**缩进 + 字形列 + 名字**（目录带尾斜杠）。
///
/// 字形列每行都占两格 —— 目录那里是 `▸ `，文件那里**留空**。于是同层的名字落在同一列上，
/// 扫读时眼睛不必每一行各自往前挪两格。窄档（28 列）下这一格的代价是两列名字。
/// 名字超宽时截断 —— 28 列的窄档是这一页最容易读不下去的地方。
fn files_line(row: &files::Row, width: usize, focused: bool) -> Line<'static> {
    let glyph = if row.dir {
        let mark = if row.expanded {
            wording::UNFOLDED
        } else {
            wording::FOLDABLE
        };
        format!("{mark} ")
    } else {
        // 这一格是**字形列**，不是「有折叠能力」的标记：文件行留空，只是为了与同层的目录行
        // 严格同列，而不是说自己少了个字形。
        wording::INDENT.to_owned()
    };
    let slash = if row.dir { "/" } else { "" };
    let text = format!(
        "{}{glyph}{}{slash}",
        wording::INDENT.repeat(row.depth),
        row.name
    );
    // 「过程退后、内容保持、信号着色」：目录与文件之间只靠结构区分，而**焦点行**是信号
    // —— 它用常驻选中那一档（`ACCENT` + `BOLD`），与页签条的选中同一个档
    // （`.scratch/files-page/spec.md` §3）。
    let style = if focused {
        Style::default()
            .fg(palette::ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette::PLAIN)
    };
    // 放不下就用一个 `…` 收尾（而不是悄悄少几个字）：28 列的窄档里，读的人要知道这一行
    // 还有下文。
    if text_columns(&text) > width {
        return ellipsize_line(Line::from(Span::styled(text, style)), width);
    }
    Line::from(Span::styled(text, style))
}

/// 一页只有一句话时的那一行：静音、截到页宽。文件页的两个空态走它 ——
/// 「在等数据」与「读完了，就是空的」是两句话，但形状是同一种。
fn draw_sidebar_note(frame: &mut ratatui::Frame, page: Rect, note: &str, state: &mut TuiState) {
    state.files_page.rows.clear();
    let line = draw_page_note(frame, page, note);
    note_rows(state, page, std::slice::from_ref(&line), &[], &[]);
}

/// 一页只有一句话时的那一行本体。
///
/// 与 [`draw_sidebar_note`] 分开，是因为「页里写一句」这件事本身不该替哪一页做主 —— 那一位
/// 属于各自清理自己那份行状态的调用方（文件页清 `rows`，改动页清它自己那份）。
fn draw_page_note(frame: &mut ratatui::Frame, page: Rect, note: &str) -> Line<'static> {
    let line = Line::from(Span::styled(
        truncate_columns(note, page.width as usize),
        Style::default().fg(palette::MUTED),
    ));
    frame.render_widget(Paragraph::new(line.clone()), page);
    line
}

/// 改动页：相对 HEAD 改过的那些文件，按状态分好组
/// （`.scratch/diff-page/spec.md` 实现决定 §5）。
///
/// 这一页**不给色**：字形与分组标题都是正文档那一档，层次只交给结构与字形
/// （「过程退后、内容保持、信号着色」）；唯一带色的是焦点行 —— 那是信号。页区**没有滚动**：
/// 装不下就画满，末行说一句还有多少。
fn draw_changes_page(frame: &mut ratatui::Frame, page: Rect, state: &mut TuiState) {
    state.changes.rect = Some(page);
    // 这一帧画的是改动页：文件页那一块不再是在屏幕上的东西。
    state.files_page.rect = None;
    if let Some(note) = state.changes_note() {
        state.changes.rows.clear();
        let line = draw_page_note(frame, page, note);
        note_rows(state, page, std::slice::from_ref(&line), &[], &[]);
        return;
    }
    let rows = changes::rows(&state.changes.files);
    let total = state.changes.files.len();
    let window = page.height as usize;
    // 装不下就画满页区，最后一行留给那一句「还有 M 处改动」（§5）。**不给这一页加滚动**：
    // 那是新机制，等真机反馈再谈。
    let overflow = rows.len() > window;
    let shown = if overflow {
        window.saturating_sub(1)
    } else {
        rows.len()
    };
    let mut lines: Vec<Line<'static>> = rows
        .iter()
        .take(shown)
        .enumerate()
        .map(|(index, item)| {
            changes_line(
                item,
                page.width as usize,
                state.changes.focus == Some(index),
            )
        })
        .collect();
    if overflow {
        let drawn = rows
            .iter()
            .take(shown)
            .filter(|item| matches!(item, changes::Item::File(_)))
            .count();
        lines.push(Line::from(Span::styled(
            wording::changes_more(total.saturating_sub(drawn)),
            Style::default().fg(palette::MUTED),
        )));
    }
    // 焦点与点击要拿屏幕行换下标，所以整份可见行都留着（不只是窗口里那几行）。
    state.changes.rows = rows;
    note_rows(state, page, &lines, &[], &[]);
    frame.render_widget(Paragraph::new(lines), page);
}

/// 从 `rows` 的第 `from` 行起找最近的一个**文件行**：先往后找，找不到再回头往前找。整份清单
/// 里一个文件都没有（只有标题，理论上不会发生）时是 `None`。
fn nearest_file_row(rows: &[changes::Item], from: usize) -> Option<usize> {
    if rows.is_empty() {
        return None;
    }
    let from = from.min(rows.len() - 1);
    (from..rows.len())
        .chain((0..from).rev())
        .find(|index| matches!(rows[*index], changes::Item::File(_)))
}

/// 改动页上的一行：分组标题占整行，文件行是**字形（占满两格）+ 空格 + 全路径**。
///
/// 字形占满两格，于是名字严格落在同一列上（与文件页那条真机反馈后的规矩同源：那一列每行都
/// 占满，扫读时眼睛不必重新找列）。路径超宽用 `…` 收尾 —— 28 列的窄档里那是唯一读得下去
/// 的写法。
fn changes_line(item: &changes::Item, width: usize, focused: bool) -> Line<'static> {
    let style = if focused {
        // 常驻选中那一档（`.scratch/tui-visual-language/spec.md` §8），与文件页同一个。
        Style::default()
            .fg(palette::ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette::PLAIN)
    };
    let text = match item {
        changes::Item::Header(kind) => changes_header(*kind).to_owned(),
        changes::Item::File(file) => {
            let glyph = changes_glyph(file.kind);
            // 字形**占满两格**，然后才是那个分隔的空格 —— 于是 `M` / `A` / `D` / `??` 四种
            // 字形之后的名字严格同列。
            let pad = 2usize.saturating_sub(text_columns(glyph));
            format!("{glyph}{} {}", " ".repeat(pad), file.path)
        }
    };
    if text_columns(&text) > width {
        return ellipsize_line(Line::from(Span::styled(text, style)), width);
    }
    Line::from(Span::styled(text, style))
}

/// 一个状态字形。`??` 本来就是两格，其余三个补一格 —— 两者都占满两格。
fn changes_glyph(kind: changes::Kind) -> &'static str {
    match kind {
        changes::Kind::Modified => wording::CHANGE_MODIFIED,
        changes::Kind::Added => wording::CHANGE_ADDED,
        changes::Kind::Deleted => wording::CHANGE_DELETED,
        changes::Kind::Untracked => wording::CHANGE_UNTRACKED,
    }
}

/// 一个分组标题。不带计数 —— 件数不是这一页要回答的问题。
fn changes_header(kind: changes::Kind) -> &'static str {
    match kind {
        changes::Kind::Modified => wording::CHANGE_GROUP_MODIFIED,
        changes::Kind::Added => wording::CHANGE_GROUP_ADDED,
        changes::Kind::Deleted => wording::CHANGE_GROUP_DELETED,
        changes::Kind::Untracked => wording::CHANGE_GROUP_UNTRACKED,
    }
}

/// 左栏的页签条：两条分隔线、标签夹在中间（spec §3，
/// `.scratch/todo-and-modes/spec.md` §4）。
///
/// 两条线都从屏幕左缘开始、到分隔列结束，于是左栏读起来是一个隔间，而不是自成一体的
/// 一块。标签列表是建出来的而不是写死的，因为 `todo` 是有条件的 —— 会话有了列表它才在条上
/// （`.scratch/trace-in-main/spec.md` §2）。
fn draw_tab_bar(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
    sidebar: Rect,
    tabs: Rect,
) {
    // 页签条也是一处「点左栏」的落点（`.scratch/files-page/spec.md` §5），所以它画在哪要
    // 记下来 —— 与页区同一套「只认真画出来的东西」的规矩。
    state.tabs_rect = Some(tabs);
    for y in [tabs.y - 1, tabs.y + 1] {
        paint_rule(
            frame,
            y,
            panes.screen.x,
            panes.divide.unwrap_or(sidebar.right()),
        );
    }
    let mut entries = vec![(
        wording::TAB_USAGE,
        state.tab == Tab::Usage,
        HitAction::SwitchTab(Tab::Usage),
    )];
    if state.todo.visible() {
        entries.push((
            wording::TAB_TODO,
            state.tab == Tab::Todo,
            HitAction::SwitchTab(Tab::Todo),
        ));
    }
    entries.push((
        wording::TAB_FILES,
        state.tab == Tab::Files,
        HitAction::SwitchTab(Tab::Files),
    ));
    // 第四签**常驻**：`todo` 那一签是有条件的，这一签不是 —— 工作区干净、不是仓库、
    // 甚至找不到 `git` 时页签都还在，页里各写一句（§1、§5）。
    entries.push((
        wording::TAB_CHANGES,
        state.tab == Tab::Changes,
        HitAction::SwitchTab(Tab::Changes),
    ));
    draw_label_bar(frame, state, tabs, &entries);
}

/// 主列的页签条：`对话 ┆ 轨迹`，同一份转录的两个视图
/// （`.scratch/trace-in-main/spec.md` §1、§2）。
///
/// 它只画标签下面那一条线 —— 屏幕第一行就是标签那一行，上边界就是屏幕自己，不必再画一条。
/// 点标签切页、从不给键位：照左栏那条既有的规矩（`Tab` 归 `/` 菜单、`Shift+Tab` 归模式循环）。
fn draw_main_tab_bar(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    paint_rule(
        frame,
        panes.main_tabs.y + 1,
        panes.main.x,
        panes.screen.right(),
    );
    let entries = [
        (
            wording::TAB_CONVERSATION,
            state.main_tab == MainTab::Conversation,
            HitAction::SwitchMainTab(MainTab::Conversation),
        ),
        (
            wording::TAB_TRACE,
            state.main_tab == MainTab::Trace,
            HitAction::SwitchMainTab(MainTab::Trace),
        ),
    ];
    draw_label_bar(frame, state, panes.main_tabs, &entries);
}

/// 一条页签条的标签行：标签、之间一个分隔符、这一行其余部分用线填满，选中的那个更亮
/// （spec §3；`.scratch/trace-in-main/spec.md` §2）。
///
/// 左栏与主列各有一条，用的是同一段画法：唯一的区别是跨哪些列、以及每个标签点下去落到哪个
/// 动作上。每个标签在画出来时记下一个命中矩形 —— 指针只能打中真在那里的东西，而填满这一行
/// 其余部分的线不是页签。线条由调用方画：左栏那条夹在身份与页区之间，主列那条只在标签下面。
fn draw_label_bar(
    frame: &mut ratatui::Frame,
    state: &mut TuiState,
    row: Rect,
    entries: &[(&'static str, bool, HitAction)],
) {
    let dim = Style::default().fg(palette::MUTED);
    // 「线」与「字」在这一行上分开取色：未选中的标签是**文字**（仍旧 `DarkGray`，它得读得
    // 出来），而线与它们之间的分隔符是**框架**（`palette::CHROME`，退到后面去）。
    let rule = Style::default().fg(palette::CHROME);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0u16;
    for (index, (label, selected, action)) in entries.iter().enumerate() {
        let style = if *selected {
            // 常驻选中 = `ACCENT` + `BOLD`（`.scratch/tui-visual-language/spec.md` §8）。
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            dim
        };
        let width = text_columns(label) as u16;
        if used + width <= row.width {
            state.regions.cells.push(Region {
                rect: Rect::new(row.x + used, row.y, width, 1),
                action: *action,
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
        "┄".repeat(row.width.saturating_sub(used) as usize),
        rule,
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}

/// 状态行：哪个模型、哪个模式、窗口有多满，以及**在不在跑**（spec §5、
/// `.scratch/tui-visual-language/spec.md` §17–§18）。
///
/// 四段共享一条线，行内分三档：标签退后、值靠前、分隔符只是线。宽度阶梯住在
/// [`wording::status_row`] 里；这一行本身总是画出来的，连它最后一档都容不下的宽度会被截断
/// 而不是丢掉（spec §2）。
fn draw_status(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let share = wording::context_share_value(state.panel.last_input(), state.facts.context_window);
    let width = panes.status.width as usize;
    let parts = wording::status_row(
        &state.facts.model,
        &state.effort_label(),
        state.mode,
        &share,
        &format!(
            "{} {}",
            wording::status_spinner(state.pulse, state.busy()),
            wording::status_word(state.busy())
        ),
        width,
    );
    let spans: Vec<Span<'static>> = parts
        .iter()
        .map(|part| {
            let colour = match part.kind {
                wording::StatusKind::Label => palette::MUTED,
                wording::StatusKind::Value => palette::PLAIN,
                wording::StatusKind::Separator => palette::CHROME,
            };
            Span::styled(part.text.clone(), Style::default().fg(colour))
        })
        .collect();
    // 两格的位置按**这一帧真正画出去的**那些段落算，所以截断之后它们的矩形也跟着短。
    let drawn = parts.clone();
    let line = Line::from(spans);
    // 状态行永远画得出来（[`wording::status_row`] 没有一档把整行拿走），所以截断只在比它的
    // 最后一档还窄的帧上兜底 —— 那不是真终端能到的宽度。截断归 [`super::width`]，但
    // [`ellipsize_line`] 是**给确定要截的调用方**的（它无条件加 `…`），所以这里先量一下：
    // 放得下就原样画，放不下才交给它。
    let columns = line_columns(&line);
    let line = if columns > width {
        ellipsize_line(line, width)
    } else {
        line
    };
    frame.render_widget(Paragraph::new(line), panes.status);
    // 「记住读的人真看到了什么」：这一帧真的画了哪两格，就把它们的矩形记进 `Regions`。
    // 降级之后模型那一段可能根本没画出来 —— 那时它不可点，而不是点在上一帧的位置上。
    let (model, effort) = status_cells(panes.status, &drawn);
    if let Some(rect) = model {
        state.regions.push(rect, HitAction::SwitchModel);
    }
    if let Some(rect) = effort {
        state.regions.push(rect, HitAction::SwitchEffort);
    }
}

/// 状态行上那两格各自占的矩形：`模型 X` 与 `· 档位`。没画出来的那一格是 `None`。
///
/// 顺着 [`wording::status_row`] 的段落结构走一遍、边走边累积列号，所以答案跟着**这一帧
/// 真正画出来的文本**走：降级之后模型那一段被丢掉了，点它就什么都不会上行。
fn status_cells(row: Rect, parts: &[wording::StatusPart]) -> (Option<Rect>, Option<Rect>) {
    /// 走到哪一段了。段与段之间的 `┆` 把它带回 `Outside`。
    #[derive(Clone, Copy, PartialEq)]
    enum Cell {
        Outside,
        /// `模型 ` 标签 + 模型值。
        Model,
        /// 段内那个 `·` 之后的档位值。
        Effort,
    }
    let mut cell = Cell::Outside;
    let mut x = row.x;
    let mut model: Option<Rect> = None;
    let mut effort: Option<Rect> = None;
    // 两格都是**连续**的一段，所以合并就是「往右加宽」。
    let grow = |slot: &mut Option<Rect>, x: u16, width: u16| {
        *slot = Some(match *slot {
            Some(previous) => Rect {
                x: previous.x,
                y: previous.y,
                width: previous.width + width,
                height: previous.height,
            },
            None => Rect {
                x,
                y: row.y,
                width,
                height: row.height,
            },
        });
    };
    for part in parts {
        match (part.kind, cell) {
            (wording::StatusKind::Label, _) => {
                cell = if part.text.starts_with(wording::PANEL_MODEL) {
                    Cell::Model
                } else {
                    Cell::Outside
                };
            }
            // 段与段之间那条框架线：模型与档位各自那一格到此为止。
            (wording::StatusKind::Separator, _) => cell = Cell::Outside,
            // 段内那个 `·` 就是两格的分界：它属于档位那一格 —— 读的人看到的正是
            // `模型 X · high`，后半格从间隔号开始。
            (wording::StatusKind::Value, _) if part.text == wording::SEP => {
                cell = Cell::Effort;
                grow(&mut effort, x, part.text.cell_width());
            }
            (wording::StatusKind::Value, target @ (Cell::Model | Cell::Effort)) => {
                let width = part.text.cell_width();
                if target == Cell::Model {
                    grow(&mut model, x, width);
                } else {
                    grow(&mut effort, x, width);
                }
            }
            (wording::StatusKind::Value, _) => {}
        }
        x += part.text.cell_width();
    }
    (model, effort)
}

/// 会话中途换模型/档位的选择器浮层（spec §10）。
///
/// 与问卷**不同形**，而且必须不同：问卷是模型发起的、答完它那条工具调用就结束了；选择器是
/// **人发起**的会话属性，答完要回循环去换 provider 与请求参数。所以它不进
/// `questionnaire()` 那条路径，而是 `TuiState` 上一个独立的状态，键盘归属按「谁立着谁拿」
/// 判（与 `detail` 与 `sidebar_keyboard` 同一套分派）。
#[derive(Debug)]
struct Picker {
    title: String,
    options: Vec<crate::render::PickerOption>,
    /// 回答的那一半。取出即 `take()`，所以答案**只送得出一次**（与 `prompt_reply` 同一写法）。
    reply: Option<tokio::sync::oneshot::Sender<Option<usize>>>,
    /// 高亮那一行的下标。
    highlight: usize,
    /// 浮层内部滚到了第几行（候选多过浮层的高度时）。
    top: usize,
    /// 浮层画在哪儿。框外的一次点击关掉它（与详情覆盖层同一条），所以它要记得上一次真被
    /// 画在了哪里。
    rect: Option<Rect>,
    /// 这一帧真的画出来的那些格：`(屏幕行, 列起, 列宽, 该格的候选下标)`。与问卷的选项区同一
    /// 条纪律：只有画出来的那几格点得到 —— 而且按**列**命中，因为一行上有好几格，点第三格就该
    /// 选第三格。
    rows: Vec<(u16, u16, u16, usize)>,
}

impl Picker {
    fn new(request: crate::render::PickerRequest) -> Self {
        let crate::render::PickerRequest {
            title,
            options,
            reply,
        } = request;
        // 高亮落在「当前」那一行；清单里没有当前项就落第一行。
        let highlight = options
            .iter()
            .position(|option| option.current)
            .unwrap_or(0);
        Self {
            title,
            options,
            reply: Some(reply),
            highlight,
            top: 0,
            rect: None,
            rows: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        self.options.len()
    }

    /// 移动高亮。到两端就停住，不绕回 —— 与问卷那一套同一条纪律（绕回会让长清单难走）。
    ///
    /// `step` 是**几格**：`j/k`（或 `←/→`）给 1（相邻那一格，行尾自然折到下一行），`↑/↓` 给
    /// [`Self::columns`]（换行、留在同一列）。横向与纵向在网格里是**两件事**，与 `ls`
    /// 那一族的手感相同。
    fn move_highlight(&mut self, step: isize) {
        if self.len() == 0 {
            return;
        }
        let last = self.len() - 1;
        self.highlight = self.highlight.saturating_add_signed(step).min(last);
    }

    /// 高亮是不是一个**能选**的行。禁用行（缺密钥的模型）不响应（spec §8）。
    fn highlighted_is_enabled(&self) -> bool {
        self.options
            .get(self.highlight)
            .is_some_and(|option| option.enabled)
    }

    /// 关掉它，把答案送出去。**只做一次**。
    fn close(&mut self, answer: Option<usize>) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(answer);
        }
    }
}

/// 文件页那一页自己的状态。
///
/// 与 [`Panel`]、[`TodoPanel`] 同一族，也与 [`DetailView`] 共用同一条纪律：状态由画它的那一处
/// 顺手记下（「记住读的人真看到了什么」），只有真画出来的行才回应指针与键盘。
#[derive(Default)]
struct FilesPage {
    /// 展开着的目录，装的是索引里的拼法（目录带尾斜杠）。
    ///
    /// 一次重扫之后不在索引里的路径直接丢掉 —— 展开一个已经不存在的目录没有意义
    /// （`.scratch/files-page/spec.md` §1）。
    expanded: std::collections::HashSet<String>,
    /// 滚到第几个可见行。
    scroll: usize,
    /// 上一帧的那些**可见行**（不只是窗口里那几行 —— 键盘要能在窗口之外移焦点）。
    rows: Vec<files::Row>,
    /// 上一帧页区画在哪；没画文件页时是 `None`。一次点击、一格滚轮按它判定落没落在这
    /// 一页上（与 [`TuiState::detail_rect`] 同一条规矩）。
    rect: Option<Rect>,
    /// 焦点行：它在 [`FilesPage::rows`] 里的下标。`None` 表示还没有一行拿过焦点。
    ///
    /// `Enter` 会插的就是这一行，所以它要看得见 —— 画成常驻选中那一个档
    /// （`ACCENT` + `BOLD`）。
    focus: Option<usize>,
}

/// 改动页那一页自己的状态（`.scratch/diff-page/spec.md` §2、§4）。
///
/// 与 [`FilesPage`] 同一族：一页的状态收在一个值里，整个只活在进程内 —— 不进事件流、不落盘。
/// 它的数据来自**渲染器自己在宿主侧跑的 git**，与文件索引各取各的（git 的口径与遍历的口径
/// 不是一回事）。
#[derive(Debug, Default)]
struct ChangesPage {
    /// 上一次取到的读数（相对 HEAD 的那些改动）。**取数失败不覆盖它**（§4）。
    files: Vec<changes::ChangedFile>,
    /// 这一页此刻处在哪一态。
    state: ChangesState,
    /// 一次取数在飞：并发守卫，形状照 [`FileIndex::Loading`] —— 在飞时不重发，位留着补发。
    loading: bool,
    /// 上一帧排出来的那些行（分组标题 + 文件）。焦点与点击拿它换下标。
    rows: Vec<changes::Item>,
    /// 焦点行在 [`ChangesPage::rows`] 里的下标。它**只落在文件行上** —— 分组标题没有可点的
    /// 东西。`None` 表示还没有一行拿过焦点。
    focus: Option<usize>,
    /// 上一帧页区画在哪；没画改动页时是 `None`（与文件页同一条「只认真画出来的东西」）。
    rect: Option<Rect>,
}

/// 改动页的取数处在哪一态。
///
/// 它与 [`ChangesPage::files`] 分开，正是为了「失败保留上一次读数」那条：读数在，页里就照旧
/// 画它，那几句不可用态只在**一次都还没取到过**的时候出现（§4、§5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ChangesState {
    /// 还没取到数 —— 页里写「正在读取改动…」。
    #[default]
    NotAsked,
    /// 有读数了（可能是空的：工作区干净）。
    Ready,
    /// 这里不是 git 仓库。
    NotARepo,
    /// `PATH` 上找不到 `git`。
    NoGit,
    /// 取数失败了，而且这一页从来没有取到过数。
    ///
    /// 它必须与 [`ChangesState::NotAsked`] 分开：那两句话一个说「在读」、一个说「读不出来」，
    /// 而把前者永远挂在一次失败的取数上就是骗人。
    Failed,
}

/// 正在显示左栏的哪一页（spec §3）。
///
/// 点出来的，从不给键位：`Tab` 归 `/` 菜单、`Shift+Tab` 归模式循环。
///
/// 三页各有各的内容（读数、`todo` 列表、工作区那棵树），所以在一页上时会话的读数不在屏幕上
/// —— 那时状态行的 `上下文 n%` 是唯一剩下的那个。这不是 bug：没有第二份数字可以退回，而另
/// 一条路（用键盘走页签）在这里本来也不通。键位只有**文件页**那一套（[`TuiState::sidebar_key`]），
/// 它不切页。
///
/// `轨迹` 曾经是这一列上的第三页；2026-10-06 起它搬进了主列，成了 [`MainTab`] 的第二个标签
/// （`.scratch/trace-in-main/spec.md` §2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    /// 会话的读数：旧信息面板装着的那些。
    Usage,
    /// agent 的待办列表，也是唯一不总在条上的页签：主会话第一次提交一次列表时它出现，此后
    /// 整个会话都在（`.scratch/todo-and-modes/spec.md` §4）。
    Todo,
    /// 工作区里的文件与目录：一棵从会话级索引排出来的树
    /// （`.scratch/files-page/spec.md` §1、§3）。它回答的是「工作区长什么样」，
    /// **不是**「这个会话碰过哪些文件」—— 后者归 `sessions show --files`。
    Files,
    /// 相对 HEAD 改了什么（`.scratch/diff-page/spec.md`）。
    ///
    /// 与 `todo` 那一签**有意不同**：它是**常驻**的第四签 —— 工作区干不干净、这里是不是
    /// 一个仓库、甚至 `git` 在不在 `PATH` 上，都不该让页签本身出现或消失（§1、§5）。
    Changes,
}

/// 正在显示主列的哪一页：同一份转录的两个视图（`.scratch/trace-in-main/spec.md` §2）。
///
/// 与 [`Tab`] 同一套规矩：点出来的、从不给键位、只活在这一次进程里。默认是 `Conversation`
/// —— 起步与拆分之前一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainTab {
    /// 对话视图：用户文本、assistant 正文与保留清单。
    Conversation,
    /// 轨迹视图：全量块（`.scratch/trace-tab/spec.md` §2）。
    Trace,
}

/// 四个浮层表面共用的框架：`CHROME` 的虚线，**四角为空**
/// （`.scratch/tui-visual-language/spec.md` §25）。
///
/// 角本来就由 `border::Set` 定，不必自绘；ratatui 那套三重虚线的四个角是**实线**，正是要拆掉
/// 的第三套语法。框色也只剩 [`palette::CHROME`] 一个（问卷没有框，不参与）。
fn chrome_block() -> WidgetBlock<'static> {
    WidgetBlock::default()
        .borders(Borders::ALL)
        .border_set(border::Set {
            top_left: " ",
            top_right: " ",
            bottom_left: " ",
            bottom_right: " ",
            ..border::LIGHT_TRIPLE_DASHED
        })
        .border_style(Style::default().fg(palette::CHROME))
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
    // 边框归框架：`CHROME` 的虚线、空角。模态不再整块一个颜色 —— 框、话、要按的键各是各的
    // （`.scratch/tui-visual-language/spec.md` §26）。
    frame.render_widget(chrome_block(), area);
    // 主体与按钮行分开画，这样按钮的列能被准确记下来：问题占着它们，而一次点击必须落在它看
    // 起来落在的那个按钮上（票 04 §3）。
    let inner_area = layout::inner(area);
    let body = Rect::new(
        inner_area.x,
        inner_area.y,
        inner_area.width,
        body_rows.min(inner_area.height),
    );
    // 正文是居中的**文本**：它不可点，所以这里不需要那个偏移，交给 ratatui 摆即可。
    frame.render_widget(
        Paragraph::new(rows)
            .style(Style::default().fg(palette::PLAIN))
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
        Paragraph::new(line).style(Style::default().fg(palette::ACCENT)),
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
///
/// **它与 [`Alignment::Center`] 同时存在，不是同一需求的两份实现**，别把其中一个当遗留删掉：
/// `Alignment::Center` 把摆放交给 ratatui 在渲染时做，调用方拿不到那个偏移；而这里（模态的
/// 按钮行、问卷的回执行）要的正是那个偏移 —— 画家画在哪一列，命中测试就得按哪一列算，两件事
/// 必须是**同一份算术**（票 04 §7）。换成 `Alignment::Center` 之后按钮照常画出来，然后点不中。
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
            offset += text_columns(wording::GAP);
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
        .fg(palette::ACCENT)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(palette::ACCENT);
    let mut spans = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(wording::GAP));
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
            .style(Style::default().fg(palette::MUTED))
            .alignment(Alignment::Center),
        row,
    );
}

/// 一个脉冲帧：这个界面上**每一个**由脉冲驱动的动画共用的帧长 —— 左栏标记忙时那束扫光，与
/// 光标闪烁。
///
/// **这个时钟一直在走**：票 09 曾让它在空闲时停掉（「一个在他们打字时手底下动来动去的颜色
/// 是噪声」），`.scratch/tui-visual-language/spec.md` §32 推翻了它。提示符的色相随后随提示符
/// 一起退场（`.scratch/ui-trim/spec.md`），但空闲那一半仍有住户：光标闪烁与那几条回执的寿命。
/// 代价写在明面上：空闲的会话不再零唤醒。
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

/// 一条**回执**在提示行上待多久（`.scratch/tui-feedback/spec.md` §6）。
///
/// 比退出手势那半秒长：它说的是「刚刚发生了什么」，而读它的人手还在鼠标上。写死、不做配置项。
/// 复制与打开共用它 —— 两类回执是同一件事的两种，没有理由各走一条寿命
/// （`.scratch/clickable-links/spec.md` §4）。
const RECEIPT_WINDOW: std::time::Duration = std::time::Duration::from_secs(3);

/// 一次复制留下的那句话，还在寿命内的话：`None` 就是「这一帧不该有回执」。
///
/// 时刻由调用方给，于是寿命是一条能直接断言的算术，而不是一个要等三秒的测试。
fn copy_receipt(
    copied: Option<(std::time::Instant, usize, usize)>,
    now: std::time::Instant,
) -> Option<String> {
    let (at, chars, lines) = copied?;
    (now.duration_since(at) < RECEIPT_WINDOW).then(|| wording::copied(chars, lines))
}

/// 一次打开留下的那句话（成功或失败），还在寿命内的话
/// （`.scratch/clickable-links/spec.md` §4）。
///
/// 与 [`copy_receipt`] 同一条寿命、同一个形状：话是运行期写进来的，这里只管它还能不能露脸。
/// 提示行那一句说明，还在寿命内吗。
fn notice_line(
    notice: Option<&(std::time::Instant, String)>,
    now: std::time::Instant,
) -> Option<String> {
    let (at, text) = notice?;
    (now.duration_since(*at) < RECEIPT_WINDOW).then(|| text.clone())
}

fn open_receipt(
    opened: Option<&(std::time::Instant, String)>,
    now: std::time::Instant,
) -> Option<String> {
    let (at, text) = opened?;
    (now.duration_since(*at) < RECEIPT_WINDOW).then(|| text.clone())
}

/// 标记的那些行与它们的颜色。
///
/// 文字是 `wording` 的，它长什么样住在那里；颜色住在这里：**标着拼音的那一行暗一档，其余全亮**
/// —— 注在字形旁边，不与它抢眼。画一块字标不能像画一行字那样从上往下渐暗：字块是一个图形，
/// 深浅不一的几笔会让它读起来像缺了角。加粗与背景一概不设，而且是刻意不设背景：标记坐在用户
/// 主题已有的任何背景上，填掉字缝里的那些格会在它能匹配的同样多的终端上与那个主题打架。
///
/// 两版标记走同一条上色规则，只是拼音所在的行号不同 —— 收起来的那一版在第一行留了一段空白
/// （`wording::logo_lines_compact`），所以那个位置由调用的这一处说出，而不是让画家去猜。
///
/// 返回的是**每行的若干段**（连续同色的字符合成一段），因为扫光会让一行里出现几种颜色。
/// `sweep` 是扫光要看的那一帧：`None` 时整块就是基线色，一个像素都不动。
fn mark_lines(kind: layout::SidebarKind, sweep: Option<u64>) -> Vec<Vec<(String, Color)>> {
    match kind {
        layout::SidebarKind::Mark => paint_mark(&wording::logo_lines(), 0, sweep),
        layout::SidebarKind::MarkCompact => paint_mark(&wording::logo_lines_compact(), 1, sweep),
        other => unreachable!("只有两版标记走这条路：{other:?}"),
    }
}

/// 一串标记行各自带上颜色：`preamble` 那一行是拼音，退一档；其余是字形，全亮。扫光压在上面
/// （[`mark_sweep`]），扫不到的格保持自己的基线色。
fn paint_mark(
    rows: &[&'static str],
    preamble: usize,
    sweep: Option<u64>,
) -> Vec<Vec<(String, Color)>> {
    debug_assert!(
        rows.iter()
            .all(|row| text_columns(row) == layout::LOGO_WIDTH as usize),
        "标记要么整个画出来，要么一个都不画，所以它的宽度是布局的契约"
    );
    let columns = rows.first().map_or(0, |row| row.chars().count());
    rows.iter()
        .enumerate()
        .map(|(row, text)| {
            let baseline = if row == preamble {
                palette::MARK_DIM
            } else {
                palette::MARK_BRIGHT
            };
            let mut spans: Vec<(String, Color)> = Vec::new();
            for (column, glyph) in text.chars().enumerate() {
                // 留白不吃扫光：那里没有字形，改它的颜色只会让一帧里多出一段没人看得见的
                // 变化（也给逐格比对的测试添噪声）。块字之间那些空档因此永远是基线色。
                let color = if glyph == ' ' {
                    baseline
                } else {
                    sweep
                        .and_then(|frame| mark_sweep(column, row, columns, rows.len(), frame))
                        .unwrap_or(baseline)
                };
                match spans.last_mut() {
                    Some((run, last)) if *last == color => run.push(glyph),
                    _ => spans.push((glyph.to_string(), color)),
                }
            }
            spans
        })
        .collect()
}

/// 扫光：一格该不该被这束反光照到，照到了是什么颜色（`.scratch/mark-sweep/spec.md` §2）。
///
/// 光从**右下走到左上**。把每一格投影到那条轴上：`s = (最右一列 − 这一列) + (最下一行 −
/// 这一行)`，于是右下角是 0、左上角最大，等值线是一条条斜线、垂直于光走的方向；光带就是 `s`
/// 上的一段区间，它的头每帧朝 `s` 大的方向走 [`SWEEP_STEP`]。核心是白的
/// （[`palette::MARK_LIGHT`]），两侧各留一截过渡 [`palette::MARK_BRIGHT`] —— 过渡在字形那几
/// 行上看不出来（它们本来就是这一档），看得见的是拼音行：它从 [`palette::MARK_DIM`] 被抬到
/// 亮档，于是光带的前后沿也在那条细字上有交代。
///
/// 参数是**格子数**，不是像素：它在整块标记（含左右留白）上算，因为留白格没有字形、颜色
/// 不可见，把它们算进来只是让投影的跨度跟着块走。
///
/// 公开是为了让 `tests/render_layout.rs` 直接量它的方向与档位 —— 扫光在屏幕上的样子由它
/// 一个纯函数决定，逐格比对那一帧只是把它画出来。
pub fn mark_sweep(
    column: usize,
    row: usize,
    columns: usize,
    rows: usize,
    frame: u64,
) -> Option<Color> {
    let head = -SWEEP_HALO + (frame % SWEEP_PERIOD) as i64 * SWEEP_STEP;
    // 一轮必须把光带整个**送出**块的左上角之外，否则它会在某一帧从右下凭空跳回来。
    debug_assert!(
        -SWEEP_HALO + (SWEEP_PERIOD as i64 - 1) * SWEEP_STEP > (columns + rows) as i64,
        "扫光一轮要走出块外"
    );
    let progress = (columns - 1 - column) as i64 + (rows - 1 - row) as i64;
    let distance = (progress - head).abs();
    if distance <= SWEEP_CORE {
        Some(palette::MARK_LIGHT)
    } else if distance <= SWEEP_HALO {
        Some(palette::MARK_BRIGHT)
    } else {
        None
    }
}

/// 扫光每一帧朝左上走多远（单位是投影上的格子），以及走完一轮回到起点要用几帧。
///
/// 一轮 64 格、每帧 3 格，约 22 帧扫完（60 ms 一帧，**1.3 秒**），剩下几帧是两轮之间的空档：
/// 一次运行里反复扫，节奏是 1.8 秒一轮。速度是拿真机看着定的 —— 再快像闪，再慢就不像
/// 一束光扫过去（`.scratch/mark-sweep/spec.md` §2）。
const SWEEP_STEP: i64 = 3;
pub const SWEEP_PERIOD: u64 = 30;

/// 光带核心与它两侧过渡的半宽（同样是投影上的格子）。核心 5 格宽、连过渡一共 13 格 ——
/// 相对 38 列的块，读起来是一条斜带而不是一条线。
const SWEEP_CORE: i64 = 2;
const SWEEP_HALO: i64 = 6;

/// 主列的内容区：这条转录的两个视图之一，加上它右边缘的滚动条、指示器，以及（只在对话页上
/// 的）回合条。
///
/// 画哪一页看 [`TuiState::main_tab`]（`.scratch/trace-in-main/spec.md` §3）：两页共用同一块
/// 矩形、同一条滚动条列，各自的窗格、滚动位置、跟随与链接表互不影响。回合条只画在对话页上
/// —— 它量的是对话视口的单位位置，画在轨迹页上会指着一个跟它无关的视口（§4）。
fn draw_transcript(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    let text_area = panes.transcript_text();
    match state.main_tab {
        MainTab::Conversation => {
            let live = state.conversation_live();
            // 拖选要的那一层：同一个来源行折出来的下一片就是**软折续行**，复制时拼回去
            // （`.scratch/tui-feedback/spec.md` §5–§6）。
            //
            // 这个视图是**唯一**认可点链接的地方（`.scratch/clickable-links/spec.md` §1）：
            // 认一遍，结果一分为二 —— 下划线就地铺在这一行上，候选跟着进屏幕文本层供点击
            // 命中。认在画之前，所以两处用的是同一次识别的同一份答案。
            let mut rows = state
                .conversation
                .view(text_area.width, text_area.height, &live);
            let folded = soft_folds(&state.conversation, state.conversation.top(), rows.len());
            let hotspots = links::mark(&mut rows, &folded);
            note_rows(state, text_area, &rows, &folded, &hotspots);
            // 这一页画在哪儿：点击要拿它判「落在对话的转录里吗」。与轨迹页那条同一条纪律，
            // 每帧重填（`.scratch/clickable-links/spec.md` §1）。
            state.conversation_rect = Some(text_area);
            frame.render_widget(Paragraph::new(rows), text_area);
            draw_scrollbar(frame, panes.scrollbar(), &state.conversation);
            draw_turn_rail(frame, panes, state);
            draw_indicator(frame, text_area, state, Viewport::Conversation);
        }
        MainTab::Trace => {
            // 轨迹页只画正文尾巴：等待提示是对话视图自己的（票 12 的修订之后它也没有动画）。
            let live = TuiState::live_rows(&state.live);
            let rows = state.trace.view(text_area.width, text_area.height, &live);
            let top = state.trace.top();
            state.trace_drawn.top = text_area.y;
            state.trace_drawn.rows = (0..rows.len())
                .map(|offset| state.trace.source_at(top + offset))
                .collect();
            let folded = soft_folds(&state.trace, top, rows.len());
            note_rows(state, text_area, &rows, &folded, &[]);
            // 点击按它分派：`trace_rect` 只在上一帧真画了轨迹页时才有值（票 09 那条纪律照旧，
            // 只是现在它等于主列的内容区）。
            state.trace_rect = Some(text_area);
            frame.render_widget(Paragraph::new(rows), text_area);
            draw_scrollbar(frame, panes.scrollbar(), &state.trace);
            draw_indicator(frame, text_area, state, Viewport::Trace);
        }
    }
}

/// 一段被画出来的显示行里，哪些是**软折续行**：同一个来源行折出来的下一片。
///
/// 判据来自窗格自己：显示行 `i` 与 `i−1` 属于同一条来源行就说明前者是后者折出来的。没有来源
/// 行的那些显示行（正在流的那条尾巴）一律不算 —— 它们还没定稿，复制它们本就是少见的事，而
/// 猜错一次会把两行粘成一行。
fn soft_folds(pane: &Pane, top: usize, rows: usize) -> Vec<bool> {
    (0..rows)
        .map(|index| {
            index > 0
                && pane.source_at(top + index).is_some()
                && pane.source_at(top + index) == pane.source_at(top + index - 1)
        })
        .collect()
}

/// 把一块区域这一帧画出来的行记进屏幕文本层（`.scratch/tui-feedback/spec.md` §5）。
///
/// `folded` 与 `rows` 平行，短了就当作「没有软折」。
///
/// `hotspots` 同样与 `rows` 平行：**[`links::mark`] 的返回值**。它由调用方在画之前算出来
/// （那一次调用同时就把下划线铺在这一行上了），所以这里只负责记 —— 记的与画的出自同一次
/// 识别（`.scratch/clickable-links/spec.md` §2）。除对话视图之外的每一块都传空表：那些地方的
/// 点击各有各的语义（轨迹页开详情、左栏切页、文件页展收）。
fn note_rows(
    state: &mut TuiState,
    rect: Rect,
    rows: &[Line<'static>],
    folded: &[bool],
    hotspots: &[Vec<links::Hotspot>],
) {
    let text: Vec<selection::TextRow> = rows
        .iter()
        .enumerate()
        .map(|(index, line)| selection::TextRow {
            text: line_text(line),
            folded: folded.get(index).copied().unwrap_or(false),
            lead: row_lead(line, rect.width),
            hotspots: hotspots.get(index).cloned().unwrap_or_default(),
        })
        .collect();
    state.screen_text.push(rect, text);
}

/// 一条显示行的文本在屏幕上从区域的第几列起。
///
/// 只认**靠右**排出来的行：它们的左边界是算出来的 —— 区域宽减行宽 —— 而靠右是用户消息的气泡
/// 与它上面那一行名字贴右缘的手段（`.scratch/trace-tab/spec.md` §2 的补记）。左对齐的行一律从
/// 第零列起：这里不去猜「这一行前导有几个空格」，因为代码块与缩进正文本身就以待空格开头，猜错
/// 一次就会从复制出来的文本里啃掉一段缩进。
fn row_lead(line: &Line<'static>, width: u16) -> u16 {
    if line.alignment != Some(Alignment::Right) {
        return 0;
    }
    let columns = line_columns(line).min(usize::from(width));
    width.saturating_sub(columns as u16)
}

/// 一个视图里跨块的排版状态（`.scratch/tui-visual-language/spec.md` §23）。
#[derive(Debug, Default)]
struct Flow {
    /// 上一条画出来的块是谁说的。
    speaker: Option<crate::events::SpeakerId>,
    /// 上一条画出来的是不是一条**消息**（不是工具行、不是叙述）。
    message: bool,
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
    let style = Style::default().fg(palette::MUTED);
    let focus_style = Style::default()
        .fg(palette::ACCENT)
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
/// 转录（或轨迹页）右缘的位置指示：**只画滑块，不画轨道**。
///
/// 轨道与紧邻的回合条是同一族细竖线，两条并排读起来是噪音（`.scratch/tui-visual-language/
/// issues/07` 决定 1）；位置靠滑块本身表达。`TRAILING_COLUMNS` 那两列的预留**不动** —— 它防
/// 的是文字重新折行，与画不画无关。
fn draw_scrollbar(frame: &mut ratatui::Frame, track: Rect, pane: &Pane) {
    if track.width == 0 || pane.total() <= track.height as usize {
        return;
    }
    // 跟着末尾与在读历史看起来不一样，所以位置不用读数字就看得出来。
    let thumb = if pane.following() {
        Style::default().fg(palette::MUTED)
    } else {
        Style::default()
            .fg(palette::MUTED)
            .add_modifier(Modifier::BOLD)
    };
    let mut scrollbar = ScrollbarState::new(pane.total())
        .position(pane.top())
        .viewport_content_length(track.height as usize);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            // 轨道与它两端的箭头都不画：只留滑块，位置靠它自己表达。
            .track_symbol(None)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_style(thumb),
        track,
        &mut scrollbar,
    );
}

/// 窗格右下那个「来了什么、以及回去的路」的指示器。
///
/// 它的整块都是点击目标，所以这个矩形记在窗格上 —— 一次点击只能落在上一帧画出来的东西上。
fn draw_indicator(frame: &mut ratatui::Frame, area: Rect, state: &mut TuiState, view: Viewport) {
    let pane = match view {
        Viewport::Conversation => &state.conversation,
        Viewport::Trace => &state.trace,
    };
    // 详情覆盖层占着**打开它的那一页**的时候，回去的路是覆盖层自己的页脚：指示器的计数暂停了，
    // 它的点击不属于任何人（票 02 §4）。对话页的指示器照画（票 13）—— 今天打开方恒为轨迹页
    // （`.scratch/tui-feedback/spec.md` §9）。
    let frozen = state.detail_open() && view == Viewport::Trace;
    if pane.following() || frozen || area.width == 0 || area.height == 0 {
        match view {
            Viewport::Conversation => state.indicator = None,
            Viewport::Trace => state.trace_indicator = None,
        }
        return;
    }
    let fresh = pane.fresh();
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
            // 新内容指示器归焦点色（`.scratch/tui-visual-language/spec.md` §6）。
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        ))),
        rect,
    );
    match view {
        Viewport::Conversation => state.indicator = Some(rect),
        Viewport::Trace => state.trace_indicator = Some(rect),
    }
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
    let (rows, cursor) = state.editor.view(
        layout::content_width(frame.area(), state.sidebar_wanted),
        panes.input.height,
    );
    // 草稿归正文档，自己不带任何前缀：提示符与它的色相随 `.scratch/ui-trim/spec.md` 一起退场
    // （整段 `BOLD` 则是更早退的，`.scratch/tui-visual-language/spec.md` §19）。
    // 输入区也进屏幕文本层（没有软折：草稿的换行是用户自己敲的）。
    note_rows(state, panes.input, &rows, &[], &[]);
    frame.render_widget(Paragraph::new(rows), panes.input);
    // 草稿在问题之下仍然可见 —— 那是用户正在写的东西 —— 但光标收起来：键盘正在回答，不是在
    // 编辑（spec §9）。光标是按刚画出来的那些行摆的，从不按帧与帧之间保存的状态摆，正是后者
    // 让内联视口的光标漂移（ADR 0002）。
    // 菜单的锚点与「光标露不露面」是两件事：锚点只问有没有东西占着键盘（`/` 菜单按它算
    // 位置），而光标还多两层判据（键盘真在输入区、以及此刻是不是亮着那一半）。
    let anchor = state.pending.is_none().then_some(cursor);
    // 光标只在**键盘真的在输入区**时出现；闪不闪由 `paint_frame` 收放，这里只管它在哪。
    //
    // 位置必须**每一帧都落在同一格**，连不露面的那一半也不例外：终端的光标被隐藏时停在最后
    // 写入的那一格（ratatui 的 `hide_cursor` 一个移动光标的字节都不发），而输入法的预编辑
    // 正是画在物理光标上的 —— 空闲时唯一在动的是状态行那个月相，于是攒拼音的字母会长在
    // `🌑` 与「就绪」之间。可见性分出去，位置才留得住。
    //
    // 它要回答的是「焦点在哪」：键盘交给文件页、详情覆盖层或文件查看器立着时都不该有它 ——
    // 否则屏幕上那个静止的光标在说谎，而人分不出自己敲的字会落到哪里。问卷有自己的光标
    // （上面那条分支已经返回了）。
    if keyboard_in_the_input(state) {
        frame.set_cursor_position((
            (panes.input.x + cursor.column).min(panes.input.right().saturating_sub(1)),
            panes.input.y + cursor.row,
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            state.hint_line(panes.hints.width),
            Style::default().fg(palette::MUTED),
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
    // 问卷这一块也进屏幕文本层：它画的就是这个窗口，行是排好的、没有软折
    // （`.scratch/tui-feedback/spec.md` §5）。
    let rows: Vec<Line<'static>> = window.clone();
    note_rows(
        state,
        Rect::new(
            panes.input.x,
            panes.input.y,
            panes.input.width,
            rows.len().min(panes.input.height as usize) as u16,
        ),
        &rows,
        &[],
        &[],
    );
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
    // 输入区那几行钉在底部，选项最多画到它们之前；题面写下的行数取几何留下的那个 ——
    // 题面被削时它就在更上面。
    let last_option_row = input_height.saturating_sub(custom.len());
    let mut row = geometry.prefix_rows;
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
    // 进度计数器（当前题被交回去时后面跟一句「已跳过」）与第一个按钮前那三个字宽的间隔是同一
    // 个 span，所以第一个按钮起始的列就是这个 span 自己的宽度 —— 而不是对它的第二次猜测
    // （票 04 §7、§11）。
    let progress = wording::questionnaire_progress(questionnaire.index, total);
    let progress = if questionnaire.drafts[questionnaire.index].skipped {
        format!("{progress} {}", wording::questionnaire_skipped())
    } else {
        progress
    };
    let mut cursor = text_columns(&progress) + text_columns(wording::GAP);
    let mut spans: Vec<Span<'static>> = vec![Span::styled(
        format!("{progress}{}", wording::GAP),
        Style::default().fg(palette::MUTED),
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
            spans.push(Span::raw(wording::GAP));
            cursor += text_columns(wording::GAP);
        }
        let width = text_columns(label);
        if let Some(rect) = hint_region(panes.hints, cursor, width) {
            state.regions.cells.push(Region { rect, action });
        }
        spans.push(Span::styled(
            label.to_owned(),
            Style::default().fg(palette::MUTED),
        ));
        cursor += width;
        drawn = true;
    }
    // 最后那一段：举手时是回执 —— 任何宽度下都保，因为它回答的是「刚才那下生效了没有」；
    // 否则是键位提示，按剩下的列数降级（`.scratch/questionnaire-keys/spec.md` §6）。
    let tail = match state.raised_gesture(std::time::Instant::now()) {
        Some(Gesture::Exit) => wording::questionnaire_exit_raised(),
        Some(Gesture::DeclineQuestion) => wording::questionnaire_decline_raised(),
        None => wording::questionnaire_hint(
            (panes.hints.width as usize).saturating_sub(cursor + text_columns(wording::GAP)),
            questionnaire.enter_submits(),
        ),
    };
    if !tail.is_empty() {
        spans.push(Span::raw(wording::GAP));
        spans.push(Span::styled(
            tail.to_owned(),
            Style::default().fg(palette::MUTED),
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

/// 记号菜单：`/` 后面能变成哪些名字、`@` 后面能指到哪些文件与目录 —— 浮在光标处，并按
/// 前缀后面打了什么过滤（spec §2、§3）。
///
/// 它是一个**提示**，不是一个问题：它从不会把某个键从草稿那里拿走，而它确实占着的键（`↑`、
/// `↓`、`Tab`、`Enter`）只在它立着时才有。
fn draw_menu(
    frame: &mut ratatui::Frame,
    panes: &layout::Regions,
    state: &mut TuiState,
    anchor: editor::Placed,
) {
    let Some(menu) = state.token_menu() else {
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
        .map(|entry| text_columns(&entry.name) + 1)
        .max()
        .unwrap_or(0);
    // 一行实际要占的列数：一列左内边距 + 名字列（`/名字` 补齐到最宽的那个名字）+ 两列间隔
    // + 描述 + 一列右内边距 + 行尾那一列「不碰边框」的留白。少算最后那一列时，最长的一条
    // 会掉最后两个字 —— 宽度上限抬到多少都救不了它（`.scratch/tui-feedback/spec.md` §11）。
    let widest = visible
        .iter()
        .map(|entry| name_width + 5 + text_columns(&entry.description))
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
        .map(|(offset, entry)| {
            let selected = menu.selected == Some(first + offset);
            menu_row(menu.sigil, entry, inner, name_width, selected)
        })
        .collect();
    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    frame.render_widget(chrome_block(), area);
    frame.render_widget(Paragraph::new(rows), layout::inner(area));
}

/// 一行菜单：`/名字` 或 `@路径`，补齐到名字列宽，然后是描述。
///
/// `@` 的候选没有描述，于是名字独占这一行 —— 那仍然是一个完整的提示。
///
/// **名字按类别上色**（`.scratch/tui-feedback/spec.md` §11）：命令、技能、MCP 模板各一色，
/// 于是「哪些是程序自带的」一眼看得出；描述留在正文档，它是一句解释，不参与分类。高亮的那
/// 一行仍反色画，好让它读起来是 `Enter` 会按下的那个按钮，而且**不叠类别色** —— 光标是临时
/// 的，颜色说的是这一行是什么（`tui-visual-language` §27 的另一半照旧）。
fn menu_row(
    sigil: char,
    entry: &MenuEntry,
    inner: usize,
    name_width: usize,
    selected: bool,
) -> Line<'static> {
    let label = format!("{sigil}{}", entry.name);
    // 描述列，在有地方放一个描述、也有它要占的那一行的时候。太窄就让名字独占这一行，那仍然是
    // 一个完整的提示。
    let gap = name_width.saturating_sub(text_columns(&label)) + 2;
    let fits = !entry.description.is_empty() && text_columns(&label) + gap + 2 <= inner;
    let head = if fits {
        format!("{label}{}", " ".repeat(gap))
    } else {
        label
    };
    let tail = if fits { entry.description.as_str() } else { "" };

    // 开头一列内边距，然后是这一行，然后是剩下的部分 —— 于是文字永远不碰边框，而高亮盖住整行。
    let body = truncate_columns(&format!("{head}{tail}"), inner.saturating_sub(1));
    // 整行截断之后，名字与描述的分界仍在同一个地方：`head` 没被截断时它整个是 `body` 的前缀，
    // 被截断时它已经把这一行占满、描述一个字都没进来。
    let shown_head = truncate_columns(&head, inner.saturating_sub(1));
    let shown_tail = &body[shown_head.len().min(body.len())..];
    let padding = inner.saturating_sub(1 + text_columns(&body));

    let name_style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(entry.colour())
    };
    // 菜单去黄（§27）：未选中行归正文档，光标行是**临时光标** —— 反显，不占颜色。分类色只上
    // 名字，描述与整行的底子都归正文档。
    let plain = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(palette::PLAIN)
    };
    Line::from(vec![
        Span::styled(format!(" {shown_head}"), name_style),
        Span::styled(shown_tail.to_owned(), plain),
        Span::styled(" ".repeat(padding), plain),
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
    style: PrefixStyle,
) -> Vec<Line<'static>> {
    let prefix = style.prefix(speaker);
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
    style: PrefixStyle,
) -> Vec<Line<'static>> {
    let prefix = style.prefix(speaker);
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
fn prefix_columns(speaker: &crate::events::SpeakerId, style: PrefixStyle) -> u16 {
    style.prefix(speaker).as_str().cell_width()
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
    style: PrefixStyle,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(style.label(speaker), name_style(speaker, colors)),
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

/// 发言者前缀怎么写：宽档 `[名字] `，窄档把方括号去掉省给内容
/// （`.scratch/trace-tab/spec.md` §3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrefixStyle {
    Bracketed,
    Bare,
}

impl PrefixStyle {
    /// 这个名字在这一档里怎么写。
    fn label(self, speaker: &crate::events::SpeakerId) -> String {
        let label = speaker_label(speaker);
        match self {
            Self::Bracketed => label,
            Self::Bare => label
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned(),
        }
    }

    /// 名字加它后面那个空格 —— 每一条带名字的行都以它开头。
    fn prefix(self, speaker: &crate::events::SpeakerId) -> String {
        format!("{} ", self.label(speaker))
    }
}

/// 一条带名字的行在这个视图里怎么写前缀。
///
/// 对话视图永远照旧带方括号；轨迹视图按左栏自己的两档来 —— 宽档 40 列带方括号、窄档 28 列
/// 去掉（票 09）。
fn prefix_style(view: Viewport, width: u16) -> PrefixStyle {
    match view {
        Viewport::Conversation => PrefixStyle::Bracketed,
        Viewport::Trace if width >= layout::SIDEBAR_WIDE => PrefixStyle::Bracketed,
        Viewport::Trace => PrefixStyle::Bare,
    }
}

/// 把一个定稿的块变成带样式的终端行。
///
/// 这是共享呈现层的 TUI 那一半：块已经被 [`Transcript`] 决定过一次，这里只发生绘制。
///
/// `colors` 是转录的名字调色板。手上没有名册的调用方 —— `plain` 的那一半渲染，以及只关心文字
/// 的测试 —— 通过 [`render_block_uncoloured`] 传一个空的进来，于是每个名字都画成叙述灰。
pub fn render_block(block: &Block, colors: &mut SpeakerColors) -> Vec<Line<'static>> {
    let style = prefix_style(Viewport::Conversation, SHARED_RENDER_WIDTH);
    paint_block(
        block,
        colors,
        SHARED_RENDER_WIDTH,
        style,
        Viewport::Conversation,
        true,
    )
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
/// （`.scratch/markdown-render/spec.md` §1）。`style` 是发言者前缀的分档 —— 它由调用方按
/// **视图与那个视图的档位宽**算好传来，因为轨迹页的正文比页面窄两列（滚动条那一列），前缀的
/// 「宽档 40 列」不能拿正文列数去量（`.scratch/tui-visual-language/issues/07` 决定 2）。
/// `name` 是**对话视图**要不要画那一行名字：同一个人连着说的第二段起不画
/// （`.scratch/tui-visual-language/spec.md` §23）；轨迹页与 [`render_block`] 一律画。
fn paint_block(
    block: &Block,
    colors: &mut SpeakerColors,
    width: u16,
    style: PrefixStyle,
    view: Viewport,
    name: bool,
) -> Vec<RenderedLine> {
    match block {
        // 轨迹视图里的一条 assistant 消息：只画**首行 + `…`**，全文进详情覆盖层
        // （`.scratch/trace-tab/spec.md` §3）。
        Block::Message {
            speaker,
            role: Role::Assistant,
            text,
            reasoning: _,
        } if view == Viewport::Trace => {
            if text.is_empty() {
                return Vec::new();
            }
            let indent = prefix_columns(speaker, style);
            let rows = attribute_document(
                speaker,
                super::markdown::to_lines_indented(text, width, indent),
                colors,
                indent,
                style,
            );
            vec![trace_message_row(speaker, rows, text, width, colors, style)]
        }
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
            // 回答按 Markdown 以全亮度渲染；只有发言者标签上色（spec §5）。名字独占一行，
            // 回答从下一行起、**顶格** —— 前缀不再占正文的列，所以 Markdown 的前导是零
            // （2026-10-05 维护者的排版修订）。
            let body = super::markdown::to_lines_indented(text, width, 0);
            if name {
                named_rows(speaker, body, colors, style)
            } else {
                body
            }
            .into_iter()
            .map(RenderedLine::from)
            .collect()
        }
        // 轨迹视图里用户（或非 assistant 的系统行）的消息同样只画首行 + `…`。
        Block::Message { speaker, text, .. } if view == Viewport::Trace => {
            let rows = attribute_speech(
                speaker,
                text.split('\n')
                    .map(|raw| Line::from(raw.to_owned()))
                    .collect(),
                colors,
                style,
            );
            vec![trace_message_row(speaker, rows, text, width, colors, style)]
        }
        // 用户自己的输入 —— 以及非 assistant 的系统行 —— 按写下来的样子显示：每一行都在，
        // 什么都不略去，也不上 Markdown，因为这不是一份文档（spec §3）。名字独占一行，
        // 话从下一行起、顶格（2026-10-05 维护者的排版修订）。
        Block::Message { speaker, text, .. } => {
            let body: Vec<Line<'static>> = if text.is_empty() {
                Vec::new()
            } else {
                text.split('\n')
                    .map(|raw| Line::from(raw.to_owned()))
                    .collect()
            };
            let rows = if name {
                named_rows(speaker, body, colors, style)
            } else {
                body
            };
            // 例外一（靠右）不在这里：那是**对话视图窗格**的排版，不是这一行的内容 ——
            // 共享渲染还要把它交给 `plain`，而那里的用户话照旧顶格（见
            // [`TuiState::emit_block`] 与 `.scratch/trace-tab/spec.md` §2）。
            rows.into_iter().map(RenderedLine::from).collect()
        }
        Block::Delta { .. } => Vec::new(),
        // 用户的回答走**用户消息**那条路 —— 同一个气泡、同一档颜色、同一份前缀。这一处转发
        // 是刻意的：样式只有一份，两条路就没有地方漂移（`.scratch/ui-trim/spec.md`）。
        Block::Answer { text } => {
            let as_speech = Block::Message {
                speaker: crate::events::SpeakerId::User,
                role: Role::User,
                text: text.clone(),
                reasoning: None,
            };
            paint_block(&as_speech, colors, width, style, view, name)
        }
        Block::RoundStarted { round, mode } => vec![
            Line::from(Span::styled(
                wording::round_section(*round, *mode),
                Style::default()
                    .fg(palette::MUTED)
                    .add_modifier(Modifier::BOLD),
            ))
            .into(),
        ],
        Block::RoundEnded { round, reason } => {
            vec![severity_line(*reason, wording::round_ended(*round, *reason)).into()]
        }
        Block::Divergence { topic, positions } => {
            let mut lines: Vec<RenderedLine> = vec![
                Line::from(Span::styled(
                    format!("!! {}", wording::divergence(topic)),
                    Style::default()
                        .fg(palette::PLAIN)
                        .add_modifier(Modifier::BOLD),
                ))
                .into(),
            ];
            for position in positions {
                lines.push(Line::from(format!("{}- {position}", wording::INDENT)).into());
            }
            lines
        }
        Block::Tool(tool) => tool_block_lines(tool, colors, style),
        // 后置 hook 的反馈，关于刚画出来的那次调用：一行普通的缩进行，黄色，因为说话的是策略
        // 而不是工具。
        Block::ToolFeedback { outcome, .. } => vec![
            Line::from(Span::styled(
                format!("{}{}", wording::INDENT, wording::hook_feedback(outcome)),
                Style::default().fg(palette::WARN),
            ))
            .into(),
        ],
        Block::TurnStarted { speaker, iteration } => vec![
            speaker_line(
                speaker,
                wording::turn_started(*iteration),
                Style::default().fg(palette::MUTED),
                colors,
                style,
            )
            .into(),
        ],
        Block::TurnEnded { speaker, reason } => {
            vec![
                severity_speaker_line(
                    speaker,
                    *reason,
                    wording::turn_ended(*reason),
                    colors,
                    style,
                )
                .into(),
            ]
        }
        Block::PermissionAsked {
            speaker,
            tool_name,
            args,
        } => vec![
            speaker_line(
                speaker,
                wording::permission_asked(tool_name.as_deref(), &summarize_args(args)),
                Style::default().fg(palette::MUTED),
                colors,
                style,
            )
            .into(),
        ],
        Block::PermissionDecided {
            speaker,
            decision,
            source,
            reason,
        } => vec![
            speaker_line(
                speaker,
                wording::permission_decided(*decision, *source, reason.as_deref()),
                Style::default().fg(palette::MUTED),
                colors,
                style,
            )
            .into(),
        ],
        Block::Hook {
            speaker,
            point,
            outcome,
        } => vec![
            speaker_line(
                speaker,
                wording::hook(point, outcome),
                Style::default().fg(palette::MUTED),
                colors,
                style,
            )
            .into(),
        ],
        Block::ExecutorSpawned {
            speaker,
            executor_id,
        } => vec![
            speaker_line(
                speaker,
                wording::executor_spawned(executor_id.as_str()),
                Style::default().fg(palette::MUTED),
                colors,
                style,
            )
            .into(),
        ],
        Block::ExecutorFinished {
            executor_id,
            reason,
            summary,
        } => vec![
            severity_line(
                *reason,
                wording::executor_finished(executor_id.as_str(), *reason, summary),
            )
            .into(),
        ],
        Block::Usage { speaker, usage } => vec![
            speaker_line(
                speaker,
                wording::usage_summary(usage),
                Style::default().fg(palette::MUTED),
                colors,
                style,
            )
            .into(),
        ],
        Block::AgentError { speaker, message } => vec![
            severity_speaker_line(
                speaker,
                StopReason::Error,
                wording::agent_error(message),
                colors,
                style,
            )
            .into(),
        ],
        Block::SessionError { code, detail } => {
            vec![severity_line(StopReason::Error, wording::session_error(code, detail)).into()]
        }
        Block::SessionEnded { reason } => {
            vec![severity_line(*reason, wording::session_ended(*reason)).into()]
        }
        Block::ContextInjected { source, content } => {
            // 这条记录**点得开**（票 19）：转录上只有一行来源名，加载数据本身在详情里。
            // 它拿一个**专色**（票 10，`.scratch/trace-tab/spec.md` §2 例外二）：注入行
            // 与别的叙述行同灰，于是「注入 / 用户 / 助手」在轨迹页上分不开。
            let text = wording::context_injected(source.clone());
            // `▸` 说这一行点得开 —— 注入行是**折起来**的：屏幕上只有来源名，加载数据本身在
            // 详情里，所以它按判据拿这个字形（`.scratch/tui-visual-language/spec.md` §13）。
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", wording::FOLDABLE),
                    Style::default().fg(palette::MUTED),
                ),
                Span::styled(text.clone(), Style::default().fg(palette::INJECTED)),
            ]);
            let detail = Detail {
                title: text,
                color: palette::INJECTED,
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
        Block::Diagnostic(message) => vec![
            Line::from(Span::styled(
                wording::diagnostic(message),
                Style::default().fg(palette::WARN),
            ))
            .into(),
        ],
        Block::CommandRun { text } => vec![command_line(text).into()],
        Block::Notice(message) => vec![narration(message.clone()).into()],
    }
}

/// 一条渲染事件发生在什么时候（`.scratch/trace-in-main/spec.md` §5）。
///
/// 进流的那些带着信封，所以时刻是准的；渲染层自己那两条（告知与诊断）的时刻由源头打上 ——
/// 它们不是事件，没有别的信封可依。增量绕过事件流、本就没有时刻，用收到它的那一刻顶上：
/// 它产出的行只有那条活尾巴与还开着的思考行，而思考行定稿时用的正是这里给的那一刻。
fn event_at(event: &RenderEvent) -> DateTime<Utc> {
    match event {
        RenderEvent::Logged(event) => event.at,
        RenderEvent::Diagnostic { at, .. } | RenderEvent::Notice { at, .. } => *at,
        RenderEvent::Delta { .. } => Utc::now(),
        // 系统提示词不属于任何一刻：它是**当前**的拼法，不是"那时"发生的事。
        RenderEvent::Identity { .. } => Utc::now(),
        // 它到不了这里：`apply` 在那之前就把它接走了。给一个当下的时刻只是为了 match 完整。
        RenderEvent::WorkspaceChanged => Utc::now(),
    }
}

/// 给一行按上它的时刻（块的**首行**与定稿的思考行都走它）。
fn stamped_line(line: Line<'static>, at: DateTime<Utc>) -> Line<'static> {
    let mut line = line;
    line.spans.insert(
        0,
        Span::styled(wording::stamp(at), Style::default().fg(palette::MUTED)),
    );
    line
}

/// 给一个块画出来的那些行按上时刻：只有**第一条**行带它，续行原样
/// （`.scratch/trace-in-main/spec.md` §5）。
///
/// 折行（一条太长的叙述被窗格切成几行）发生在**窗格**里，所以那些续行从第 0 列起 —— 一块
/// 一个时刻读起来仍然清楚，而让每一行都重复时刻反而是噪音。块自己排出来的续行（markdown
/// 折行的那几种）也一样：时刻是**块**的属性，不是每一行的。
fn stamp_lines(mut lines: Vec<RenderedLine>, at: DateTime<Utc>) -> Vec<RenderedLine> {
    if let Some(head) = lines.first_mut() {
        head.line.spans.insert(
            0,
            Span::styled(wording::stamp(at), Style::default().fg(palette::MUTED)),
        );
    }
    lines
}

/// 组头那一行：**行首的起时刻 + 组头的字 + 把它两侧填满的虚线**。
///
/// 它**并进**单位之间那条虚线（`.scratch/trace-ledger/spec.md` §5）：那一行本来就要画，而零额外
/// 行数正是这个形状的全部理由 —— 一个 15 行高的视口里，「头自己一行」要吃掉三分之一屏。
///
/// **一级头与二级头是两种形状**：一级占满一整行虚线（它是一条边界），二级是弱色单行、**不带
/// 虚线**（它只是那一段的标题）。
///
/// **展开态一律不用 `▸`** —— 它只表示「有折起来的东西」（票 21），而组头在这里是**边界**不是把手。
/// 无主段落小标题那一行：**起时刻 + 弱色的一行字**，不带虚线、**不带 `▸`**。
///
/// `▸` 只留给折叠态（票 21）—— 它在账本上表示「这里有折起来的东西」。所以展开态的小标题与组头
/// 都不用它，尽管 04 票的例子里带过（那一处与它 §10 的结论不一致，按结论走）。
fn section_header_line(header: &SectionHeader) -> Line<'static> {
    let text = match header.kind {
        SectionKind::Preamble => {
            let mut text = wording::section_preamble().to_owned();
            if header.injections > 0 {
                text.push_str(wording::header_separator());
                text.push_str(&wording::section_injections(header.injections));
            }
            text
        }
        SectionKind::Compaction => wording::section_compaction().to_owned(),
    };
    Line::from(vec![
        Span::styled(
            wording::stamp(header.at),
            Style::default().fg(palette::MUTED),
        ),
        Span::styled(text, Style::default().fg(palette::MUTED)),
    ])
}

fn group_header_line(header: &GroupHeader, width: u16, discussion: bool) -> Line<'static> {
    let sep = wording::header_separator();
    let span = wording::header_span(group_span(header));
    let head = match header.level {
        HeaderLevel::Unit => {
            let mut text = format!(
                "{}{sep}{span}",
                wording::header_unit(header.ordinal, discussion)
            );
            // **拿不到的字段整个不画**：工具直方图此刻还是空的（工具还没跑）就不给它留地方。
            let tools = wording::header_tools(&header.tools);
            if !tools.is_empty() {
                text.push_str(sep);
                text.push_str(&tools);
            }
            text
        }
        HeaderLevel::Iteration => {
            format!("{}{sep}{span}", wording::header_iteration(header.ordinal))
        }
    };
    let style = match header.level {
        // 一级是**边界**：它与虚线同一支，文字亮一档。
        HeaderLevel::Unit => Style::default().add_modifier(Modifier::BOLD),
        // 二级只是那一段的标题，退后。
        HeaderLevel::Iteration => Style::default().fg(palette::MUTED),
    };
    let mut spans = vec![
        // 行首那九列放**这一组自己的**起时刻 ⇒ 所有行的时刻列竖向对齐不变。
        Span::styled(
            wording::stamp(header.at),
            Style::default().fg(palette::MUTED),
        ),
    ];
    match header.level {
        HeaderLevel::Unit => {
            spans.push(Span::styled("┄ ", Style::default().fg(palette::CHROME)));
            spans.push(Span::styled(head.clone(), style));
            // 虚线填到屏幕右缘 —— 那一行因此读起来是**一条边界**，而不是一行孤零零的字。
            let used = layout::STAMP_COLUMNS as usize + 2 + text_columns(&head) + 1;
            spans.push(Span::styled(" ", Style::default()));
            spans.push(Span::styled(
                "┄".repeat((width as usize).saturating_sub(used)),
                Style::default().fg(palette::CHROME),
            ));
        }
        HeaderLevel::Iteration => {
            spans.push(Span::styled(head, style));
        }
    }
    Line::from(spans)
}

/// 一条**中间**叙述行：用静音档，好让模型那个以正文档渲染的回答成为显眼的东西。带严重度的
/// 行改为问色板（见 [`severity_line`]）。
fn narration(text: String) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(palette::MUTED)))
}

fn severity_line(reason: StopReason, text: String) -> Line<'static> {
    Line::from(Span::styled(text, severity_style(reason)))
}

/// 人运行的一条 `/` 命令（`.scratch/command-echo/spec.md`）。
///
/// 用草稿里那个 `TOKEN_COMMAND` 蓝：同一个东西同一个颜色，命令在输入区里是这个样子、
/// 在转录里也是（`/ask` 那条纪律的另一处）。它**不是** `narration` 那一档静音 —— 这一行是
/// 读的人唯一能查「我刚才敲了什么」的地方，而命令常常什么都不说。
fn command_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        wording::command_run(text),
        Style::default().fg(palette::TOKEN_COMMAND),
    ))
}

/// 一条点名了发言者的严重度行：名字保持发言者的颜色，这一行的其余部分保持严重度的颜色
/// （票 07 §2）。这样一个错误仍然读起来像个错误，而读的人不会丢掉它是谁弄出来的。
fn severity_speaker_line(
    speaker: &crate::events::SpeakerId,
    reason: StopReason,
    text: String,
    colors: &mut SpeakerColors,
    style: PrefixStyle,
) -> Line<'static> {
    speaker_line(speaker, text, severity_style(reason), colors, style)
}

/// 一个停止点把它那一行画成什么颜色：语义归属仍只有 [`Severity::of`] 一处，颜色问色板
/// （`.scratch/tui-visual-language/spec.md` §4）。`Good` 与 `Note` 因此退成静音 —— 正常完成
/// 不再抢注意力，只有出问题时屏幕才亮。
fn severity_style(reason: StopReason) -> Style {
    palette::style(Severity::of(reason))
}

/// 一次已完成的工具调用，折成一行：读的人能打开的**那次调用**，整份输出在它后面（票 02 §3）。
///
/// 失败是同一行在**末尾**多一个 `失败` —— 不是第二行 —— 而错误正文的**首行**另起一行、从内容
/// 起点（第 9 列）起排、穿静音档：不点开也知道坏在哪，而失败仍然一眼读得出来（票 17 第 5 条）。
/// `ok = false` 却没有错误正文时那一行不画。行首**不加 `✗`** —— 那要让全体行的内容宽降两列。
///
/// 后置 hook 的反馈是自己的一个块、留在屏幕上：它是策略的反馈，不是工具输出，所以不点也必须是
/// 可读的（票 02 §3）。
fn tool_block_lines(
    tool: &ToolBlock,
    colors: &mut SpeakerColors,
    style: PrefixStyle,
) -> Vec<RenderedLine> {
    let outcome = tool.outcome.as_ref();
    let failed = matches!(outcome, Some(outcome) if !outcome.ok);
    let color = colors.of(&tool.speaker);
    let mut call = vec![
        // 名字打头，于是每条转录行都以谁在说话开头；它后面那个标记说的是这行可以打开。它是
        // 绘制、不是文案，所以不是那句话的一部分（票 03 §Answer）。
        Span::styled(style.prefix(&tool.speaker), Style::default().fg(color)),
        Span::styled(
            format!("{} ", wording::FOLDABLE),
            Style::default().fg(palette::MUTED),
        ),
        // 这次调用是*为了*什么：一条**过程**行，所以穿静音档；参数本身离一次点击之遥
        // （票 02 §2，2026-09-23 修正）。
        Span::styled(
            wording::tool_call_line(&tool.tool, &tool.args),
            Style::default()
                .fg(palette::MUTED)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if failed {
        // 失败是**信号**：界面域里只有它和诊断这一类会亮（§6）。
        call.push(Span::styled(
            format!(" {}", wording::tool_failed()),
            Style::default().fg(palette::BAD),
        ));
    }
    // 结果摘要**只在异常时给一句**：常规成功行一个字也不加（票 17 第 6 条）。
    if let Some(note) = tool_result_note(outcome) {
        call.push(Span::styled(note, Style::default().fg(palette::MUTED)));
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
    let mut lines = vec![RenderedLine::linked(Line::from(call), detail.clone())];
    if let Some(first) = error_first_line(outcome) {
        // 内容起点就是行首那 9 列之后：这一行没有自己的时刻戳，所以补足那 9 列，好让错误正文
        // 与它上面那次调用的摘要**同一列起**（票 17 第 5 条）。
        lines.push(RenderedLine::linked(
            Line::from(vec![
                Span::raw(" ".repeat(layout::STAMP_COLUMNS as usize)),
                Span::styled(first, Style::default().fg(palette::MUTED)),
            ]),
            detail,
        ));
    }
    lines
}

/// 一次调用的**结果**值得在行上说一句时的那一句话（票 17 第 6 条）。
///
/// 只有两种异常说：空输出，与被切过。常规成功行一个字也不加，失败由那个红 `失败` 与它下面
/// 那一行错误正文说 —— 所以这里不再补。
///
/// 「空输出」按票 04 §6 的口径：`ok = true` 而 `output` 是 `None`、或者是一段空的都算 ——
/// 两者对这个读者是同一件事。
fn tool_result_note(outcome: Option<&ToolOutcome>) -> Option<String> {
    let outcome = outcome?;
    if !outcome.ok {
        return None;
    }
    // 结果**还没有到**的那一块走的是上面那一行（`outcome` 是 `None`），所以走到这里的
    // `None` 只有「结果字段是空的」一种意思。
    let output = outcome.output.as_deref().unwrap_or_default();
    if let Some(note) = wording::tool_truncation_note(output) {
        return Some(note);
    }
    output
        .trim()
        .is_empty()
        .then(|| wording::tool_no_output().to_owned())
}

/// 一次**失败**留下的错误正文首行 —— 失败行下面那一行画的就是它。
///
/// 整段错误仍然在详情里；这里只取一行，且空的那一行不画（票 17 第 5 条）。
fn error_first_line(outcome: Option<&ToolOutcome>) -> Option<String> {
    let outcome = outcome?;
    if outcome.ok {
        return None;
    }
    let error = outcome.error.as_deref()?;
    let first = error.lines().next().unwrap_or_default();
    (!first.trim().is_empty()).then(|| first.to_owned())
}

/// 这一块行尾该挂的**耗时**那一段 —— 只有工具块有这一笔（票 17 第 1 条）。
///
/// **拿不到就不画**：一次调用还没有结果时（还在跑、或者它那一块根本没有结果），它的墙钟不在
/// 手上，于是这里给 `None`，行上空着 —— 不写 0，也不写占位符（第 3 条）。
fn tool_duration(block: &Block) -> Option<String> {
    let Block::Tool(tool) = block else {
        return None;
    };
    tool.outcome
        .as_ref()
        .map(|outcome| wording::tool_duration_tail(outcome.duration_ms))
}

/// 把行尾那两段账目拼到最后一条行上：**这次调用自己花了多久**（只有工具块有这一笔），
/// 然后是**尾巴**（一笔用量，或一次发言的合计）。
///
/// 尾巴补在**最后一条**行尾：那是这一块读下来的落脚点，于是实时与重放画出来的是同一行
/// （ADR 0016、`.scratch/trace-usage-tail/spec.md` §3、§5）。
///
/// 顺序是耗时在前、尾巴在后（票 17 第 1 条）。**放不下时丢的是耗时**：行尾是用量的家，
/// 而耗时是后来者 —— 两段一起挤不下时留下的是那笔用量（第 2 条）。拿不到耗时的那些情形
/// （这次调用没有结果、行太窄）那一段就空着，不写 0 也不写占位符（第 3 条）。
fn attach_row_tail(
    line: Option<&mut RenderedLine>,
    duration: Option<String>,
    tail: Option<Tail>,
    room: usize,
) {
    let Some(last) = line else { return };
    let duration_width = duration.as_deref().map(text_columns).unwrap_or(0);
    let tail_width = tail.map(|tail| tail.span().width()).unwrap_or(0);
    if line_columns(&last.line) + duration_width + tail_width <= room
        && let Some(duration) = duration
    {
        last.line
            .spans
            .push(Span::styled(duration, Style::default().fg(palette::MUTED)));
    }
    if let Some(tail) = tail {
        last.line.spans.push(tail.span());
    }
}

/// 对话视图里一条消息的排版：**名字独占一行**，话从下一行起、顶格
/// （2026-10-05 维护者的排版修订）。
fn named_rows(
    speaker: &crate::events::SpeakerId,
    rows: Vec<Line<'static>>,
    colors: &mut SpeakerColors,
    style: PrefixStyle,
) -> Vec<Line<'static>> {
    let mut out = vec![Line::from(Span::styled(
        style.label(speaker),
        name_style(speaker, colors),
    ))];
    out.extend(rows);
    out
}

/// 用户自己的话在**对话视图**里排成一个**气泡**：一块等宽的底色，贴着转录的右缘，宽度封在
/// 可用列数的 [`BUBBLE_WIDTH`] 以内（`.scratch/trace-tab/spec.md` §2 的补记）。
///
/// 名字行**不进气泡** —— 它自己一行，右端与气泡的右缘对齐。
///
/// 靠右用的是 `Alignment::Right`，而左边界由**等宽**保证：每一行右侧都补齐到同一个宽度，于是
/// 右对齐之后每一行从同一列起。补齐的那些空格留在行尾，取文本时被 `trim_end` 去掉，所以拖选
/// 复制出来的只有正文本身（`.scratch/tui-feedback/spec.md` §6）。底色铺在行自己的样式上，
/// 填充那些空格也带着它 —— 于是这一块是**一个**矩形，而不是每行各一条。
fn bubble(lines: &mut Vec<RenderedLine>, width: u16, named: bool) {
    let named_rows = usize::from(named);
    // 名字行先行：连正文都没有的那条消息（空文本）也留着一个靠右的名字。
    for row in lines.iter_mut().take(named_rows) {
        row.line.alignment = Some(Alignment::Right);
    }
    if lines.len() <= named_rows {
        return;
    }
    let inner = bubble_inner(width);
    // 先按气泡的宽度折行，再拿**折出来的最宽那行**当块宽：一条短消息的气泡就是窄的。
    let mut body: Vec<Line<'static>> = Vec::new();
    for line in lines.drain(named_rows..) {
        body.extend(pane::wrap_line(&line.line, inner));
    }
    let block = body.iter().map(line_columns).max().unwrap_or(0).min(inner);
    for line in &mut body {
        let pad = block.saturating_sub(line_columns(line));
        if pad > 0 {
            line.spans.push(Span::raw(" ".repeat(pad)));
        }
        line.style = Style::default().bg(palette::BUBBLE);
        line.alignment = Some(Alignment::Right);
    }
    lines.extend(body.into_iter().map(RenderedLine::from));
}

/// 气泡里的内容能占几列。
fn bubble_inner(width: u16) -> usize {
    let (numerator, denominator) = BUBBLE_WIDTH;
    (usize::from(width) * usize::from(numerator) / usize::from(denominator)).max(1)
}

/// 一个气泡最多占转录内容宽度的几分之几。
///
/// 留白是气泡感的来源：一条顶满宽度的用户消息读起来与助手正文没有分别，而右边那一大片空白
/// 正是「这句是我说的」最省事的说法（`.scratch/trace-tab/spec.md` §2 的补记）。
const BUBBLE_WIDTH: (u16, u16) = (2, 3);

/// 一条带样式的显示行占多少列。
fn line_columns(line: &Line<'static>) -> usize {
    line.spans
        .iter()
        .flat_map(|span| span.content.chars())
        .map(char_columns)
        .sum()
}

/// 轨迹视图里的一条消息行：**首行 + `…`**，全文挂在详情里
/// （`.scratch/trace-tab/spec.md` §3）。
///
/// `rows` 是这条消息按轨迹宽度照排出来的那些行 —— 只取第一行，所以表格、代码块的首行走的
/// 仍是它们本来的排版。还有更多行、或者首行本身就超宽时，就截到那一行并加 `…`。
fn trace_message_row(
    speaker: &crate::events::SpeakerId,
    rows: Vec<Line<'static>>,
    text: &str,
    width: u16,
    colors: &mut SpeakerColors,
    style: PrefixStyle,
) -> RenderedLine {
    let mut rows = rows.into_iter();
    let mut head = rows.next().unwrap_or_else(|| {
        // 一条空消息也要有个名字，好让读者知道这里有一条消息。
        Line::from(Span::styled(
            style.prefix(speaker),
            Style::default().fg(colors.of(speaker)),
        ))
    });
    // 轨迹视图的消息行只画首行，全文在详情里 —— 谁把内容折起来谁就有 `▸`，所以这一行有，
    // 而对话视图那条（已显全文）没有（`.scratch/tui-visual-language/spec.md` §13）。
    if !head.spans.is_empty() {
        head.spans.insert(
            1,
            Span::styled(
                format!("{} ", wording::FOLDABLE),
                Style::default().fg(palette::MUTED),
            ),
        );
    }
    let more = rows.next().is_some();
    let over = text_columns(&line_text(&head)) > width as usize;
    if more || over {
        head = ellipsize_line(head, width as usize);
    }
    let detail = Detail {
        title: line_text(&head),
        color: colors.of(speaker),
        kind: DetailKind::Message {
            text: text.to_owned(),
        },
    };
    RenderedLine::linked(head, detail)
}

/// 一条来源行是不是用户自己的消息，那正是回合条一格跳转所瞄准的（spec §4）。
/// 这个块是不是**用户说的话**（气泡、以及对话窗格里靠右那一版排版都认它）。
///
/// 两条来路：他自己打的那条消息，以及他对一次问卷的作答（`Block::Answer`）—— 后者不是流上
/// 的消息，但屏幕上它是同一件事（`.scratch/ui-trim/spec.md`）。
fn is_user_message(block: &Block) -> bool {
    matches!(block, Block::Answer { .. })
        || matches!(
            block,
            Block::Message {
                speaker: crate::events::SpeakerId::User,
                ..
            }
        )
}

/// 一个块是谁说的（不说话的那些是 `None`）。
///
/// 「换发言者空一行」只认它：叙述行、轮次边界、上下文注入这些没有发言者的块**不改写**上一次
/// 的发言者，于是两次之间不会凭空多出空行。
fn block_speaker(block: &Block) -> Option<&crate::events::SpeakerId> {
    match block {
        Block::Delta { speaker, .. }
        | Block::Message { speaker, .. }
        | Block::TurnStarted { speaker, .. }
        | Block::TurnEnded { speaker, .. }
        | Block::PermissionAsked { speaker, .. }
        | Block::PermissionDecided { speaker, .. }
        | Block::Hook { speaker, .. }
        | Block::ExecutorSpawned { speaker, .. }
        | Block::Usage { speaker, .. }
        | Block::AgentError { speaker, .. } => Some(speaker),
        Block::Tool(tool) => Some(&tool.speaker),
        // 问卷的作答是用户说的：与他自己打的那条消息同一档（`.scratch/ui-trim/spec.md`）。
        Block::Answer { .. } => Some(&crate::events::SpeakerId::User),
        _ => None,
    }
}

/// 一个块进不进这个视图 —— 分工的**唯一**判据（`.scratch/trace-tab/spec.md` §2）。
///
/// 轨迹视图是**全量**；对话视图只留用户文本、assistant 正文，加一份枚举出来的保留清单：
/// 错误、会话中断、失败的 hook、`Notice` 整类，以及**非正常**的收尾行（`TurnEnded` /
/// `RoundEnded` 里严重度不是 `Good` 的那些）。其余全归轨迹 —— 回合与轮次的开始、权限询问与
/// 裁决、诊断、工具、思考、用量、注入这些过程行都不在对话视图里（2026-10-05 维护者收紧）。
fn selects(view: Viewport, block: &Block) -> bool {
    if view == Viewport::Trace {
        return true;
    }
    match block {
        // 执行者自己的字全归轨迹（冻结项 10）。
        Block::Message {
            speaker: crate::events::SpeakerId::Executor(_),
            ..
        } => false,
        // 用户文本与 assistant 正文 —— 对话本来就该只有这些。
        Block::Message { .. } => true,
        // 用户对问卷的作答与他打的话同一档，所以同样留在对话视图里。
        Block::Answer { .. } => true,
        // 命令记录与作答同一档的理由：它回答的是「我刚才做了什么」，而它不进模型上下文
        // （`.scratch/command-echo/spec.md`）。轨迹视图照旧有全量。
        Block::CommandRun { .. } => true,
        // 「为什么没有回答」的那几类立刻要知道（冻结项 8）。
        Block::AgentError { .. } | Block::SessionError { .. } | Block::SessionEnded { .. } => true,
        // hook 只在**失败**时是说给用户的；成功的那条留在轨迹里。
        Block::Hook { outcome, .. } => crate::events::hook_format::is_failed(outcome),
        // 收尾行：**正常**的收尾是过程行，进轨迹；非正常的收尾是说给人听的「为什么停下来」，
        // 留在对话视图（2026-10-05 维护者收紧，取代了「边界行都留对话」那条）。
        Block::TurnEnded { reason, .. } | Block::RoundEnded { reason, .. } => {
            Severity::of(*reason) != Severity::Good
        }
        // `Notice` 整类留下（冻结项 8、票 06）：命令回执、启动横幅、错误报告、目标与重试
        // 提示、历史分隔线都在内。
        Block::Notice(_) => true,
        // 其余全进轨迹：回合 / 轮次的**开始**、权限询问与裁决、工具与它的反馈、用量、分歧、
        // 沙箱、历史、上下文注入、执行者进出、诊断、流式增量。
        _ => false,
    }
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
    /// 一条消息的全文 —— 轨迹视图里那行只画首行 + `…`，正文住在这里
    /// （`.scratch/trace-tab/spec.md` §3）。
    Message { text: String },
    /// 工作区里的一个文件的内容（`.scratch/files-page/spec.md` §6）。正文在**打开那一刻**
    /// 由渲染器直接读盘，带着它自己的有界截断 —— 这个弹窗不进事件流、不进模型上下文。
    File { body: files::FileBody },
    /// 一个文件的改动（`.scratch/diff-page/spec.md` §6）。
    ///
    /// 正文与 `File` 一样在**打开那一刻**取，只是取法分两条：已跟踪的跑一次
    /// `git diff HEAD -- <path>`（那是子进程，所以弹窗先立起来、正文后到），未跟踪的直接读盘
    /// 给全文。这个弹窗与 `File` 同一条纪律：不进事件流、不进模型上下文、不打码。
    Diff { path: String, body: changes::Body },
    /// 一份待办清单：左栏 `todo` 页整份点开就是它（`.scratch/todo-page/spec.md` §6）。
    ///
    /// 正文是**打开那一刻**的那份列表的一份拷贝 —— 弹窗是快照，模型之后提交的新列表不重画
    /// 它，关掉重开才是新的那份。
    Todo {
        items: Vec<crate::tools::todo::Item>,
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

/// 打开详情覆盖层的那个视图，以及它打开前的滚动状态；关掉时还原给**它**
/// （`.scratch/files-page/spec.md` §5、§6）。
///
/// 打开方决定覆盖层立着时**谁冻在原处**，以及关掉时把什么还原回去 —— 那件事过去写死是轨迹页，
/// 因为它是唯一的打开方（票 05）。只有轨迹页需要冻住与还原：文件页在覆盖层下面的位置由它自己
/// 那份滚动与展开状态拿着，关掉一个弹窗不该动它。
#[derive(Debug, Clone, Copy)]
enum DetailOpener {
    /// 轨迹页打开：记下它当时在哪儿（视口顶端那一个显示行与它跟不跟底），关掉时还原。
    Trace {
        /// 打开时轨迹页视口顶端所在的显示行。
        top: usize,
        /// 打开时它跟不跟底。
        follow: bool,
    },
    /// 文件页打开：不冻也不还原任何视图。
    Files,
    /// 改动页打开：与文件页一样，不冻也不还原 —— 这一页的位置由它自己那份列表与焦点拿着
    /// （`.scratch/diff-page/spec.md` §6）。
    Changes,
    /// `todo` 页打开：同样什么都不冻、什么都不还 —— 这一页没有滚动、没有焦点行，
    /// 关掉之后左栏仍停在它上面（`.scratch/todo-page/spec.md` §5）。
    Todo,
}

/// 详情覆盖层的打开状态（票 02 §4）。
///
/// 它是一个**视图模式，不是一个待答的问题**：转录冻在原处，键盘与滚轮在它被关掉之前归主体
/// 所有，而且没有置任何 `pending` —— 这正是让问题守卫不会把瞄准覆盖层的滚轮吞掉的原因。
/// 文件查看器那一档的浮层：一屏外来画面，加上它上一次被画在哪一块网格上。
///
/// 与 [`DetailView`] 同一族，也共用同一条纪律：这些值由**画它的那一处**填上，指针只回应
/// 这一帧真画出来的东西。
struct FileViewerPane {
    viewer: Box<dyn Viewer>,
    /// 交给 nvim 的那块网格。它变了才 `resize` —— 每帧都发一次 `TIOCSWINSZ` 是白花的开销。
    grid: Rect,
}

/// 一份正在读的 diff（`.scratch/diff-page/spec.md` §6）。
#[derive(Debug, Clone)]
struct DiffReading {
    /// 它属于哪一次打开。回来时对不上号就丢掉 —— 那时弹窗已经换成别的文件、或者关掉了。
    serial: u64,
    /// 读哪个文件。
    file: changes::ChangedFile,
    /// 正文文本区有多宽。外部工具那一档要把它当 `COLUMNS` 传出去，好让它按同一个宽度排
    /// （`.scratch/diff-page/spec.md` §8）。
    columns: usize,
    /// 循环已经把这一次发出去了吗。同一次不重复发（`take_diff_read` 的守卫）。
    sent: bool,
}

struct DetailView {
    /// 正在显示什么。
    detail: Detail,
    /// 主体，按它被打开时的宽度排版。
    body: Vec<DetailLine>,
    /// 打开时用的**正文文本区**宽度。改动页那一档要拿它重排：一次 `git diff` 的结果比弹窗
    /// 晚到，那时要按同一个宽度再排一遍（`.scratch/diff-page/spec.md` §6）。
    width: usize,
    /// 屏幕上主体的第一行。
    top: usize,
    /// 覆盖层一次能显示多少主体行。
    height: usize,
}

/// 详情主体的一条显示行：它自己，以及它是不是上一行**折出来**的续行。
///
/// 折行信息是给拖选用的（`.scratch/tui-feedback/spec.md` §6）：主体按覆盖层的宽度排过版，复制
/// 时那些续行要拼回一条，而正文里自己带的换行保留。
#[derive(Debug, Clone)]
struct DetailLine {
    line: Line<'static>,
    folded: bool,
}

impl DetailLine {
    /// 一条没有软折的行（小节标题、一句话的降级说明）。
    fn plain(line: Line<'static>) -> Self {
        Self {
            line,
            folded: false,
        }
    }
}

/// 轨迹页两级分组的当前进度 —— 推块的副产物，重放时从头再来一遍
/// （`.scratch/trace-ledger/spec.md` §5）。
#[derive(Debug, Default)]
struct TraceGroups {
    /// 已经开过几个一级组：下一个组头的序号是它 + 1。
    units: u32,
    /// 当前这一级组（一个回合 / 一轮），开着的时候是 `Some`。
    unit: Option<OpenGroup>,
    /// 当前这一个迭代（一次模型调用）。它在**每个** `TurnStarted` 上换新，所以它比一级组
    /// 换得勤。
    iteration: Option<OpenGroup>,
    /// 开场那一段的小标题 —— 一个单位还没开出来之前的那些块属于它。
    preamble: Option<BlockId>,
}

/// 一个正开着的那一组。
#[derive(Debug)]
struct OpenGroup {
    /// 组头那条**已经画出去**的行 —— 改写它靠 `first_source_of` 找回那一个来源行下标，
    /// 再交给 `Pane::replace_at`。
    header: BlockId,
}

impl TraceGroups {
    /// 这一批块里有没有**开一个新的一级组**（单位）。
    fn opens_unit(&self, block: &Block, discussion: bool) -> bool {
        if discussion {
            matches!(block, Block::RoundStarted { .. })
        } else {
            matches!(block, Block::TurnStarted { iteration: 1, .. })
        }
    }

    /// 这一批块里有没有**开一个新的迭代**（二级组）。讨论里**每次** `TurnStarted` 都是一个
    /// 迭代 —— 那时候的分组靠 `[名字]` 前缀区分发言者，不另立一级（`.scratch/trace-ledger/
    /// spec.md` §5）。
    fn opens_iteration(&self, block: &Block, discussion: bool) -> bool {
        if discussion {
            matches!(block, Block::TurnStarted { .. })
        } else {
            matches!(block, Block::TurnStarted { iteration, .. } if *iteration > 1)
        }
    }
}

/// 这一级组的序号。
///
/// 一级从 **1** 起数（第一个回合就是「回合 1」）；二级的序号**直接取事件里的 `iteration`** ——
/// 它本来就是那个数，自己另编一套只会与它漂开（`.scratch/trace-ledger/spec.md` §5）。
fn unit_ordinal(groups: &TraceGroups) -> u32 {
    groups.units + 1
}

/// 组的跨度：定稿了就用定稿那一刻的，**还在跑就用此刻的**。于是实时与重放画出来的是同一行
/// （重放时那一组多半还没定稿，而此刻正是重放的那一刻）。
fn group_span(header: &GroupHeader) -> std::time::Duration {
    header.span.unwrap_or_else(|| {
        Utc::now()
            .signed_duration_since(header.at)
            .to_std()
            .unwrap_or_default()
    })
}

/// 一段正文按 `width` 折行，并标出哪些是折出来的**续行**。
///
/// 与 [`pane::wrap_text`] 同一套折法，只是多带一个「这一行是哪个逻辑段折出来的第几片」——
/// 换行按逻辑段判，不按显示行判。
fn folded_text(text: &str, width: usize) -> Vec<DetailLine> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n')
        .flat_map(|raw| {
            pane::wrap_line(&Line::from(raw.to_owned()), width)
                .into_iter()
                .enumerate()
                .map(|(index, line)| DetailLine {
                    line,
                    folded: index > 0,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// 一个详情主体会从落盘的工具输出里读的最多字符数。
///
/// 一条工具结果在进事件流之前就被截过，但落盘的那个文件没有：这是读的人自己的上限，超过它
/// 主体以 [`wording::detail_truncated`] 结尾（票 02 §4）。
const DETAIL_MAX_CHARS: usize = 200_000;

impl TuiState {
    /// 为读的人点的那一行打开详情覆盖层。
    ///
    /// 主体在这里、在打开的那一刻读，并按**正文文本区**将被画出来的宽度排版（框宽减掉边框
    /// 与内边距），于是此后滚动是纯算术，而正文不会被框边裁掉一截
    /// （`.scratch/files-page/spec.md` §6）。
    ///
    /// 打开方一起记下来：覆盖层立着时**只冻打开它的那一页**，关掉时也只还原它
    /// （票 05）。轨迹页那条路径的行为与改动前逐字相同。
    fn open_detail(&mut self, detail: Detail, width: usize, opener: DetailOpener) {
        self.detail_opener = Some(opener);
        let body = detail_body(&detail, &self.facts.session_dir, width);
        self.detail = Some(DetailView {
            detail,
            body,
            width,
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
            // 那一份还在读的 diff 跟着一起作废：弹窗都关了，回来的结果没有去处
            // （下一次打开会给自己一个新的序号）。
            self.diff_reading = None;
            // 还原给**打开它的那一页**：轨迹页回到底部或原处，文件页什么都不动
            // （票 13、`.scratch/files-page/spec.md` §5）。
            match self.detail_opener.take() {
                Some(DetailOpener::Trace { top, follow }) => {
                    self.trace.set_holding(false);
                    self.trace.restore(top, follow);
                }
                Some(DetailOpener::Files)
                | Some(DetailOpener::Changes)
                | Some(DetailOpener::Todo)
                | None => {}
            }
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
fn detail_body(detail: &Detail, session_dir: &str, width: usize) -> Vec<DetailLine> {
    let mut rows: Vec<DetailLine> = Vec::new();
    match &detail.kind {
        DetailKind::Message { text } => {
            rows.push(DetailLine::plain(section_header(
                wording::detail_message_section(),
            )));
            rows.extend(folded_text(text, width));
        }
        DetailKind::Thinking { text } => {
            rows.push(DetailLine::plain(section_header(
                wording::detail_thinking_section(),
            )));
            match text {
                Some(text) if !text.trim().is_empty() => {
                    rows.extend(folded_text(text.trim_end(), width));
                }
                // 没有记录下来的 trace —— 合成器的形状 —— 所以主体把它说出来，而不是开成空白
                // （票 02 §1）。
                _ => rows.push(DetailLine::plain(Line::from(Span::styled(
                    wording::detail_reasoning_unrecorded(),
                    Style::default().fg(palette::MUTED),
                )))),
            }
        }
        DetailKind::File { body } => {
            // 路径本身就是标题，正文里不再重复一个小节标题。
            match body {
                files::FileBody::Text { text, truncated } => {
                    rows.extend(file_body_lines(&detail.title, text, width));
                    if *truncated {
                        rows.push(DetailLine::plain(Line::from(Span::styled(
                            wording::detail_truncated(),
                            Style::default().fg(palette::MUTED),
                        ))));
                    }
                }
                files::FileBody::Binary => rows.push(DetailLine::plain(Line::from(Span::styled(
                    wording::file_binary(),
                    Style::default().fg(palette::MUTED),
                )))),
                files::FileBody::Unreadable => {
                    rows.push(DetailLine::plain(Line::from(Span::styled(
                        wording::file_unreadable(),
                        Style::default().fg(palette::MUTED),
                    ))))
                }
                files::FileBody::NotText => rows.push(DetailLine::plain(Line::from(Span::styled(
                    wording::file_not_text(),
                    Style::default().fg(palette::MUTED),
                )))),
            }
        }
        DetailKind::Diff { path, body } => match body {
            // 还在读：一句话占位。弹窗先立起来，是为了让那一下点击**立刻**有回应 ——
            // 一次 `git diff` 的结果下一轮才回来（§6）。
            changes::Body::Pending => rows.push(DetailLine::plain(Line::from(Span::styled(
                wording::changes_diff_loading(),
                Style::default().fg(palette::MUTED),
            )))),
            changes::Body::Patch(text) => {
                let (lines, skipped) = changes::clamp(text);
                let patch = changes::patch_rows(&lines);
                // 只有头行的 diff（`old mode` / `new mode` 那类，或者一份空 diff）：给一句
                // 话，而不是一块空白 —— 空白说明不了「这份改动本来就没有正文」。
                if patch.is_empty() {
                    rows.push(DetailLine::plain(Line::from(Span::styled(
                        wording::changes_nothing_to_show(),
                        Style::default().fg(palette::MUTED),
                    ))));
                }
                // N 数的是**画出来的那些行**（头行不算），这样读者不必自己减掉四行。
                let shown = patch.len();
                rows.extend(diff_body_lines(path, &patch, width));
                if skipped > 0 {
                    rows.push(DetailLine::plain(Line::from(Span::styled(
                        wording::changes_truncated(shown, skipped),
                        Style::default().fg(palette::MUTED),
                    ))));
                }
            }
            // 未跟踪的新文件：没有 diff 可比，正文就是全文（标题已经写了「新文件」）。
            changes::Body::NewFile(body) => match body {
                files::FileBody::Text { text, truncated } => {
                    rows.extend(file_body_lines(path, text, width));
                    if *truncated {
                        rows.push(DetailLine::plain(Line::from(Span::styled(
                            wording::detail_truncated(),
                            Style::default().fg(palette::MUTED),
                        ))));
                    }
                }
                files::FileBody::Binary => rows.push(DetailLine::plain(Line::from(Span::styled(
                    wording::changes_binary(),
                    Style::default().fg(palette::MUTED),
                )))),
                files::FileBody::Unreadable => {
                    rows.push(DetailLine::plain(Line::from(Span::styled(
                        wording::changes_diff_unreadable(),
                        Style::default().fg(palette::MUTED),
                    ))))
                }
                files::FileBody::NotText => rows.push(DetailLine::plain(Line::from(Span::styled(
                    wording::file_not_text(),
                    Style::default().fg(palette::MUTED),
                )))),
            },
            // 外部工具画的那一版（`[ui] diff_viewer`）：它吐的东西已经解成了带样式的片，
            // 仍走我们自己的折行与截断（§8）。
            changes::Body::External {
                lines,
                skipped,
                tool: _,
            } => {
                let shown = lines.len();
                rows.extend(external_body_lines(lines, width));
                if *skipped > 0 {
                    rows.push(DetailLine::plain(Line::from(Span::styled(
                        wording::changes_truncated(shown, *skipped),
                        Style::default().fg(palette::MUTED),
                    ))));
                }
            }
            changes::Body::Binary => rows.push(DetailLine::plain(Line::from(Span::styled(
                wording::changes_binary(),
                Style::default().fg(palette::MUTED),
            )))),
            changes::Body::Unreadable => rows.push(DetailLine::plain(Line::from(Span::styled(
                wording::changes_diff_unreadable(),
                Style::default().fg(palette::MUTED),
            )))),
        },
        DetailKind::Context { source, content } => {
            rows.push(DetailLine::plain(section_header(&wording::context_source(
                source,
            ))));
            rows.extend(folded_text(content, width));
        }
        DetailKind::Todo { items } => {
            rows.extend(
                crate::render::todo::detail_rows(items, width)
                    .into_iter()
                    .map(DetailLine::plain),
            );
        }
        DetailKind::Tool {
            tool_call_id,
            output,
            error,
            args,
            no_result,
        } => {
            rows.push(DetailLine::plain(section_header(
                wording::detail_args_section(),
            )));
            let args = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
            rows.extend(folded_text(&args, width));
            rows.push(DetailLine::plain(section_header(
                wording::detail_output_section(),
            )));
            if *no_result {
                rows.push(DetailLine::plain(Line::from(Span::styled(
                    wording::no_tool_result(),
                    Style::default().fg(palette::MUTED),
                ))));
            } else if let Some(error) = error {
                rows.extend(folded_text(error, width));
            } else if let Some(output) = output {
                let (body, truncated) = read_tool_body(tool_call_id, output, session_dir);
                rows.extend(folded_text(&body, width));
                if truncated {
                    rows.push(DetailLine::plain(Line::from(Span::styled(
                        wording::detail_truncated(),
                        Style::default().fg(palette::MUTED),
                    ))));
                }
            } else {
                rows.push(DetailLine::plain(Line::from(Span::styled(
                    wording::detail_output_unavailable(),
                    Style::default().fg(palette::MUTED),
                ))));
            }
        }
    }
    rows
}

/// 一个文件正文的那些行：语法高亮（按路径扩展名认语言）加上每个逻辑行前面的行号
/// （`.scratch/files-page/spec.md` §6）。
///
/// 语言那一层与 markdown 代码块**同一个源**：认出的语言交给 `highlight_code`，它吐逐行的
/// 样式 span；认不出（或那一层没有这份文法）时正文退纯文本，但**不消失**。
///
/// 行号插在折行**之前** —— 折出来的续行因此自然不带行号，这是那份 spec 选中的形状。行号
/// 那一列从折行预算里扣掉，所以它不会把正文挤出文本区，窄档也一样。
fn file_body_lines(path: &str, text: &str, width: usize) -> Vec<DetailLine> {
    let highlighted =
        files::language_for(path).and_then(|language| highlight::highlight_code(language, text));
    let logical: Vec<Vec<Span<'static>>> = match highlighted {
        Some(rows) => rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|span| Span::styled(span.text, span.class.style()))
                    .collect()
            })
            .collect(),
        None => text
            .split('\n')
            .map(|line| vec![Span::raw(line.to_owned())])
            .collect(),
    };
    let digits = logical.len().to_string().len();
    let budget = width.saturating_sub(digits + 1).max(1);
    logical
        .into_iter()
        .enumerate()
        .flat_map(|(index, row)| {
            let mut spans = vec![Span::styled(
                format!("{:>digits$} ", index + 1),
                Style::default().fg(palette::MUTED),
            )];
            spans.extend(row);
            pane::wrap_line(&Line::from(spans), budget)
                .into_iter()
                .enumerate()
                .map(|(folded, line)| DetailLine {
                    line,
                    folded: folded > 0,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// 一份补丁的正文：行号列 + 补丁本身，按 `width` 折行
/// （`.scratch/diff-page/spec.md` §6）。
///
/// 行号列 = 最长行号的位数 + 1，右对齐、后面跟一格空格；折行在插前缀**之后**按剩余预算算，
/// 续行顶格 —— 与 [`file_body_lines`] 同形。没有行号的行（hunk 头、`\ No newline`）在那一列
/// 留白。
///
/// 上色是**两层合成**（与 markdown 代码块、文件弹窗同一套）：剥掉 diff 标记之后，剩下的代码
/// 按这个文件的**扩展名**认语言做语法高亮，再把 diff 那一档铺上去 —— 新增给背景、删除给另一
/// 种背景、hunk 头给前景。于是一行既是「新增」又是「字符串」，谁也不用让位。
fn diff_body_lines(path: &str, patch: &[changes::PatchLine], width: usize) -> Vec<DetailLine> {
    let source = patch
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let highlighted = highlight::highlight_diff_with(files::language_for(path), &source);
    let digits = patch
        .iter()
        .filter_map(|line| line.number)
        .max()
        .unwrap_or(0)
        .to_string()
        .len();
    let gutter = " ".repeat(digits + 1);
    let budget = width.saturating_sub(digits + 1).max(1);
    patch
        .iter()
        .zip(highlighted)
        .flat_map(|(line, spans)| {
            let number = match line.number {
                Some(number) => format!("{number:>digits$} "),
                None => gutter.clone(),
            };
            let tag = line.tag.style();
            let content: Vec<Span<'static>> = match line.tag {
                // hunk 头不是一行代码：它不带语法色，只带自己那一档（前景 + 粗体）。
                highlight::DiffTag::Hunk => vec![Span::styled(line.text.clone(), tag)],
                _ => spans
                    .into_iter()
                    .map(|span| {
                        // 语法前景盖在 diff 背景上 —— 顺序要紧：`patch` 保留已有的前景。
                        Span::styled(span.text, span.class.style().patch(tag))
                    })
                    .collect(),
            };
            let mut rendered = vec![Span::styled(number, Style::default().fg(palette::MUTED))];
            rendered.extend(content);
            pane::wrap_line(&Line::from(rendered), budget)
                .into_iter()
                .enumerate()
                .map(|(folded, line)| DetailLine {
                    line,
                    folded: folded > 0,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// 外部工具那一档的正文：它吐的那些片**原样**按我们的正文宽折行。
///
/// 颜色是它自己的（不经过语义色板 —— 那是 [ADR 0018](../../docs/adr/0018-external-diff-viewer-colours-sit-outside-the-palette.md)
/// 记下的代价），但折行、截断与滚动仍旧是我们的；而转义序列已经在 [`changes::parse_sgr`] 里
/// 解成了样式，所以拖选复制出来的还是干净的文本。
fn external_body_lines(lines: &[Vec<changes::Piece>], width: usize) -> Vec<DetailLine> {
    lines
        .iter()
        .flat_map(|line| {
            let spans: Vec<Span<'static>> = line
                .iter()
                .map(|piece| Span::styled(piece.text.clone(), piece.style.style()))
                .collect();
            pane::wrap_line(&Line::from(spans), width)
                .into_iter()
                .enumerate()
                .map(|(folded, line)| DetailLine {
                    line,
                    folded: folded > 0,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// 一个小节标题，画成一道分隔线：文字嵌在一串 `─` 里。
fn section_header(name: &str) -> Line<'static> {
    Line::from(Span::styled(
        wording::detail_section(name),
        Style::default().fg(palette::MUTED),
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
/// 把文件查看器那一屏画在浮层里（`[ui] file_viewer = "nvim"`）。
///
/// **没有框线**：nvim 自己会把那块底色铺满（它的每一格都带背景色），那一整块色块就是它与
/// 转录的分界；四周留的那一格白让内容不顶到屏幕边缘
/// （`.scratch/nvim-file-viewer/spec.md` §3，维护者 2026-10-06 定的那一档）。
fn draw_file_viewer(frame: &mut ratatui::Frame, panes: &layout::Regions, state: &mut TuiState) {
    if state.file_viewer.is_none() {
        state.file_viewer_rect = None;
        return;
    }
    let Some(area) = panes.overlay_area(panes.viewer_width(state.facts.file_viewer.width)) else {
        // 没地方画它：收掉，别把键盘扣在一个没人看得见的进程上。
        state.close_file_viewer();
        return;
    };
    // 这一帧画在哪儿，指针才可能落在哪儿（与 `detail_rect` 同一条纪律）。
    state.file_viewer_rect = Some(area);
    let grid = layout::inner(area);
    let Some(pane) = state.file_viewer.as_mut() else {
        return;
    };
    // 尺寸跟着浮层走：终端 resize、`Ctrl-O` 收起左栏、切边框档都从这条路进来。
    if pane.grid != grid {
        pane.viewer.resize(grid.width, grid.height);
        pane.grid = grid;
    }
    let screen = pane.viewer.screen();
    let alive = pane.viewer.alive();
    // 跨在左边缘上的那个宽字形让位：它占着浮层外一格、第二格落在浮层里，而第二格在差分里
    // 会被跳过 —— 于是半个字形盖在留白上。与详情覆盖层同一条来路
    // （[`blank_half_covered_glyphs`]）。
    blank_half_covered_glyphs(frame, area);
    // 整块先清成空：**留白那一圈由这里交代**，不留给「缓冲里本来就该是空的」那个假设 ——
    // 它底下压着左栏，不清就会从留白里透出来（`.scratch/nvim-file-viewer/spec.md` §3 的
    // 「去框留白」说的正是这块地；2026-10-07 维护者报的）。里圈随后由外来那一屏自己铺满。
    frame.render_widget(Clear, area);
    frame.render_widget(viewer::ScreenWidget { screen: &screen }, grid);
    if !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        if row < grid.height && col < grid.width {
            frame.set_cursor_position(ratatui::layout::Position::new(grid.x + col, grid.y + row));
        }
    }
    // 它自己退了（`:q`）：浮层跟着收 —— 那是最自然的一条出路，不必再让读的人按一次
    // `Ctrl-C`。
    if !alive {
        state.close_file_viewer();
    }
}

/// 光标这一帧亮着吗：每 [`BLINK_FRAMES`] 帧翻一次。
///
/// 脉冲是 60 ms 一跳，所以八帧 ≈ 0.5 秒一次翻转 —— 与终端自己的光标闪烁同量级。
/// 空闲时脉冲照旧在走（`.scratch/tui-visual-language/spec.md` §32），所以闪也不停。
fn blink_on(pulse: u64) -> bool {
    (pulse / BLINK_FRAMES).is_multiple_of(2)
}

/// 光标闪一下要几帧。
const BLINK_FRAMES: u64 = 8;

/// 键盘现在在输入区吗 —— 输入区那个光标据此决定露不露面。
///
/// 三个「键盘不在输入区」的情形：键盘交给了文件页（`sidebar_keyboard`）、一块覆盖层立着
/// （详情或文件查看器）、问卷占着底部（它有自己的光标与自己的区域）。
fn keyboard_in_the_input(state: &TuiState) -> bool {
    state.pending.is_none()
        && !state.sidebar_keyboard
        && state.detail.is_none()
        && state.file_viewer.is_none()
}

/// 选择器浮层：一块居中的盒子，标题 + 一列候选 + 页脚键位（spec §10）。
///
/// 宽度取**最长的一行**加余量（一个 model id 可以很长），高度按候选数、上限是主列装得下的
/// 那些 —— 候选多过高度就在内部滚动，于是 `Enter` 回的永远是**高亮那一行**而不是第几行。
///
/// 不复用 [`DetailView`]：那个是只读正文、且独占整个指针；选择器更接近**问卷**（一块浮层、
/// 上下键移动、回车确认、`Esc` 取消），而它与问卷那处不同的地方在状态机那边，不在这里。
fn draw_picker(frame: &mut ratatui::Frame, panes: &layout::Regions, picker: &mut Picker) {
    // 框 + 标题 + 页脚。放不下就连候选都不画 —— 一个只有标题的浮层回答不了任何问题。
    let chrome = 2u16;
    let rows_room = panes.main.height.saturating_sub(chrome + 2);
    if rows_room == 0 {
        return;
    }
    // **宽度由内容定**：三列各自按自己那一列最长的值对齐，列与列之间是 [`wording::PICKER_COLUMN`]。
    // 表格就该是对齐的，所以短的那些右面留白，而不是各自贴紧下一列。
    let layout = picker_columns(&picker.options);
    let width = layout
        .total()
        .min(panes.main.width.saturating_sub(4) as usize) as u16;
    let visible = (picker.len() as u16).min(rows_room);
    // 滚动：高亮跑出窗口就把它带回窗口，滚到高亮之上就把窗口挪到高亮那里。
    if picker.highlight < picker.top {
        picker.top = picker.highlight;
    }
    if visible > 0 && picker.highlight >= picker.top + visible as usize {
        picker.top = picker.highlight + 1 - visible as usize;
    }
    let height = visible + chrome + 2;
    let Some(area) = panes.modal_sized(width.max(3), height) else {
        return;
    };
    picker.rect = Some(area);
    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    frame.render_widget(chrome_block(), area);
    let inner = layout::inner(area);
    // 标题行：`模型` / `思考强度`。
    let title = Line::from(Span::styled(
        picker.title.clone(),
        Style::default()
            .fg(palette::PLAIN)
            .add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(
        Paragraph::new(title),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    // 候选：一行一个，每行三列。每一格记下它画在哪儿，于是只有**画出来的**那几行点得到（与
    // 问卷的选项区同一条纪律）。
    picker.rows.clear();
    for offset in 0..visible as usize {
        let index = picker.top + offset;
        let Some(option) = picker.options.get(index) else {
            break;
        };
        let row = inner.y + 1 + offset as u16;
        picker.rows.push((row, inner.x, inner.width, index));
        frame.render_widget(
            Paragraph::new(picker_row(option, index == picker.highlight, &layout)),
            Rect::new(inner.x, row, inner.width, 1),
        );
    }
    // 页脚：那些键是干什么的。它不参与命中 —— 没有键可点。
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            wording::picker_keys(),
            Style::default().fg(palette::MUTED),
        ))),
        Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
    );
}

/// 表格的三列各自多宽，以及**总共**多宽。
///
/// 宽度由内容定，所以一个 model id 很长时整块浮层跟着长 —— 而它仍然**不**超过主列（那一档就
/// 截断，不是滚动）。
struct PickerColumns {
    /// 模型 id 那一列（**含**行首的记号与它后面的空格）。
    model: usize,
    /// profile 那一列。
    profile: usize,
    /// 状态那一列。
    status: usize,
}

impl PickerColumns {
    /// 一行有多宽：两条款与三个列分隔。
    fn row(&self) -> usize {
        self.model + self.profile + self.status + 2 * PickerColumnRule::COLUMNS
    }

    /// 连边框一起多宽。
    fn total(&self) -> usize {
        self.row() + 2
    }
}

/// 列与列之间那一条（含它两侧各一格空格）。
struct PickerColumnRule;

impl PickerColumnRule {
    const COLUMNS: usize = wording::PICKER_COLUMN.len() + 2;
}

/// 这一份清单的列宽。
fn picker_columns(options: &[crate::render::PickerOption]) -> PickerColumns {
    // 行首两列是记号与它后面的空格；每个值左对齐到本列最长的那个，右面留白。
    let widest = |width: fn(&crate::render::PickerOption) -> usize| {
        options.iter().map(width).max().unwrap_or(0)
    };
    PickerColumns {
        model: widest(|option| option.label.cell_width() as usize + 2).max(4),
        profile: widest(|option| option.detail.cell_width() as usize).max(3),
        status: widest(|option| {
            if option.current {
                wording::picker_status_current()
            } else {
                wording::picker_status_switchable()
            }
            .cell_width() as usize
        })
        .max(3),
    }
}

/// 一行三个单元格：模型 id ┆ profile ┆ 状态。
fn picker_row(
    option: &crate::render::PickerOption,
    highlighted: bool,
    columns: &PickerColumns,
) -> Line<'static> {
    let status = if option.current {
        wording::picker_status_current()
    } else {
        wording::picker_status_switchable()
    };
    // 当前项一个 `▸`；选不了的那行**没有**记号 —— 状态列说了它是「可切换」，而它灰着且点了
    // 不响应，两处都不响就是选不了。
    let mut spans = vec![Span::styled(
        format!(
            "{} {}",
            if option.current {
                wording::PICKER_CURRENT
            } else {
                " "
            },
            option.label
        ),
        Style::default().fg(if option.enabled {
            palette::PLAIN
        } else {
            // 禁用项暗一档：它点了不响应，读的人该先看见这一点。
            palette::MUTED
        }),
    )];
    let cells = [
        (option.label.cell_width() as usize + 2, columns.model),
        (option.detail.cell_width() as usize, columns.profile),
        (status.cell_width() as usize, columns.status),
    ];
    for (index, (used, column_width)) in cells.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                format!(" {} ", wording::PICKER_COLUMN),
                Style::default().fg(palette::CHROME),
            ));
        }
        if *used < *column_width {
            spans.push(Span::raw(" ".repeat(column_width - used)));
        }
        match index {
            // 第二列：它走哪个 provider profile（`detail` 就是那一格）。
            1 => spans.push(Span::styled(
                option.detail.clone(),
                Style::default().fg(palette::MUTED),
            )),
            // 第三列：这一行是不是当前这一场会话在用的。
            2 => spans.push(Span::styled(
                status.to_owned(),
                Style::default().fg(if option.current {
                    palette::ACCENT
                } else {
                    palette::MUTED
                }),
            )),
            // 第 0 列（模型 id）的内容在上面那个 span 里画完了，这里只补过它的留白。
            _ => {}
        }
    }
    if highlighted {
        for span in &mut spans {
            span.style = span.style.fg(palette::ACCENT).add_modifier(Modifier::BOLD);
        }
    }
    Line::from(spans)
}

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
    // 打开它的那一页冻在原处：读的人正在看一行，一波输出不许把它拽走 —— 也不许让
    // 「N 行新内容」的计数在他们正读的覆盖层底下往上爬（票 02 §4）。**只冻打开方**：对话页
    // 继续跟着新内容（票 13），文件页由它自己那份滚动与展开状态守着
    // （`.scratch/files-page/spec.md` §5）。
    if matches!(state.detail_opener, Some(DetailOpener::Trace { .. })) {
        state.trace.set_following(false);
        state.trace.set_holding(true);
    }
    // 框外的一次点击能打中什么，只有记下来之后才存在。
    state.detail_rect = Some(area);
    let Some(view) = state.detail.as_ref() else {
        return;
    };
    let inner = layout::inner(area);
    // 边框里面一列空气，好让文字不碰外框。它是被**让出来**的，而不是吃掉主体：在低于「主体
    // 两行加标题与页脚」的那个高度时，内边距会藏起读的人正是为了它才打开覆盖层的内容
    // （2026-09-23）。
    // 判据与 [`layout::Regions::detail_text_width`] 共用一处 —— 「正文按多宽排版」与「正文画
    // 在哪儿」必须是同一个答案，两边各写一遍那条不等式正是行号会被算错的由来。
    let (pad_x, pad_y) = layout::detail_padding(inner);
    let text = Rect::new(
        inner.x + pad_x,
        inner.y + pad_y,
        inner.width.saturating_sub(pad_x * 2),
        inner.height.saturating_sub(pad_y * 2),
    );
    // 下面的一切都按**带内边距**的矩形定尺寸：一个比显示它的框高一行的主体窗口，会把末尾几行
    // 裁掉，内边距当初就是这样吃掉了它正在为之腾地方的那份主体的一行。
    let body_rows = text.height.saturating_sub(2) as usize;
    let max_top = view.body.len().saturating_sub(body_rows);
    let top = view.top.min(max_top);
    let rows: Vec<DetailLine> = view
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
    let color = view.detail.color;
    let lines: Vec<Line<'static>> = rows.iter().map(|row| row.line.clone()).collect();
    let folded: Vec<bool> = rows.iter().map(|row| row.folded).collect();

    blank_half_covered_glyphs(frame, area);
    frame.render_widget(Clear, area);
    // 边框回归框架（`CHROME` + 空角），而「这是谁的行」退到标题行上 —— 那条记录的意图
    // （不读一个字就知道是谁的行）没丢，标题本来就在框内第一行、紧挨边框
    // （`.scratch/tui-visual-language/spec.md` §28）。
    frame.render_widget(chrome_block(), area);
    // 标题行是被点那一行自己的文字，这样读的人知道他们打开的是哪一行。超宽时用 `…` 收尾
    // （`.scratch/diff-page/spec.md` §6）—— 一条长路径的尾巴比它的开头更有辨识度。
    let title_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    let title_line = if text_columns(&title) > text.width as usize {
        ellipsize_line(
            Line::from(Span::styled(title.clone(), title_style)),
            text.width as usize,
        )
    } else {
        Line::from(Span::styled(title.clone(), title_style))
    };
    frame.render_widget(
        Paragraph::new(title_line),
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
    // 详情正文是最常被复制的东西（一段工具输出、一段解释），所以它也进屏幕文本层
    // （`.scratch/tui-feedback/spec.md` §5）。
    note_rows(state, body, &lines, &folded, &[]);
    frame.render_widget(Paragraph::new(lines), body);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(palette::MUTED),
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

    /// 一次调用还没有结果时行尾没有耗时那一段。拿不到就不画，不写 0 也不写占位符 ——
    /// 「还在跑」的调用在流上就是这样一块：没有结果
    /// （`.scratch/trace-ledger/issues/17-row-numbers-and-anomalies.md` 第 3 条）。
    ///
    /// 被取消的调用是另一回事：循环为它合成结果，那笔墙钟是排队加取消那一刻、不是工具花的时间，
    /// 而流上它与真跑过的调用同形，所以行上照画（同一张票的「落地注记」第二条）。
    #[test]
    fn a_call_without_a_result_has_no_duration_to_draw() {
        use crate::render::transcript::ToolOutcome;

        let tool = |outcome: Option<ToolOutcome>| {
            Block::Tool(Box::new(ToolBlock {
                speaker: crate::events::SpeakerId::Debater("kimi".into()),
                tool_call_id: ToolCallId::new("c-1"),
                tool: "bash".to_owned(),
                args: serde_json::json!({"command": "ls"}),
                outcome,
            }))
        };
        assert_eq!(tool_duration(&tool(None)), None);
        assert_eq!(
            tool_duration(&tool(Some(ToolOutcome {
                ok: true,
                output: Some("out".to_owned()),
                error: None,
                duration_ms: 1_500,
            }))),
            Some(" · 1.5 s".to_owned())
        );
    }

    /// 行尾那两段一起挤不下时，先走的是**耗时**：用量尾巴完整保留
    /// （同上那张票第 2 条）。
    #[test]
    fn a_tail_that_does_not_fit_sheds_the_duration_first() {
        let usage = Usage {
            input_tokens: 19_502,
            output_tokens: 1_880,
            cached_tokens: 0,
            miss_tokens: 0,
            reasoning_tokens: None,
        };
        let mut line = RenderedLine::from(Line::from("调用 bash 运行 cargo test"));
        // 刚好放得下用量、放不下再一段耗时。
        let room = line_columns(&line.line) + Tail::Usage(usage).span().width() + 4;
        attach_row_tail(
            Some(&mut line),
            Some(" · 1.5 s".to_owned()),
            Some(Tail::Usage(usage)),
            room,
        );
        let text = line_text(&line.line);
        assert!(text.ends_with("in=19502 out=1880"), "用量留着：{text}");
        assert!(!text.contains("1.5 s"), "耗时让位：{text}");

        // 宽得下时两段都在：耗时排在用量之前（第 1 条）。
        attach_row_tail(Some(&mut line), Some(" · 1.5 s".to_owned()), None, 200);
        assert!(
            line_text(&line.line).ends_with("· 1.5 s"),
            "宽得下时耗时在行尾：{:?}",
            line_text(&line.line)
        );
    }

    /// 平行表跟着窗格交回来的丢弃数裁，而不是自己数 `CAP`：推过上限之后，行链接与回合条
    /// 仍与窗格的源行一一对应（`.scratch/trace-tab/issues/07-pane-evict-accounting.md`）。
    #[test]
    fn the_link_table_keeps_pace_with_the_pane_at_the_cap() {
        let mut trace_page = state();
        for _ in 0..pane::CAP + 2 {
            trace_page.push_line(Viewport::Trace, Line::from("x"), None, Some(false), None);
        }
        assert_eq!(trace_page.trace.sources(), pane::CAP);
        assert_eq!(trace_page.trace_links.len(), trace_page.trace.sources());
        // 身份表与链接表同一理由：同一个丢弃数裁，所以它也与窗格同长（ADR 0021）。
        assert_eq!(trace_page.trace_block_ids.len(), trace_page.trace.sources());
        // 回合条与对话窗格平行（链接表只服务轨迹页），所以它跟着对话那一侧。
        let mut conversation = state();
        for _ in 0..pane::CAP + 2 {
            conversation.push_line(
                Viewport::Conversation,
                Line::from("x"),
                None,
                Some(false),
                None,
            );
        }
        assert_eq!(conversation.conversation.sources(), pane::CAP);
        assert_eq!(
            conversation.turn_rail.lines.len(),
            conversation.conversation.sources()
        );
    }

    /// 每一类块进哪个视图 —— 分工是一个穷尽的 match，所以穷举地测它，而不是只在帧里
    /// 间接覆盖（`.scratch/trace-tab/spec.md` §2 与它的测试决定）。
    #[test]
    fn every_kind_of_block_lands_in_the_views_the_split_names() {
        use crate::events::{
            Decision, DecisionSource, HistoryReason, ParticipantId, RoundMode, SpeakerId, Usage,
            hook_format,
        };
        use crate::render::transcript::{ToolBlock, ToolOutcome};

        let debater = SpeakerId::Debater("kimi".into());
        let executor = SpeakerId::Executor(ParticipantId::new("kimi-1"));
        let tool = |speaker: SpeakerId| {
            Block::Tool(Box::new(ToolBlock {
                speaker,
                tool_call_id: ToolCallId::new("c-1"),
                tool: "bash".to_owned(),
                args: serde_json::json!({"command": "ls"}),
                outcome: Some(ToolOutcome {
                    ok: true,
                    output: Some("out".to_owned()),
                    error: None,
                    duration_ms: 1,
                }),
            }))
        };
        let message = |speaker: SpeakerId, text: &str| Block::Message {
            speaker,
            role: Role::Assistant,
            text: text.to_owned(),
            reasoning: None,
        };
        // （块，留在对话视图吗）
        let cases: Vec<(Block, bool)> = vec![
            (
                Block::Message {
                    speaker: SpeakerId::User,
                    role: Role::User,
                    text: "问".to_owned(),
                    reasoning: None,
                },
                true,
            ),
            (message(debater.clone(), "答"), true),
            (message(executor.clone(), "派出去的活"), false),
            // 回合 / 轮次的**开始**是过程行：只住在轨迹页（2026-10-05 维护者收紧）。
            (
                Block::TurnStarted {
                    speaker: debater.clone(),
                    iteration: 1,
                },
                false,
            ),
            // 正常的收尾也是过程行……
            (
                Block::TurnEnded {
                    speaker: debater.clone(),
                    reason: StopReason::Completed,
                },
                false,
            ),
            // ……而**非正常**的收尾是说给人听的「为什么停下来」，留在对话视图。
            (
                Block::TurnEnded {
                    speaker: debater.clone(),
                    reason: StopReason::Error,
                },
                true,
            ),
            (
                Block::RoundStarted {
                    round: 1,
                    mode: RoundMode::Independent,
                },
                false,
            ),
            (
                Block::RoundEnded {
                    round: 1,
                    reason: StopReason::Completed,
                },
                false,
            ),
            (
                Block::RoundEnded {
                    round: 1,
                    reason: StopReason::BudgetExhausted,
                },
                true,
            ),
            (
                Block::AgentError {
                    speaker: debater.clone(),
                    message: "炸了".to_owned(),
                },
                true,
            ),
            (
                Block::SessionError {
                    code: "x".to_owned(),
                    detail: "细节".to_owned(),
                },
                true,
            ),
            (
                Block::SessionEnded {
                    reason: StopReason::Aborted,
                },
                true,
            ),
            // 权限询问与裁决也是过程行：只住在轨迹页。
            (
                Block::PermissionAsked {
                    speaker: debater.clone(),
                    tool_name: Some("bash".to_owned()),
                    args: serde_json::json!({}),
                },
                false,
            ),
            (
                Block::PermissionDecided {
                    speaker: debater.clone(),
                    decision: Decision::Allow,
                    source: DecisionSource::User,
                    reason: None,
                },
                false,
            ),
            (
                Block::Hook {
                    speaker: debater.clone(),
                    point: hook_format::POINT_PRE.to_owned(),
                    outcome: hook_format::failed("拒绝"),
                },
                true,
            ),
            (
                Block::Hook {
                    speaker: debater.clone(),
                    point: hook_format::POINT_PRE.to_owned(),
                    outcome: hook_format::OUTCOME_CONTINUE.to_owned(),
                },
                false,
            ),
            (Block::Notice("回执".to_owned()), true),
            // 诊断也进轨迹：它是模型的运行日志，不是对话。
            (Block::Diagnostic("诊断".to_owned()), false),
            (tool(debater.clone()), false),
            (tool(executor), false),
            (
                Block::ToolFeedback {
                    outcome: "[钩子] 反馈".to_owned(),
                },
                false,
            ),
            (
                Block::Usage {
                    speaker: debater.clone(),
                    usage: Usage::default(),
                },
                false,
            ),
            (
                Block::Divergence {
                    topic: "题".to_owned(),
                    positions: Vec::new(),
                },
                false,
            ),
            (
                Block::Sandbox {
                    mode: "bwrap".to_owned(),
                    unavailable_reason: None,
                },
                false,
            ),
            (
                Block::History {
                    reason: HistoryReason::Undo,
                    summary: None,
                },
                false,
            ),
            (
                Block::ContextInjected {
                    source: ContextSource::McpCatalog,
                    content: "正文".to_owned(),
                },
                false,
            ),
            (
                Block::ExecutorSpawned {
                    speaker: debater.clone(),
                    executor_id: ParticipantId::new("kimi-1"),
                },
                false,
            ),
            (
                Block::ExecutorFinished {
                    executor_id: ParticipantId::new("kimi-1"),
                    reason: StopReason::Completed,
                    summary: "完了".to_owned(),
                },
                false,
            ),
            (
                Block::Delta {
                    speaker: debater,
                    kind: DeltaKind::Text,
                    text: "增量".to_owned(),
                },
                false,
            ),
        ];
        for (block, kept) in cases {
            assert_eq!(
                selects(Viewport::Conversation, &block),
                kept,
                "对话视图：{block:?}"
            );
            assert!(
                selects(Viewport::Trace, &block),
                "轨迹视图是全量：{block:?}"
            );
        }
    }

    /// 一条上下文注入在转录里是一行，在详情里是它的正文（票 19）。
    #[test]
    fn a_context_injection_row_opens_its_loaded_data() {
        let block = Block::ContextInjected {
            source: ContextSource::McpCatalog,
            content: "[注入] MCP 加载\n\n- `fake`（stdio）：已连接\n".to_owned(),
        };
        let mut colors = SpeakerColors::new(&[]);
        let lines = paint_block(
            &block,
            &mut colors,
            80,
            PrefixStyle::Bracketed,
            Viewport::Conversation,
            true,
        );
        assert_eq!(lines.len(), 1, "注入只占一行");
        let detail = lines[0].link.clone().expect("这一行该点得开");
        assert_eq!(detail.title, "[上下文注入：MCP 加载]");

        let body = detail_body(&detail, "/tmp", 80);
        let text: String = body
            .iter()
            .map(|row| line_text(&row.line))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("MCP 加载"), "{text}");
        assert!(text.contains("`fake`（stdio）：已连接"), "{text}");
    }

    /// 复制回执的寿命是 3 秒：过了就不该再有那句回执（`.scratch/tui-feedback/spec.md` §6）。
    #[test]
    fn the_copy_receipt_expires_after_three_seconds() {
        let now = std::time::Instant::now();
        let copied = Some((now, 12, 2));
        assert_eq!(
            copy_receipt(copied, now),
            Some("已复制 12 字 · 2 行".to_owned())
        );
        assert_eq!(
            copy_receipt(copied, now + std::time::Duration::from_millis(2_999)),
            Some("已复制 12 字 · 2 行".to_owned()),
            "差一毫秒还在"
        );
        assert_eq!(copy_receipt(copied, now + RECEIPT_WINDOW), None, "到点就走");
        assert_eq!(copy_receipt(None, now), None, "没复制过就没有回执");
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

    // --- 目标循环跑着时的键盘（`.scratch/goal-loop/spec.md` §5） -------------

    /// 一个能收键盘的 TUI 状态：够跑那几条键的规矩。
    fn state() -> TuiState {
        TuiState::new(
            SessionFacts {
                switchable: true,
                session_id: "s-1".to_owned(),
                session_dir: "/tmp/s-1".to_owned(),
                model: "fake-model".to_owned(),
                context_window: 100_000,
                mode: crate::permissions::Mode::Workspace,
                budget_limit: None,
                number_style: crate::render::wording::NumberStyle::Cn,
                file_viewer: crate::config::FileViewerSettings::default(),
                diff_viewer: crate::config::DiffViewerSettings::default(),
                speaker_order: vec!["kimi".to_owned()],
            },
            std::path::PathBuf::from("/x/heng"),
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
            Some(Key::Newline)
        );
        // `Shift+Enter` 是同一个动作 —— 支持键盘增强协议的终端会把它报成带修饰的 `Enter`
        // （`.scratch/tui-feedback/spec.md` §9）。
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            Some(Key::Newline)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty())),
            Some(Key::Enter),
            "裸 `Enter` 照旧是提交"
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

    // --- 文件查看器（`[ui] file_viewer = "nvim"`） ---------------------------
    // 这一档的全部分支都在这里：它跑一块真屏幕，而真屏幕不该出现在测试里。

    /// 一个开着**假**查看器的状态：浮层按 100x30 的屏幕算，观测口一起交出来。
    fn state_with_viewer() -> (TuiState, viewer::FakeWatcher) {
        let mut state = state();
        state.area = Rect::new(0, 0, 100, 30);
        let panes = layout::plan(state.area, 1, state.sidebar_wanted);
        let area = panes
            .overlay_area(panes.viewer_width(crate::config::DEFAULT_FILE_VIEWER_WIDTH))
            .expect("100x30 放得下浮层");
        let fake = viewer::FakeViewer::default();
        let watch = fake.watch();
        state.file_viewer = Some(FileViewerPane {
            viewer: Box::new(fake),
            grid: layout::inner(area),
        });
        state.file_viewer_rect = Some(area);
        (state, watch)
    }

    #[test]
    fn esc_goes_into_the_viewer_instead_of_closing_it() {
        // `Esc` 在 nvim 里是退出插入模式：抢走它就等于把编辑器弄坏。这是与详情覆盖层
        // 有意不同的一处（那边 `Esc` 是关）。
        let (mut state, watch) = state_with_viewer();

        state.key(Key::Esc);

        assert_eq!(watch.fed(), b"\x1b");
        assert!(state.file_viewer.is_some(), "`Esc` 不该关掉浮层");
    }

    #[test]
    fn ctrl_c_closes_the_viewer_and_asks_for_a_rescan() {
        let (mut state, watch) = state_with_viewer();

        state.key(Key::CtrlC);

        assert!(state.file_viewer.is_none());
        assert_eq!(watch.kills(), 1);
        assert!(state.take_file_scan(), "收掉它时补一次重扫");
    }

    #[test]
    fn ctrl_z_stays_a_suspend_gesture_inside_the_viewer() {
        // `.scratch/suspend-gesture/spec.md` §1：终端层手势，任何视图都拦不住它。
        let (mut state, watch) = state_with_viewer();

        state.key(Key::CtrlZ);

        assert!(state.take_suspend_request());
        assert!(watch.fed().is_empty(), "它不该被转发进 nvim");
    }

    #[test]
    fn every_other_key_goes_to_the_viewer() {
        // 独占键盘：`j` 该进 nvim，而不是落进草稿。
        let (mut state, watch) = state_with_viewer();

        for key in [
            Key::Char('j'),
            Key::CtrlU,
            Key::Enter,
            Key::Up,
            Key::PageDown,
        ] {
            state.key(key);
        }

        assert_eq!(watch.fed(), b"j\x15\r\x1b[A\x1b[6~");
    }

    #[test]
    fn a_click_outside_the_frame_closes_the_viewer() {
        // 浮层是居中的 96x26（屏幕 100x30），所以 (0, 0) 在它外面。
        let (mut state, watch) = state_with_viewer();

        state.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });

        assert!(state.file_viewer.is_none());
        assert_eq!(watch.kills(), 1);
    }

    #[test]
    fn a_click_inside_the_frame_goes_to_the_viewer_as_sgr() {
        // 外框从 (2, 2) 起（宽 96、高 26，屏幕 100x30 居中），留白一格 → nvim 的网格从
        // (3, 3) 起；nvim 收的是 1 起算的相对坐标，所以终端 (5, 4) 是网格里的 (3, 2)。
        let (mut state, watch) = state_with_viewer();

        state.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(watch.fed(), b"\x1b[<0;3;2M");
        assert!(state.file_viewer.is_some());
    }

    #[test]
    fn the_wheel_inside_the_frame_goes_to_the_viewer() {
        let (mut state, watch) = state_with_viewer();

        state.mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 4,
            modifiers: KeyModifiers::NONE,
        });

        assert_eq!(watch.fed(), b"\x1b[<64;3;2M");
    }

    #[test]
    fn the_viewer_frame_is_one_cell_inside_the_overlay() {
        // 「去框留白」那一档：外框再内缩一格，那一格就是留白（`.scratch/nvim-file-viewer` §3）。
        let mut state = state();
        state.area = Rect::new(0, 0, 100, 30);
        let panes = layout::plan(state.area, 1, state.sidebar_wanted);

        let area = panes
            .overlay_area(panes.viewer_width(crate::config::DEFAULT_FILE_VIEWER_WIDTH))
            .expect("放得下");
        assert_eq!((area.width, area.height), (96, 26));
        let grid = layout::inner(area);
        assert_eq!((grid.width, grid.height), (94, 24));

        // 宽度上限来自配置；窄终端上被「屏幕宽 − 4」压着。
        assert_eq!(
            panes.overlay_area(panes.viewer_width(60)).unwrap().width,
            60
        );
        assert_eq!(
            layout::plan(Rect::new(0, 0, 40, 20), 1, true).viewer_width(135),
            36
        );
    }

    #[test]
    fn the_identity_becomes_a_foldable_injection_record() {
        // 拼好的系统提示词借「注入的上下文」那条记录现身：一条**可点开**的行，来源名自己
        // 说清它是按当前代码拼的、不进事件流。它走的是渲染通道，所以这里喂的也是渲染事件
        // —— 事件流上一个字都不多。
        let mut transcript = crate::render::transcript::Transcript::new();

        let blocks = transcript.push(RenderEvent::identity(
            "你是衡（heng），一套自用的 coding agent harness…",
        ));

        match blocks.as_slice() {
            [crate::render::transcript::Block::ContextInjected { source, content }] => {
                assert_eq!(*source, crate::events::ContextSource::Identity);
                assert_eq!(content, "你是衡（heng），一套自用的 coding agent harness…");
            }
            _ => panic!("期望正好一条注入记录（可点开的那一种）"),
        }
    }

    #[test]
    fn the_cursor_blinks_and_only_when_the_keyboard_is_in_the_input_area() {
        // 闪：每八帧翻一次。
        assert!(blink_on(0));
        assert!(blink_on(7));
        assert!(!blink_on(8));
        assert!(!blink_on(15));
        assert!(blink_on(16), "十六帧之后又亮起来");

        // 露面：只有键盘真的在输入区时。
        // （`Pending` 那几个先构造：`state()` 这个名字在下面会被变量遮住。）
        let mut modal = state();
        modal.pending = Some(Pending::ClearDraft);
        assert!(!keyboard_in_the_input(&modal), "中间的模态占着键盘");

        let mut state = state();
        assert!(keyboard_in_the_input(&state));
        state.sidebar_keyboard = true;
        assert!(
            !keyboard_in_the_input(&state),
            "键盘在文件页时输入区没有光标"
        );
        state.sidebar_keyboard = false;

        let (viewer, _) = state_with_viewer();
        assert!(
            !keyboard_in_the_input(&viewer),
            "文件查看器立着时输入区没有光标"
        );
    }

    #[test]
    fn the_viewer_frame_covers_everything_under_it() {
        // 浮层是**外来屏幕**：它盖住的那一整块 —— 包括四周那一格留白 —— 由它自己交代，
        // 底下的左栏（标记、页签条、读数）不能从它的空白处透出来。窄终端上浮层与左栏重叠得
        // 更多，这条尤其显眼（2026-10-07 维护者报的）。
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut state = state();
        state.area = Rect::new(0, 0, 100, 30);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("TestBackend");
        // 第一帧：没有浮层，左栏照常画出来。
        terminal
            .draw(|frame| draw_frame(frame, &mut state))
            .unwrap();

        // 第二帧：装上浮层（与 `state_with_viewer` 同一套几何）。
        let panes = layout::plan(state.area, 1, state.sidebar_wanted);
        let area = panes
            .overlay_area(panes.viewer_width(crate::config::DEFAULT_FILE_VIEWER_WIDTH))
            .expect("100x30 放得下浮层");
        state.file_viewer = Some(FileViewerPane {
            viewer: Box::new(viewer::FakeViewer::default()),
            grid: layout::inner(area),
        });
        state.file_viewer_rect = None;

        let frame = terminal
            .draw(|frame| draw_frame(frame, &mut state))
            .unwrap();
        let buf = frame.buffer;
        let mut leaked = Vec::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if buf[(x, y)].symbol() != " " {
                    leaked.push((x, y, buf[(x, y)].symbol().to_owned()));
                }
            }
        }
        assert!(
            leaked.is_empty(),
            "浮层没盖住底下的东西：{} 处，样例 {:?}",
            leaked.len(),
            &leaked[..leaked.len().min(10)]
        );
    }

    #[test]
    fn the_builtin_viewer_still_opens_the_detail_overlay() {
        // 回归：没配 `file_viewer` 时点开文件走的是老路，一个进程都不起。
        let mut state = state();
        state.facts.file_viewer.kind = crate::config::FileViewer::Builtin;

        state.open_file("README.md");

        assert!(state.detail.is_some());
        assert!(state.file_viewer.is_none());
    }

    #[test]
    fn note_rows_keeps_the_hotspots_it_just_marked() {
        // 两件事出自**同一次**识别：下划线铺在这一行上，候选跟着进屏幕文本层
        // （`.scratch/clickable-links/spec.md` §2）。命中那一半要等点击那条路（票 02），
        // 这一条钉住的是记下来的账本身 —— 列区间与画出来的文本对齐。
        let mut state = state();
        let mut rows = vec![Line::from("见 x.html 与 https://a.example/b")];
        let folded = [false];
        let hotspots = links::mark(&mut rows, &folded);

        note_rows(
            &mut state,
            Rect::new(0, 0, 40, 1),
            &rows,
            &folded,
            &hotspots,
        );

        let block = state.screen_text.block(0).expect("记下来了一块");
        assert_eq!(block.rows[0].text, "见 x.html 与 https://a.example/b");
        let row = &block.rows[0];
        assert_eq!(
            row.hotspots
                .iter()
                .map(|hot| hot.columns.clone())
                .collect::<Vec<_>>(),
            vec![3..9, 13..32]
        );
        assert!(row.hotspots[0].covers(3) && row.hotspots[0].covers(8));
        assert!(!row.hotspots[0].covers(2), "`见` 那两列不是候选");
        assert!(!row.hotspots[1].covers(12), "`与` 后面那个空格不是候选");
    }

    /// 块的身份来自**事件信封**：信封的行号就是 JSONL 的行号，实时与重放是同一个值
    /// （ADR 0021）。它与「它在窗格里的第几行」是两种东西，而后者被上限裁剪与宽度重放推翻。
    #[test]
    fn a_blocks_identity_is_its_events_line_in_the_log() {
        let mut trace_page = state();
        trace_page.apply(logged(
            41,
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("c-1"),
                tool_name: "bash".to_owned(),
                args: serde_json::json!({"command": "ls"}),
            },
        ));
        trace_page.apply(logged(
            42,
            EventPayload::ToolCallCompleted {
                tool_call_id: ToolCallId::new("c-1"),
                ok: true,
                output: Some("body".to_owned()),
                error: None,
                duration_ms: 3,
            },
        ));

        let ids: Vec<Option<BlockId>> = trace_page.trace_block_ids.iter().copied().collect();
        let found: Vec<BlockId> = ids.iter().flatten().copied().collect();
        assert!(
            !found.is_empty(),
            "那些行得有身份，否则「点开详情」之外无处可寻"
        );
        assert!(
            found.iter().all(|id| id.seq.is_some()),
            "流上的块取信封的行号：{:?}",
            found
        );
        // 那一次调用的块来自第 42 条。
        assert!(
            found.iter().any(|id| id.seq == Some(42)),
            "完成事件产出的块带着它自己的行号：{:?}",
            found
        );
    }

    /// 一条事件可以产出**零个到多个**块（一次 `ask_user_question` 的结果连同由它推出来的
    /// 问卷作答），所以身份必须是「行号 + 该来源内的第几件」，而两个块不能撞。
    #[test]
    fn one_event_that_makes_several_blocks_gives_each_its_own_identity() {
        let mut trace_page = state();
        let answer = serde_json::json!({
            "answers": [{"id": "q1", "selected": ["第一个选项"], "custom": null}]
        })
        .to_string();
        trace_page.apply(logged(
            7,
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("ask-1"),
                tool_name: crate::tools::ASK_USER_QUESTION_TOOL.to_owned(),
                args: serde_json::json!({"questions": [{"id": "q1", "header": "选一个",
                "options": [{"label": "第一个选项"}], "multi_select": false}]}),
            },
        ));
        trace_page.apply(logged(
            8,
            EventPayload::ToolCallCompleted {
                tool_call_id: ToolCallId::new("ask-1"),
                ok: true,
                output: Some(answer),
                error: None,
                duration_ms: 1,
            },
        ));

        // 一个块可以占**多条**来源行（折行、附属行），而它们身份相同 —— 所以这里数的是
        // 不同的身份，不是行数。
        let mut distinct: Vec<BlockId> = trace_page
            .trace_block_ids
            .iter()
            .flatten()
            .copied()
            .filter(|id| id.seq == Some(8))
            .collect();
        distinct.sort_by_key(|id| id.index);
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            2,
            "完成事件产出两个块（那次调用 + 问卷作答）：{:?}",
            distinct
        );
        assert_ne!(distinct[0], distinct[1], "同一个来源内的两个块身份不同");
    }

    /// 宽度变化会**整批重放**轨迹窗格，而重放之后同一个块还是同一个身份 —— 那正是选中的
    /// 那一行不跳的原因（ADR 0021）。这一条要是破了，上面两张是安静地坏的。
    #[test]
    fn a_rebuild_after_a_width_change_keeps_the_same_identities() {
        let mut trace_page = state();
        for seq in 1..=4 {
            trace_page.apply(logged(
                seq,
                EventPayload::ToolCallStarted {
                    tool_call_id: ToolCallId::new(format!("c-{seq}")),
                    tool_name: "bash".to_owned(),
                    args: serde_json::json!({"command": format!("echo {seq}")}),
                },
            ));
            trace_page.apply(logged(
                seq + 100,
                EventPayload::ToolCallCompleted {
                    tool_call_id: ToolCallId::new(format!("c-{seq}")),
                    ok: true,
                    output: Some("body".to_owned()),
                    error: None,
                    duration_ms: 3,
                },
            ));
        }
        let before: Vec<Option<BlockId>> = trace_page.trace_block_ids.iter().copied().collect();
        assert!(before.iter().any(Option::is_some), "先有身份可比");

        // 宽度变了：重放那条路会把窗格与两张平行表清空再逐块重推。
        trace_page.rerender_if_width_changed(SHARED_RENDER_WIDTH, 60);
        let after: Vec<Option<BlockId>> = trace_page.trace_block_ids.iter().copied().collect();
        assert_eq!(before, after, "重放之后身份逐项相同，顺序也相同");
    }

    /// 一段思考从「正在思考」到「思考完成」是**同一行的两个阶段**，不是两条记录：定稿是就地
    /// 重写，所以身份跟着搬过去（ADR 0021）—— 选中那一行在定稿那一刻不跳。
    #[test]
    fn a_thought_keeps_its_identity_when_it_settles() {
        let mut trace_page = state();
        trace_page.apply(RenderEvent::Delta {
            speaker: crate::events::SpeakerId::Debater("kimi".into()),
            kind: DeltaKind::Reasoning,
            text: "想一下".to_owned(),
        });
        let open: Vec<Option<BlockId>> = trace_page.trace_block_ids.iter().copied().collect();
        trace_page.apply(RenderEvent::Delta {
            speaker: crate::events::SpeakerId::Debater("kimi".into()),
            kind: DeltaKind::Reasoning,
            text: "再想".to_owned(),
        });
        trace_page.apply(logged(
            9,
            EventPayload::MessageCompleted {
                role: Role::Assistant,
                text: "答案".to_owned(),
                reasoning: Some("想完了".to_owned()),

                first_token_ms: None,
            },
        ));
        let settled: Vec<Option<BlockId>> = trace_page.trace_block_ids.iter().copied().collect();
        assert_eq!(open.len(), 1, "开着的时候只有那一条思考行：{:?}", open);
        assert_eq!(
            open[0], settled[0],
            "定稿是就地重写那一行，身份跟着搬（增量没有信封，于是身份由渲染层那条计数补）"
        );
        assert!(settled[0].is_some(), "思考行也有身份：{:?}", settled[0]);
    }

    /// 一个状态上跑起来的最短路径：把一条流上事件包成渲染事件（信封的行号由调用方给）。
    fn logged(seq: u64, payload: EventPayload) -> RenderEvent {
        RenderEvent::Logged(crate::events::Event::new(
            seq,
            crate::events::SpeakerId::Debater("kimi".into()),
            payload,
        ))
    }
    /// 两级分组：回合一级、迭代二级，而**成员不缩进** —— 工具行保住它的内容宽度
    /// （`.scratch/trace-ledger/spec.md` §5）。
    #[test]
    fn a_turn_is_a_unit_and_an_iteration_is_its_own_header() {
        let mut trace_page = state();
        trace_page.apply(logged(
            1,
            EventPayload::MessageCompleted {
                role: Role::User,
                text: "问题".to_owned(),
                reasoning: None,
                first_token_ms: None,
            },
        ));
        // 第一个迭代 = `iteration == 1`，那是**开一级组**，不是开二级组。
        trace_page.apply(logged(
            2,
            EventPayload::TurnStarted {
                agent: crate::events::SpeakerId::Debater("kimi".into()),
                iteration: 1,
            },
        ));
        trace_page.apply(logged(
            3,
            EventPayload::TurnStarted {
                agent: crate::events::SpeakerId::Debater("kimi".into()),
                iteration: 2,
            },
        ));
        trace_page.apply(logged(
            4,
            EventPayload::ToolCallStarted {
                tool_call_id: ToolCallId::new("c-1"),
                tool_name: "bash".to_owned(),
                args: serde_json::json!({"command": "ls"}),
            },
        ));
        trace_page.apply(logged(
            5,
            EventPayload::ToolCallCompleted {
                tool_call_id: ToolCallId::new("c-1"),
                ok: true,
                output: Some("body".to_owned()),
                error: None,
                duration_ms: 3,
            },
        ));

        let headers: Vec<(HeaderLevel, u32)> = trace_page
            .painted
            .iter()
            .filter_map(|painted| match painted {
                Painted::GroupHeader(header) => Some((header.level, header.ordinal)),
                _ => None,
            })
            .collect();
        assert_eq!(
            headers,
            vec![(HeaderLevel::Unit, 1), (HeaderLevel::Iteration, 2),],
            "一级从 1 起、二级直接用事件里的 iteration，而第二次迭代才开二级组"
        );
    }

    /// 组头**就地改写**而不是另起一行 —— 直方图每来一条长一格、跨度跟着长，而它在屏上仍然
    /// 只占一行（票 12 那个窗格入口的第一个真实消费者）。
    #[test]
    fn the_unit_header_grows_in_place_and_stays_one_line() {
        let mut trace_page = state();
        let kimi = crate::events::SpeakerId::Debater("kimi".into());
        trace_page.apply(logged(
            1,
            EventPayload::TurnStarted {
                agent: kimi.clone(),
                iteration: 1,
            },
        ));
        // 组头画完之后记住它在哪一行 —— 它随后被就地改写，而**不是**被移走或另起一行。
        let header_id = trace_page
            .painted
            .iter()
            .find_map(|painted| match painted {
                Painted::GroupHeader(header) => Some(header.id),
                _ => None,
            })
            .expect("那个回合有一个一级组头");
        let header_row = trace_page
            .first_source_of(header_id)
            .expect("组头已经画出去了");
        for (offset, name) in ["bash", "bash", "read"].into_iter().enumerate() {
            let id = format!("c-{offset}");
            trace_page.apply(logged(
                10 + offset as u64 * 2,
                EventPayload::ToolCallStarted {
                    tool_call_id: ToolCallId::new(&id),
                    tool_name: name.to_owned(),
                    args: serde_json::json!({"command": name}),
                },
            ));
            trace_page.apply(logged(
                11 + offset as u64 * 2,
                EventPayload::ToolCallCompleted {
                    tool_call_id: ToolCallId::new(&id),
                    ok: true,
                    output: Some("body".to_owned()),
                    error: None,
                    duration_ms: 3,
                },
            ));
        }
        assert_eq!(
            trace_page.first_source_of(header_id),
            Some(header_row),
            "组头**就地改写**：直方图长到三次之后它仍在原来那一行上，没有被移到下面去"
        );
        let header = trace_page
            .painted
            .iter()
            .find_map(|painted| match painted {
                Painted::GroupHeader(header) if header.level == HeaderLevel::Unit => {
                    Some(header.clone())
                }
                _ => None,
            })
            .expect("那个回合有一个一级组头");
        assert_eq!(
            header.tools,
            vec![("bash".to_owned(), 2), ("read".to_owned(), 1)],
            "同类归并，按首次出现排序（`bash×2 read×1`）"
        );
    }

    /// 开场与压缩那两段**不属于任何一级组**，所以它们各有自己的小标题；而开场那条随注入数
    /// 增长而就地改写。
    #[test]
    fn a_preamble_gets_its_own_header_that_counts_its_injections() {
        let mut trace_page = state();
        for index in 0..3u64 {
            trace_page.apply(logged(
                index,
                EventPayload::ContextInjected {
                    source: crate::events::ContextSource::Identity,
                    content: format!("注入 {index}"),
                },
            ));
        }
        let header = trace_page
            .painted
            .iter()
            .find_map(|painted| match painted {
                Painted::SectionHeader(header) if header.kind == SectionKind::Preamble => {
                    Some(header.clone())
                }
                _ => None,
            })
            .expect("开场那一段有小标题");
        assert_eq!(
            header.injections, 3,
            "三条注入数上了，而那一行只占一条来源行"
        );
        assert!(
            section_header_line(&header)
                .to_string()
                .contains("3 条注入"),
            "它自己写着数：{:?}",
            section_header_line(&header).to_string()
        );
    }

    /// **展开态一律不用 `▸`**：它只表示「有折起来的东西」（票 21）。组头与小标题都不带它 ——
    /// 哪怕 04 票的例子里带过。
    #[test]
    fn no_header_carries_the_fold_glyph_while_everything_is_expanded() {
        let mut trace_page = state();
        trace_page.apply(logged(
            1,
            EventPayload::TurnStarted {
                agent: crate::events::SpeakerId::Debater("kimi".into()),
                iteration: 1,
            },
        ));
        trace_page.apply(logged(
            2,
            EventPayload::HistorySuperseded {
                targets: vec![1, 2],
                reason: crate::events::HistoryReason::Compaction,
                summary: Some("压成摘要".to_owned()),
            },
        ));
        for painted in &trace_page.painted {
            let line = match painted {
                Painted::GroupHeader(header) => group_header_line(header, 79, false),
                Painted::SectionHeader(header) => section_header_line(header),
                _ => continue,
            };
            assert!(
                !line.to_string().contains('▸'),
                "展开态的标题里不该有折叠那个字形：{:?}",
                line.to_string()
            );
        }
    }
}
