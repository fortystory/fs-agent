//! 目标：跨会话的工作单位（`.scratch/goal-loop/spec.md` §1、§2）。
//!
//! 一个**目标**是一份具名的清单文件：`<goals_dir>/<名字>.md`，人可读的 markdown。文件里
//! 只有条目与 id，**不带状态** —— 状态只有事件流一个真相源，进度由各会话的 `todo` 调用
//! 派生（见 [`crate::session`] 与循环侧的重算）。所以这个模块的读者与写者都是纯函数：
//! 给它一份文本或一个来源目录，它给你一份清单；它不读环境、不碰事件流。
//!
//! 清单**开工前封闭**：`/goal new` 生成一次，此后它与来源票各自独立。执行中冒出来的新工作
//! 由模型用 `goal_note` 记（§11），不回头改这份文件。

use std::fmt;
use std::path::{Path, PathBuf};

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
