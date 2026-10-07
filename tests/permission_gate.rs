//! 权限门，就按它本来的样子当纯函数直接测。
//!
//! 真值表覆盖 `mode × action × Scope × propagate`，外加断路器的
//! 短路、各档模式的地板，以及 `.env` 家族默认的
//! 拒绝。没有 provider、没有会话、没有文件系统：`(policy, caller, call)`
//! 进去，裁决出来。

use std::path::PathBuf;

use heng::events::{Decision, ParticipantId, SpeakerId};
use heng::permissions::{
    Call, Escalation, Mode, PathError, Policy, Rule, Scope, Subject, Verdict, decide,
};
use heng::tools::{Effect, ToolError};

/// 一次交给权限门的调用，用 owned 形式写，好让测试读起来像一张用例表。
struct Invocation {
    tool: String,
    effect: Effect,
    writes: Vec<PathBuf>,
    reads: Vec<PathBuf>,
    argv: Option<Vec<String>>,
    cwd: PathBuf,
    home: Option<PathBuf>,
    path_error: Option<PathError>,
    escalation: Option<Escalation>,
    masks: Vec<PathBuf>,
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
            escalation: None,
            masks: Vec::new(),
        }
    }

    fn read(tool: &str) -> Self {
        Invocation::new(tool, Effect::ReadOnly)
    }

    /// 一次 `WritePaths` 调用，它解析后的目标就是这些绝对路径。
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

    /// 一个越界的**写**目标（这条 helper 沿用旧测试的读法）。
    fn path_error(mut self, message: &str) -> Self {
        self.path_error = Some(PathError::write(ToolError::message(message)));
        self
    }

    /// 一个越界的**读**目标。
    fn read_error(mut self, message: &str) -> Self {
        self.path_error = Some(PathError::read(ToolError::message(message)));
        self
    }

    /// 一次升级申请：理由与要放开的路径都已解析成绝对路径。
    fn escalation(mut self, justification: &str, paths: &[&str]) -> Self {
        self.escalation = Some(Escalation {
            justification: justification.to_owned(),
            writable_paths: paths.iter().map(PathBuf::from).collect(),
        });
        self
    }

    fn masks(mut self, masks: &[&str]) -> Self {
        self.masks = masks.iter().map(PathBuf::from).collect();
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
            path_error: self.path_error.as_ref(),
            escalation: self.escalation.as_ref(),
            masks: &self.masks,
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

/// 同一个门，但把 `[permissions] outside_read` 那一档旋钮换个值。
fn gate_with(mode: Mode, outside_read: Decision, rules: Vec<Rule>, call: &Invocation) -> Verdict {
    decide(
        &policy(mode, rules).with_outside_read(outside_read),
        &kimi(),
        &call.call(),
    )
}

fn decision_with(
    mode: Mode,
    outside_read: Decision,
    rules: Vec<Rule>,
    call: &Invocation,
) -> Decision {
    gate_with(mode, outside_read, rules, call).decision
}

fn allow_any() -> Rule {
    Rule::new(Subject::Any, Scope::All, Decision::Allow)
}

fn deny_any() -> Rule {
    Rule::new(Subject::Any, Scope::All, Decision::Deny)
}

// --- 三档模式 -------------------------------------------------------------

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

// --- 模式循环（`.scratch/todo-and-modes` 的票 01） ------------------------

#[test]
fn a_mode_cycles_readonly_ask_workspace_auto_and_back() {
    // 这个手势的全部代数：按一次走一步，按四次让会话
    // 回到它开始的那一档（`.scratch/todo-and-modes/spec.md` §1、
    // `.scratch/workspace-mode/spec.md` §1）。
    assert_eq!(Mode::Readonly.next(), Mode::Ask);
    assert_eq!(Mode::Ask.next(), Mode::Workspace);
    assert_eq!(Mode::Workspace.next(), Mode::Auto);
    assert_eq!(Mode::Auto.next(), Mode::Readonly);
    assert_eq!(Mode::Ask.next().next().next().next(), Mode::Ask);
}

#[test]
fn the_four_modes_are_the_four_words_a_configuration_may_write() {
    // 一档模式一个拼写，别的都解析不出来：`plan` 曾经是第四档，
    // 已经退场 —— 顶替它的是 `todo` 工具，而不是一档模式。
    for mode in [Mode::Readonly, Mode::Ask, Mode::Workspace, Mode::Auto] {
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
    // 老的计划模式为保护而存在的那个文件，现在只是一次普通写：
    // 没有任何一档为它开特例，也没有任何一档点名拒它。
    let plan = Invocation::write("write_file").writes(&["/w/PLAN.md"]);

    assert_eq!(decision(Mode::Readonly, vec![], &read), Decision::Allow);
    assert_eq!(decision(Mode::Readonly, vec![], &notes), Decision::Deny);
    assert_eq!(decision(Mode::Readonly, vec![], &plan), Decision::Deny);
    assert_eq!(decision(Mode::Ask, vec![], &notes), Decision::Ask);
    assert_eq!(decision(Mode::Ask, vec![], &plan), Decision::Ask);
    assert_eq!(decision(Mode::Auto, vec![], &notes), Decision::Allow);
    assert_eq!(decision(Mode::Auto, vec![], &plan), Decision::Allow);
}

// --- 规则覆盖模式的默认，绝不覆盖它的地板 ---------------------------------

#[test]
fn an_allow_rule_overrides_the_ask_mode_default() {
    // 会话范围内的「总是允许」有意义，全靠这个：模式是
    // 默认值，而一条显式的规则能把它了结。
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Tool("edit_file".to_owned()),
        Decision::Allow,
    );
    assert_eq!(decision(Mode::Ask, vec![rule], &write), Decision::Allow);
    // 换一个工具照样要问。
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
    // 「不豁免写」是这一档的定义，所以一条放行规则买不到
    // 豁免；用户改为切换模式。
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    assert_eq!(
        decision(Mode::Readonly, vec![allow_any()], &write),
        Decision::Deny
    );
}

// --- 一套合并代数：deny > ask > allow，不看具体程度 -----------------------

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
    // 规则、前置钩子的收紧（票 05）与继承来的约束全都
    // 在这一条序上合并，所以「钩子只能收紧」是一条代数
    // 性质，而不是运行时检查。
    assert_eq!(Decision::Allow.join(Decision::Ask), Decision::Ask);
    assert_eq!(Decision::Ask.join(Decision::Deny), Decision::Deny);
    assert_eq!(Decision::Allow.join(Decision::Deny), Decision::Deny);
    assert_eq!(Decision::Deny.join(Decision::Allow), Decision::Deny);
}

// --- 范围是对这次调用的谓词 -----------------------------------------------

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

    // 一个 `**/` 前缀在 cwd 根上同样匹配。
    let root = Invocation::read("read_file").reads(&["/w/lib.rs"]);
    let rule = Rule::new(
        Subject::Any,
        Scope::Path("**/*.rs".to_owned()),
        Decision::Deny,
    );
    assert_eq!(decision(Mode::Auto, vec![rule], &root), Decision::Deny);

    // `*` 不跨目录分隔符。
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

    // 一个路径集合讲的是**整个**写集合：同时还写别处的调用
    // 不是这条规则描述的那次调用，而 `Exclusive` 根本没有
    // 可匹配的写集合。
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

// --- 主体与传播 -----------------------------------------------------------

#[test]
fn subject_scopes_who_a_rule_applies_to() {
    let write = Invocation::write("edit_file").writes(&["/w/notes.txt"]);
    let executor_deny = Rule::new(Subject::Executor, Scope::All, Decision::Deny);

    // 一条执行者规则碰不到讨论者。
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
    assert!(!allow.propagate, "放行默认不被继承");
    assert!(ask.propagate, "询问是一条约束，会被继承");
    assert!(deny.propagate, "拒绝会被继承");
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

    // 子会话的模式是它自己的组装决定；走的只有那些会传播的规则，
    // 所以父会话那种 `auto` 式的放行永远漏不下去。
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
        "父会话的那条放行没有走下来"
    );
    assert_eq!(
        decide(&child, &executor(), &write.call()).decision,
        Decision::Deny,
        "父会话的那条拒绝走下来了"
    );
    assert_eq!(
        decide(&child, &executor(), &read.call()).decision,
        Decision::Allow,
        "一次显式的覆盖能让一条放行走下来"
    );
}

// --- 断路器 ---------------------------------------------------------------

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
            "就算有一条放行一切的规则，rm {target} 也必须被拒"
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
            "在 auto 档下 rm {target} 是普通活儿"
        );
    }
}

#[test]
fn rm_relative_parents_are_folded_against_the_session_cwd() {
    // 相对的一串 `..` 是按它运行所在的 cwd 去查的，所以这种
    // 相对写法溜不过绝对写法被抓住的地方。
    let under_home = Invocation::exclusive("bash")
        .argv(&["rm", "-rf", "../.."])
        .cwd("/home/u/proj")
        .home("/home/u");
    assert_eq!(
        decision(Mode::Auto, vec![allow_any()], &under_home),
        Decision::Deny
    );

    // `~/a/../..` 折回去是 home 的父目录，而不是一串平的 `..`。
    let home_relative = Invocation::exclusive("bash")
        .argv(&["rm", "-rf", "~/a/../.."])
        .home("/home/u");
    assert_eq!(
        decision(Mode::Auto, vec![allow_any()], &home_relative),
        Decision::Deny
    );

    // 从一个不在 home 树里的工作区看，`../..` 并不灾难。
    let elsewhere = Invocation::exclusive("bash")
        .argv(&["rm", "-rf", "../.."])
        .cwd("/tmp/a/b/proj")
        .home("/home/u");
    assert_eq!(decision(Mode::Auto, vec![], &elsewhere), Decision::Allow);
}

#[test]
fn rm_behind_a_shell_wrapper_is_denied_through_any_allow_rule() {
    // `bash` 工具声明的是 `["bash", "-lc", command]`（票 20），所以
    // 断路器必须读 shell 将要跑的那条命令，而不是
    // 起它的那层包装。
    let denied: [&[&str]; 10] = [
        &["bash", "-lc", "rm -rf /"],
        &["bash", "-lc", "cd /tmp && rm -rf /"],
        &["bash", "-lc", "echo hi; rm -rf ~"],
        &["sh", "-c", "rm -rf /home/u"],
        &["bash", "-lc", r#"rm -rf "/""#],
        &["bash", "-lc", "rm -rf '~'"],
        // 命令前面那点 shell 自己的语法盖不住它。
        &["bash", "-lc", "(rm -rf /)"],
        &["bash", "-lc", "if x; then rm -rf /; fi"],
        &["bash", "-lc", "! rm -rf ~"],
        // 一个在 `-c` 之前收参数的 shell 选项拦不住这次扫描。
        &["bash", "-o", "pipefail", "-c", "rm -rf /"],
    ];
    for argv in denied {
        let call = Invocation::exclusive("bash").argv(argv).home("/home/u");
        let verdict = gate(Mode::Auto, vec![allow_any()], &call);
        assert_eq!(verdict.decision, Decision::Deny, "{argv:?}");
        assert!(
            verdict.reason.contains("断路器"),
            "{argv:?}：{}",
            verdict.reason
        );
    }

    // 普通活儿：断路器读的是命令，它不拒 shell 本身。
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
fn the_path_limit_is_a_deny_floor_in_every_mode_but_workspace() {
    // 工作区解析不了的目标在另外三档里都被拒，所以记下来的
    // 裁决与那次拒绝对得上，而不是报一个这次调用
    // 根本没用上的 `Allow`。
    let call = Invocation::write("write_file")
        .writes(&["/etc/hostname"])
        .path_error("路径 /etc/hostname 在会话工作区之外（/w）");
    for mode in [Mode::Readonly, Mode::Ask, Mode::Auto] {
        let verdict = gate(mode, vec![allow_any()], &call);
        assert_eq!(verdict.decision, Decision::Deny, "{mode:?}");
        assert!(verdict.reason.contains("路径上限"), "{}", verdict.reason);
    }

    // 唯独 `workspace` 档：这条地板在这一档下是 `Ask` —— 选这一档就是同意「区外要问」
    // （`.scratch/workspace-mode/spec.md` §3）。一条放行一切的规则也降不下它。
    let verdict = gate(Mode::Workspace, vec![allow_any()], &call);
    assert_eq!(verdict.decision, Decision::Ask, "{}", verdict.reason);
    assert!(
        verdict.reason.contains("路径上限（写）"),
        "{}",
        verdict.reason
    );
}

// --- 第四档 `workspace`（.scratch/workspace-mode/spec.md §1、§3）-----------

#[test]
fn workspace_allows_writes_inside_the_workspace_and_asks_outside() {
    let inside = Invocation::write("write_file").writes(&["/w/notes.txt"]);
    let outside = Invocation::write("write_file")
        .writes(&["/etc/hostname"])
        .path_error("路径 /etc/hostname 在会话工作区之外（/w）");

    assert_eq!(decision(Mode::Workspace, vec![], &inside), Decision::Allow);
    assert_eq!(decision(Mode::Workspace, vec![], &outside), Decision::Ask);
    // 只读与 shell 也一路放行：判不出区内区外的那一类由沙箱那一侧接住。
    let read = Invocation::read("read_file").reads(&["/w/notes.txt"]);
    let shell = Invocation::exclusive("bash").argv(&["bash", "-lc", "cargo test"]);
    assert_eq!(decision(Mode::Workspace, vec![], &read), Decision::Allow);
    assert_eq!(decision(Mode::Workspace, vec![], &shell), Decision::Allow);
}

#[test]
fn only_workspace_asks_for_an_outside_write() {
    let outside = Invocation::write("write_file")
        .writes(&["/etc/hostname"])
        .path_error("路径 /etc/hostname 在会话工作区之外（/w）");
    // `ask` 档的区外写照旧是地板：不加对称旋钮（spec §3）。
    assert_eq!(decision(Mode::Ask, vec![], &outside), Decision::Deny);
    assert_eq!(decision(Mode::Readonly, vec![], &outside), Decision::Deny);
    assert_eq!(decision(Mode::Auto, vec![], &outside), Decision::Deny);
}

// --- 区外读那条全局旋钮（.scratch/workspace-mode/spec.md §2）---------------

#[test]
fn outside_read_is_deny_by_default_and_a_knob_everywhere() {
    let outside = Invocation::read("read_file")
        .reads(&["/home/ada/.config/heng/config.toml"])
        .read_error("路径 … 在会话工作区之外（/w）");

    for mode in [Mode::Readonly, Mode::Ask, Mode::Workspace, Mode::Auto] {
        assert_eq!(
            decision(mode, vec![allow_any()], &outside),
            Decision::Deny,
            "{mode:?}"
        );
        assert_eq!(
            decision_with(mode, Decision::Ask, vec![allow_any()], &outside),
            Decision::Ask,
            "{mode:?}"
        );
        assert_eq!(
            decision_with(mode, Decision::Allow, vec![], &outside),
            Decision::Allow,
            "{mode:?}"
        );
    }
    // `deny` 时不许出现任何 `Allow`：一条放行一切的规则也降不下这条地板。
    for mode in [Mode::Readonly, Mode::Ask, Mode::Workspace, Mode::Auto] {
        assert_eq!(
            gate_with(mode, Decision::Deny, vec![allow_any()], &outside).decision,
            Decision::Deny,
            "{mode:?}"
        );
    }
}

#[test]
fn the_outside_read_knob_does_not_touch_the_write_side() {
    let outside_write = Invocation::write("write_file")
        .writes(&["/etc/hostname"])
        .path_error("路径 /etc/hostname 在会话工作区之外（/w）");
    // 写那一侧的越界按档位给，`outside_read` 管不着它。
    assert_eq!(
        decision_with(Mode::Ask, Decision::Allow, vec![], &outside_write),
        Decision::Deny
    );
    assert_eq!(
        decision_with(Mode::Workspace, Decision::Allow, vec![], &outside_write),
        Decision::Ask
    );
}

// --- 升级手势（.scratch/workspace-mode/spec.md §4、§5）--------------------

#[test]
fn an_escalation_asks_once_in_every_mode_but_readonly() {
    let call = Invocation::exclusive("bash")
        .argv(&["bash", "-lc", "echo x > /home/ada/.npm/probe"])
        .escalation("构建产物要写到 ~/.npm 的缓存目录", &["/home/ada/.npm"]);

    for mode in [Mode::Ask, Mode::Workspace, Mode::Auto] {
        let verdict = gate(mode, vec![], &call);
        assert_eq!(verdict.decision, Decision::Ask, "{mode:?}");
        assert!(verdict.reason.contains("沙箱升级"), "{}", verdict.reason);
        assert!(
            verdict.reason.contains("/home/ada/.npm"),
            "理由里要看得到要放开的路径：{}",
            verdict.reason
        );
    }
    // `readonly` 连跑都不让，谈不上放开沙箱。
    assert_eq!(decision(Mode::Readonly, vec![], &call), Decision::Deny);
    // 一条「总是允许」的规则也不会让升级不再问：它只对这一次调用有效。
    assert_eq!(
        decision(Mode::Auto, vec![allow_any()], &call),
        Decision::Ask
    );
}

#[test]
fn an_escalation_into_a_mask_or_a_protected_path_is_denied_outright() {
    let masked = Invocation::exclusive("bash")
        .masks(&["/home/ada/.ssh", "/home/ada/.config/heng"])
        .escalation("要写 authorized_keys", &["/home/ada/.ssh/authorized_keys"]);
    for mode in [Mode::Ask, Mode::Workspace, Mode::Auto] {
        let verdict = gate(mode, vec![allow_any()], &masked);
        assert_eq!(verdict.decision, Decision::Deny, "{mode:?}");
        assert!(
            verdict.reason.contains("没有任何通道放宽"),
            "{}",
            verdict.reason
        );
    }

    // 保护路径按**路径**判，不看在不在磁盘上：批准一条还不存在的 `.env` 同样会让下一次
    // 调用以为自己在保护。
    for path in [
        "/w/.env",
        "/w/.env.local",
        "/w/prod.env",
        "/w/.git/config",
        "/w/.git/hooks/pre-commit",
    ] {
        let call = Invocation::exclusive("bash").escalation("想写这儿", &[path]);
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &call),
            Decision::Deny,
            "{path} 是写死的安全默认"
        );
    }

    // 模板不是保护路径：它不携带真密钥。
    let template = Invocation::exclusive("bash").escalation("想写这儿", &["/w/.env.example"]);
    assert_eq!(decision(Mode::Auto, vec![], &template), Decision::Ask);
}

#[test]
fn a_write_inside_git_or_ssh_is_denied_through_any_allow_rule() {
    for path in ["/w/.git/config", "/w/sub/.ssh/id_ed25519"] {
        let call = Invocation::write("write_file").writes(&[path]);
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &call),
            Decision::Deny,
            "{path} 受保护"
        );
    }
}

#[test]
fn shell_rc_writes_are_never_auto_approved() {
    let call = Invocation::write("write_file").writes(&["/w/.bashrc"]);
    assert_eq!(
        decision(Mode::Auto, vec![allow_any()], &call),
        Decision::Ask,
        "一条放行一切的规则仍然放不了 shell rc 的写"
    );

    let npmrc = Invocation::write("write_file").writes(&["/w/.npmrc"]);
    assert_eq!(decision(Mode::Auto, vec![], &npmrc), Decision::Ask);
}

// --- .env 家族 ------------------------------------------------------------

#[test]
fn the_env_family_is_denied_and_templates_are_not() {
    for path in ["/w/.env", "/w/.env.local", "/w/prod.env"] {
        let write = Invocation::write("write_file").writes(&[path]);
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &write),
            Decision::Deny,
            "写 {path} 被拒"
        );
        let read = Invocation::read("read_file").reads(&[path]);
        assert_eq!(
            decision(Mode::Auto, vec![allow_any()], &read),
            Decision::Deny,
            "读 {path} 被拒"
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
            "{path} 是模板，不带任何秘密"
        );
    }
}

// --- 理由永远被填上 -------------------------------------------------------

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
                "一条没有理由的裁决是诊断不出来的"
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
