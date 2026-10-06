//! 派发接缝：每个文件类工具都要过的那套护栏。
//!
//! 这些测试直接驱动 `Registry::dispatch`，而不是从 provider 那边绕，
//! 因为这里测的契约是「派发器对每一个调用者都强制这一点」，
//! 它不关心是谁要的这次调用。

use std::path::Path;
use std::path::PathBuf;

use heng::tools::{
    builtin, Effect, PathLocks, PendingCall, ReadSet, Registry, SessionPaths,
    READ_BEFORE_WRITE_PREFIX,
};
use serde_json::json;
use tempfile::TempDir;

struct Fixture {
    /// 在整个 fixture 存活期间保持活着；那些路径都指向它里面。
    #[allow(dead_code)]
    dir: TempDir,
    workspace: PathBuf,
    outputs: PathBuf,
    paths: SessionPaths,
    locks: PathLocks,
    registry: Registry,
    read_set: ReadSet,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let outputs = dir.path().join("outputs");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let workspace = std::fs::canonicalize(&workspace).unwrap();
        Self {
            paths: SessionPaths::new(&workspace),
            locks: PathLocks::new(),
            registry: builtin(false),
            read_set: ReadSet::default(),
            dir,
            workspace,
            outputs,
        }
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.workspace.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn call(&self, id: &str, tool: &str, args: serde_json::Value) -> PendingCall {
        PendingCall {
            tool_call_id: id.to_owned(),
            tool_name: tool.to_owned(),
            args,
            outputs_dir: self.outputs.clone(),
            paths: self.paths.clone(),
            locks: self.locks.clone(),
            // 这里测的派发接缝不涉及技能；一个空的技能库
            // 让内置的 `skill` 工具解析得到但不动手。
            skills: std::sync::Arc::new(heng::context::skills::Skills::default()),
            // 仓库地图同样是空的会话上下文加默认的
            // 上下文预算：这个文件里没有谁调它。
            repo_map: heng::context::repo_map::RepoMapInput::default(),
            // `bash` 工具配置的限额；这个文件不派发它，
            // 所以默认值就是最诚实的取值。
            bash: heng::tools::BashLimits::default(),
            // 沙箱这一层在这个文件里是关的：这里测的派发接缝与它无关，
            // 而关着的那一档对 `process::run` 就是单位函数。
            sandbox: heng::tools::Sandbox::new(&heng::config::SandboxSettings::off()),
            // 没有执行者端口：这个文件直接驱动派发接缝，
            // 而 `task` 不是它派发的工具之一。
            executor: None,
            // 也没有问题端口：`ask_user_question` 不在这里派发。
            questions: None,
        }
    }

    /// 这个 fixture 自己那份读集走的整条派发路径：先护栏，
    /// 再这次调用，最后像循环那样把裁决落到读集上
    /// —— 一模一样。
    async fn dispatch(&mut self, call: &PendingCall) -> heng::tools::DispatchOutcome {
        let mut read_set = std::mem::take(&mut self.read_set);
        let outcome = self.dispatch_with(call, &mut read_set).await;
        self.read_set = read_set;
        outcome
    }

    async fn dispatch_with(
        &self,
        call: &PendingCall,
        read_set: &mut ReadSet,
    ) -> heng::tools::DispatchOutcome {
        let facts = match self
            .registry
            .facts(&call.tool_name, &call.args, &call.paths, None)
        {
            Ok(facts) => facts,
            Err(error) => return heng::tools::DispatchOutcome::failure(error, false),
        };
        match facts.guardrails(read_set) {
            heng::tools::GuardedCall::Refused(error) => {
                heng::tools::DispatchOutcome::failure(error, false)
            }
            heng::tools::GuardedCall::Run(allowed) => {
                let outcome = self.registry.dispatch(call, &allowed).await;
                if outcome.is_ok() {
                    read_set.record_all(allowed.read_paths.iter().cloned());
                }
                if outcome.invalidated_reads {
                    if let Some(path) = outcome
                        .result
                        .as_ref()
                        .err()
                        .and_then(heng::tools::ToolError::invalidated_path)
                    {
                        read_set.invalidate(path);
                    }
                }
                outcome
            }
        }
    }

    /// 解析一次调用并施加那套共享护栏，给那些只想断言裁决、
    /// 不跑工具的测试用。
    fn guardrails(
        &self,
        tool: &str,
        args: &serde_json::Value,
        read_set: &ReadSet,
    ) -> heng::tools::GuardedCall {
        self.registry
            .facts(tool, args, &self.paths, None)
            .expect("一个注册过的工具")
            .guardrails(read_set)
    }
}

fn read_call(fixture: &Fixture, id: &str, file: &Path) -> PendingCall {
    fixture.call(
        id,
        "read_file",
        json!({ "file_path": file.to_str().unwrap() }),
    )
}

fn edit_call(fixture: &Fixture, id: &str, file: &Path, old: &str, new: &str) -> PendingCall {
    fixture.call(
        id,
        "edit_file",
        json!({
            "file_path": file.to_str().unwrap(),
            "old_string": old,
            "new_string": new,
        }),
    )
}

#[tokio::test]
async fn a_write_to_an_unread_file_is_refused_before_the_tool_runs() {
    let mut fixture = Fixture::new();
    let file = fixture.write("notes.txt", "one\ntwo\n");

    let outcome = fixture
        .dispatch(&edit_call(&fixture, "call-1", &file, "two", "three"))
        .await;

    let error = outcome.result.unwrap_err().to_string();
    assert!(error.contains(READ_BEFORE_WRITE_PREFIX), "{error}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "one\ntwo\n",
        "文件没被动过"
    );
    assert!(
        !fixture.outputs.join("call-1.before").exists(),
        "没跑过的调用不会写快照"
    );
}

#[tokio::test]
async fn reading_then_editing_succeeds_and_returns_the_match_level() {
    let mut fixture = Fixture::new();
    let file = fixture.write("notes.txt", "one\ntwo\n");

    let read = fixture
        .dispatch(&read_call(&fixture, "call-1", &file))
        .await;
    assert!(read.result.is_ok());

    let edit = fixture
        .dispatch(&edit_call(&fixture, "call-2", &file, "two", "three"))
        .await;
    let output = edit.result.unwrap();
    assert!(output.text.contains("exact"), "{}", output.text);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\nthree\n");
}

#[tokio::test]
async fn a_non_unique_match_is_refused_unless_replace_all_is_asked_for() {
    let mut fixture = Fixture::new();
    let file = fixture.write("notes.txt", "let a = 1;\nlet b = 1;\n");
    fixture
        .dispatch(&read_call(&fixture, "call-1", &file))
        .await;

    // 不带 replace_all，派发器就拒绝、文件原封不动：模型拿回的是
    // 那个计数，而不是随便挑中的第一处。
    let refused = fixture
        .dispatch(&edit_call(&fixture, "call-2", &file, "= 1;", "= 2;"))
        .await;
    let error = refused.result.unwrap_err().to_string();
    assert!(
        error.contains("`old_string` 匹配上了 2 处"),
        "拒绝时把那个计数原样交回：{error}"
    );
    assert!(!refused.invalidated_reads, "一次拒绝不是一次读过期");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "let a = 1;\nlet b = 1;\n"
    );

    // 明确要每一处，就是一个刻意的请求，于是它落地了。
    let replaced = fixture
        .dispatch(&fixture.call(
            "call-3",
            "edit_file",
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "= 1;",
                "new_string": "= 2;",
                "replace_all": true,
            }),
        ))
        .await;
    assert!(replaced.result.is_ok(), "{:?}", replaced.result);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "let a = 2;\nlet b = 2;\n"
    );
}

#[tokio::test]
async fn a_failed_match_withdraws_the_read_permission_for_that_path() {
    let mut fixture = Fixture::new();
    let file = fixture.write("notes.txt", "one\ntwo\n");

    fixture
        .dispatch(&read_call(&fixture, "call-1", &file))
        .await;

    let miss = fixture
        .dispatch(&edit_call(
            &fixture,
            "call-2",
            &file,
            "not in the file",
            "whatever",
        ))
        .await;
    assert!(miss.result.is_err());
    assert!(miss.invalidated_reads);

    // 模型必须重读：下一次编辑是因为缺一次读而被拒，而不是
    // 因为没匹配上 —— 这正是那次作废带来的区别。
    let retry = fixture
        .dispatch(&edit_call(&fixture, "call-3", &file, "one", "zero"))
        .await;
    let error = retry.result.unwrap_err().to_string();
    assert!(error.contains(READ_BEFORE_WRITE_PREFIX), "{error}");
}

#[tokio::test]
async fn a_failed_read_does_not_license_a_later_write() {
    // 「先读再改」讲的是真的看见过这个文件。一次报了错的读
    // 什么都没看见，所以它不能给后面那次写授权。
    let mut fixture = Fixture::new();
    let missing = fixture.workspace.join("not-yet.txt");

    // 这次读本身失败，而且文件不存在，所以创建它不需要
    // 事先读过 —— 但这次失败的读也不该记下任何东西。
    let read = fixture
        .dispatch(&read_call(&fixture, "call-1", &missing))
        .await;
    assert!(read.result.is_err(), "文件不存在");
    assert!(fixture.read_set.is_empty(), "读失败什么都没记下");

    let write = fixture
        .dispatch(&fixture.call(
            "call-2",
            "write_file",
            json!({
                "file_path": missing.to_str().unwrap(),
                "content": "created\n",
            }),
        ))
        .await;
    assert!(write.result.is_ok(), "{:?}", write.result);
    assert!(missing.exists(), "新文件不需要先读一次");
}

#[tokio::test]
async fn write_file_over_an_existing_file_needs_a_read_first_but_creating_one_does_not() {
    // 同一条规矩的另一半：覆盖一个已有的文件，正是「先读再改」
    // 要管的情形，所以在这个文件被读过之前一律拒绝；
    // 而创建一个不存在的文件，压不坏任何东西。
    let mut fixture = Fixture::new();
    let existing = fixture.write("exists.txt", "old\n");
    let fresh = fixture.workspace.join("fresh.txt");

    let overwrite = fixture
        .dispatch(&fixture.call(
            "call-1",
            "write_file",
            json!({ "file_path": existing.to_str().unwrap(), "content": "new\n" }),
        ))
        .await;
    let error = overwrite.result.unwrap_err().to_string();
    assert!(error.contains(READ_BEFORE_WRITE_PREFIX), "{error}");
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "old\n");

    let read = fixture
        .dispatch(&read_call(&fixture, "call-2", &existing))
        .await;
    assert!(read.result.is_ok());
    let allowed = fixture
        .dispatch(&fixture.call(
            "call-3",
            "write_file",
            json!({ "file_path": existing.to_str().unwrap(), "content": "new\n" }),
        ))
        .await;
    assert!(allowed.result.is_ok(), "{:?}", allowed.result);
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "new\n");

    // 一个全新的文件没有可读的，也没有可压坏的。
    let created = fixture
        .dispatch(&fixture.call(
            "call-4",
            "write_file",
            json!({ "file_path": fresh.to_str().unwrap(), "content": "hi\n" }),
        ))
        .await;
    assert!(created.result.is_ok(), "{:?}", created.result);
    assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "hi\n");
}

#[tokio::test]
async fn a_read_set_is_per_agent_so_executors_do_not_inherit_reads() {
    let fixture = Fixture::new();
    let file = fixture.write("notes.txt", "one\ntwo\n");

    // 甲 agent 读了这个文件。
    let mut first_agent = ReadSet::default();
    let read = fixture
        .dispatch_with(&read_call(&fixture, "call-1", &file), &mut first_agent)
        .await;
    assert!(read.result.is_ok());
    assert!(!first_agent.is_empty());

    // 乙 agent 有自己那份读集：同一个工作区、同一个注册表、同一张
    // 锁表，但对它来说这个文件没读过。读权限在 agent 之间
    // 两个方向都不流动。
    let mut second_agent = ReadSet::default();
    let edit = fixture
        .dispatch_with(
            &edit_call(&fixture, "call-2", &file, "two", "three"),
            &mut second_agent,
        )
        .await;
    let error = edit.result.unwrap_err().to_string();
    assert!(error.contains(READ_BEFORE_WRITE_PREFIX), "{error}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
}

#[tokio::test]
async fn read_only_calls_of_one_path_do_not_contend_for_the_write_lock() {
    // 只读那一条分区是允许并发跑的：它不拿路径锁，所以「并行读」
    // 是在一条本来就对的派发上接线，而不是重写调度器。
    // 对同一条路径的写**确实**会拿锁，
    // 读那条路没有争用这件事才有意义。
    let fixture = Fixture::new();
    let file = fixture.write("shared.txt", "start\n");
    let resolved = std::fs::canonicalize(&file).unwrap();

    let first = fixture.guardrails(
        "read_file",
        &json!({ "file_path": file.to_str().unwrap() }),
        &ReadSet::default(),
    );
    let second = fixture.guardrails(
        "read_file",
        &json!({ "file_path": file.to_str().unwrap() }),
        &ReadSet::default(),
    );
    let allowed = match (first, second) {
        (heng::tools::GuardedCall::Run(a), heng::tools::GuardedCall::Run(b)) => {
            assert!(a.write_targets.is_empty(), "一次读不拿写锁");
            assert!(b.write_targets.is_empty(), "一次读不拿写锁");
            (a, b)
        }
        other => panic!("期望两次被放行的读，实际得到 {other:?}"),
    };

    // 派发器拿的那些写锁就是 `effect()` 声明、再解析过的那些
    // 路径：工具自己的分类是全部输入，而不是另一份可能
    // 跟它漂开的清单。
    let declared = fixture.registry.get("edit_file").unwrap().effect(&json!({
        "file_path": file.to_str().unwrap(),
        "old_string": "a",
        "new_string": "b",
    }));
    let mut read_set = ReadSet::default();
    read_set.record(resolved.clone());
    let edit = fixture.guardrails(
        "edit_file",
        &json!({
            "file_path": file.to_str().unwrap(),
            "old_string": "a",
            "new_string": "b",
        }),
        &read_set,
    );
    let write = match edit {
        heng::tools::GuardedCall::Run(allowed) => allowed.write_targets,
        other => panic!("期望一次被放行的编辑，实际得到 {other:?}"),
    };
    match declared {
        Effect::WritePaths(paths) => {
            let declared: Vec<PathBuf> = paths
                .iter()
                .map(|path| fixture.paths.resolve(path).unwrap())
                .collect();
            assert_eq!(write, declared);
        }
        other => panic!("期望 WritePaths，实际得到 {other:?}"),
    }
    assert_eq!(write, vec![resolved.clone()]);

    // 与此同时一次写正握着锁；那两次读照样跑完，因为
    // 它们从不要那把锁。
    let guard = fixture.locks.lock(&resolved).await;
    let first_call = fixture.call(
        "call-1",
        "read_file",
        json!({ "file_path": file.to_str().unwrap() }),
    );
    let second_call = fixture.call(
        "call-2",
        "read_file",
        json!({ "file_path": file.to_str().unwrap() }),
    );
    let reads = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        let one = fixture.registry.dispatch(&first_call, &allowed.0);
        let two = fixture.registry.dispatch(&second_call, &allowed.1);
        tokio::join!(one, two)
    })
    .await
    .expect("读不等那把写锁");
    assert!(reads.0.result.is_ok(), "{:?}", reads.0.result);
    assert!(reads.1.result.is_ok(), "{:?}", reads.1.result);
    drop(guard);
}

#[tokio::test]
async fn two_calls_to_one_path_serialize_on_the_shared_lock_table() {
    // 锁表在组装时共享，所以同一条路径上的两个写者会串行化。
    // 握着那个守卫就挡住第二次获取；同一张表交给嵌套会话
    // 也会挡住它，这正是注入这张表、
    // 而不是每个会话各建一张的意义。
    let fixture = Fixture::new();
    let file = fixture.write("shared.txt", "start\n");
    let locks = fixture.locks.clone();
    let path = std::fs::canonicalize(&file).unwrap();

    let guard = locks.lock(&path).await;
    let blocked =
        tokio::time::timeout(std::time::Duration::from_millis(50), locks.lock(&path)).await;
    assert!(blocked.is_err(), "第二个写者等了第一个");
    drop(guard);

    let acquired =
        tokio::time::timeout(std::time::Duration::from_millis(500), locks.lock(&path)).await;
    assert!(acquired.is_ok(), "守卫一被丢掉，锁就释放了");
}

#[tokio::test]
async fn the_scheduler_partitions_calls_by_declared_effect() {
    let mut fixture = Fixture::new();

    let read = fixture.registry.get("read_file").unwrap();
    assert_eq!(read.effect(&json!({ "file_path": "x" })), Effect::ReadOnly);

    let edit = fixture.registry.get("edit_file").unwrap();
    match edit.effect(&json!({ "file_path": "x", "old_string": "a", "new_string": "b" })) {
        Effect::WritePaths(paths) => assert_eq!(paths, vec![PathBuf::from("x")]),
        other => panic!("期望 WritePaths，实际得到 {other:?}"),
    }

    // 工作区之外的一次读被拒，哪怕它是只读的。
    let outside = fixture
        .dispatch(&fixture.call(
            "call-1",
            "read_file",
            json!({ "file_path": "/etc/hostname" }),
        ))
        .await;
    let error = outside.result.unwrap_err().to_string();
    assert!(
        error.contains("在会话工作区之外"),
        "工作区之外的读被收容规则拒掉：{error}"
    );

    // 注册表是一个运行时值：挂上一个工具就改变发出去的
    // spec 列表，中间没有任何全局状态。
    let mut registry = Registry::new();
    assert!(registry.is_empty());
    registry.register(Box::new(heng::tools::ReadFile));
    assert_eq!(registry.specs().len(), 1);
    assert_eq!(registry.specs()[0].name, "read_file");
}

// ── `read_file` 的读窗口：`offset` / `limit`（`.scratch/file-read-window/spec.md`） ──
//
// 这一组钉三件互相独立的事：窗口取的是哪一段、行号是谁的坐标系、以及读完之后还能不能改。

/// 造一个 `lines` 行的文件，第 n 行是 `line-n`。
fn numbered_file(fixture: &Fixture, name: &str, lines: usize) -> PathBuf {
    let content: String = (1..=lines).map(|n| format!("line-{n}\n")).collect();
    fixture.write(name, &content)
}

/// 一次带窗口参数的 `read_file` 调用：`extra` 里的键并进基本参数。
fn window_call(fixture: &Fixture, file: &Path, extra: serde_json::Value) -> PendingCall {
    let mut args = json!({ "file_path": file.to_str().unwrap() });
    let object = args.as_object_mut().expect("一个对象");
    for (key, value) in extra.as_object().cloned().unwrap_or_default() {
        object.insert(key, value);
    }
    fixture.call("call-read", "read_file", args)
}

async fn read_text(fixture: &mut Fixture, file: &Path, extra: serde_json::Value) -> String {
    let call = window_call(fixture, file, extra);
    fixture
        .dispatch(&call)
        .await
        .result
        .expect("这次读成功")
        .text
}

#[tokio::test]
async fn a_window_reads_that_stretch_with_the_files_own_line_numbers() {
    let mut fixture = Fixture::new();
    let file = numbered_file(&fixture, "notes.txt", 5);

    let text = read_text(&mut fixture, &file, json!({ "offset": 2, "limit": 2 })).await;

    assert!(text.contains("2\tline-2"), "{text}");
    assert!(text.contains("3\tline-3"), "{text}");
    assert!(!text.contains("1\tline-1"), "窗口前的行不出现：{text}");
    assert!(!text.contains("4\tline-4"), "窗口后的行不出现：{text}");
    assert!(
        text.contains("续读 offset=4"),
        "还有未读行就说清下一句：{text}"
    );
    assert!(text.contains("共 5 行"), "{text}");
}

#[tokio::test]
async fn the_default_window_stops_at_the_default_line_count_and_says_where_to_resume() {
    let mut fixture = Fixture::new();
    let default = heng::tools::DEFAULT_READ_LINES;
    let file = numbered_file(&fixture, "big.txt", default + 1);

    let text = read_text(&mut fixture, &file, json!({})).await;

    assert!(
        text.contains(&format!("{default}\tline-{default}")),
        "读到第 {default} 行：{text}"
    );
    assert!(
        !text.contains(&format!("{}\tline-{}", default + 1, default + 1)),
        "第 {} 行还没读：{text}",
        default + 1
    );
    assert!(text.contains(&format!("共 {} 行", default + 1)), "{text}");
    assert!(
        text.contains(&format!("续读 offset={}", default + 1)),
        "{text}"
    );
}

#[tokio::test]
async fn a_small_file_still_reads_exactly_like_before() {
    let mut fixture = Fixture::new();
    let file = numbered_file(&fixture, "small.txt", 3);

    let text = read_text(&mut fixture, &file, json!({})).await;

    assert_eq!(
        text,
        format!("{}\n1\tline-1\n2\tline-2\n3\tline-3\n", file.display()),
        "整读一个小文件：输出与加窗口之前一字不差（没有续读那一行）"
    );
}

#[tokio::test]
async fn a_window_that_reaches_the_end_has_no_resume_note() {
    let mut fixture = Fixture::new();
    let file = numbered_file(&fixture, "notes.txt", 5);

    let text = read_text(&mut fixture, &file, json!({ "offset": 4, "limit": 100 })).await;

    assert!(text.contains("5\tline-5"), "{text}");
    assert!(!text.contains("续读"), "读到末尾就没什么可续的：{text}");
}

#[tokio::test]
async fn an_offset_past_the_end_reports_how_long_the_file_is() {
    let mut fixture = Fixture::new();
    let file = numbered_file(&fixture, "notes.txt", 3);

    let call = window_call(&fixture, &file, json!({ "offset": 99 }));
    let outcome = fixture.dispatch(&call).await;

    assert!(!outcome.invalidated_reads, "参数错不该收回读权限");
    let error = outcome.result.unwrap_err().to_string();
    assert!(error.contains("文件只有 3 行"), "{error}");
}

#[tokio::test]
async fn a_window_read_authorizes_an_edit_outside_that_window() {
    let mut fixture = Fixture::new();
    let file = numbered_file(&fixture, "notes.txt", 5);

    // 只读了第 1 行……
    let _ = read_text(&mut fixture, &file, json!({ "offset": 1, "limit": 1 })).await;
    // ……改第 5 行照样放行：读集按路径登记，没有行区间（spec §5 的已知边界）。
    let outcome = fixture
        .dispatch(&edit_call(&fixture, "call-edit", &file, "line-5", "edited"))
        .await;

    assert!(outcome.is_ok(), "窗口读之后 edit_file 照常放行");
    assert!(std::fs::read_to_string(&file).unwrap().contains("edited"));
}

#[tokio::test]
async fn a_failed_window_does_not_authorize_a_write() {
    let mut fixture = Fixture::new();
    let file = numbered_file(&fixture, "notes.txt", 3);

    let call = window_call(&fixture, &file, json!({ "offset": 99 }));
    let _ = fixture.dispatch(&call).await;

    let outcome = fixture
        .dispatch(&edit_call(&fixture, "call-edit", &file, "line-1", "x"))
        .await;
    let error = outcome.result.unwrap_err().to_string();
    assert!(
        error.contains(READ_BEFORE_WRITE_PREFIX),
        "失败读不留授权：{error}"
    );
}

#[tokio::test]
async fn malformed_offsets_and_limits_are_argument_errors() {
    let mut fixture = Fixture::new();
    let file = numbered_file(&fixture, "notes.txt", 3);

    let cases = [
        (json!({ "offset": 0 }), "offset"),
        (json!({ "offset": -1 }), "offset"),
        (json!({ "offset": "2" }), "offset"),
        (json!({ "limit": 0 }), "limit"),
        (json!({ "limit": "many" }), "limit"),
    ];
    for (extra, field) in cases {
        let call = window_call(&fixture, &file, extra.clone());
        let outcome = fixture.dispatch(&call).await;
        let error = outcome
            .result
            .unwrap_err()
            .to_string()
            .replace("read_file", "");
        assert!(
            error.contains(&format!("`{field}`")),
            "{extra} 该报 {field} 的参数错误：{error}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "line-1\nline-2\nline-3\n",
        "参数错不会碰文件"
    );
}

#[tokio::test]
async fn an_empty_file_reads_empty_and_an_explicit_offset_says_zero_lines() {
    let mut fixture = Fixture::new();
    let file = fixture.write("empty.txt", "");

    let text = read_text(&mut fixture, &file, json!({})).await;
    assert_eq!(text, format!("{}\n", file.display()), "空文件仍然读得动");

    let call = window_call(&fixture, &file, json!({ "offset": 1 }));
    let error = fixture
        .dispatch(&call)
        .await
        .result
        .unwrap_err()
        .to_string();
    assert!(error.contains("文件只有 0 行"), "{error}");
}

#[tokio::test]
async fn a_window_read_is_still_read_only() {
    let fixture = Fixture::new();
    let read = fixture.registry.get("read_file").unwrap();
    assert_eq!(
        read.effect(&json!({ "file_path": "x", "offset": 40001, "limit": 200 })),
        Effect::ReadOnly,
        "窗口是参数，不是新能力"
    );
}

#[tokio::test]
async fn an_absurd_limit_reads_to_the_end_instead_of_overflowing() {
    let mut fixture = Fixture::new();
    let file = numbered_file(&fixture, "notes.txt", 3);

    let text = read_text(&mut fixture, &file, json!({ "limit": u64::MAX })).await;

    assert!(text.contains("3\tline-3"), "{text}");
    assert!(!text.contains("续读"), "读到末尾就没什么可续的：{text}");
}
