//! 技能：按需披露的指令包（spec §9）。
//!
//! 一个技能是一个含 `SKILL.md` 的目录：一段带 `name` 与 `description` 的 YAML frontmatter，加上
//! 一段 Markdown 指令正文。便宜的那一半 —— `<name>: <description>` 那份清单 —— 被钉在上下文头部、
//! 每一回合都在场；贵的那一半由内建的 `skill(name)` 工具按需加载，作为一条普通的工具结果追加在
//! 尾部，所以缓存前缀永远不动。
//!
//! 发现遵循事实标准：项目级在用户级之前，每级三个根（cwd 下的 `.fs-agent` > `.agents` > `.claude`，
//! 然后是 `~/.config/fs-agent` > `~/.agents` > `~/.claude`）—— 一台本来就有 `.claude` 或
//! `.agents` 技能库的机器白得它。先定义某个名字的那个根胜出。
//!
//! 三份互不相干的预算，都用 [`estimate_tokens`] 度量：
//!
//! * [`MAX_CATALOG_TOKENS`] 限制被钉住的那份描述清单；
//! * [`MAX_SKILL_TOKENS`] 限制单份正文，超了是裁剪并带一个指向该文件的指针，而不是拒绝；
//! * [`MAX_LOADED_SKILL_TOKENS`] 限制一次请求里加载进来的正文总量，由 [`crate::context::trim`]
//!   强制（最旧的先丢）。
//!
//! 一个 `disable-model-invocation: true` 的技能对模型是不可见的：它不在清单里，而 [`Skills::load`]
//! 按名字拒掉它，所以猜一个名字也绕不过这个旗标。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::events::{Event, EventPayload};

use super::{estimate_tokens, CHARS_PER_TOKEN};

/// 加载一份技能正文的那个内建工具。只在这里命名一次，因为「加载一个技能」意味着什么由技能模块
/// 拥有：工具、黏性丢弃类别与重算出来的已加载集合，都必须对这个名字达成一致。
pub const SKILL_TOOL: &str = "skill";

/// 单份已加载技能正文的上限（spec §9）。超了，正文被裁剪并带一个指向全文文件的指针，而不是被拒。
pub const MAX_SKILL_TOKENS: u64 = 5_000;

/// 一次请求里已加载技能正文的聚合上限（spec §9）。由 `trim` 强制：最旧的正文先丢。
pub const MAX_LOADED_SKILL_TOKENS: u64 = 25_000;

/// 被钉住的描述清单的上限（spec §9），与另外两条互不相干。
pub const MAX_CATALOG_TOKENS: u64 = 3_000;

/// 每个技能目录都必须含有的那个文件。
const SKILL_FILE: &str = "SKILL.md";

/// 项目级的根，最具体的在前（spec §9）。
const PROJECT_ROOTS: [&str; 3] = [".fs-agent", ".agents", ".claude"];

/// 用户级的根，相对注入进来的家目录，最具体的在前。
const USER_ROOTS: [&str; 3] = [".config/fs-agent", ".agents", ".claude"];

/// 一个已发现的技能。
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// `disable-model-invocation: true`：模型既看不见它、也加载不了它。
    pub model_invocation_disabled: bool,
    /// 这一份来自的那个 `SKILL.md`；一份被裁剪的正文会点名它。
    pub path: PathBuf,
    body: String,
}

/// 一个技能为什么加载不了。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillError {
    #[error("没有这个技能：`{name}`；你上下文里的技能清单列着可用的名字")]
    Unknown { name: String },
    #[error(
        "技能 `{name}` 设了 `disable-model-invocation: true`；只有用户能调用它，模型不许加载它"
    )]
    Disabled { name: String },
}

/// 已发现的技能，按优先级顺序。是一个值：组装期发现一次、会话携带它、永不改动。
#[derive(Debug, Clone, Default)]
pub struct Skills {
    skills: Vec<Skill>,
}

impl Skills {
    /// 扫项目级与用户级的根，最具体的在前，并为每个名字保留第一个定义（spec §9）。
    ///
    /// 缺失或读不出来的根什么都贡献不了：一个没有技能目录的仓库就是没有技能而已。
    pub fn discover(cwd: &Path, home: Option<&Path>) -> Self {
        let mut roots: Vec<PathBuf> = PROJECT_ROOTS
            .iter()
            .map(|dir| cwd.join(dir).join("skills"))
            .collect();
        if let Some(home) = home {
            roots.extend(USER_ROOTS.iter().map(|dir| home.join(dir).join("skills")));
        }

        let mut skills: Vec<Skill> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for root in roots {
            for skill in skills_in_root(&root) {
                if seen.insert(skill.name.clone()) {
                    skills.push(skill);
                }
            }
        }
        Self { skills }
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// 每一个已发现的名字，按优先级顺序。
    pub fn names(&self) -> Vec<&str> {
        self.skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect()
    }

    /// 每一个已发现的技能，形如 `(名字, 描述)`，按优先级顺序：前端把它当作 `/<名字>` 提供，连同
    /// 那句说明它是干什么的。
    ///
    /// 与 [`catalog`](Self::catalog) 不同，这一份保留 `disable-model-invocation` 的技能。前端的
    /// `/` 菜单是**用户的**列表，而那些技能只对用户存在 —— 一个把它们丢掉的菜单，恰好丢掉的就是
    /// 模型自己提供不了的那些名字。
    pub fn entries(&self) -> Vec<(&str, &str)> {
        self.skills
            .iter()
            .map(|skill| (skill.name.as_str(), skill.description.as_str()))
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    /// 按名字查一个，或者给出点名它的那个错误。
    fn lookup(&self, name: &str) -> Result<&Skill, SkillError> {
        self.get(name).ok_or_else(|| SkillError::Unknown {
            name: name.to_owned(),
        })
    }

    /// 放进工具结果的那份正文，以 [`MAX_SKILL_TOKENS`] 为上限。
    ///
    /// 超上限时正文被裁剪，而裁剪里点名那个文件，好让模型去读剩下的；改成拒绝会让一个稍微长一点的
    /// 技能变得不可用。
    pub fn load(&self, name: &str) -> Result<String, SkillError> {
        let skill = self.lookup(name)?;
        if skill.model_invocation_disabled {
            return Err(SkillError::Disabled {
                name: skill.name.clone(),
            });
        }
        Ok(capped_body(skill))
    }

    /// **用户**要一个技能时放进上下文的那份正文，`/<名字>` 就是这么做的（spec §9）。
    ///
    /// 与 [`Skills::load`] 不同，它不管 `disable-model-invocation`：那个旗标把技能挡在*模型的*猜测
    /// 之外，而用户点它的名正是它所保留的那种调用。正文用同样的方式设上限。
    pub fn invoke(&self, name: &str) -> Result<String, SkillError> {
        Ok(capped_body(self.lookup(name)?))
    }

    /// 被钉住的那份描述清单；没有任何技能可调用时是 `None`。
    ///
    /// 每一回合都在场，而且跨回合逐字节稳定：这段文本是缓存前缀的一部分。
    pub fn catalog(&self) -> Option<String> {
        self.render_catalog(MAX_CATALOG_TOKENS)
    }

    /// 在显式预算内渲染出来的清单。整个条目保留，靠后的条目连同计数一起省掉，于是这条上限是一个
    /// 界，而不是从行中间切一刀。
    pub fn render_catalog(&self, budget: u64) -> Option<String> {
        const HEADER: &str =
            "按需取用的技能（某个技能的描述与任务相符时，用 `skill` 工具带上它的名字来调用）：";

        if estimate_tokens(HEADER) > budget {
            return None;
        }
        let lines: Vec<String> = self
            .skills
            .iter()
            .filter(|skill| !skill.model_invocation_disabled)
            .map(|skill| format!("- {}: {}", skill.name, skill.description))
            .collect();
        if lines.is_empty() {
            return None;
        }

        // 装得下的最大前缀，从保留最多条目开始试。估计值对长度单调，所以这找到的是预算内最大的
        // 那份清单。
        for kept in (0..=lines.len()).rev() {
            let text = catalog_text(HEADER, &lines[..kept], lines.len() - kept);
            if estimate_tokens(&text) <= budget {
                return Some(text);
            }
        }
        None
    }
}

fn catalog_text(header: &str, lines: &[String], omitted: usize) -> String {
    let mut text = header.to_owned();
    for line in lines {
        text.push('\n');
        text.push_str(line);
    }
    if omitted > 0 {
        text.push_str(&format!("\n[为装进清单预算，另有 {omitted} 个技能被省略]"));
    }
    text
}

fn capped_body(skill: &Skill) -> String {
    if estimate_tokens(&skill.body) <= MAX_SKILL_TOKENS {
        return skill.body.clone();
    }
    let note = format!(
        "\n\n[已截断：这个技能的全文超过 {MAX_SKILL_TOKENS} token 的上限；它在 {}（工作区允许时用 \
         read_file 读）]",
        skill.path.display()
    );
    let cap_chars = (MAX_SKILL_TOKENS as usize).saturating_mul(CHARS_PER_TOKEN);
    let keep = cap_chars.saturating_sub(note.chars().count());
    let head: String = skill.body.chars().take(keep).collect();
    format!("{head}{note}")
}

/// 模型实际加载过的那些技能，从流上重算（spec §9）：每一次完成的 `skill(name)` 调用，按首次加载
/// 顺序。
///
/// 这是一个查询，不是状态，所以将来的一次压缩可以重新注入这个已加载集合，而不必在会话上新增字段。
pub fn loaded_skill_names(events: &[Event]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut pending: BTreeMap<String, String> = BTreeMap::new();
    for event in events {
        match &event.payload {
            EventPayload::ToolCallStarted {
                tool_call_id,
                tool_name,
                args,
            } if tool_name == SKILL_TOOL => {
                if let Some(name) = args.get("name").and_then(serde_json::Value::as_str) {
                    pending.insert(tool_call_id.as_str().to_owned(), name.to_owned());
                }
            }
            EventPayload::ToolCallCompleted {
                tool_call_id,
                ok: true,
                ..
            } => {
                if let Some(name) = pending.remove(tool_call_id.as_str()) {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
            _ => {}
        }
    }
    names
}

/// 一个根下的每一个技能目录，按稳定的名字顺序。
fn skills_in_root(root: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut directories: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    directories
        .into_iter()
        .filter_map(|directory| skill_at(&directory))
        .collect()
}

/// 解析一个技能目录；它不是一个可用技能时返回 `None`。
fn skill_at(directory: &Path) -> Option<Skill> {
    let path = directory.join(SKILL_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let fallback_name = directory.file_name()?.to_string_lossy().into_owned();
    let (frontmatter, body) = split_frontmatter(&text)?;

    // 描述就是清单的原料：没有它这个技能就无法被提供，所以一个没有描述的文件不是技能。
    let description = field(&frontmatter, "description")?;
    if description.trim().is_empty() {
        return None;
    }
    let name = field(&frontmatter, "name")
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback_name);
    let model_invocation_disabled = field(&frontmatter, "disable-model-invocation")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"));

    Some(Skill {
        name,
        description: description.trim().to_owned(),
        model_invocation_disabled,
        path,
        body: body.trim().to_owned(),
    })
}

/// 把开头那段 `---` frontmatter 块与正文分开。
fn split_frontmatter(text: &str) -> Option<(String, String)> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut frontmatter = String::new();
    let mut body = String::new();
    let mut in_body = false;
    for line in lines {
        if !in_body {
            if line.trim_end() == "---" {
                in_body = true;
                continue;
            }
            frontmatter.push_str(line);
            frontmatter.push('\n');
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    in_body.then_some((frontmatter, body))
}

/// 一个标量 frontmatter 字段。键很简单；值可以不带引号，也可以裹在单引号或双引号里（后者里的
/// `\"` / `\\` 是转义的）。
fn field(frontmatter: &str, key: &str) -> Option<String> {
    frontmatter.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate.trim() == key).then(|| unquote(value.trim()))
    })
}

fn unquote(value: &str) -> String {
    if let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(character) = chars.next() {
            if character == '\\' {
                match chars.next() {
                    Some(escaped) => out.push(escaped),
                    None => out.push('\\'),
                }
            } else {
                out.push(character);
            }
        }
        return out;
    }
    if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return inner.replace("''", "'");
    }
    value.to_owned()
}
