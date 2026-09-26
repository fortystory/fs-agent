//! The permission gate, tested directly as the pure function it is.
//!
//! The truth table covers `mode × action × Scope × propagate`, plus the circuit
//! breaker's short-circuit, the mode floors, and the `.env` family's default
//! denial. No provider, no session, no filesystem: `(policy, caller, call)` in,
//! verdict out.

use std::path::PathBuf;

use fs_agent::events::{Decision, ParticipantId, SpeakerId};
use fs_agent::permissions::{decide, Call, Mode, Policy, Rule, Scope, Subject, Verdict};
use fs_agent::tools::Effect;

/// One call to gate, in owned form so the tests read like a table of cases.
struct Invocation {
    tool: String,
    effect: Effect,
    writes: Vec<PathBuf>,
    reads: Vec<PathBuf>,
    argv: Option<Vec<String>>,
    cwd: PathBuf,
    home: Option<PathBuf>,
    path_error: Option<String>,
}

impl Invocation {
    fn new(tool: &str, effect: Effect) -> Self {
        Self {
            tool: tool.to_owned(),
            effect,
            writes: Vec::new(),
            reads: Vec::new(),
            argv: None,
            cwd: PathBuf::from("/w"),
            home: None,
            path_error: None,
        }
    }

    fn read(tool: &str) -> Self {
        Invocation::new(tool, Effect::ReadOnly)
    }

    /// A `WritePaths` call whose resolved targets are these absolute paths.
    fn write(tool: &str) -> Self {
        Invocation::new(tool, Effect::WritePaths(Vec::new()))
    }

    fn exclusive(tool: &str) -> Self {
        Invocation::new(tool, Effect::Exclusive)
    }

    fn writes(mut self, paths: &[&str]) -> Self {
        self.writes = paths.iter().map(PathBuf::from).collect();
        self
    }

    fn reads(mut self, paths: &[&str]) -> Self {
        self.reads = paths.iter().map(PathBuf::from).collect();
        self
    }

    fn argv(mut self, argv: &[&str]) -> Self {
        self.argv = Some(argv.iter().map(|arg| (*arg).to_owned()).collect());
        self
    }

    fn home(mut self, home: &str) -> Self {
        self.home = Some(PathBuf::from(home));
        self
    }

    fn cwd(mut self, cwd: &str) -> Self {
        self.cwd = PathBuf::from(cwd);
        self
    }

    fn path_error(mut self, message: &str) -> Self {
        self.path_error = Some(message.to_owned());
        self
    }

    fn call(&self) -> Call<'_> {
        Call {
            tool_name: &self.tool,
            effect: &self.effect,
            write_targets: &self.writes,
            read_targets: &self.reads,
            argv: self.argv.as_deref(),
            cwd: &self.cwd,
            home: self.home.as_deref(),
            path_error: self.path_error.as_deref(),
        }
    }
}

fn kimi() -> SpeakerId {
    SpeakerId::Debater(ParticipantId::new("kimi"))
}

fn executor() -> SpeakerId {
    SpeakerId::Executor(ParticipantId::new("exec-1"))
}

fn policy(mode: Mode, rules: Vec<Rule>) -> Policy {
    let mut policy = Policy::for_mode(mode);
    for rule in rules {
        policy.push(rule);
    }
    policy
}

fn gate(mode: Mode, rules: Vec<Rule>, call: &Invocation) -> Verdict {
    decide(&policy(mode, rules), &kimi(), &call.call())
}

fn decision(mode: Mode, rules: Vec<Rule>, call: &Invocation) -> Decision {
    gate(mode, rules, call).decision
}

fn allow_any() -> Rule {
    Rule::new(Subject::Any, Scope::All, Decision::Allow)
}

fn deny_any() -> Rule {
    Rule::new(Subject::Any, Scope::All, Decision::Deny)
}

// --- the three modes ------------------------------------------------------

#[test]
fn readonly_denies_every_non_read_only_call() {
    let read = Invocation::read("read_file");
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let exclusive = Invocation::exclusive("bash");

    assert_eq!(decision(Mode::Readonly, vec![], &read), Decision::Allow);
    assert_eq!(decision(Mode::Readonly, vec![], &write), Decision::Deny);
    assert_eq!(decision(Mode::Readonly, vec![], &exclusive), Decision::Deny);
}

#[test]
fn ask_allows_reads_and_asks_on_writes() {
    let read = Invocation::read("read_file");
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let exclusive = Invocation::exclusive("bash");

    assert_eq!(decision(Mode::Ask, vec![], &read), Decision::Allow);
    assert_eq!(decision(Mode::Ask, vec![], &write), Decision::Ask);
    assert_eq!(decision(Mode::Ask, vec![], &exclusive), Decision::Ask);
}

#[test]
fn auto_allows_by_default() {
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    assert_eq!(decision(Mode::Auto, vec![], &write), Decision::Allow);
}

#[test]
fn auto_is_not_permission_free_a_deny_rule_still_applies() {
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Tool("edit_file".to_owned()),
        Decision::Deny,
    );
    assert_eq!(decision(Mode::Auto, vec![rule], &write), Decision::Deny);
}

// --- the mode cycle (票 01 of `.scratch/todo-and-modes`) -------------------

#[test]
fn a_mode_cycles_readonly_ask_auto_and_back() {
    // The gesture's whole algebra: one step per press, and three presses return
    // a session to the mode it started in (`.scratch/todo-and-modes/spec.md` §1).
    assert_eq!(Mode::Readonly.next(), Mode::Ask);
    assert_eq!(Mode::Ask.next(), Mode::Auto);
    assert_eq!(Mode::Auto.next(), Mode::Readonly);
    assert_eq!(Mode::Ask.next().next().next(), Mode::Ask);
}

#[test]
fn the_three_modes_are_the_three_words_a_configuration_may_write() {
    // One spelling per mode, and nothing else parses: `plan` was the fourth and
    // is gone — what took its place is the `todo` tool, not a mode.
    for mode in [Mode::Readonly, Mode::Ask, Mode::Auto] {
        assert_eq!(Mode::parse(mode.as_str()), Some(mode));
    }
    assert_eq!(Mode::parse("plan"), None);
    assert_eq!(Mode::parse("AUTO"), None);
    assert_eq!(Mode::parse(""), None);
}

#[test]
fn every_mode_keeps_its_own_stance_on_a_write() {
    let read = Invocation::read("read_file");
    let notes = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    // The file the old plan mode existed to protect is now an ordinary write: no
    // mode exempts it and none refuses it by name.
    let plan = Invocation::write("write_file").writes(&["/w/PLAN.md"]);

    assert_eq!(decision(Mode::Readonly, vec![], &read), Decision::Allow);
    assert_eq!(decision(Mode::Readonly, vec![], &notes), Decision::Deny);
    assert_eq!(decision(Mode::Readonly, vec![], &plan), Decision::Deny);
    assert_eq!(decision(Mode::Ask, vec![], &notes), Decision::Ask);
    assert_eq!(decision(Mode::Ask, vec![], &plan), Decision::Ask);
    assert_eq!(decision(Mode::Auto, vec![], &notes), Decision::Allow);
    assert_eq!(decision(Mode::Auto, vec![], &plan), Decision::Allow);
}

// --- rules override the mode's default, never its floor -------------------

#[test]
fn an_allow_rule_overrides_the_ask_mode_default() {
    // This is what makes a session-scoped "always allow" mean anything: the
    // mode is the default, and an explicit rule can settle it.
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Tool("edit_file".to_owned()),
        Decision::Allow,
    );
    assert_eq!(decision(Mode::Ask, vec![rule], &write), Decision::Allow);
    // A different tool still asks.
    let other = Invocation::write("write_file").writes(&["/w/other.txt"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Tool("edit_file".to_owned()),
        Decision::Allow,
    );
    assert_eq!(decision(Mode::Ask, vec![rule], &other), Decision::Ask);
}

#[test]
fn the_readonly_floor_cannot_be_lowered_by_an_allow_rule() {
    // "No write exemption" is the mode's definition, so an allow rule cannot
    // buy one; the user switches modes instead.
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    assert_eq!(
        decision(Mode::Readonly, vec![allow_any()], &write),
        Decision::Deny
    );
}

// --- one merge algebra: deny > ask > allow, ignoring specificity ----------

#[test]
fn a_broad_deny_beats_a_narrow_allow() {
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let rules = vec![
        deny_any(),
        Rule::new(
            Subject::Any,
            Scope::Tool("edit_file".to_owned()),
            Decision::Allow,
        ),
    ];
    assert_eq!(decision(Mode::Auto, rules, &write), Decision::Deny);
}

#[test]
fn ask_beats_allow_among_matching_rules() {
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let rules = vec![
        Rule::new(Subject::Any, Scope::Tool("*".to_owned()), Decision::Ask),
        Rule::new(
            Subject::Any,
            Scope::Tool("edit_file".to_owned()),
            Decision::Allow,
        ),
    ];
    assert_eq!(decision(Mode::Auto, rules, &write), Decision::Ask);
}

#[test]
fn the_decision_lattice_only_tightens() {
    // Rules, a pre-hook's tightening (ticket 05) and inherited constraints all
    // merge on this one order, so "a hook can only tighten" is an algebraic
    // property rather than a runtime check.
    assert_eq!(Decision::Allow.join(Decision::Ask), Decision::Ask);
    assert_eq!(Decision::Ask.join(Decision::Deny), Decision::Deny);
    assert_eq!(Decision::Allow.join(Decision::Deny), Decision::Deny);
    assert_eq!(Decision::Deny.join(Decision::Allow), Decision::Deny);
}

// --- scopes are predicates over the call ----------------------------------

#[test]
fn tool_scope_is_a_glob() {
    let custom = Invocation::write("custom__git__commit").writes(&["/w/x"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Tool("custom__*".to_owned()),
        Decision::Deny,
    );
    assert_eq!(decision(Mode::Auto, vec![rule], &custom), Decision::Deny);

    let builtin = Invocation::write("edit_file").writes(&["/w/x"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Tool("custom__*".to_owned()),
        Decision::Deny,
    );
    assert_eq!(decision(Mode::Auto, vec![rule], &builtin), Decision::Allow);
}

#[test]
fn command_prefix_matches_argv_from_the_front() {
    let call = Invocation::exclusive("bash").argv(&["git", "status", "--short"]);
    let matching = Rule::new(
        Subject::Any,
        Scope::CommandPrefix(vec!["git".to_owned()]),
        Decision::Allow,
    );
    let not_matching = Rule::new(
        Subject::Any,
        Scope::CommandPrefix(vec!["status".to_owned()]),
        Decision::Allow,
    );
    assert_eq!(decision(Mode::Ask, vec![matching], &call), Decision::Allow);
    assert_eq!(
        decision(Mode::Ask, vec![not_matching], &call),
        Decision::Ask
    );
}

#[test]
fn path_scope_matches_write_and_read_targets() {
    let read = Invocation::read("read_file").reads(&["/w/src/lib.rs"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Path("src/**".to_owned()),
        Decision::Deny,
    );
    assert_eq!(decision(Mode::Auto, vec![rule], &read), Decision::Deny);

    // A `**/` prefix also matches at the cwd root.
    let root = Invocation::read("read_file").reads(&["/w/lib.rs"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Path("**/*.rs".to_owned()),
        Decision::Deny,
    );
    assert_eq!(decision(Mode::Auto, vec![rule], &root), Decision::Deny);

    // `*` does not cross a directory separator.
    let nested = Invocation::read("read_file").reads(&["/w/src/deep/lib.rs"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Path("src/*.rs".to_owned()),
        Decision::Deny,
    );
    assert_eq!(decision(Mode::Auto, vec![rule], &nested), Decision::Allow);
}

#[test]
fn path_set_requires_the_write_set_to_be_exactly_equal() {
    let exact = "/w/generated.rs";
    let rule = Rule::new(
        Subject::Any,
        Scope::PathSet(vec![PathBuf::from(exact)]),
        Decision::Deny,
    );

    // A path set is about the **whole** write set: a call that also writes
    // elsewhere is not the call the rule describes, and `Exclusive` has no
    // write set to match at all.
    let alone = Invocation::write("write_file").writes(&[exact]);
    let borrowed = Invocation::write("write_file").writes(&[exact, "/w/src/main.rs"]);
    let exclusive = Invocation::exclusive("bash");
    assert_eq!(
        decision(Mode::Auto, vec![rule.clone()], &alone),
        Decision::Deny
    );
    assert_eq!(
        decision(Mode::Auto, vec![rule.clone()], &borrowed),
        Decision::Allow
    );
    assert_eq!(
        decision(Mode::Auto, vec![rule], &exclusive),
        Decision::Allow
    );
}

#[test]
fn all_scope_matches_every_call() {
    let read = Invocation::read("read_file");
    assert_eq!(
        decision(Mode::Auto, vec![deny_any()], &read),
        Decision::Deny
    );
}

// --- subject and propagation ----------------------------------------------

#[test]
fn subject_scopes_who_a_rule_applies_to() {
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let executor_deny = Rule::new(Subject::Executor, Scope::All, Decision::Deny);

    // The debater is untouched by an executor rule.
    assert_eq!(
        decision(Mode::Auto, vec![executor_deny.clone()], &write),
        Decision::Allow
    );
    assert_eq!(
        decide(
            &policy(Mode::Auto, vec![executor_deny]),
            &executor(),
            &write.call()
        )
        .decision,
        Decision::Deny
    );

    let kimi_only = Rule::new(
        Subject::Participant(ParticipantId::new("kimi")),
        Scope::All,
        Decision::Deny,
    );
    assert_eq!(
        decision(Mode::Auto, vec![kimi_only.clone()], &write),
        Decision::Deny
    );
    assert_eq!(
        decide(
            &policy(Mode::Auto, vec![kimi_only]),
            &executor(),
            &write.call()
        )
        .decision,
        Decision::Allow
    );
}

#[test]
fn propagate_defaults_by_action() {
    let allow = Rule::new(Subject::Any, Scope::All, Decision::Allow);
    let ask = Rule::new(Subject::Any, Scope::All, Decision::Ask);
    let deny = Rule::new(Subject::Any, Scope::All, Decision::Deny);
    assert!(!allow.propagate, "an allowance is not inherited by default");
    assert!(ask.propagate, "a question is a constraint and is inherited");
    assert!(deny.propagate, "a denial is inherited");
}

#[test]
fn an_executor_inherits_constraints_but_not_allowances() {
    let mut parent = Policy::for_mode(Mode::Ask);
    parent.push(Rule::new(
        Subject::Any,
        Scope::Tool("edit_file".to_owned()),
        Decision::Allow,
    ));
    parent.push(Rule::new(
        Subject::Any,
        Scope::Tool("write_file".to_owned()),
        Decision::Deny,
    ));
    parent.push(
        Rule::new(
            Subject::Any,
            Scope::Tool("read_file".to_owned()),
            Decision::Allow,
        )
        .with_propagate(true),
    );

    // The child's mode is its own assembly decision; only the propagating rules
    // travel, so the parent's `auto`-style allowance can never leak.
    let mut child = Policy::for_mode(Mode::Ask);
    for rule in parent.inherited_rules() {
        assert!(rule.propagate);
        child.push(rule);
    }

    let edit = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let write = Invocation::write("write_file").writes(&["/w/notes.txt"]);
    let read = Invocation::read("read_file");
    assert_eq!(
        decide(&child, &executor(), &edit.call()).decision,
        Decision::Ask,
        "the parent's allowance did not travel"
    );
    assert_eq!(
        decide(&child, &executor(), &write.call()).decision,
        Decision::Deny,
        "the parent's denial travelled"
    );
    assert_eq!(
        decide(&child, &executor(), &read.call()).decision,
        Decision::Allow,
        "an explicit override can make an allowance travel"
    );
}

// --- the circuit breaker --------------------------------------------------

#[test]
fn rm_at_root_or_home_is_denied_through_any_allow_rule() {
    let denied = [
        "/",
        "~",
        "~/..",
        "~/../..",
        "/home",
        "/home/u",
        "/home/u/..",
    ];
    for target in denied {
        let call = Invocation::exclusive("bash")
            .argv(&["rm", "-rf", target])
            .home("/home/u");
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &call),
            Decision::Deny,
            "rm {target} must be denied even with an allow-everything rule"
        );
    }

    let allowed = ["/tmp", "/home/other", "~/project", "build/", "/var/tmp/x"];
    for target in allowed {
        let call = Invocation::exclusive("bash")
            .argv(&["rm", "-rf", target])
            .home("/home/u");
        assert_eq!(
            decision(Mode::Auto, vec![], &call),
            Decision::Allow,
            "rm {target} is ordinary work in auto mode"
        );
    }
}

#[test]
fn rm_relative_parents_are_folded_against_the_session_cwd() {
    // A relative `..` chain is checked against the cwd it runs in, so the
    // relative spelling cannot slip past where the absolute one is caught.
    let under_home = Invocation::exclusive("bash")
        .argv(&["rm", "-rf", "../.."])
        .cwd("/home/u/proj")
        .home("/home/u");
    assert_eq!(
        decision(Mode::Auto, vec![allow_any()], &under_home),
        Decision::Deny
    );

    // `~/a/../..` folds back to the parent of home, not to a plain `..` chain.
    let home_relative = Invocation::exclusive("bash")
        .argv(&["rm", "-rf", "~/a/../.."])
        .home("/home/u");
    assert_eq!(
        decision(Mode::Auto, vec![allow_any()], &home_relative),
        Decision::Deny
    );

    // From a workspace outside the home tree, `../..` is not catastrophic.
    let elsewhere = Invocation::exclusive("bash")
        .argv(&["rm", "-rf", "../.."])
        .cwd("/tmp/a/b/proj")
        .home("/home/u");
    assert_eq!(decision(Mode::Auto, vec![], &elsewhere), Decision::Allow);
}

#[test]
fn rm_behind_a_shell_wrapper_is_denied_through_any_allow_rule() {
    // The `bash` tool declares `["bash", "-lc", command]` (ticket 20), so the
    // breaker has to read the command the shell will run, not the wrapper that
    // starts it.
    let denied: [&[&str]; 10] = [
        &["bash", "-lc", "rm -rf /"],
        &["bash", "-lc", "cd /tmp && rm -rf /"],
        &["bash", "-lc", "echo hi; rm -rf ~"],
        &["sh", "-c", "rm -rf /home/u"],
        &["bash", "-lc", r#"rm -rf "/""#],
        &["bash", "-lc", "rm -rf '~'"],
        // The shell's own grammar in front of the command does not hide it.
        &["bash", "-lc", "(rm -rf /)"],
        &["bash", "-lc", "if x; then rm -rf /; fi"],
        &["bash", "-lc", "! rm -rf ~"],
        // A shell option that takes an argument before `-c` does not stop the scan.
        &["bash", "-o", "pipefail", "-c", "rm -rf /"],
    ];
    for argv in denied {
        let call = Invocation::exclusive("bash").argv(argv).home("/home/u");
        let verdict = gate(Mode::Auto, vec![allow_any()], &call);
        assert_eq!(verdict.decision, Decision::Deny, "{argv:?}");
        assert!(
            verdict.reason.contains("circuit breaker"),
            "{argv:?}: {}",
            verdict.reason
        );
    }

    // Ordinary work: the breaker reads commands, it does not refuse shells.
    let allowed: [&[&str]; 5] = [
        &["bash", "-lc", "rm -rf build/"],
        &["bash", "-lc", "rm -rf /tmp/scratch"],
        &["bash", "-lc", "echo rm -rf /"],
        &["bash", "-lc", "git status"],
        &["bash", "build.sh"],
    ];
    for argv in allowed {
        let call = Invocation::exclusive("bash").argv(argv).home("/home/u");
        assert_eq!(
            decision(Mode::Auto, vec![], &call),
            Decision::Allow,
            "{argv:?}"
        );
    }
}

#[test]
fn the_path_limit_is_a_deny_floor() {
    // A target the workspace cannot resolve is denied in every mode, so the
    // recorded verdict matches the refusal instead of reporting an `Allow` the
    // call never got to use.
    let call = Invocation::write("write_file")
        .writes(&["/etc/hostname"])
        .path_error("path /etc/hostname is outside the session workspace /w");
    for mode in [Mode::Readonly, Mode::Ask, Mode::Auto] {
        let verdict = gate(mode, vec![allow_any()], &call);
        assert_eq!(verdict.decision, Decision::Deny, "{mode:?}");
        assert!(verdict.reason.contains("path limit"), "{}", verdict.reason);
    }
}

#[test]
fn a_write_inside_git_or_ssh_is_denied_through_any_allow_rule() {
    for path in ["/w/.git/config", "/w/sub/.ssh/id_ed25519"] {
        let call = Invocation::write("write_file").writes(&[path]);
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &call),
            Decision::Deny,
            "{path} is protected"
        );
    }
}

#[test]
fn shell_rc_writes_are_never_auto_approved() {
    let call = Invocation::write("write_file").writes(&["/w/.bashrc"]);
    assert_eq!(
        decision(Mode::Auto, vec![allow_any()], &call),
        Decision::Ask,
        "an allow-everything rule still cannot auto-approve a shell rc write"
    );

    let npmrc = Invocation::write("write_file").writes(&["/w/.npmrc"]);
    assert_eq!(decision(Mode::Auto, vec![], &npmrc), Decision::Ask);
}

// --- the .env family ------------------------------------------------------

#[test]
fn the_env_family_is_denied_and_templates_are_not() {
    for path in ["/w/.env", "/w/.env.local", "/w/prod.env"] {
        let write = Invocation::write("write_file").writes(&[path]);
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &write),
            Decision::Deny,
            "writing {path} is denied"
        );
        let read = Invocation::read("read_file").reads(&[path]);
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &read),
            Decision::Deny,
            "reading {path} is denied"
        );
    }

    for path in [
        "/w/.env.example",
        "/w/.env.sample",
        "/w/.env.template",
        "/w/prod.env.example",
    ] {
        let read = Invocation::read("read_file").reads(&[path]);
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &read),
            Decision::Allow,
            "{path} is a template and carries no secret"
        );
    }
}

// --- the reason is always filled ------------------------------------------

#[test]
fn every_verdict_carries_a_reason() {
    let calls = [
        Invocation::read("read_file"),
        Invocation::write("edit_file").writes(&["/w/notes.txt"]),
        Invocation::exclusive("bash")
            .argv(&["rm", "-rf", "/"])
            .home("/home/u"),
        Invocation::read("read_file").reads(&["/w/.env"]),
    ];
    for mode in [Mode::Readonly, Mode::Ask, Mode::Auto] {
        for call in &calls {
            let verdict = gate(mode, vec![deny_any()], call);
            assert!(
                !verdict.reason.is_empty(),
                "a verdict without a reason is not diagnosable"
            );
        }
    }
}

#[test]
fn a_path_rule_is_relative_to_the_session_cwd() {
    let call = Invocation::read("read_file").reads(&["/w/src/lib.rs"]);
    let absolute = Rule::new(
        Subject::Any,
        Scope::Path("/w/src/**".to_owned()),
        Decision::Deny,
    );
    let relative = Rule::new(
        Subject::Any,
        Scope::Path("src/**".to_owned()),
        Decision::Deny,
    );
    let outside = Rule::new(
        Subject::Any,
        Scope::Path("other/**".to_owned()),
        Decision::Deny,
    );
    assert_eq!(decision(Mode::Auto, vec![absolute], &call), Decision::Deny);
    assert_eq!(decision(Mode::Auto, vec![relative], &call), Decision::Deny);
    assert_eq!(decision(Mode::Auto, vec![outside], &call), Decision::Allow);
}
