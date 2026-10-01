//! 目标：跨会话的工作单位（`.scratch/goal-loop/spec.md` §1、§2）。
//!
//! 一个**目标**是一份具名的清单文件：`<goals_dir>/<名字>.md`，人可读的 markdown。文件里
//! 只有条目与 id，**不带状态** —— 状态只有事件流一个真相源，进度由各会话的 `todo` 调用
//! 派生（见 [`crate::session`] 与循环侧的重算）。所以这个模块的读者与写者都是纯函数：
//! 给它一份文本或一个来源目录，它给你一份清单；它不读环境、不碰事件流。
//!
//! 清单**开工前封闭**：`/goal new` 生成一次，此后它与来源票各自独立。执行中冒出来的新工作
//! 由模型用 `goal_note` 记（§11），不回头改这份文件。

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::events::{Event, EventPayload};
use crate::tools::todo::{self, Item, Status};

/// 一个条目 id 的位数。两位十进制、从 `01` 起（§1）。
const ID_DIGITS: usize = 2;

/// 一个目标的名字最多多少个字符。
///
/// 与讨论者的名字同一条规矩：它是一个身份，会出现在 `/loop` 的参数与转录里，短到两边都读
/// 得下就好（`src/config.rs` 的 `MAX_DEBATER_NAME` 是同一个理由）。
pub const MAX_GOAL_NAME: usize = 64;

/// 清单里的一项：一个稳定 id 与一行内容。
///
/// id 在生成时分配，此后不动 —— 它是「清单第 3 条」与「`todo` 里那一项」认出彼此的凭据
/// （§3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub content: String,
}

/// 一份目标清单：名字，加上按顺序的条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub entries: Vec<Entry>,
}

/// 清单这一族动作失败的方式。
///
/// 每一种都是一句能被人据以行动的话：它们大多会被打到终端上（`/goal new` 是手势，不是工具
/// 调用）。
#[derive(Debug, thiserror::Error)]
pub enum GoalError {
    #[error("`{name}` 不能当目标的名字：名字必须是一个不断开的词，且不含路径分隔符")]
    InvalidName { name: String },
    #[error("找不到来源 {path}；它是 feature 目录（`.scratch/<slug>/`）或一份票文件")]
    SourceNotFound { path: String },
    #[error("{path} 里一张票都没有：它下面没有 `issues/NN-*.md`，也没有票文件")]
    NoTickets { path: String },
    #[error("来源里有 {count} 张票；清单的 id 只有两位十进制（`01`…`99`），装不下")]
    TooManyTickets { count: usize },
    #[error("目标 {path} 已经存在；要覆盖它，加 `--force`")]
    AlreadyExists { path: String },
    #[error("{path} 第 {line} 行：{message}")]
    Malformed {
        path: String,
        line: usize,
        message: String,
    },
    #[error("目标文件 {path} 读不了：{source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("目标文件 {path} 写不了：{source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
}

/// 这个名字能不能当目标名。
///
/// 三条：非空、一个不断开的词（不含空白与控制字符）、不含路径分隔符。第三条是文件系统那半
/// —— 这个名字就是文件名。
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_GOAL_NAME
        && !name.chars().any(char::is_whitespace)
        && !name.chars().any(char::is_control)
        && !name.contains(['/', '\\'])
        && name != "."
        && name != ".."
}

/// 把一个名字校验成目标名，校验不过时是一句能读懂的话。
pub fn validate_name(name: &str) -> Result<(), GoalError> {
    if is_valid_name(name) {
        Ok(())
    } else {
        Err(GoalError::InvalidName {
            name: name.to_owned(),
        })
    }
}

impl Manifest {
    /// 把一份清单渲染成人能读的 markdown。
    ///
    /// 形状就是开头那个标题加一列条目：没有复选框，因为状态不在这个文件里（§1）。
    pub fn render(&self) -> String {
        let mut text = format!("# {}\n\n", self.name);
        for entry in &self.entries {
            text.push_str(&format!("- {} {}\n", entry.id, entry.content));
        }
        text
    }

    /// 从一份手写的 markdown 里读回一份清单。
    ///
    /// 严格，而且错误指向那一行：读不出的半份清单比读不出来更糟 —— 人会以为那半份就是全部
    /// 的活。id 必须唯一且是两位十进制数字。
    pub fn parse(path: &Path, text: &str) -> Result<Manifest, GoalError> {
        let at = path.display().to_string();
        let malformed = |line: usize, message: String| GoalError::Malformed {
            path: at.clone(),
            line,
            message,
        };

        let mut name: Option<String> = None;
        let mut entries: Vec<Entry> = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            if name.is_none() {
                let Some(heading) = trimmed.strip_prefix('#') else {
                    return Err(malformed(
                        line,
                        format!("第一行非空的行必须是 `# <名字>` 这个标题，读到的是 `{trimmed}`"),
                    ));
                };
                let heading = heading.trim();
                if heading.is_empty() {
                    return Err(malformed(line, "标题里没有名字：写成 `# <名字>`".to_owned()));
                }
                name = Some(heading.to_owned());
                continue;
            }
            let Some(body) = trimmed.strip_prefix('-') else {
                return Err(malformed(
                    line,
                    format!("条目必须是 `- <id> <内容>` 这一形状，读到的是 `{trimmed}`"),
                ));
            };
            let body = body.trim_start();
            let mut rest = body.chars();
            let id: String = rest.by_ref().take(ID_DIGITS).collect();
            if id.chars().count() != ID_DIGITS || !id.chars().all(|ch| ch.is_ascii_digit()) {
                return Err(malformed(
                    line,
                    format!(
                        "条目的 id 必须是 {ID_DIGITS} 位十进制数字（`01`…`99`），\
                         读到的是 `{}`",
                        body.split_whitespace().next().unwrap_or("")
                    ),
                ));
            }
            let content: String = rest.collect();
            // id 后面必须断开：`003` 是三位数，不是 id `00` 加一句以 `3` 开头的内容。
            if !content.is_empty() && !content.starts_with(char::is_whitespace) {
                return Err(malformed(
                    line,
                    format!(
                        "条目 `{id}` 后面要有一个空格再接内容，读到的是 `{body}`"
                    ),
                ));
            }
            let content = content.trim().to_owned();
            if content.is_empty() {
                return Err(malformed(
                    line,
                    format!("条目 `{id}` 没有内容：写成 `- {id} <一行说它是什么>`"),
                ));
            }
            if entries.iter().any(|entry| entry.id == id) {
                return Err(malformed(
                    line,
                    format!("id `{id}` 重复了；条目 id 在一个清单里必须唯一"),
                ));
            }
            entries.push(Entry { id, content });
        }

        let Some(name) = name else {
            return Err(malformed(
                1,
                "清单是空的：它至少要有 `# <名字>` 这个标题".to_owned(),
            ));
        };
        Ok(Manifest { name, entries })
    }

    /// 这个清单的每个 id 与内容，按文件里的顺序。
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.id.as_str())
    }
}

impl fmt::Display for Manifest {
    /// 给人看的一行：`<名字>（N 条）`。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}（{} 条）", self.name, self.entries.len())
    }
}

/// 从一批票生成一份清单（§2）。
///
/// `<来源>` 是一个 feature 目录（读它下面的 `issues/NN-*.md`，或者目录本身就直接放着票），
/// 或者单独一份票文件。**只读票，不写回票**：生成一次，此后清单独立。
///
/// 票的编号就是条目的 id：`03-foo.md` → id `03`。所以来源里的编号必须落在两位十进制能表示
/// 的范围内，否则 [`GoalError::TooManyTickets`]。
pub fn generate(name: &str, source: &Path) -> Result<Manifest, GoalError> {
    validate_name(name)?;
    let files = ticket_files(source)?;
    let mut tickets: Vec<(u32, PathBuf)> = Vec::with_capacity(files.len());
    for file in files {
        let Some(number) = ticket_number(&file) else {
            continue;
        };
        tickets.push((number, file));
    }
    if tickets.is_empty() {
        return Err(GoalError::NoTickets {
            path: source.display().to_string(),
        });
    }
    tickets.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let mut entries = Vec::with_capacity(tickets.len());
    for (number, file) in tickets {
        if number > 10u32.pow(ID_DIGITS as u32) - 1 {
            return Err(GoalError::TooManyTickets { count: number as usize });
        }
        let content = ticket_title(&file)?;
        entries.push(Entry {
            id: format!("{number:0width$}", width = ID_DIGITS),
            content,
        });
    }
    Ok(Manifest {
        name: name.to_owned(),
        entries,
    })
}

/// 从磁盘上读一份清单。
pub fn load(dir: &Path, name: &str) -> Result<Manifest, GoalError> {
    validate_name(name)?;
    let path = manifest_path(dir, name);
    let text = std::fs::read_to_string(&path).map_err(|source| GoalError::Read {
        path: path.display().to_string(),
        source,
    })?;
    Manifest::parse(&path, &text)
}

/// 一份清单写在哪儿。
pub fn manifest_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.md"))
}

/// 一份清单在不在磁盘上。
pub fn exists(dir: &Path, name: &str) -> bool {
    manifest_path(dir, name).is_file()
}

/// 把一份清单写到它的位置上，拒绝覆盖已有的那份（除非 `force`）。
///
/// 只被属主读写（`0600`）：一份清单说的是这个人打算做什么。
pub fn store(dir: &Path, manifest: &Manifest, force: bool) -> Result<PathBuf, GoalError> {
    validate_name(&manifest.name)?;
    let path = manifest_path(dir, &manifest.name);
    if path.exists() && !force {
        return Err(GoalError::AlreadyExists {
            path: path.display().to_string(),
        });
    }
    std::fs::create_dir_all(dir).map_err(|source| GoalError::Write {
        path: dir.display().to_string(),
        source,
    })?;
    crate::tools::paths::write_owner_only(&path, manifest.render().as_bytes()).map_err(|source| {
        GoalError::Write {
            path: path.display().to_string(),
            source,
        }
    })?;
    Ok(path)
}

/// 生成并写下——`/goal new` 的那一步（§2）。
pub fn create(name: &str, source: &Path, dir: &Path, force: bool) -> Result<(Manifest, PathBuf), GoalError> {
    validate_name(name)?;
    // 重名先判：默默覆盖会抹掉一份已生成的目标定义，而生成来源可能是另一批票。
    let path = manifest_path(dir, name);
    if path.exists() && !force {
        return Err(GoalError::AlreadyExists {
            path: path.display().to_string(),
        });
    }
    let manifest = generate(name, source)?;
    let path = store(dir, &manifest, force)?;
    Ok((manifest, path))
}

/// 来源指向的那些票文件。目录先看它下面的 `issues/`，没有就直接看它自己。
fn ticket_files(source: &Path) -> Result<Vec<PathBuf>, GoalError> {
    if source.is_file() {
        return Ok(vec![source.to_path_buf()]);
    }
    if !source.is_dir() {
        return Err(GoalError::SourceNotFound {
            path: source.display().to_string(),
        });
    }
    let nested = source.join("issues");
    let dir = if nested.is_dir() { nested } else { source.to_path_buf() };
    let entries = std::fs::read_dir(&dir).map_err(|_| GoalError::SourceNotFound {
        path: source.display().to_string(),
    })?;
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| GoalError::SourceNotFound {
            path: source.display().to_string(),
        })?;
        let path = entry.path();
        if path.is_file() && ticket_number(&path).is_some() {
            files.push(path);
        }
    }
    if files.is_empty() {
        return Err(GoalError::NoTickets {
            path: source.display().to_string(),
        });
    }
    files.sort();
    Ok(files)
}

/// `03-foo.md` 的 `03`；文件名不是这个形状时是 `None`。
fn ticket_number(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    if !name.ends_with(".md") {
        return None;
    }
    let (number, rest) = name.split_once('-')?;
    if rest.is_empty() || number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// 一份票的标题：第一个非空行，去掉开头的 `#`。
fn ticket_title(path: &Path) -> Result<String, GoalError> {
    let text = std::fs::read_to_string(path).map_err(|source| GoalError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let title = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim().to_owned())
        .unwrap_or_default();
    if title.is_empty() {
        return Err(GoalError::Malformed {
            path: path.display().to_string(),
            line: 1,
            message: "这张票没有标题：它至少要有 `# <标题>` 这一行".to_owned(),
        });
    }
    Ok(title)
}

// --- 进度：从各会话的 `todo` 调用派生（§3） --------------------------------

/// 一条 `todo` 调用在流上的样子。
///
/// 真相源是那条 `tool_call` 的参数（与 `todo` 同构），所以派生只需要事件切片 —— 不读文件、
/// 不读环境。`session_at` 是**这个调用所在会话**的开场时刻，跨会话排序的第一把钥匙。
#[derive(Debug, Clone, PartialEq)]
pub struct TodoCall {
    pub session_at: DateTime<Utc>,
    pub seq: u64,
    pub items: Vec<Item>,
}

/// 从一条流上读出它所有的 `todo` 调用，按 `seq`。
///
/// 会话的开场时刻取这条流第一件事的时间：它只用来在跨会话合并时定先后，而一场会话自己的
/// 内部顺序由 `seq` 定。
pub fn todo_calls(events: &[Event]) -> Vec<TodoCall> {
    let session_at = events.first().map(|event| event.at).unwrap_or_else(Utc::now);
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallStarted {
                tool_name, args, ..
            } if tool_name == todo::TODO_TOOL => Some(TodoCall {
                session_at,
                seq: event.seq,
                items: todo::read_items(args),
            }),
            _ => None,
        })
        .collect()
}

/// 这条流有没有认领某个目标（§4 的归属筛）。
pub fn has_goal(events: &[Event], goal: &str) -> bool {
    events.iter().any(|event| {
        matches!(&event.payload, EventPayload::GoalSelected { goal: claimed } if claimed == goal)
    })
}

/// 一个目标当下的进度：每个条目的状态，以及那些引用了清单外 id 的调用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// 每个条目 id 的最新状态。清单是封闭的，所以这里的键就是清单的全部条目。
    pub statuses: BTreeMap<String, Status>,
    /// `todo` 里出现、而清单里没有的 id：**忽略它，但不静默**（§3）。
    pub unknown: Vec<String>,
}

impl Progress {
    pub fn completed(&self) -> usize {
        self.statuses
            .values()
            .filter(|status| **status == Status::Completed)
            .count()
    }

    pub fn total(&self) -> usize {
        self.statuses.len()
    }

    /// 全部条目都 `completed` 就算完成 —— 机械判据，不需要人点头。
    ///
    /// 信任假设：`completed` 是**模型自己填的**，所以判据机械**不等于**结果可靠。一条条目都没
    /// 有的清单也算完成（没活可干），而 `/goal new` 从不生成这样的清单。
    pub fn is_complete(&self) -> bool {
        self.statuses
            .values()
            .all(|status| *status == Status::Completed)
    }
}

/// 合并一个目标横跨的所有会话说出的状态（§3）。
///
/// **按 id 取最新**，排序键是（会话时间，`seq`）。所以「会话 1 把 `03` 标成完成、会话 2 的
/// `todo` 里根本没有 `03`」仍然算完成 —— 那正是「一个目标跨几个会话」要保住的信息。一条都
/// 没被提过的条目是 `pending`。
pub fn progress(entries: &[Entry], calls: &[TodoCall]) -> Progress {
    let mut statuses: BTreeMap<String, Status> = entries
        .iter()
        .map(|entry| (entry.id.clone(), Status::Pending))
        .collect();
    let mut unknown: Vec<String> = Vec::new();

    let mut ordered: Vec<&TodoCall> = calls.iter().collect();
    ordered.sort_by(|left, right| {
        left.session_at
            .cmp(&right.session_at)
            .then_with(|| left.seq.cmp(&right.seq))
    });
    for call in ordered {
        for item in &call.items {
            let Some(id) = item.id.as_deref() else {
                continue;
            };
            match statuses.get_mut(id) {
                Some(slot) => *slot = item.status,
                None if !unknown.iter().any(|seen| seen == id) => unknown.push(id.to_owned()),
                None => {}
            }
        }
    }
    Progress { statuses, unknown }
}

// --- `/loop` 的启动边界（§4） ----------------------------------------------

/// `/loop <名字>` 为什么不启动。
///
/// 三种边界各说各的人话，而且**都在写任何事件之前**判 —— 一次被拒的启动在流上不留痕迹。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRefusal {
    /// 清单文件找不到：提示先 `/goal new`。
    Unknown,
    /// 所有条目都完成了：没活可干。
    NoWork,
    /// 这个会话已经有一个 loop 在跑。
    AlreadyRunning,
    /// 权限档不够：`readonly` / `ask` 档下第一次写就停在等人，那时「无人值守」是空话（§5）。
    Unattended,
}

/// 判三条启动边界。
///
/// 顺序上「已经有一个 loop 在跑」最先：那是这个会话此刻的状态，与另一个名字好不好无关；紧跟
/// 着的是档位 —— 那一档不够时，跑都跑不起来，名字对不对是下一步的事。
pub fn check_start(
    manifest: Option<&Manifest>,
    complete: bool,
    running: bool,
    unattended: bool,
) -> Result<(), StartRefusal> {
    if running {
        return Err(StartRefusal::AlreadyRunning);
    }
    // 档位那一条排在清单前面：它说的是这个会话**能不能**无人值守，而名字对不对是下一步。
    if !unattended {
        return Err(StartRefusal::Unattended);
    }
    match manifest {
        None => Err(StartRefusal::Unknown),
        Some(_) if complete => Err(StartRefusal::NoWork),
        Some(_) => Ok(()),
    }
}

// --- 阈值与提醒（§6、§7） --------------------------------------------------

/// 回合边界上，窗口用量该触发什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdStep {
    /// 什么都没跨过。
    None,
    /// 过提醒线，而且这一档还没提醒过：注入一次提醒。
    Remind,
    /// 过翻页线：压缩 + 翻页（§7）。
    Compact,
}

/// 判一次窗口用量该触发什么。
///
/// 翻页优先：过八成就是压缩 + 开新会话，提醒轮不到（它本来就是更早的那一档）。`reminded` 是
/// 「这一档已经提醒过」这个跨回合的标记 —— **跨过阈值时注入一次，不是每轮**：每轮注入会每轮
/// 打掉前缀缓存，而本仓库有「前缀只增不改」的不变量。翻页之后调用方把它复位，于是新会话过线
/// 时还会再提醒一次 —— 那是**新会话**的提醒，正确。
///
/// 判据**只用传进来的这一个数**，所以压缩那次调用自己不会触发第二次翻页：量是在动作之前取
/// 的。
pub fn threshold_step(percent: u64, remind_at: u8, compact_at: u8, reminded: bool) -> ThresholdStep {
    if percent >= u64::from(compact_at) {
        ThresholdStep::Compact
    } else if percent >= u64::from(remind_at) && !reminded {
        ThresholdStep::Remind
    } else {
        ThresholdStep::None
    }
}

// --- 无进展与停止（§9） -----------------------------------------------------

/// 无进展的计数：**连续几次翻页零条目完成**。
///
/// 计数对象是**翻页**，不是回合 —— 单位更长，误判更少。它和完成判据同源（都是
/// [`Progress`]），所以是机械可算的：没有它，一个卡住的目标会一直翻页重试到烧完预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoProgress {
    streak: u32,
    /// 上一次翻页时已完成几条。
    completed: usize,
}

impl NoProgress {
    /// 从一个目标的当前进度起算。
    pub fn new(completed: usize) -> Self {
        Self {
            streak: 0,
            completed,
        }
    }

    /// 一次翻页之后记一笔，返回这一刻的连续次数。
    ///
    /// **中途完成任意一条就归零**，重新计 —— 那正是「有进展」的定义。
    pub fn after_rollover(&mut self, completed: usize) -> u32 {
        if completed > self.completed {
            self.streak = 0;
        } else {
            self.streak += 1;
        }
        self.completed = completed;
        self.streak
    }

    /// 这一刻连续几次翻页没有进展。
    pub fn streak(&self) -> u32 {
        self.streak
    }

    /// 到了停下来报告的那条线吗（连续 `limit` 次零完成）。
    pub fn reached(&self, limit: u32) -> bool {
        self.streak >= limit
    }
}

/// provider 调用的重试预算（§9）。
///
/// 它数的是**回合级**的重试：一个回合以 `Error` 收场时再驱动一次，而不是重发一次调用 ——
/// 失败那一次留在流上的东西照旧是真相。重试之间固定等 [`RETRY_DELAY`](crate::cli) 那一档
/// 时间（在循环侧定死），耗尽之后停下并报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    failures: u32,
    limit: u32,
}

impl Retry {
    pub fn new(limit: u32) -> Self {
        Self { failures: 0, limit }
    }

    /// 记一次失败：还能再试就 `true`。
    pub fn failed(&mut self) -> bool {
        self.failures += 1;
        self.failures <= self.limit
    }

    /// 记一次成功：预算回满。
    pub fn succeeded(&mut self) {
        self.failures = 0;
    }

    /// 到这一刻为止失败了几次（报告里要可核对）。
    pub fn failures(&self) -> u32 {
        self.failures
    }
}

/// 停下时报告里点名的那些条目：还没完成的那些。
///
/// 报告只说「停了」没有用 —— 要说清卡在哪些条目上，回头才核对得出它是怎么卡住的。
pub fn unfinished(entries: &[Entry], progress: &Progress) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| {
            progress
                .statuses
                .get(&entry.id)
                .is_none_or(|status| *status != Status::Completed)
        })
        .map(|entry| entry.id.clone())
        .collect()
}

// --- 崩溃恢复与主动停（§10） -----------------------------------------------

/// 一条流是怎么结束的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// **正常收尾**：从当前目标的归属那一刻起，流上有一条收尾事件（完成 / 无进展停 /
    /// provider 失败停 / 人主动停 / 额度撞顶）。
    Closed,
    /// **异常中断**：没有收尾事件 —— 进程被杀、机器重启，或者一场崩掉的运行留下的任何样子。
    Interrupted,
}

/// 一条收尾事件：目标完成，或者一条「停下了」。
///
/// 恢复只认这两种。其余任何东西 —— 包括 `SessionError` —— 都不是收尾：一次崩溃的进程也可能
/// 刚好留下一条错误。
pub fn is_closing(payload: &EventPayload) -> bool {
    matches!(
        payload,
        EventPayload::GoalCompleted { .. } | EventPayload::GoalStopped { .. }
    )
}

/// 从流派生「这一次是怎么结束的」（§10）。
///
/// 判据取**当前目标归属那一刻之后**的那一段：一个会话做完目标 A、又 `/loop` 了目标 B、然后崩
/// 掉，A 那条完成事件不能替 B 说话。没有归属时它答 `Interrupted` —— 调用方（`/loop` 的恢复）
/// 先问「有没有当前目标」，不会走到这里。
pub fn ending(events: &[Event]) -> Ending {
    let from = events.iter().rposition(|event| {
        matches!(event.payload, EventPayload::GoalSelected { .. })
    });
    let Some(from) = from else {
        return Ending::Interrupted;
    };
    if events[from..]
        .iter()
        .any(|event| is_closing(&event.payload))
    {
        Ending::Closed
    } else {
        Ending::Interrupted
    }
}

// --- 收尾汇总（§11） -------------------------------------------------------

/// 一个目标下各会话记下的新工作。
///
/// 清单是封闭的，所以执行中冒出来的新工作只能由 `goal_note` 记 —— 而那里的真相同样只是那次
/// 调用的参数。这里把它们按流上的顺序读回来，交给收尾汇总。
pub fn notes_of(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ToolCallStarted {
                tool_name, args, ..
            } if tool_name == crate::tools::goal_note::GOAL_NOTE_TOOL => {
                Some(crate::tools::goal_note::read_notes(args))
            }
            _ => None,
        })
        .flatten()
        .collect()
}

/// 收尾汇总那次模型调用的简报（§11）。
///
/// 四项一个都不能省：目标名、条目完成情况、跨了几个会话、执行中冒出来但没进清单的新工作 ——
/// 最后一项最不能省，因为清单是封闭的，新工作没有别的地方交代。
pub fn summary_prompt(
    goal: &str,
    manifest: &Manifest,
    progress: &Progress,
    sessions: usize,
    notes: &[String],
) -> String {
    let mut prompt = String::from(
        "无人值守的目标循环刚刚做完。写一段收尾汇总，给把这个目标交出去的人看。\
         用中文，直接写那段汇总，不要标题、不要客套。必须交代这四件事：\n\
         1. 目标名；\n\
         2. 条目完成情况（几条完成 / 共几条）；\n\
         3. 这个目标跨了几个会话；\n\
         4. 执行中冒出来、但没有进清单的新工作 —— 一条一条说，一条都没有就说明没有。\n\n",
    );
    prompt.push_str(&format!("目标：{goal}\n\n清单：\n"));
    for entry in &manifest.entries {
        let status = progress
            .statuses
            .get(&entry.id)
            .map(|status| status.as_str())
            .unwrap_or("pending");
        prompt.push_str(&format!("- {} {}（{status}）\n", entry.id, entry.content));
    }
    prompt.push_str(&format!(
        "\n完成情况：{}/{} 条完成\n跨会话：{sessions} 个会话\n\n",
        progress.completed(),
        progress.total()
    ));
    if notes.is_empty() {
        prompt.push_str("执行中冒出来的新工作：没有记下任何一条。\n");
    } else {
        prompt.push_str("执行中冒出来的新工作：\n");
        for note in notes {
            prompt.push_str(&format!("- {note}\n"));
        }
    }
    prompt
}

/// 汇总那次模型调用没成时用的那份说明（§11）。
///
/// 判据是机械的，所以「做完了」这件事不依赖模型能不能开口；缺的只是那段叙述。这一份把四项
/// 照原样摆出来，谁读都知道发生了什么。
pub fn fallback_summary(goal: &str, progress: &Progress, sessions: usize, notes: &[String]) -> String {
    let mut text = format!(
        "目标 {goal} 完成：{} / {} 条完成，跨 {sessions} 个会话。",
        progress.completed(),
        progress.total()
    );
    if notes.is_empty() {
        text.push_str("执行中没有记下清单外的新工作。");
    } else {
        text.push_str("执行中冒出来、没进清单的新工作：");
        for note in notes {
            text.push_str(&format!("\n- {note}"));
        }
    }
    text
}


