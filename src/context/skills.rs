//! Skills: progressive-disclosure instruction packs (spec §9).
//!
//! A skill is a directory containing `SKILL.md`: a YAML frontmatter block with a
//! `name` and a `description`, plus a Markdown body of instructions. The cheap
//! half — the catalog of `<name>: <description>` lines — is pinned into the head
//! of the context and is present every turn; the expensive half is loaded on
//! demand by the built-in `skill(name)` tool and lands on the stream as an
//! ordinary tool result appended at the tail, so the cached prefix never moves.
//!
//! Discovery follows the de-facto standard, project level before user level and
//! three roots each (`.fs-agent` > `.agents` > `.claude` under the cwd, then
//! `~/.config/fs-agent` > `~/.agents` > `~/.claude`): a machine that already has
//! a `.claude` or `.agents` library gets it for free. The first root that defines
//! a name wins.
//!
//! Three independent budgets, all measured with [`estimate_tokens`]:
//!
//! * [`MAX_CATALOG_TOKENS`] caps the pinned description catalog;
//! * [`MAX_SKILL_TOKENS`] caps one body, which is truncated with a pointer to the
//!   file rather than refused;
//! * [`MAX_LOADED_SKILL_TOKENS`] caps the bodies loaded into one request, enforced
//!   by [`crate::context::trim`] (oldest dropped first).
//!
//! A `disable-model-invocation: true` skill is invisible to the model: it is
//! absent from the catalog and [`Skills::load`] refuses it by name, so guessing a
//! name cannot get around the flag.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::events::{Event, EventPayload};

use super::{estimate_tokens, CHARS_PER_TOKEN};

/// The built-in tool that loads a skill body. Named once here, because the
/// skills module owns what "loading a skill" means: the tool, the sticky drop
/// class and the recomputed loaded set all have to agree on the name.
pub const SKILL_TOOL: &str = "skill";

/// Cap on one loaded skill body (spec §9). Over it, the body is truncated with a
/// pointer to the full file rather than refused.
pub const MAX_SKILL_TOKENS: u64 = 5_000;

/// Cap on the aggregate of loaded skill bodies in one request (spec §9).
/// Enforced by `trim`: the oldest bodies are dropped first.
pub const MAX_LOADED_SKILL_TOKENS: u64 = 25_000;

/// Cap on the pinned description catalog (spec §9), independent of the other two.
pub const MAX_CATALOG_TOKENS: u64 = 3_000;

/// The file every skill directory must contain.
const SKILL_FILE: &str = "SKILL.md";

/// Project-level roots, most specific first (spec §9).
const PROJECT_ROOTS: [&str; 3] = [".fs-agent", ".agents", ".claude"];

/// User-level roots, relative to the injected home, most specific first.
const USER_ROOTS: [&str; 3] = [".config/fs-agent", ".agents", ".claude"];

/// One discovered skill.
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// `disable-model-invocation: true`: the model can neither see nor load it.
    pub model_invocation_disabled: bool,
    /// The `SKILL.md` this came from; the pointer a truncated body names.
    pub path: PathBuf,
    body: String,
}

/// Why a skill cannot be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillError {
    #[error(
        "no such skill: `{name}`; the skills catalog in your context lists the available names"
    )]
    Unknown { name: String },
    #[error(
        "skill `{name}` sets `disable-model-invocation: true`; only the user can invoke it, \
         and the model may not load it"
    )]
    Disabled { name: String },
}

/// The discovered skills, in precedence order. A value: discovered once at
/// assembly, carried by the session, and never mutated.
#[derive(Debug, Clone, Default)]
pub struct Skills {
    skills: Vec<Skill>,
}

impl Skills {
    /// Scan the project and user roots, most specific first, and keep the first
    /// definition of each name (spec §9).
    ///
    /// A missing or unreadable root contributes nothing: a repository without a
    /// skills directory simply has no skills.
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

    /// Every discovered name, in precedence order.
    pub fn names(&self) -> Vec<&str> {
        self.skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    /// The body to put in a tool result, capped at [`MAX_SKILL_TOKENS`].
    ///
    /// Over cap the body is truncated and the truncation names the file, so the
    /// model can read the rest; refusing instead would make a slightly long skill
    /// unusable.
    pub fn load(&self, name: &str) -> Result<String, SkillError> {
        let skill = self.get(name).ok_or_else(|| SkillError::Unknown {
            name: name.to_owned(),
        })?;
        if skill.model_invocation_disabled {
            return Err(SkillError::Disabled {
                name: skill.name.clone(),
            });
        }
        Ok(capped_body(skill))
    }

    /// The pinned description catalog, or `None` when no skill is invocable.
    ///
    /// Present every turn and byte-stable across turns: this text is part of the
    /// cached prefix.
    pub fn catalog(&self) -> Option<String> {
        self.render_catalog(MAX_CATALOG_TOKENS)
    }

    /// The catalog rendered within an explicit budget. Whole entries are kept and
    /// late ones are omitted with a count, so the cap is a bound rather than a cut
    /// through a line.
    pub fn render_catalog(&self, budget: u64) -> Option<String> {
        const HEADER: &str =
            "Skills available on demand (call the `skill` tool with a name when its description \
             matches the task):";

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

        // Largest prefix that fits, trying the most entries first. The estimate is
        // monotonic in length, so this finds the biggest catalog within budget.
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
        text.push_str(&format!(
            "\n[{omitted} more skill(s) omitted to fit the catalog budget]"
        ));
    }
    text
}

fn capped_body(skill: &Skill) -> String {
    if estimate_tokens(&skill.body) <= MAX_SKILL_TOKENS {
        return skill.body.clone();
    }
    let note = format!(
        "\n\n[truncated: this skill's full text exceeds the {MAX_SKILL_TOKENS}-token cap; \
         it is at {} (use read_file when the workspace allows it)]",
        skill.path.display()
    );
    let cap_chars = (MAX_SKILL_TOKENS as usize).saturating_mul(CHARS_PER_TOKEN);
    let keep = cap_chars.saturating_sub(note.chars().count());
    let head: String = skill.body.chars().take(keep).collect();
    format!("{head}{note}")
}

/// The skills the model actually loaded, recomputed from the stream (spec §9):
/// every completed `skill(name)` call, in first-load order.
///
/// This is a query, not state, so a future compaction can re-inject the loaded
/// set without a new field on the session.
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

/// Every skill directory under one root, in stable name order.
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

/// Parse one skill directory, or `None` when it is not a usable skill.
fn skill_at(directory: &Path) -> Option<Skill> {
    let path = directory.join(SKILL_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let fallback_name = directory.file_name()?.to_string_lossy().into_owned();
    let (frontmatter, body) = split_frontmatter(&text)?;

    // The description is what the catalog is made of: without one the skill
    // cannot be offered, so a file without a description is not a skill.
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

/// Split a leading `---` frontmatter block from the body.
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

/// One scalar frontmatter field. Keys are simple; a value may be bare, or
/// wrapped in single or double quotes (with `\"`/`\\` escaped in the latter).
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
