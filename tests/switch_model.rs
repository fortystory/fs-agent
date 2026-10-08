//! 会话中途换模型与档位的那两个换挡口：`Session::retarget` 与
//! `Harness::switch_model`。
//!
//! 最要紧的一条是「provider 与配置必须同时换」：下一次请求要拿新
//! 模型 id 去问新模型的能力表，而这两样住在两个字段里。

mod support;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use heng::config::{ReasoningEffort, SandboxAvailability, SandboxMode, SessionConfig};
use heng::context::skills::Skills;
use heng::events::{EventLog, SessionId, SpeakerId};
use heng::permissions::{Mode, Policy};
use heng::provider::capability::caps_for;
use heng::render::{RenderSinks, Renderer};
use heng::session::{Session, SessionParts};
use heng::tools::{PathLocks, Sandbox, builtin};
use heng::{AssemblyParts, Harness, SessionScaffold, assemble};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply};

/// 一个最小的 `Session`：除了 `config` 之外的一切都只是被用来断言「它没变」。
fn session_with(config: SessionConfig) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let log = EventLog::create(dir.path().join("log.jsonl")).unwrap();
    let session = Session::new(SessionParts {
        id: SessionId::new("s-1"),
        cwd: dir.path().join("workspace"),
        log,
        config,
        tools: Arc::new(builtin(false)),
        locks: PathLocks::new(),
        outputs_dir: dir.path().join("outputs"),
        policy: Arc::new(Mutex::new(Policy::for_mode(Mode::Ask))),
        asker: None,
        questions: None,
        hook: None,
        home: None,
        skills: Arc::new(Skills::discover(dir.path(), None)),
        identity: None,
    });
    (dir, session)
}

#[test]
fn retargeting_a_session_swaps_the_configuration_and_nothing_else() {
    let (_dir, mut session) = session_with(SessionConfig::new("kimi-k3"));
    let id = session.id().clone();
    let cwd = session.cwd().to_path_buf();
    let log_path = session.log_path().to_path_buf();
    let tools = session.shared_tools();

    session.retarget(
        SessionConfig::new("MiniMax-M3.1-Flash-Preview")
            .with_reasoning_effort(ReasoningEffort::Xhigh),
    );

    assert_eq!(session.config().model, "MiniMax-M3.1-Flash-Preview");
    assert_eq!(
        session.config().params.reasoning_effort,
        Some(ReasoningEffort::Xhigh)
    );
    // 身份、工作区、日志与工具表都不动：换的是模型，不是这一场会话。
    assert_eq!(session.id(), &id);
    assert_eq!(session.cwd(), cwd);
    assert_eq!(session.log_path(), log_path);
    assert!(Arc::ptr_eq(&tools, &session.shared_tools()));
}

#[test]
fn retargeting_keeps_the_sandbox_the_session_already_probed() {
    // 沙箱是**会话**的事实（`.scratch/sandbox/spec.md` §3），而 `retarget_to` 拿的是
    // `Config::session_config` 造的新配置 —— 那一格还没探过。不把它带过来，换一次模型就让
    // `bash` 从那一刻起永久拒绝，理由还是一句与这台机器无关的「沙箱状态还没有定下来」
    // （2026-10-08 实测到的那次）。
    let (_dir, mut session) = {
        let mut probed = SessionConfig::new("kimi-k3");
        probed.sandbox.mode = SandboxMode::Bwrap;
        probed.sandbox.availability = SandboxAvailability::Available {
            bwrap: PathBuf::from("/bin/true"),
        };
        session_with(probed)
    };

    let mut fresh = SessionConfig::new("MiniMax-M3.1-Flash-Preview");
    fresh.sandbox.mode = SandboxMode::Bwrap;
    assert!(
        fresh.sandbox.needs_probe(),
        "前提：`retarget_to` 造出来的那份配置里那一格还没探过"
    );

    session.retarget(fresh);

    assert!(
        !session.config().sandbox.needs_probe(),
        "换模型不该把沙箱打回没探过的样子"
    );
    let sandbox = Sandbox::new(&session.config().sandbox);
    assert!(
        sandbox
            .wrap(&["/bin/echo".to_owned()], session.cwd())
            .is_ok(),
        "换过模型之后 shell 照旧能跑"
    );
}

struct Fixture {
    harness: Harness,
    /// 换之前的那个 provider：换完之后它不该再收到任何请求。
    before: FakeProvider,
    after: FakeProvider,
    _dir: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    let before =
        FakeProvider::with_caps(vec![Reply::text("kimi 答的")], caps_for("kimi-k3").unwrap());
    let after = FakeProvider::with_caps(
        vec![Reply::text("minimax 答的")],
        caps_for("MiniMax-M3.1-Flash-Preview").unwrap(),
    );

    let harness = assemble(AssemblyParts {
        provider: Box::new(before.clone()),
        speaker: SpeakerId::Debater("kimi".into()),
        config: SessionConfig::new("kimi-k3").with_reasoning_effort(ReasoningEffort::Max),
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd,
            log_path: dir.path().join("log.jsonl"),
            session_id: SessionId::new("s-1"),
            tools: builtin(false),
            locks: PathLocks::new(),
            policy: Policy::for_mode(Mode::Ask),
            asker: Some(Arc::new(AlwaysAllow)),
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        before,
        after,
        _dir: dir,
    }
}

#[tokio::test]
async fn switching_models_moves_the_provider_and_the_configuration_together() {
    let mut fixture = fixture().await;
    let switched = fixture.harness.switch_model(
        Arc::new(fixture.after.clone()),
        SessionConfig::new("MiniMax-M3.1-Flash-Preview")
            .with_reasoning_effort(ReasoningEffort::Xhigh),
        "minimax-cn",
    );

    // 循环推进去的那几条新事实：模型、档位、窗口与名册。
    assert_eq!(switched.model, "MiniMax-M3.1-Flash-Preview");
    assert_eq!(switched.effort, Some(ReasoningEffort::Xhigh));
    assert_eq!(
        switched.context_window,
        heng::context::usable_input(&caps_for("MiniMax-M3.1-Flash-Preview").unwrap())
    );
    // 发言者的名字是 profile 的名字，所以换到另一个厂商的 profile 就换名（spec §4）。
    assert_eq!(switched.speakers, vec!["minimax-cn".to_owned()]);

    fixture.harness.run_turn("hi").await.unwrap();
    fixture.harness.shutdown().await;

    // 这一次请求带着新 id 与新档位，而且**打给了新 provider**。
    let requests = fixture.after.requests();
    assert_eq!(requests.len(), 1, "换完之后请求必须走新 provider");
    assert_eq!(requests[0].model, "MiniMax-M3.1-Flash-Preview");
    assert_eq!(
        requests[0].params.reasoning_effort,
        Some(ReasoningEffort::Xhigh)
    );
    assert!(
        fixture.before.requests().is_empty(),
        "旧 provider 不该再被问一次"
    );
}
