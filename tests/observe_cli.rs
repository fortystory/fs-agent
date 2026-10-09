//! `sessions` 这个 CLI（spec §18）。
//!
//! 各个动词只是那批纯查询外面的一层薄壳，所以测试用捕获写入器
//! 和一个扎根在临时 `XDG_DATA_HOME` 里的存储去驱动真正的
//! 入口点。这就够断言这张票钉住的那两件事了：
//! stdout 只扛结果，而每个动词都有 `--json` 形态。

mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use heng::cli::run_sessions;
use heng::config::EnvMap;
use heng::events::{
    EventLog, EventPayload, Role, RoundMode, SCHEMA_VERSION, SessionId, SpeakerId, StopReason,
    ToolCallId,
};
use heng::session::SessionStore;
use heng::tools::{MATCH_LEVEL_PREFIX, WROTE_PATH_PREFIX};
use support::CaptureBuf;

struct Fixture {
    _dir: tempfile::TempDir,
    env: EnvMap,
    cwd: PathBuf,
    id: SessionId,
    log_path: PathBuf,
}

/// 一个存着的会话，带一条小而完整的讨论事件流。
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let env: EnvMap = BTreeMap::from([(
        "XDG_DATA_HOME".to_owned(),
        data.to_string_lossy().into_owned(),
    )]);
    let store = SessionStore::new(data.join("heng").join("sessions"));
    let cwd = dir.path().join("workspace");
    std::fs::create_dir_all(&cwd).unwrap();
    let stored = store.create(&cwd).unwrap();

    let mut log = EventLog::create(&stored.log_path).unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SessionStarted {
            session_id: stored.id.clone(),
            cwd: cwd.to_string_lossy().into_owned(),
            schema_version: SCHEMA_VERSION,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::User,
        EventPayload::MessageCompleted {
            role: Role::User,
            text: "怎么共享状态？".to_owned(),
            reasoning: None,

            first_token_ms: None,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::RoundStarted {
            round: 1,
            mode: RoundMode::Independent,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::Debater("kimi".into()),
        EventPayload::TurnStarted {
            agent: SpeakerId::Debater("kimi".into()),
            iteration: 1,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::Debater("kimi".into()),
        EventPayload::MessageCompleted {
            role: Role::Assistant,
            text: "用共享事件流。\nCONCLUSION: 共享事件流".to_owned(),
            reasoning: None,

            first_token_ms: None,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallStarted {
            tool_call_id: ToolCallId::new("call-1"),
            tool_name: "edit_file".to_owned(),
            args: serde_json::json!({"file_path": "/workspace/a.rs"}),
        },
    )
    .unwrap();
    log.append(
        SpeakerId::Debater("kimi".into()),
        EventPayload::ToolCallCompleted {
            tool_call_id: ToolCallId::new("call-1"),
            ok: true,
            output: Some(format!(
                "{WROTE_PATH_PREFIX}/workspace/a.rs\n{MATCH_LEVEL_PREFIX}line-trim: 1 处替换"
            )),
            error: None,
            duration_ms: 4,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::Debater("deepseek".into()),
        EventPayload::TurnEnded {
            reason: StopReason::Error,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::RoundEnded {
            round: 1,
            reason: StopReason::NoDivergence,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SessionEnded {
            reason: StopReason::Completed,
        },
    )
    .unwrap();

    Fixture {
        _dir: dir,
        env,
        cwd,
        id: stored.id,
        log_path: stored.log_path,
    }
}

fn run(fixture: &Fixture, items: &[&str]) -> (ExitCode, String, String) {
    let mut out = CaptureBuf::default();
    let mut err = CaptureBuf::default();
    let mut full: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
    if !items.contains(&"--cwd") {
        full.push("--cwd".to_owned());
        full.push(fixture.cwd.to_string_lossy().into_owned());
    }
    let code = run_sessions(&full, &fixture.env, &mut out, &mut err);
    (code, out.text(), err.text())
}

#[test]
fn ls_lists_the_session_and_its_json_comes_from_stdout_alone() {
    let fixture = fixture();
    let (code, out, err) = run(&fixture, &["ls"]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.contains(fixture.id.as_str()), "{out}");
    // 先一行表头，再一行会话：表格就得像个表格。
    assert!(
        out.lines().count() >= 2 && !out.lines().next().unwrap().contains(fixture.id.as_str()),
        "数据上面有一行表头：{out}"
    );
    assert!(err.is_empty(), "诊断留在 stderr 上：{err}");

    let (code, out, _) = run(&fixture, &["ls", "--json"]);
    assert_eq!(code, ExitCode::SUCCESS);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value[0]["id"], fixture.id.as_str());
    assert_eq!(value[0]["ended"], "Completed");
}

#[test]
fn show_groups_by_round_and_merges_the_tool_call_with_its_result() {
    let fixture = fixture();
    let (code, out, _) = run(&fixture, &["show", fixture.id.as_str(), "--json"]);
    assert_eq!(code, ExitCode::SUCCESS);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    let round = &value["groups"][1];
    assert_eq!(round["round"], 1);
    let tools: Vec<&serde_json::Value> = round["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["entry"] == "tool")
        .collect();
    assert_eq!(tools.len(), 1, "一次调用一行");
    assert_eq!(tools[0]["ok"], true);
    assert_eq!(tools[0]["tool"], "edit_file");
}

#[test]
fn show_files_is_the_workspace_object_view() {
    let fixture = fixture();
    let (code, out, _) = run(
        &fixture,
        &["show", fixture.id.as_str(), "--files", "--json"],
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value[0]["path"], "/workspace/a.rs");
    assert_eq!(value[0]["round"], 1);
}

#[test]
fn show_only_error_keeps_the_failure_and_drops_the_rest() {
    let fixture = fixture();
    let (code, out, _) = run(
        &fixture,
        &["show", fixture.id.as_str(), "--only-error", "--json"],
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    let entries: Vec<&serde_json::Value> = value["groups"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["entries"].as_array().unwrap())
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["entry"], "turn_ended");
    assert_eq!(entries[0]["reason"], "Error");
}

#[test]
fn stats_reports_the_silent_quantities_and_prices_the_named_model() {
    let fixture = fixture();
    let (code, out, _) = run(
        &fixture,
        &["stats", fixture.id.as_str(), "--model", "deepseek-flash"],
    );
    assert_eq!(code, ExitCode::SUCCESS);
    // 给人看的那一视图会点名它量到的事实；确切的数值要
    // 从下面的 JSON 形态里读回来。
    assert!(out.contains("line-trim"), "匹配层级被报出来了：{out}");
    assert!(out.contains("deepseek"), "缺席的讨论者被点了名：{out}");
    assert!(
        out.contains("deepseek-flash"),
        "被定价的模型被点了名：{out}"
    );

    let (code, out, _) = run(
        &fixture,
        &[
            "stats",
            fixture.id.as_str(),
            "--model",
            "deepseek-flash",
            "--json",
        ],
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let value: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["absence"]["one_sided"], 1);
    assert_eq!(value["edits"]["levels"][0]["name"], "line-trim");
    assert_eq!(value["edits"]["failed_matches"], 0);
}

#[test]
fn replay_recomputes_the_call_from_the_stream_and_refuses_without_a_speaker() {
    let fixture = fixture();
    let (code, out, _) = run(
        &fixture,
        &[
            "replay",
            fixture.id.as_str(),
            "--speaker",
            "kimi",
            "--round",
            "1",
            "--model",
            "deepseek-flash",
            "--json",
        ],
    );
    assert_eq!(code, ExitCode::SUCCESS);
    let messages: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert_eq!(
        messages[0]["System"]["content"],
        heng::discussion::debater_identity("kimi"),
        "私有身份站在重算出来的那个请求最前面"
    );
    assert!(
        messages.iter().any(|message| message["User"]["content"]
            .as_str()
            .is_some_and(|text| text.contains("怎么共享状态"))),
        "那个问题在重算出来的窗口里：{messages:?}"
    );

    let (code, _, err) = run(
        &fixture,
        &[
            "replay",
            fixture.id.as_str(),
            "--round",
            "1",
            "--model",
            "deepseek-flash",
        ],
    );
    assert_eq!(code, ExitCode::FAILURE);
    assert!(err.contains("--speaker"), "{err}");
}

#[test]
fn an_unknown_verb_fails_loudly_with_nothing_on_stdout() {
    let fixture = fixture();
    let (code, out, err) = run(&fixture, &["explain"]);
    assert_eq!(code, ExitCode::FAILURE);
    assert!(out.is_empty(), "stdout 只扛结果：{out}");
    assert!(
        err.contains("explain"),
        "这次拒绝点出了它不认识的那个动词：{err}"
    );
}

#[test]
fn show_reports_the_sandbox_state_on_its_own_line() {
    // 沙箱状态只进日志：TUI 不显示它，而复盘视图显示 —— 审计者要核对「那条命令当时
    // 有没有被关着」，这里是唯一可读的出处（沙箱 spec §8）。
    let fixture = fixture();
    let mut log = EventLog::open(&fixture.log_path).unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SandboxStatus {
            mode: "bwrap".to_owned(),
            unavailable_reason: None,
        },
    )
    .unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::SandboxStatus {
            mode: "bwrap".to_owned(),
            unavailable_reason: Some("PATH 上没有 `bwrap`".to_owned()),
        },
    )
    .unwrap();
    drop(log);

    let (code, out, _) = run(&fixture, &["show", fixture.id.as_str()]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.contains("[沙箱：bwrap]"), "{out}");
    assert!(
        out.contains("[沙箱：bwrap · 不可用：PATH 上没有 `bwrap`]"),
        "不可用时把原因一起摊开：{out}"
    );
}

#[test]
fn show_reports_a_command_record_on_its_own_line() {
    // 命令不进模型上下文，所以它在别处一个字都不留（`.scratch/command-echo/spec.md`）——
    // 而复盘的人恰恰要查「当时我敲了什么」，于是 `sessions show` 是它的第二个出处。
    let fixture = fixture();
    let mut log = EventLog::open(&fixture.log_path).unwrap();
    log.append(
        SpeakerId::System,
        EventPayload::CommandRun {
            text: "/model kimi-k3".to_owned(),
        },
    )
    .unwrap();
    drop(log);

    let (code, out, _) = run(&fixture, &["show", fixture.id.as_str()]);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.contains("[命令] /model kimi-k3"), "{out}");
}
