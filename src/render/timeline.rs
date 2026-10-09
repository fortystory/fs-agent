//! 时间轴那三行横带的账与几何（[`spec.md` §10](../../.scratch/trace-ledger/spec.md)、票 22）。
//!
//! 两件事分开住在这里，因为它们的**判据必须只有一处**，而消费者在别处：
//!
//! * [`Timeline`] —— **整趟会话的 span 表**，在事件到达时增量累积（票 22 第 7 条）。横带每帧
//!   只读它，不重扫 `painted`：那里的块数随会话线性长，而今天别处也没有每帧遍历全部块的地方。
//! * [`Axis`] —— **一格多少时间、某个时刻落在哪一格**。它是纯算术，于是「不足一格不画」这条
//!   保底规则可以在单测里逐条钉住，而不必先画一帧。
//!
//! 这里一个字形都不画：泳道怎么上屏归 `tui` 的画家，本模块只回答「哪几格是谁」。

use chrono::{DateTime, TimeDelta, Utc};

use crate::events::{SpeakerId, ToolCallId};

/// 泳道标签占的列：`模型 ` / `工具 ` —— 两个汉字加一格空白（票 22 的宽度口径）。
///
/// 横轴 = 正文宽扣掉它。它是**横带自己的排版**，与账本行首那九列时刻列无关：那是另一笔账，
/// 而横带三行也照它缩进，于是泳道与正文左对齐。
pub const LABEL_COLUMNS: u16 = 5;

/// 一个刻度标签的列数：`HH:MM:SS`。
pub const TICK_COLUMNS: u16 = 8;

/// 完整刻度要的轴宽：五个标签之间各留一格。
const FULL_TICKS_FROM: u16 = 44;

/// 稀疏刻度要的轴宽：两个标签之间留得出一段空白，好让「中间那一段」读得出是一段。
const SPARSE_TICKS_FROM: u16 = 32;

/// 一行三态版要的轴宽：九格是「三种状态各自读得出一格」的地板。
const COMPACT_BAND_FROM: u16 = 9;

/// 横带在这个宽度上退成哪一档（票 22 第 10 条的宽度阶梯）。
///
/// 宽度说的是**横轴**的列数，不是终端宽：终端越窄，那条轴上的一格就越粗，而粗到一定程度
/// 刻度标签先挤，再就是整条横带都读不出来了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandKind {
    /// 三行完整版：刻度五个（起止 + 三个中点），两条泳道各画各的。
    Full,
    /// 三行，刻度只留起止两个 —— 形状还在，读数稀疏了。
    Sparse,
    /// 两行：刻度加一条**一行三态版**（模型在跑 / 工具在跑 / 工具叠着）。
    Compact,
    /// 整条不画：轴窄到读不出东西，那一行还给账本。
    Empty,
}

impl BandKind {
    /// 这个轴宽退成哪一档。
    pub fn of_axis(axis_width: u16) -> BandKind {
        if axis_width >= FULL_TICKS_FROM {
            BandKind::Full
        } else if axis_width >= SPARSE_TICKS_FROM {
            BandKind::Sparse
        } else if axis_width >= COMPACT_BAND_FROM {
            BandKind::Compact
        } else {
            BandKind::Empty
        }
    }

    /// 这一档占掉的行数。降级省下来的行**还给账本** —— 横带是读法，账本是内容。
    pub fn rows(self) -> u16 {
        match self {
            BandKind::Full | BandKind::Sparse => 3,
            BandKind::Compact => 2,
            BandKind::Empty => 0,
        }
    }

    /// 还画泳道标签吗。一行三态版把两条泳道压进一行，于是没有哪条泳道需要名字。
    pub fn labels(self) -> bool {
        matches!(self, BandKind::Full | BandKind::Sparse)
    }
}

/// 模型泳道的一格：这一段在**等首 token** 还是在**吐字**（票 22 第 2 条）。
///
/// 只在拿得到首 token 时刻时才有 `Wait`：老会话整段都是一个 `Decode`，因为那时轴只知道
/// 「这一段是模型调用」，切点不存在。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCell {
    /// 等首 token —— 这一次调用起头那一段，供应商还没吐第一个增量。
    Wait,
    /// 吐字 —— 第一个增量之后的那一段。
    Decode,
}

/// 模型泳道的一段：一次模型调用。
#[derive(Debug, Clone)]
struct ModelSpan {
    speaker: SpeakerId,
    start: DateTime<Utc>,
    /// 首 token 那一刻（`start + first_token_ms`）。老流与用户消息那一条都没有它。
    first_token: Option<DateTime<Utc>>,
    /// 收尾那一刻；还开着就是 `None`（画到当前时刻）。
    end: Option<DateTime<Utc>>,
}

/// 工具泳道的一段：一次工具调用。**并发调用会重叠**，所以泳道按叠层数画（票 22 第 3 条）。
#[derive(Debug, Clone)]
struct ToolSpan {
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
}

/// 整趟会话的时间轴账：两泳道的段，加上单位边界那一族时刻。
///
/// 它由**数据到达**驱动（[`Timeline::turn_started`] 那一族），不每帧重扫块 —— 票 22 第 7 条
/// 就是这一条。宽度重放不重建它：重放换的是窗格里的行，这一趟的秒数一个都没变。
#[derive(Debug, Default)]
pub struct Timeline {
    model: Vec<ModelSpan>,
    tools: Vec<ToolSpan>,
    /// 还开着的那几条模型段（按发言者）。一个发言者同时只开一段，但讨论会话里两个发言者
    /// 可以同时开着 —— 所以它是一张表，不是一个 `Option`。
    open_model: Vec<(SpeakerId, usize)>,
    /// 还开着的那几次工具调用。
    open_tools: Vec<(ToolCallId, usize)>,
    /// 整趟会话的起止。只增不减：轴域是**整趟**（票 22 第 6 条），不随视口滑动。
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    /// 单位边界（回合 / 轮次的起头那一刻）—— 模型泳道里那道竖线画在这里。
    bounds: Vec<DateTime<Utc>>,
}

impl Timeline {
    /// 一次发言的起始迭代：它开一段模型调用，并把同一个发言者上一段收在**这一刻**
    /// （票 22 第 2 条的「下一个同发言者的起始事件」）。
    pub fn turn_started(&mut self, speaker: &SpeakerId, iteration: u32, at: DateTime<Utc>) {
        self.mark(at);
        self.close_model(speaker, at);
        self.model.push(ModelSpan {
            speaker: speaker.clone(),
            start: at,
            first_token: None,
            end: None,
        });
        self.open_model
            .push((speaker.clone(), self.model.len() - 1));
        // 一个单位的头是**起始迭代为 1** 的那一条（交互会话），或者是轮次的开始
        // （讨论会话，见 [`Timeline::round_started`]）。迭代边界不标：一个回合 1–557 次迭代，
        // 太密（票 22 第 4 条）。
        if iteration == 1 {
            self.bound(at);
        }
    }

    /// 讨论会话的一轮开始了。它是那一级的单位边界，于是也画一道竖线。
    pub fn round_started(&mut self, at: DateTime<Utc>) {
        self.mark(at);
        self.bound(at);
    }

    /// 一个单位收尾了：还开着的那些模型段收在这一刻。
    pub fn turn_ended(&mut self, speaker: &SpeakerId, at: DateTime<Utc>) {
        self.mark(at);
        self.close_model(speaker, at);
    }

    /// 轮到下一个发言者了（讨论会话）：除他之外还开着的那几段都收在这一刻。
    pub fn round_ended(&mut self, at: DateTime<Utc>) {
        self.mark(at);
        let open: Vec<SpeakerId> = self.open_model.iter().map(|(who, _)| who.clone()).collect();
        for speaker in open {
            self.close_model(&speaker, at);
        }
    }

    /// 一次模型调用完成了，并带回了它的首 token 时刻（ADR 0020）。
    ///
    /// 切点 = 这次调用的起点 + `first_token_ms`。老流没有这个字段，于是那一段保持整段
    /// —— 轴不知道切点在哪，就不切（票 22 第 2 条那一句「宁缺勿假」的邻居）。
    pub fn message_completed(&mut self, speaker: &SpeakerId, first_token_ms: Option<u64>) {
        let Some(ms) = first_token_ms else {
            return;
        };
        let Some(index) = self
            .model
            .iter()
            .rposition(|span| span.speaker == *speaker && span.first_token.is_none())
        else {
            return;
        };
        let start = self.model[index].start;
        self.model[index].first_token = Some(start + TimeDelta::milliseconds(ms as i64));
    }

    /// 一次工具调用开始了。它自己一段；**同时跑着的其他调用照旧开着** —— 泳道要能叠
    /// （票 22 第 3 条）。
    pub fn tool_started(&mut self, id: &ToolCallId, at: DateTime<Utc>) {
        self.mark(at);
        self.tools.push(ToolSpan {
            start: at,
            end: None,
        });
        self.open_tools.push((id.clone(), self.tools.len() - 1));
    }

    /// 一次工具调用完成了。
    pub fn tool_completed(&mut self, id: &ToolCallId, at: DateTime<Utc>) {
        self.mark(at);
        if let Some(slot) = self.open_tools.iter().position(|(open, _)| open == id) {
            let (_, index) = self.open_tools.remove(slot);
            self.tools[index].end = Some(at);
        }
    }

    /// 这一趟里有什么事可画吗。
    pub fn is_empty(&self) -> bool {
        self.model.is_empty() && self.tools.is_empty()
    }

    /// 这一帧的轴域：**整趟会话**（票 22 第 6 条）。进行中的段画到 `now`，于是轴的右端跟着
    /// 当前时刻长 —— 流式时轴也在走。
    ///
    /// 什么都还没发生时是 `None`（那时整条横带不画：没有时间可以铺）。
    pub fn domain(&self, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let start = self.start?;
        let end = self.end.unwrap_or(start);
        // 没有进行中的段时轴的右端**不跟墙钟走**：那会让一趟静止的会话每帧都在变宽。
        let end = if self.running() { end.max(now) } else { end };
        Some((start, end))
    }

    /// 还有段在跑吗（画到当前时刻的那几段）。
    pub fn running(&self) -> bool {
        !self.open_model.is_empty() || !self.open_tools.is_empty()
    }

    /// 模型泳道每一格是什么。`now` 是进行中的那几段的右端。
    pub fn model_cells(&self, axis: Axis, now: DateTime<Utc>) -> Vec<Option<ModelCell>> {
        let mut cells = vec![None; axis.width as usize];
        for span in &self.model {
            let end = span.end.unwrap_or(now);
            match span.first_token {
                // 切两段：等首 token 与吐字**各自**判「不足一格不画」—— 一个 0.3 s 的等首
                // 段不该被撑成一格（票 22 第 2 条）。
                Some(first) => {
                    paint(&mut cells, axis.cells(span.start, first), ModelCell::Wait);
                    paint(&mut cells, axis.cells(first, end), ModelCell::Decode);
                }
                None => paint(&mut cells, axis.cells(span.start, end), ModelCell::Decode),
            }
        }
        cells
    }

    /// 工具泳道每一格叠了几段：`0` 是空、`1` 是一段、`2` 起是并发叠着。
    pub fn tool_depth(&self, axis: Axis, now: DateTime<Utc>) -> Vec<u8> {
        let mut depth = vec![0u8; axis.width as usize];
        for span in &self.tools {
            let Some((first, last)) = axis.cells(span.start, span.end.unwrap_or(now)) else {
                continue;
            };
            for cell in depth.iter_mut().take(last as usize).skip(first as usize) {
                *cell = cell.saturating_add(1);
            }
        }
        depth
    }

    /// 单位边界落在哪几格（模型泳道里那道竖线）。
    pub fn turn_marks(&self, axis: Axis) -> Vec<u16> {
        let mut marks: Vec<u16> = self.bounds.iter().map(|at| axis.column(*at)).collect();
        marks.dedup();
        marks
    }

    /// 记下这一趟里的一刻 —— 只把轴域往两头撑，不产生任何 span。
    ///
    /// 喂进来的是**每一条带信封的事件**：轴域是整趟会话（票 22 第 6 条），而它从这一趟的
    /// 第一条事件起、到最后一条止。用户消息、注入、用量各是一刻，于是它们前后的空白读作
    /// 留白，而不是被裁掉 —— 一个刚开出来的会话也有一根从第一条消息起算的轴。
    pub fn observed(&mut self, at: DateTime<Utc>) {
        self.mark(at);
    }

    /// 记下这一趟的第一刻与最后一刻。
    fn mark(&mut self, at: DateTime<Utc>) {
        self.start = Some(self.start.map_or(at, |start| start.min(at)));
        self.end = Some(self.end.map_or(at, |end| end.max(at)));
    }

    /// 记一个单位边界，同刻只记一次（`RoundStarted` 与它那一次的 `TurnStarted(1)` 常常同刻）。
    fn bound(&mut self, at: DateTime<Utc>) {
        if self.bounds.last() != Some(&at) {
            self.bounds.push(at);
        }
    }

    /// 把某个发言者还开着的那一段收在 `at`。
    fn close_model(&mut self, speaker: &SpeakerId, at: DateTime<Utc>) {
        for slot in (0..self.open_model.len()).rev() {
            if self.open_model[slot].0 != *speaker {
                continue;
            }
            let (_, index) = self.open_model.remove(slot);
            // 一个发言者同时只有一段开着，所以「找到第一段就够」不成立 —— 但收尾那一刻
            // 不早于起点：同刻的收尾（`TurnEnded` 紧跟着 `TurnStarted`）不该把段变成负的。
            if at >= self.model[index].start {
                self.model[index].end = Some(at);
            }
        }
    }
}

/// 把一段格子涂上。落在轴外的部分由 [`Axis::cells`] 夹掉。
fn paint(cells: &mut [Option<ModelCell>], span: Option<(u16, u16)>, value: ModelCell) {
    let Some((first, last)) = span else {
        return;
    };
    for cell in cells.iter_mut().take(last as usize).skip(first as usize) {
        *cell = Some(value);
    }
}

/// 横轴的域与分辨率：整趟会话铺在 `width` 格上。
///
/// 一格多长**不是常数** —— 会话越长一格越粗，这是诚实的形状（票 06 §3），也是为什么每次
/// 换算都要带上这个轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axis {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    width: u16,
}

impl Axis {
    /// 一个新轴。`end <= start` 或一格都没有时是 `None` —— 那时横带整条不画。
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>, width: u16) -> Option<Axis> {
        if width == 0 || end <= start {
            return None;
        }
        Some(Axis { start, end, width })
    }

    /// 轴有多少格。
    pub fn width(&self) -> u16 {
        self.width
    }

    /// 这一刻落在哪一格（0 起数）。域外的时刻夹到两端 —— 调用方给的都是域内的那一族。
    pub fn column(&self, at: DateTime<Utc>) -> u16 {
        self.position(at)
            .floor()
            .clamp(0.0, (self.width - 1) as f64) as u16
    }

    /// 一段落在哪些格上（半开区间 `[first, last)`）。
    ///
    /// **不足一格的一段是 `None`**：轴不画它，而不是把它撑成一格 —— 「保底画一格就是说谎」
    /// （票 22 第 2 条、票 11 §5）。
    pub fn cells(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Option<(u16, u16)> {
        let (left, right) = (self.position(from), self.position(to));
        // 「不足一格」按**段的宽度**判，而不是按取整后的两端：0.3 格的段向上取整也会占一格，
        // 而占一格就等于把它说成比实际长。
        if right - left < 1.0 {
            return None;
        }
        let first = left.floor().clamp(0.0, self.width as f64) as u16;
        let last = right.ceil().clamp(0.0, self.width as f64) as u16;
        if last <= first {
            return None;
        }
        Some((first, last))
    }

    /// 一格左缘那一刻 —— 刻度标签按它取时刻。
    pub fn at_of(&self, column: u16) -> DateTime<Utc> {
        let span = (self.end - self.start).num_milliseconds();
        let offset = span * i64::from(column.min(self.width)) / i64::from(self.width);
        self.start + TimeDelta::milliseconds(offset)
    }

    /// 这一刻在轴上离起点多远（以格计，可以是小数，也可以在域外）。
    fn position(&self, at: DateTime<Utc>) -> f64 {
        let span = (self.end - self.start).num_milliseconds() as f64;
        let offset = (at - self.start).num_milliseconds() as f64;
        offset / span * f64::from(self.width)
    }
}

/// 刻度行要画的那些标签：`(列, 时刻)`。
///
/// 完整档是**起止加三个中点**，位置首尾贴边、中间均分；稀疏与一行三态版只留起止两个
/// （票 06 §2 那张刻度排法）。
pub fn ticks(axis: Axis, kind: BandKind) -> Vec<(u16, DateTime<Utc>)> {
    let last = axis.width.saturating_sub(TICK_COLUMNS);
    let columns: Vec<u16> = match kind {
        BandKind::Full => (0..5).map(|index| last * index / 4).collect(),
        BandKind::Sparse | BandKind::Compact => vec![0, last],
        BandKind::Empty => Vec::new(),
    };
    columns
        .into_iter()
        .map(|column| (column, axis.at_of(column)))
        .collect()
}
