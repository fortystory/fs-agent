//! The dispatch seam: the guardrails every file tool goes through.
//!
//! These drive `Registry::dispatch` directly rather than through a provider,
//! because the contract under test is "the dispatcher enforces this for every
//! caller", which is independent of who asked for the call.

use std::path::Path;
use std::path::PathBuf;

use fs_agent::tools::{builtin, Effect, PathLocks, PendingCall, ReadSet, Registry, SessionPaths};
use serde_json::json;
use tempfile::TempDir;

struct Fixture {
    /// Kept alive for the lifetime of the fixture; the paths point into it.
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
            registry: builtin(),
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
            // The dispatch seam under test does not involve skills; an empty
            // library keeps the built-in `skill` tool resolvable but inert.
            skills: std::sync::Arc::new(fs_agent::context::skills::Skills::default()),
            // Likewise the repo map keeps an empty session context and the
            // default budget: nothing in this file calls it.
            repo_map: fs_agent::context::repo_map::RepoMapInput::default(),
            // No executor port: this file drives the dispatch seam directly, and
            // `task` is not one of the tools it dispatches.
            executor: None,
        }
    }

    /// The whole dispatch path for this fixture's own read set: guardrails, then
    /// the call, then applying the decision to the read set exactly as the loop
    /// does.
    async fn dispatch(&mut self, call: &PendingCall) -> fs_agent::tools::DispatchOutcome {
        let mut read_set = std::mem::take(&mut self.read_set);
        let outcome = self.dispatch_with(call, &mut read_set).await;
        self.read_set = read_set;
        outcome
    }

    async fn dispatch_with(
        &self,
        call: &PendingCall,
        read_set: &mut ReadSet,
    ) -> fs_agent::tools::DispatchOutcome {
        let facts = match self
            .registry
            .facts(&call.tool_name, &call.args, &call.paths)
        {
            Ok(facts) => facts,
            Err(error) => return fs_agent::tools::DispatchOutcome::failure(error, false),
        };
        match facts.guardrails(read_set) {
            fs_agent::tools::GuardedCall::Refused(error) => {
                fs_agent::tools::DispatchOutcome::failure(error, false)
            }
            fs_agent::tools::GuardedCall::Run(allowed) => {
                let outcome = self.registry.dispatch(call, &allowed).await;
                if outcome.is_ok() {
                    read_set.record_all(allowed.read_paths.iter().cloned());
                }
                if outcome.invalidated_reads {
                    if let Some(path) = outcome
                        .result
                        .as_ref()
                        .err()
                        .and_then(fs_agent::tools::ToolError::invalidated_path)
                    {
                        read_set.invalidate(path);
                    }
                }
                outcome
            }
        }
    }

    /// Resolve one call and apply the shared guardrails, for tests that assert a
    /// decision without running the tool.
    fn guardrails(
        &self,
        tool: &str,
        args: &serde_json::Value,
        read_set: &ReadSet,
    ) -> fs_agent::tools::GuardedCall {
        self.registry
            .facts(tool, args, &self.paths)
            .expect("a registered tool")
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
    assert!(error.contains("read before write"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "one\ntwo\n",
        "the file is untouched"
    );
    assert!(
        !fixture.outputs.join("call-1.before").exists(),
        "no snapshot was written for a call that never ran"
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

    // Without replace_all the dispatcher refuses and the file is untouched: the
    // model gets the count back rather than an arbitrary first hit.
    let refused = fixture
        .dispatch(&edit_call(&fixture, "call-2", &file, "= 1;", "= 2;"))
        .await;
    let error = refused.result.unwrap_err().to_string();
    assert!(error.contains("matches 2 times"), "{error}");
    assert!(!refused.invalidated_reads, "a refusal is not a stale read");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "let a = 1;\nlet b = 1;\n"
    );

    // Asking for every occurrence is a deliberate request, so it lands.
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

    // The model must re-read: the next edit is refused for lack of a read, not
    // for lack of a match, which is the difference the invalidation makes.
    let retry = fixture
        .dispatch(&edit_call(&fixture, "call-3", &file, "one", "zero"))
        .await;
    let error = retry.result.unwrap_err().to_string();
    assert!(error.contains("read before write"), "{error}");
}

#[tokio::test]
async fn a_failed_read_does_not_license_a_later_write() {
    // "Read before edit" is about having actually seen the file. A read that
    // errored saw nothing, so it must not authorize the write that follows.
    let mut fixture = Fixture::new();
    let missing = fixture.workspace.join("not-yet.txt");

    // The read itself fails, and the file does not exist, so creating it needs
    // no prior read — but the failed read must not have recorded anything either.
    let read = fixture
        .dispatch(&read_call(&fixture, "call-1", &missing))
        .await;
    assert!(read.result.is_err(), "the file does not exist");
    assert!(
        fixture.read_set.is_empty(),
        "a failed read recorded nothing"
    );

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
    assert!(missing.exists(), "a new file needs no prior read");
}

#[tokio::test]
async fn write_file_over_an_existing_file_needs_a_read_first_but_creating_one_does_not() {
    // The other half of the same rule: overwriting an existing file is the case
    // read-before-edit is for, so it is refused until the file has been read;
    // creating a file that does not exist cannot clobber anything.
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
    assert!(error.contains("read before write"), "{error}");
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

    // A brand-new file has nothing to read and nothing to clobber.
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

    // Agent A reads the file.
    let mut first_agent = ReadSet::default();
    let read = fixture
        .dispatch_with(&read_call(&fixture, "call-1", &file), &mut first_agent)
        .await;
    assert!(read.result.is_ok());
    assert!(!first_agent.is_empty());

    // Agent B has its own read set: same workspace, same registry, same lock
    // table, but the file is unread to it. Read permission never flows across
    // agents in either direction.
    let mut second_agent = ReadSet::default();
    let edit = fixture
        .dispatch_with(
            &edit_call(&fixture, "call-2", &file, "two", "three"),
            &mut second_agent,
        )
        .await;
    let error = edit.result.unwrap_err().to_string();
    assert!(error.contains("read before write"), "{error}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\ntwo\n");
}

#[tokio::test]
async fn read_only_calls_of_one_path_do_not_contend_for_the_write_lock() {
    // Read-only work is the partition that may run concurrently: it takes no
    // path lock, so "parallel reads" is wiring on an already-correct dispatch
    // rather than a scheduler rewrite. A write to the same path *does* take the
    // lock, which is what makes the read path's absence of contention meaningful.
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
        (fs_agent::tools::GuardedCall::Run(a), fs_agent::tools::GuardedCall::Run(b)) => {
            assert!(a.write_targets.is_empty(), "a read takes no write lock");
            assert!(b.write_targets.is_empty(), "a read takes no write lock");
            (a, b)
        }
        other => panic!("expected two allowed reads, got {other:?}"),
    };

    // The write locks the dispatcher takes are the paths `effect()` declares,
    // resolved: the tool's own classification is the whole input, not a second
    // list that could drift from it.
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
        fs_agent::tools::GuardedCall::Run(allowed) => allowed.write_targets,
        other => panic!("expected an allowed edit, got {other:?}"),
    };
    match declared {
        Effect::WritePaths(paths) => {
            let declared: Vec<PathBuf> = paths
                .iter()
                .map(|path| fixture.paths.resolve(path).unwrap())
                .collect();
            assert_eq!(write, declared);
        }
        other => panic!("expected WritePaths, got {other:?}"),
    }
    assert_eq!(write, vec![resolved.clone()]);

    // A write holds the lock in the meantime; the reads still complete, because
    // they never ask for it.
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
    .expect("reads do not wait on the write lock");
    assert!(reads.0.result.is_ok(), "{:?}", reads.0.result);
    assert!(reads.1.result.is_ok(), "{:?}", reads.1.result);
    drop(guard);
}

#[tokio::test]
async fn two_calls_to_one_path_serialize_on_the_shared_lock_table() {
    // The lock table is shared at assembly time, so two writers of one path
    // serialize. Holding the guard blocks the second acquisition; the same table
    // handed to a nested session would block it too, which is the point of
    // injecting the table instead of building one per session.
    let fixture = Fixture::new();
    let file = fixture.write("shared.txt", "start\n");
    let locks = fixture.locks.clone();
    let path = std::fs::canonicalize(&file).unwrap();

    let guard = locks.lock(&path).await;
    let blocked =
        tokio::time::timeout(std::time::Duration::from_millis(50), locks.lock(&path)).await;
    assert!(blocked.is_err(), "the second writer waited for the first");
    drop(guard);

    let acquired =
        tokio::time::timeout(std::time::Duration::from_millis(500), locks.lock(&path)).await;
    assert!(acquired.is_ok(), "the lock released when the guard dropped");
}

#[tokio::test]
async fn the_scheduler_partitions_calls_by_declared_effect() {
    let mut fixture = Fixture::new();

    let read = fixture.registry.get("read_file").unwrap();
    assert_eq!(read.effect(&json!({ "file_path": "x" })), Effect::ReadOnly);

    let edit = fixture.registry.get("edit_file").unwrap();
    match edit.effect(&json!({ "file_path": "x", "old_string": "a", "new_string": "b" })) {
        Effect::WritePaths(paths) => assert_eq!(paths, vec![PathBuf::from("x")]),
        other => panic!("expected WritePaths, got {other:?}"),
    }

    // A read outside the workspace is refused even though it is read-only.
    let outside = fixture
        .dispatch(&fixture.call(
            "call-1",
            "read_file",
            json!({ "file_path": "/etc/hostname" }),
        ))
        .await;
    let error = outside.result.unwrap_err().to_string();
    assert!(error.contains("outside the session workspace"), "{error}");

    // The registry is a runtime value: mounting a tool changes what specs are
    // sent, with no global state involved.
    let mut registry = Registry::new();
    assert!(registry.is_empty());
    registry.register(Box::new(fs_agent::tools::ReadFile));
    assert_eq!(registry.specs().len(), 1);
    assert_eq!(registry.specs()[0].name, "read_file");
}
