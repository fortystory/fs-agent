//! Permission boundary (spec §12): rules, modes, the circuit breaker, and
//! delegation-chain inheritance.
//!
//! Every tool call passes through one pure function, [`decide`]. It reads the
//! policy and one [`Call`] — the tool name, the resolved paths, and the argv a
//! command tool will run — and answers the closed three-state [`Decision`].
//! It never reads conversation text, never reads the environment, never asks a
//! question and never appends an event. Everything interactive happens in the
//! `agent` loop, outside the gate: the loop asks through an [`Asker`], and when
//! there is none it downgrades the gate's faithful `Ask` to `Deny` and records
//! the reason in `PermissionDecided`.
//!
//! The algebra is one lattice with one merge: `deny > ask > allow`, ignoring
//! specificity, so rule priority, a hook's tightening and a child's inherited
//! constraints all take the supremum on the same order. Two things sit outside
//! that merge, both deliberately:
//!
//! - the **circuit breaker** short-circuits a hard `Deny` before any rule is
//!   evaluated, so no allow and no hook can flip it;
//! - a **mode's floor** (only `readonly` has one) cannot be lowered by a rule,
//!   while a mode's *default* can — which is what makes "always allow" work in
//!   `ask` mode.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::events::{Decision, ParticipantId, SpeakerId};
use crate::tools::Effect;

/// The shell startup files whose writes are never auto-approved: a silent write
/// here changes every future shell, which is home-directory-scale damage.
const SHELL_RC_FILES: &[&str] = &[
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".npmrc",
];

/// Directories whose contents are the user's own safety net. A write inside
/// them is denied outright rather than merely asked about.
const PROTECTED_DIRS: &[&str] = &[".git", ".ssh"];

/// The `.env` family's escapes: files that carry no real secret and are meant to
/// be committed.
const ENV_TEMPLATE_SUFFIXES: &[&str] = &[".example", ".sample", ".template"];

/// The permission modes. Ticket 04 lands the three interactive ones; `plan`
/// (ticket 15) is a fourth preset on the same machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Any non-read-only call is denied; there are no write exemptions.
    Readonly,
    /// Writes ask, reads are allowed — the interactive default.
    Ask,
    /// Allowed by default. **Not** "permissions skipped": the breaker, the
    /// `.env` floor and every rule still apply.
    Auto,
}

/// A mode's standing verdict for one effect.
///
/// The three fields travel together on purpose: `default` is what applies when
/// no rule matches, `floor` is what no rule may lower, and `reason` is what the
/// verdict says out loud. One table keeps them in step.
struct Stance {
    default: Decision,
    floor: Option<Decision>,
    reason: &'static str,
}

impl Mode {
    /// The wire and CLI spelling of the mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Readonly => "readonly",
            Mode::Ask => "ask",
            Mode::Auto => "auto",
        }
    }

    /// The mode's stance on this effect. Only `readonly` has a floor, because
    /// "no write exemption" is that mode's whole definition.
    fn stance(self, effect: &Effect) -> Stance {
        match (self, effect) {
            (Mode::Readonly, Effect::ReadOnly) => Stance {
                default: Decision::Allow,
                floor: None,
                reason: "mode readonly: reads are allowed",
            },
            (Mode::Readonly, _) => Stance {
                default: Decision::Deny,
                floor: Some(Decision::Deny),
                reason: "mode readonly: a non-read-only call is denied (switch modes to allow it)",
            },
            (Mode::Ask, Effect::ReadOnly) => Stance {
                default: Decision::Allow,
                floor: None,
                reason: "mode ask: reads are allowed",
            },
            (Mode::Ask, _) => Stance {
                default: Decision::Ask,
                floor: None,
                reason: "mode ask: a write asks the user",
            },
            (Mode::Auto, _) => Stance {
                default: Decision::Allow,
                floor: None,
                reason: "mode auto: allowed by default",
            },
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who a rule applies to. `Any` is the common case; `Deny`/`Ask` rules that
/// propagate are how one participant's constraint reaches the executors it
/// spawns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Any,
    Debater,
    Executor,
    /// One named participant, debater or executor.
    Participant(ParticipantId),
}

impl Subject {
    /// The subject that names one speaker, for a rule remembered about this
    /// agent (a session-scoped "always allow").
    pub fn for_speaker(speaker: &SpeakerId) -> Subject {
        match speaker {
            SpeakerId::Debater(id) | SpeakerId::Executor(id) => Subject::Participant(id.clone()),
            SpeakerId::User | SpeakerId::System => Subject::Any,
        }
    }

    /// Whether this subject covers `speaker`.
    pub fn matches(&self, speaker: &SpeakerId) -> bool {
        match (self, speaker) {
            (Subject::Any, _) => true,
            (Subject::Debater, SpeakerId::Debater(_)) => true,
            (Subject::Executor, SpeakerId::Executor(_)) => true,
            (Subject::Participant(want), SpeakerId::Debater(id) | SpeakerId::Executor(id)) => {
                want == id
            }
            _ => false,
        }
    }

    fn describe(&self) -> String {
        match self {
            Subject::Any => "any".to_owned(),
            Subject::Debater => "debater".to_owned(),
            Subject::Executor => "executor".to_owned(),
            Subject::Participant(id) => format!("participant {id}"),
        }
    }
}

/// The scope of a rule: a predicate over the call, not a name matcher.
///
/// Because a scope can look at the whole call (its effect, its write set, its
/// argv), a condition like "not read-only **and** the write set is not exactly
/// `PLAN.md`" is expressible directly, instead of a broad deny fighting a narrow
/// allow on a specificity axis that does not exist here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Tool name, as a glob (`edit_file`, `custom__*`).
    Tool(String),
    /// A command's argv prefix (`["git", "status"]`), element-wise.
    CommandPrefix(Vec<String>),
    /// A path glob, matched relative to the session cwd (absolute when the
    /// pattern starts with `/`) against this call's write and read targets.
    Path(String),
    /// The **write** set is exactly this set: the shape a safe exemption needs,
    /// because a call that also writes something else cannot borrow it.
    PathSet(Vec<PathBuf>),
    /// Every call.
    All,
}

impl Scope {
    /// Whether this scope covers the call.
    pub fn matches(&self, call: &Call<'_>) -> bool {
        match self {
            Scope::Tool(pattern) => glob_match(pattern, call.tool_name),
            Scope::CommandPrefix(prefix) => call
                .argv
                .is_some_and(|argv| argv.starts_with(prefix.as_slice())),
            Scope::Path(pattern) => call
                .write_targets
                .iter()
                .chain(call.read_targets.iter())
                .any(|path| path_matches(pattern, path, call.cwd)),
            Scope::PathSet(exact) => write_set_equals(call, exact),
            Scope::All => true,
        }
    }

    fn describe(&self) -> String {
        match self {
            Scope::Tool(pattern) => format!("tool {pattern}"),
            Scope::CommandPrefix(prefix) => format!("command {}", prefix.join(" ")),
            Scope::Path(pattern) => format!("path {pattern}"),
            Scope::PathSet(exact) => {
                let paths: Vec<String> = exact.iter().map(|p| p.display().to_string()).collect();
                format!("write set exactly [{}]", paths.join(", "))
            }
            Scope::All => "any call".to_owned(),
        }
    }
}

/// One permission rule: who, over what, how, and whether it reaches executors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Who the rule applies to.
    pub subject: Subject,
    /// The predicate this rule matches a call against.
    pub scope: Scope,
    /// What matching this rule means.
    pub action: Decision,
    /// Whether this rule is inherited by spawned executors. Defaults by action
    /// (`Deny`/`Ask` yes, `Allow` no).
    pub propagate: bool,
}

impl Rule {
    /// Build a rule whose `propagate` takes the action's default.
    pub fn new(subject: Subject, scope: Scope, action: Decision) -> Self {
        Self {
            propagate: action.default_propagate(),
            subject,
            scope,
            action,
        }
    }

    /// Override the action's default propagation for this rule.
    pub fn with_propagate(mut self, propagate: bool) -> Self {
        self.propagate = propagate;
        self
    }

    /// The rule a session-scoped "always allow" appends: this participant may
    /// use this tool. It never propagates and never leaves the session.
    pub fn always_allow(speaker: &SpeakerId, tool_name: &str) -> Rule {
        Rule::new(
            Subject::for_speaker(speaker),
            Scope::Tool(tool_name.to_owned()),
            Decision::Allow,
        )
    }

    fn describe(&self) -> String {
        format!(
            "rule {} {} -> {}",
            self.subject.describe(),
            self.scope.describe(),
            self.action.as_str()
        )
    }
}

/// A mode plus its rules. The mode is a `Session` value: it never enters the
/// event stream, and `--continue` returns to the configured value (spec §12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    mode: Mode,
    rules: Vec<Rule>,
}

impl Policy {
    /// A policy with this mode and no rules.
    pub fn for_mode(mode: Mode) -> Self {
        Self {
            mode,
            rules: Vec::new(),
        }
    }

    /// The mode this policy runs in.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The rules, in the order they were pushed.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Add a rule. This is how "always allow" changes the session policy — and
    /// the only way it changes anything: no `config.toml` write, no event.
    pub fn push(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// The rules that travel to a spawned executor: only the ones marked
    /// `propagate` — denials and questions by default — so inheritance can only
    /// tighten.
    ///
    /// Working out the child's whole policy is [`crate::agent::executor`]'s job,
    /// and what it does with this list is the point: the child runs under the
    /// **same mode** as its dispatcher (a mode is the session's stance on writes,
    /// and a delegation that inherited a looser one would be a widening), keeps
    /// every propagated rule, and starts with an empty read set. Its authority is
    /// therefore a subset of the dispatcher's; what never travels is an
    /// *allowance*, which is what these rules would carry if `Allow` propagated
    /// (spec §12, §16).
    pub fn inherited_rules(&self) -> Vec<Rule> {
        self.rules
            .iter()
            .filter(|rule| rule.propagate)
            .cloned()
            .collect()
    }
}

/// One call, as the gate needs to see it. Built by the loop from the registry's
/// resolved facts; borrowed so the gate stays a pure read.
#[derive(Debug)]
pub struct Call<'a> {
    /// The tool name the model asked for.
    pub tool_name: &'a str,
    /// The tool's declared workspace effect.
    pub effect: &'a Effect,
    /// Resolved absolute write targets (empty unless the effect is `WritePaths`).
    /// A target that could not be resolved is kept in its lexical form, so the
    /// gate still sees the call.
    pub write_targets: &'a [PathBuf],
    /// Resolved absolute read targets.
    pub read_targets: &'a [PathBuf],
    /// The argv a command tool will run, when it runs one.
    pub argv: Option<&'a [String]>,
    /// Session cwd, the base for relative path patterns and for the `rm` breaker.
    pub cwd: &'a Path,
    /// The user's home, when it is known. Only the `rm` breaker reads it.
    pub home: Option<&'a Path>,
    /// Why a target could not be resolved against the workspace, if one could
    /// not. The path limit denies such a call, so the recorded verdict matches
    /// the outcome instead of reporting an `Allow` the call never got to use.
    pub path_error: Option<&'a str>,
}

/// The gate's answer: a decision and a reason that is always filled (ticket 19
/// reads `reason` to answer "is my permission policy too annoying?").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// The gate's decision.
    pub decision: Decision,
    /// Why, in one sentence; never empty.
    pub reason: String,
}

/// The gate. Pure: no environment, no interaction, no event.
pub fn decide(policy: &Policy, speaker: &SpeakerId, call: &Call<'_>) -> Verdict {
    // ① The circuit breaker short-circuits a hard deny before any rule is
    //    evaluated, so no allow (and, later, no hook) can flip it.
    if let Some(verdict) = circuit_breaker(call) {
        return verdict;
    }

    let stance = policy.mode.stance(call.effect);

    // ② The mode's default, or — when any rule matches — the supremum of the
    //    matching rules. Specificity is ignored on purpose: a broad deny beats a
    //    narrow allow, so exemptions must be written into the deny's predicate.
    let matching: Vec<&Rule> = policy
        .rules
        .iter()
        .filter(|rule| rule.subject.matches(speaker) && rule.scope.matches(call))
        .collect();

    let mut parts: Vec<(Decision, String)> = if matching.is_empty() {
        vec![(stance.default, stance.reason.to_owned())]
    } else {
        matching
            .iter()
            .map(|rule| (rule.action, rule.describe()))
            .collect()
    };

    // ③ A mode floor (only `readonly` has one) is maxed in even when rules
    //    replaced the mode's default.
    if !matching.is_empty() {
        if let Some(floor) = stance.floor {
            parts.push((floor, stance.reason.to_owned()));
        }
    }

    // ④ Constraints that are defaults of the policy rather than rules from a
    //    file: the workspace's path limit, sensitive writes that are never
    //    auto-approved, and the `.env` family. All are floors, so no rule can
    //    lower them.
    if let Some(message) = call.path_error {
        parts.push((Decision::Deny, format!("path limit: {message}")));
    }
    if let Some(verdict) = never_auto_approved(call) {
        parts.push((verdict.decision, verdict.reason));
    }
    if let Some(verdict) = env_family(call) {
        parts.push((verdict.decision, verdict.reason));
    }

    let decision = parts
        .iter()
        .map(|(decision, _)| *decision)
        .max()
        .unwrap_or(Decision::Allow);
    let mut reasons: Vec<String> = parts
        .iter()
        .filter(|(part, _)| *part == decision)
        .map(|(_, reason)| reason.clone())
        .collect();
    reasons.dedup();
    Verdict {
        decision,
        reason: reasons.join("; "),
    }
}

/// The breakers, checked before every rule. A `Deny` here is final.
fn circuit_breaker(call: &Call<'_>) -> Option<Verdict> {
    if let Some(verdict) = rm_breaker(call) {
        return Some(verdict);
    }
    protected_write(call).map(|path| Verdict {
        decision: Decision::Deny,
        reason: format!(
            "circuit breaker: writing into {} is denied regardless of any rule",
            path.display()
        ),
    })
}

/// `rm` against the filesystem root or the home directory (or an ancestor of
/// either) is the one class no rule may approve: it is the least reversible
/// thing an agent can do.
fn rm_breaker(call: &Call<'_>) -> Option<Verdict> {
    let argv = call.argv?;
    if argv.first().map(String::as_str) != Some("rm") {
        return None;
    }
    let target = argv.iter().skip(1).find(|arg| {
        !arg.starts_with('-') && !arg.is_empty() && targets_root_or_home(arg, call.cwd, call.home)
    })?;
    Some(Verdict {
        decision: Decision::Deny,
        reason: format!("circuit breaker: rm targeting {target} is denied regardless of any rule"),
    })
}

/// Whether one `rm` argument names `/`, `~`, or an ancestor of either.
///
/// Relative arguments are folded against the session cwd, so `rm -rf ../..`
/// from a directory under the home directory is caught the same way its
/// absolute spelling is.
fn targets_root_or_home(arg: &str, cwd: &Path, home: Option<&Path>) -> bool {
    let trimmed = arg.trim_end_matches('/');
    // `""` is `/` or `//`.
    if trimmed.is_empty() {
        return true;
    }
    if trimmed == "~" {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if rest.is_empty() {
            return true;
        }
        if let Some(home) = home {
            return reaches_root_or_home(&fold(&home.join(rest)), Some(home));
        }
        // Without a home, only the plainly-folding spellings can be judged.
        return rest.split('/').all(|component| component == "..");
    }
    let path = Path::new(trimmed);
    let folded = if path.is_absolute() {
        fold(path)
    } else {
        fold(&cwd.join(path))
    };
    reaches_root_or_home(&folded, home)
}

/// Whether a lexically folded path is the filesystem root or an ancestor of the
/// home directory (home itself included).
fn reaches_root_or_home(folded: &Path, home: Option<&Path>) -> bool {
    if folded == Path::new("/") {
        return true;
    }
    home.is_some_and(|home| home.starts_with(folded))
}

/// Fold `.` and `..` lexically. The gate cannot canonicalize — it is pure — and
/// a lexical fold is what catches `/home/u/../..` without touching the disk.
fn fold(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A write inside `.git/` or `.ssh/`.
fn protected_write<'a>(call: &Call<'a>) -> Option<&'a Path> {
    call.write_targets
        .iter()
        .find(|path| {
            path.components().any(|component| match component {
                Component::Normal(name) => PROTECTED_DIRS
                    .iter()
                    .any(|dir| name == std::ffi::OsStr::new(dir)),
                _ => false,
            })
        })
        .map(PathBuf::as_path)
}

/// A write to a shell startup file: never auto-approved, but the user may still
/// approve it case by case.
fn never_auto_approved(call: &Call<'_>) -> Option<Verdict> {
    call.write_targets
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SHELL_RC_FILES.contains(&name))
        })
        .map(|path| Verdict {
            decision: Decision::Ask,
            reason: format!(
                "sensitive write: {} is never auto-approved (the user may still approve it)",
                path.display()
            ),
        })
}

/// The `.env` family: its whole content is credentials, and a read lands in the
/// session file and `outputs/` forever. Templates carry no real secret.
fn env_family(call: &Call<'_>) -> Option<Verdict> {
    call.write_targets
        .iter()
        .chain(call.read_targets.iter())
        .find(|path| is_env_file(path))
        .map(|path| Verdict {
            decision: Decision::Deny,
            reason: format!(
                "default policy: the .env family is denied ({})",
                path.display()
            ),
        })
}

fn is_env_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if ENV_TEMPLATE_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        return false;
    }
    name == ".env" || name.starts_with(".env.") || name.ends_with(".env")
}

/// `PathSet` compares the **write** set exactly, and only for a `WritePaths`
/// call: `Exclusive` has no path set to be equal to, so it can never borrow a
/// path-shaped exemption.
fn write_set_equals(call: &Call<'_>, exact: &[PathBuf]) -> bool {
    if !matches!(call.effect, Effect::WritePaths(_)) {
        return false;
    }
    let mut actual: Vec<&PathBuf> = call.write_targets.iter().collect();
    let mut want: Vec<&PathBuf> = exact.iter().collect();
    actual.sort();
    actual.dedup();
    want.sort();
    want.dedup();
    actual == want
}

fn path_matches(pattern: &str, path: &Path, cwd: &Path) -> bool {
    if pattern.starts_with('/') {
        return glob_match(pattern, &path.to_string_lossy());
    }
    let Ok(relative) = path.strip_prefix(cwd) else {
        return false;
    };
    let relative = relative.to_string_lossy();
    if glob_match(pattern, &relative) {
        return true;
    }
    // `**/name` also matches `name` at the cwd root: a leading `**/` may stand
    // for zero directories, which is what users mean by it.
    pattern
        .strip_prefix("**/")
        .is_some_and(|rest| glob_match(rest, &relative))
}

/// A small glob: `*` matches any run of characters that does not cross `/`,
/// `**` crosses `/`, `?` matches one character (never `/`), everything else is
/// literal. No dependency, and no surprises about what a deny pattern covers.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();

    // dp[j]: does the pattern consumed so far match text[..j]?
    let mut dp = vec![false; text.len() + 1];
    dp[0] = true;

    let mut index = 0;
    while index < pattern.len() {
        let mut next = vec![false; text.len() + 1];
        if pattern[index] == '*' {
            if pattern.get(index + 1) == Some(&'*') {
                next[0] = dp[0];
                for j in 1..=text.len() {
                    next[j] = dp[j] || next[j - 1];
                }
                index += 2;
            } else {
                next[0] = dp[0];
                for j in 1..=text.len() {
                    next[j] = dp[j] || (next[j - 1] && text[j - 1] != '/');
                }
                index += 1;
            }
        } else if pattern[index] == '?' {
            for j in 1..=text.len() {
                next[j] = dp[j - 1] && text[j - 1] != '/';
            }
            index += 1;
        } else {
            for j in 1..=text.len() {
                next[j] = dp[j - 1] && text[j - 1] == pattern[index];
            }
            index += 1;
        }
        dp = next;
    }

    dp[text.len()]
}

/// One question the loop asks, outside the gate.
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    /// Identifier shared by the `PermissionAsked` and `PermissionDecided` pair.
    pub request_id: String,
    /// The call this question is about.
    pub tool_call_id: String,
    /// The tool name, for a prompt that names the action.
    pub tool_name: String,
    /// The arguments, so a prompt can show what would run.
    pub args: Value,
    /// Why the gate asked, carried so the prompt can explain itself.
    pub reason: String,
}

/// What the user answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Run this call.
    Allow,
    /// Run this call and remember the allowance for the rest of the session.
    AlwaysAllow,
    /// Refuse this call.
    Deny,
}

/// The port the loop asks through. A headless session injects none, and the
/// loop downgrades the gate's `Ask` to `Deny`; an interactive renderer injects
/// an implementation that reads the keyboard and answers.
#[async_trait]
pub trait Asker: Send + Sync {
    /// Ask about one call and return the user's answer.
    async fn ask(&self, request: &PermissionRequest) -> Answer;
}
