//! 沙箱的纯函数断言（`.scratch/sandbox/issues/01-sandbox-wrap-pure-function.md`）。
//!
//! 主力就是 [`wrap`]：它的输出可以逐字比对，所以这一层不需要真有 bubblewrap 也能覆盖每一个
//! 分支。真机那一层（内核真的拦住了）在 `docs/tui-manual-checklist.md` 的沙箱一节。

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use heng::config::{SandboxAvailability, SandboxMode, SandboxSettings, SessionConfig};
use heng::tools::process;
use heng::tools::sandbox::{
    PROBE_PROFILE, Sandbox, SandboxSpec, escalation_path, probe, resolve_availability, sealed, wrap,
};

/// 一条 shell 命令将要跑的那条 argv，形状与 `bash` 工具给它的一样。
fn shell(command: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-lc".to_owned(), command.to_owned()]
}

/// 一份 bwrap 规格：这些根可写、这些目录被遮住。
fn spec(roots: &[&Path], masks: &[&Path]) -> SandboxSpec {
    SandboxSpec {
        mode: SandboxMode::Bwrap,
        writable_roots: roots.iter().map(|path| path.to_path_buf()).collect(),
        masks: masks.iter().map(|path| path.to_path_buf()).collect(),
    }
}

/// 一条路径的规范形：`wrap` 交出来的必须是这一种。
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

/// 把字符串切片铺成 `Vec<String>`，好让期望值的写法和输出一样是逐字的。
fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn a_minimal_spec_is_assembled_flag_by_flag() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let cache = dir.path().join("cache");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(workspace.join(".git")).unwrap();
    std::fs::write(workspace.join(".git/config"), "").unwrap();
    std::fs::write(workspace.join(".env"), "SECRET=1\n").unwrap();

    let workspace = canonical(&workspace);
    let cache = canonical(&cache);
    let ssh = canonical(&home.join(".ssh"));
    let git_config = canonical(&workspace.join(".git/config"));
    let env_file = canonical(&workspace.join(".env"));

    let assembled = wrap(&shell("echo hi"), &workspace, &spec(&[&cache], &[&ssh]));

    let mut expected = strings(&[
        "bwrap",
        "--new-session",
        "--die-with-parent",
        "--ro-bind",
        "/",
        "/",
        "--tmpfs",
        "/tmp",
    ]);
    expected.push("--bind".to_owned());
    expected.push(workspace.display().to_string());
    expected.push(workspace.display().to_string());
    expected.push("--bind".to_owned());
    expected.push(cache.display().to_string());
    expected.push(cache.display().to_string());
    expected.extend(strings(&[
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
    ]));
    expected.push("--tmpfs".to_owned());
    expected.push(ssh.display().to_string());
    expected.push("--remount-ro".to_owned());
    expected.push(ssh.display().to_string());
    expected.push("--ro-bind".to_owned());
    expected.push(git_config.display().to_string());
    expected.push(git_config.display().to_string());
    expected.push("--ro-bind".to_owned());
    expected.push(env_file.display().to_string());
    expected.push(env_file.display().to_string());
    expected.push("--".to_owned());
    expected.extend(shell("echo hi"));

    assert_eq!(assembled, expected);
}

#[test]
fn the_private_tmp_is_mounted_before_any_writable_root() {
    // 真机教训（2026-10-01）：挂载是叠上去的。若 `--tmpfs /tmp` 排在 `--bind` 之后，
    // 一个落在 `/tmp` 下的会话工作区会被整个盖掉 —— 它写自己的文件会报「只读文件系统」。
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let assembled = wrap(&shell("true"), &canonical(&workspace), &spec(&[], &[]));

    let tmpfs = assembled
        .windows(2)
        .position(|window| window == ["--tmpfs", "/tmp"])
        .expect("私有一份 /tmp");
    assert!(!assembled[..tmpfs].contains(&"--bind".to_owned()));
}

#[test]
fn every_writable_root_gets_its_own_bind_in_input_order() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    for path in [&workspace, &first, &second] {
        std::fs::create_dir_all(path).unwrap();
    }

    let assembled = wrap(
        &shell("true"),
        &canonical(&workspace),
        &spec(&[&first, &second], &[]),
    );

    let binds: Vec<&String> = assembled
        .iter()
        .enumerate()
        .filter(|(_, item)| item.as_str() == "--bind")
        .map(|(index, _)| &assembled[index + 1])
        .collect();
    assert_eq!(
        binds,
        vec![
            &workspace.canonicalize().unwrap().display().to_string(),
            &first.canonicalize().unwrap().display().to_string(),
            &second.canonicalize().unwrap().display().to_string(),
        ]
    );
}

#[test]
fn a_mask_is_a_tmpfs_that_is_then_remounted_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let keys = dir.path().join("keys");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&keys).unwrap();

    let assembled = wrap(&shell("true"), &canonical(&workspace), &spec(&[], &[&keys]));

    let mask = canonical(&keys).display().to_string();
    let expected = strings(&["--tmpfs", &mask, "--remount-ro", &mask]);
    let position = assembled
        .windows(expected.len())
        .position(|window| window == expected.as_slice())
        .expect("遮罩必须是紧挨着的 `--tmpfs` + `--remount-ro` 两条");
    // 遮罩落在 `--ro-bind / /` 之后，于是它盖住的是那层只读挂载。
    assert!(position > 6);
}

#[test]
fn protected_paths_come_after_every_writable_bind() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(workspace.join(".git")).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(workspace.join(".git/config"), "").unwrap();

    let assembled = wrap(
        &shell("true"),
        &canonical(&workspace),
        &spec(&[&cache], &[]),
    );

    let last_bind = assembled
        .iter()
        .rposition(|item| item == "--bind")
        .expect("至少有一条可写根");
    let protected = assembled
        .iter()
        .position(|item| {
            item == &canonical(&workspace.join(".git/config"))
                .display()
                .to_string()
        })
        .expect(".git/config 在保护之列");
    assert!(
        protected > last_bind,
        "保护路径的 `--ro-bind` 必须排在每一个 `--bind` 之后，否则等于没保护"
    );
}

#[test]
fn missing_roots_masks_and_protected_files_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let missing_root = dir.path().join("no-such-cache");
    let missing_mask = dir.path().join("no-such-ssh");

    let assembled = wrap(
        &shell("true"),
        &canonical(&workspace),
        &spec(&[&missing_root], &[&missing_mask]),
    );

    let rendered = assembled.join(" ");
    assert!(!rendered.contains("no-such-cache"));
    assert!(!rendered.contains("no-such-ssh"));
    // 工作区里没有 `.git` 时，除了整机那条 `--ro-bind / /` 之外没有别的只读挂载。
    assert_eq!(
        assembled.iter().filter(|item| *item == "--ro-bind").count(),
        1,
        "不存在的保护路径整条跳过，只留下整机只读那一对"
    );
    assert_eq!(
        assembled.iter().filter(|item| *item == "--bind").count(),
        1,
        "只有 cwd 那一条可写根留下"
    );
}

#[test]
fn the_env_family_is_protected_but_templates_are_not() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    for name in [
        ".env",
        ".env.local",
        ".env.example",
        ".env.sample",
        ".env.template",
    ] {
        std::fs::write(workspace.join(name), "").unwrap();
    }

    let assembled = wrap(&shell("true"), &canonical(&workspace), &spec(&[], &[]));

    let protected: Vec<String> = assembled
        .iter()
        .filter(|item| item.contains(".env"))
        .cloned()
        .collect();
    assert!(protected.contains(&canonical(&workspace.join(".env")).display().to_string()));
    assert!(
        protected.contains(
            &canonical(&workspace.join(".env.local"))
                .display()
                .to_string()
        )
    );
    for name in [".env.example", ".env.sample", ".env.template"] {
        let path = canonical(&workspace.join(name)).display().to_string();
        assert!(
            !protected.contains(&path),
            "模板后缀不携带真密钥，不该被压回只读：{name}"
        );
    }
}

#[test]
fn git_config_and_hooks_are_protected_but_the_index_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(workspace.join(".git/hooks")).unwrap();
    std::fs::write(workspace.join(".git/config"), "").unwrap();
    std::fs::write(workspace.join(".git/hooks/pre-commit"), "").unwrap();
    std::fs::write(workspace.join(".git/index"), "").unwrap();

    let assembled = wrap(&shell("true"), &canonical(&workspace), &spec(&[], &[]));

    let rendered = assembled.join(" ");
    for relative in [".git/config", ".git/hooks"] {
        let path = canonical(&workspace.join(relative)).display().to_string();
        assert!(assembled.contains(&path), "{relative} 必须在保护之列");
    }
    let index = canonical(&workspace.join(".git/index"))
        .display()
        .to_string();
    assert!(
        !rendered.contains(&index),
        "`.git/index` 必须可写，否则 `git add` / `git commit` 全废"
    );
}

#[test]
fn cwd_and_writable_roots_are_canonicalized() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    let workspace = real.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&workspace, &link).unwrap();

    let assembled = wrap(&shell("true"), &link, &spec(&[&link], &[]));

    let real = canonical(&workspace).display().to_string();
    assert!(assembled.iter().filter(|item| **item == real).count() >= 2);
    assert!(
        !assembled.iter().any(|item| item.contains("link")),
        "symlink 会骗过「看起来在工作区里」的判定，所以两边都用规范路径"
    );
}

#[test]
fn mode_off_is_the_identity_function() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut off = spec(&[], &[]);
    off.mode = SandboxMode::Off;

    let argv = shell("echo hi");
    assert_eq!(wrap(&argv, &workspace, &off), argv);
}

// --- 写死的边界与升级路径（`.scratch/workspace-mode/issues/01`）------------

#[test]
fn sealed_covers_masks_and_protected_paths() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(workspace.join(".git/hooks")).unwrap();
    std::fs::write(workspace.join(".git/config"), "").unwrap();
    std::fs::write(workspace.join(".env"), "").unwrap();
    std::fs::write(workspace.join(".env.example"), "").unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(home.join(".npm")).unwrap();

    let cwd = canonical(&workspace);
    let masks = vec![canonical(&home.join(".ssh")), home.join(".config/heng")];

    assert!(
        sealed(&cwd.join(".git/config"), &cwd, &masks),
        ".git/config 是保护路径"
    );
    assert!(
        sealed(&cwd.join(".git/hooks/pre-commit"), &cwd, &masks),
        ".git/hooks 是保护路径"
    );
    assert!(sealed(&cwd.join(".env"), &cwd, &masks), ".env 是保护路径");
    assert!(
        sealed(&cwd.join("sub/.env"), &cwd, &masks),
        ".env 一族整族都在列"
    );
    assert!(
        !sealed(&cwd.join(".env.example"), &cwd, &masks),
        "模板不携带真密钥"
    );
    assert!(
        sealed(
            &canonical(&home.join(".ssh")).join("authorized_keys"),
            &cwd,
            &masks
        ),
        "遮罩目录整棵都在列"
    );
    assert!(
        !sealed(&home.join(".npm"), &cwd, &masks),
        "工作区外的普通路径不在列"
    );
    assert!(
        !sealed(&cwd.join("notes.txt"), &cwd, &masks),
        "工作区内的普通路径不在列"
    );
}

#[test]
fn an_escalation_path_is_expanded_and_lexically_folded() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let cwd = canonical(&workspace);
    let home = canonical(&home);

    assert_eq!(
        escalation_path(Path::new("~/.npm"), &cwd, Some(&home)),
        home.join(".npm"),
        "`~` 按 home 展开，目标不必存在"
    );
    assert_eq!(
        escalation_path(Path::new("build/../.npm"), &cwd, Some(&home)),
        cwd.join(".npm"),
        "相对路径折到 cwd 上，`..` 按字面折掉"
    );
    assert_eq!(
        escalation_path(Path::new("/tmp/elsewhere"), &cwd, Some(&home)),
        PathBuf::from("/tmp/elsewhere")
    );
}

// --- 探测（票 02）----------------------------------------------------------

/// 一个假 `bwrap`：指向一个系统程序，而不是写一份脚本。
///
/// 指向 `/bin/true` / `/bin/false` 就能拿到可控的退出码，指向 `/bin/echo` 就能把收到的
/// argv 从标准输出读回来。用符号链接而不是「写脚本再执行」，是因为后者在并行测试下会撞上
/// `ETXTBSY`（一个线程还开着写句柄时，另一个线程去 exec）。
fn fake_bwrap(dir: &Path, target: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("bwrap");
    std::os::unix::fs::symlink(target, &path).unwrap();
    canonical(dir).join("bwrap")
}

#[test]
fn probe_answers_by_the_minimal_profiles_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let bin = fake_bwrap(&dir.path().join("bin"), "/bin/true");

    let availability = probe(
        Some(OsStr::new(bin.parent().unwrap().to_str().unwrap())),
        &workspace,
    );

    assert_eq!(availability, SandboxAvailability::Available { bwrap: bin });
    // 探测真的跑了一条 profile，而不是只看 `--version`：profile 自己也被钉住。
    assert!(PROBE_PROFILE.contains(&"--ro-bind"));
    assert!(
        PROBE_PROFILE.contains(&"--proc"),
        "`/proc` 建不起来也算不可用，所以探测必须带上它"
    );
    assert!(PROBE_PROFILE.contains(&"/bin/true"));
}

#[test]
fn probe_reports_unavailable_when_the_profile_fails() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let bin = fake_bwrap(&dir.path().join("bin"), "/bin/false");

    let availability = probe(
        Some(OsStr::new(bin.parent().unwrap().to_str().unwrap())),
        &workspace,
    );

    assert!(
        matches!(availability, SandboxAvailability::Unavailable { .. }),
        "退出码非零就是不可用：{availability:?}"
    );
}

#[test]
fn probe_reports_unavailable_when_there_is_no_bwrap_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let availability = probe(Some(OsStr::new("/nonexistent-heng-bin")), dir.path());

    assert!(matches!(
        availability,
        SandboxAvailability::Unavailable { .. }
    ));
}

#[test]
fn a_bwrap_inside_the_workspace_is_not_used() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let elsewhere = dir.path().join("bin");
    fake_bwrap(&workspace, "/bin/true");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let path = format!("{}:{}", workspace.display(), elsewhere.display());

    let availability = probe(Some(OsStr::new(&path)), &workspace);

    assert!(
        matches!(availability, SandboxAvailability::Unavailable { .. }),
        "工作区里的假 bwrap 不能被采用：{availability:?}"
    );
}

#[test]
fn mode_off_and_settled_availability_never_probe() {
    let dir = tempfile::tempdir().unwrap();
    let bin = fake_bwrap(&dir.path().join("bin"), "/bin/false");

    let mut settings = SandboxSettings::off();
    settings.search_path = Some(OsString::from(bin.parent().unwrap().to_str().unwrap()));
    assert_eq!(
        resolve_availability(&settings, dir.path()),
        SandboxAvailability::Untested,
        "`mode = \"off\"` 时探测根本不跑"
    );

    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Unavailable {
        reason: "已经探过了".to_owned(),
    };
    assert_eq!(
        resolve_availability(&settings, dir.path()),
        settings.availability,
        "探测结果已经有了，就不该重探"
    );
}

// --- 接进 process::run（票 03）--------------------------------------------

/// 一个用「探测到的绝对路径」构造出来的沙箱：`bwrap` 不必出现在 PATH 上。
fn sandbox_using(bwrap: &Path) -> Sandbox {
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Available {
        bwrap: bwrap.to_path_buf(),
    };
    Sandbox::new(&settings)
}

#[test]
fn grants_add_one_writable_root_for_this_call_only() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let granted = dir.path().join("granted");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir_all(&granted).unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/true");
    let sandbox = sandbox_using(&bwrap);

    let with = sandbox
        .with_grants(std::slice::from_ref(&granted))
        .wrap(&shell("echo hi"), &workspace)
        .unwrap();
    let granted = canonical(&granted).display().to_string();
    assert!(
        with.windows(3)
            .any(|window| window == ["--bind", &granted, &granted]),
        "批准的那条路径本身进可写根，不做父目录提升：{with:?}"
    );

    let without = sandbox.wrap(&shell("echo hi"), &workspace).unwrap();
    assert!(
        !without.contains(&granted),
        "批准只活这一次调用，沙箱值本身不变：{without:?}"
    );
}

#[test]
fn a_grant_that_does_not_exist_is_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/true");
    let missing = dir.path().join("not-there");

    let error = sandbox_using(&bwrap)
        .with_grants(std::slice::from_ref(&missing))
        .wrap(&shell("echo hi"), &workspace)
        .expect_err("批准一条不存在的路径是明确的失败，不是静默白批");
    assert!(error.to_string().contains("不存在"), "{error}");
    assert!(
        error.to_string().contains("不做父目录提升"),
        "理由要说清为什么不能替它猜：{error}"
    );
}

#[tokio::test]
async fn run_spawns_the_probed_bwrap_with_the_command_after_the_separator() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    // `/bin/echo` 把收到的 argv 原样写到标准输出，于是整条拼装可以逐字读回来。
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/echo");

    let outcome = process::run(
        &workspace,
        &shell("echo hi"),
        Duration::from_secs(10),
        &sandbox_using(&bwrap),
    )
    .await
    .unwrap();

    // `/bin/echo` 把 argv 打成一行的空格分隔形式。
    assert!(
        outcome.stdout.contains("-- bash -lc echo hi"),
        "原命令必须原样落在 `--` 之后：{:?}",
        outcome.stdout
    );
}

#[test]
fn a_bwrap_diagnostic_is_a_tool_error_and_says_the_command_did_not_run() {
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/true");
    let sandbox = sandbox_using(&bwrap);

    let message = sandbox
        .failure("bwrap: Can't mount proc on /newroot/proc: Operation not permitted\n")
        .expect("`bwrap: ` 前缀意味着沙箱没起来");
    assert!(message.contains("命令没有跑"), "{message}");
    assert!(message.contains("bwrap: Can't mount proc"), "{message}");

    // 命令自己的失败不是沙箱的失败：内核给的拒绝消息随 locale 变，不能拿它做判据。
    assert_eq!(sandbox.failure("只读文件系统\n"), None);
    assert_eq!(sandbox.failure(""), None);
    assert_eq!(
        Sandbox::new(&SandboxSettings::off()).failure("bwrap: x\n"),
        None,
        "关掉沙箱时 `bwrap: ` 只是命令自己的输出"
    );
}

#[tokio::test]
async fn a_command_failing_inside_the_sandbox_is_still_a_result() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    // `cat` 不认识 `--ro-bind`，于是它自己报错、自己退出非零 —— 一次普通的命令失败。
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/cat");

    let outcome = process::run(
        &workspace,
        &shell("echo hi"),
        Duration::from_secs(10),
        &sandbox_using(&bwrap),
    )
    .await
    .expect("命令自己失败仍然是数据，不是 ToolError");

    assert!(!outcome.status.success());
    assert!(
        !outcome.stderr.starts_with("bwrap: "),
        "命令自己的 stderr 原样交给模型：{:?}",
        outcome.stderr
    );
    assert!(!outcome.stderr.is_empty());
}

#[tokio::test]
async fn an_unavailable_sandbox_refuses_to_run_anything() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Unavailable {
        reason: "PATH 上没有 `bwrap`".to_owned(),
    };

    let error = process::run(
        &workspace,
        &shell("echo hi"),
        Duration::from_secs(10),
        &Sandbox::new(&settings),
    )
    .await
    .expect_err("不可用就是 fail closed");

    let message = error.to_string();
    assert!(message.contains("命令没有跑"), "{message}");
    assert!(message.contains("PATH 上没有 `bwrap`"), "{message}");
    assert!(message.contains("mode"), "两条出路要写出来：{message}");
}

#[tokio::test]
async fn with_the_sandbox_off_run_spawns_the_command_directly() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let outcome = process::run(
        &workspace,
        &shell("echo direct"),
        Duration::from_secs(10),
        &Sandbox::new(&SandboxSettings::off()),
    )
    .await
    .unwrap();

    assert!(outcome.stdout.contains("direct"));
}

// --- 端到端：工具真的跑在沙箱里（票 03）-----------------------------------

mod support;

use std::sync::Arc;

use heng::events::{Event, EventPayload, SessionId, SpeakerId};
use heng::permissions::{Asker, Mode, Policy};
use heng::provider::capability::caps_for;
use heng::provider::projection::project;
use heng::provider::{FinishReason, StreamEvent};
use heng::render::{RenderSinks, Renderer};
use heng::tools::{PathLocks, Registry, builtin};
use heng::{AssemblyParts, DebaterParts, Harness, SessionScaffold, SynthesizerParts, assemble};
use support::{AlwaysAllow, CaptureBuf, FakeProvider, Reply, ScriptedAsker};

struct Fixture {
    harness: Harness,
    log_path: PathBuf,
    workspace: PathBuf,
    _dir: tempfile::TempDir,
}

async fn fixture(replies: Vec<Reply>, config: SessionConfig, tools: Registry) -> Fixture {
    fixture_at(replies, config, tools, None).await
}

/// 同上，但可以指定一份**已经存在**的日志：那是一次 `--continue`。
async fn fixture_at(
    replies: Vec<Reply>,
    config: SessionConfig,
    tools: Registry,
    log_path: Option<PathBuf>,
) -> Fixture {
    fixture_full(
        replies,
        config,
        tools,
        Policy::for_mode(Mode::Auto),
        Some(Arc::new(AlwaysAllow)),
        log_path,
    )
    .await
}

/// 完整的脚手架：策略与作答者也可以换，于是升级那条路（它在 `ask` 与 `auto` 档下就已经
/// 有用）能在脚本化的作答者下面被端到端跑一遍。
async fn fixture_full(
    replies: Vec<Reply>,
    config: SessionConfig,
    tools: Registry,
    policy: Policy,
    asker: Option<Arc<dyn Asker>>,
    log_path: Option<PathBuf>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let log_path = log_path.unwrap_or_else(|| session.join("log.jsonl"));
    let provider = FakeProvider::new(replies);

    let harness = assemble(AssemblyParts {
        provider: Box::new(provider),
        speaker: SpeakerId::Debater("kimi".into()),
        config,
        renderer: Renderer::headless(RenderSinks {
            stdout_result: Box::new(CaptureBuf::default()),
            stderr_diagnostic: Box::new(CaptureBuf::default()),
        }),
        scaffold: SessionScaffold {
            cwd: workspace.clone(),
            log_path: log_path.clone(),
            session_id: SessionId::new("s-sandbox"),
            tools,
            locks: PathLocks::new(),
            policy,
            asker,
            questions: None,
            hook: None,
            home: None,
        },
    })
    .await
    .unwrap();

    Fixture {
        harness,
        log_path,
        workspace,
        _dir: dir,
    }
}

impl Fixture {
    fn events(&self) -> Vec<Event> {
        heng::events::read_events(&self.log_path).unwrap()
    }

    /// 每一次已完成的调用对应的 `(ok, output_or_error)`。
    fn results(&self) -> Vec<(bool, String)> {
        self.events()
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayload::ToolCallCompleted {
                    ok, output, error, ..
                } => Some((
                    *ok,
                    output.clone().or_else(|| error.clone()).unwrap_or_default(),
                )),
                _ => None,
            })
            .collect()
    }
}

fn call(id: &str, tool: &str, args: serde_json::Value) -> Reply {
    Reply::Stream(vec![
        StreamEvent::ToolCallCompleted {
            index: 0,
            id: id.into(),
            name: tool.into(),
            arguments: args.to_string(),
        },
        StreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
        },
    ])
}

/// 一次会话配置：沙箱开着，用探测结果指向 `bwrap`。
fn sandboxed(bwrap: &Path) -> SessionConfig {
    let mut config = SessionConfig::new("fake-model");
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Available {
        bwrap: bwrap.to_path_buf(),
    };
    config.sandbox = settings;
    config
}

#[tokio::test]
async fn the_bash_tool_runs_through_the_sandbox() {
    let dir = tempfile::tempdir().unwrap();
    // `/bin/echo` 的标准输出就是这次调用真正拿到的 argv。
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/echo");
    let mut fixture = fixture(
        vec![
            call(
                "call-bash",
                "bash",
                serde_json::json!({ "command": "echo hi" }),
            ),
            Reply::text("done"),
        ],
        sandboxed(&bwrap),
        builtin(false),
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (ok, output) = fixture.results().remove(0);
    assert!(ok, "{output}");
    assert!(
        output.contains("-- bash -lc echo hi"),
        "工具结果里能看到 bwrap 那条 argv：{output}"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_dynamic_tool_runs_through_the_same_sandbox() {
    // 两个调用点共用 `process::run`，所以动态工具与 `bash` 自动同等生效。这里把它钉住，
    // 免得哪天它变成一个偶然。
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/echo");
    let env: heng::config::EnvMap =
        std::iter::once(("HOME".to_owned(), dir.path().display().to_string())).collect();
    let text = "[tools.thing.echo]\ndescription = \"回声\"\ncommand = [\"echo\", \"{text}\"]\n\
                parameters = { type = \"object\", properties = { text = { type = \"string\" } } }\n";
    let config = heng::config::resolve(Some(text), &env).unwrap();
    let tools = heng::tools::with_dynamic(&config.tools, false);

    let mut fixture = fixture(
        vec![
            call(
                "call-custom",
                "custom__thing__echo",
                serde_json::json!({ "text": "hi" }),
            ),
            Reply::text("done"),
        ],
        sandboxed(&bwrap),
        tools,
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (ok, output) = fixture.results().remove(0);
    assert!(ok, "{output}");
    assert!(
        output.contains("-- echo hi"),
        "动态工具的原 argv 也该出现在 `--` 之后：{output:?}"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_unavailable_sandbox_makes_bash_a_tool_error_with_two_ways_out() {
    let mut config = SessionConfig::new("fake-model");
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Unavailable {
        reason: "PATH 上没有 `bwrap`".to_owned(),
    };
    config.sandbox = settings;

    let mut fixture = fixture(
        vec![
            call(
                "call-bash",
                "bash",
                serde_json::json!({ "command": "echo never" }),
            ),
            Reply::text("could not run"),
        ],
        config,
        builtin(false),
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (ok, message) = fixture.results().remove(0);
    assert!(!ok, "沙箱不可用是工具错误，不是一条命令结果：{message}");
    assert!(message.contains("命令没有跑"), "{message}");
    assert!(message.contains("PATH 上没有 `bwrap`"), "{message}");
    assert!(
        message.contains("\"off\""),
        "两条出路之一要写出来：{message}"
    );
    assert!(
        !fixture.workspace.join("made.txt").exists(),
        "命令根本没有跑"
    );

    fixture.harness.shutdown().await;
}

// --- 升级手势（`.scratch/workspace-mode/issues/01`）------------------------

#[tokio::test]
async fn an_escalation_asks_once_and_binds_the_declared_path_for_that_call() {
    let dir = tempfile::tempdir().unwrap();
    // `/bin/echo` 把收到的 argv 写回标准输出，于是「批准之后多了一条 `--bind`」可读。
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/echo");
    let granted = dir.path().join("granted");
    std::fs::create_dir_all(&granted).unwrap();
    let granted = granted.canonicalize().unwrap().display().to_string();

    let asker = ScriptedAsker::new(vec![heng::permissions::Answer::Allow]);
    let mut fixture = fixture_full(
        vec![
            call(
                "call-1",
                "bash",
                serde_json::json!({ "command": "echo x > ~/.npm/probe" }),
            ),
            call(
                "call-2",
                "bash",
                serde_json::json!({
                    "command": "echo x > ~/.npm/probe",
                    "escalation": {
                        "justification": "构建产物要写到缓存目录",
                        "writable_paths": [granted.clone()],
                    }
                }),
            ),
            Reply::text("done"),
        ],
        sandboxed(&bwrap),
        builtin(false),
        Policy::for_mode(Mode::Auto),
        Some(Arc::new(asker.clone())),
        None,
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let results = fixture.results();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        !results[0].1.contains(&granted),
        "没有升级的那一次 argv 里没有那条 bind：{}",
        results[0].1
    );
    assert!(
        results[1].1.contains(&granted),
        "批准之后这一次调用的 argv 里多一条 `--bind <声明的路径>`：{}",
        results[1].1
    );

    // 只问一次，而且问的是一次升级。
    let requests = asker.requests();
    assert_eq!(requests.len(), 1, "只问一次");
    assert!(
        requests[0].reason.contains("沙箱升级"),
        "{}",
        requests[0].reason
    );
    assert!(
        requests[0].reason.contains(&granted),
        "理由里写明了要放开的路径：{}",
        requests[0].reason
    );
    assert!(
        requests[0]
            .escalation
            .as_ref()
            .is_some_and(|escalation| escalation.justification == "构建产物要写到缓存目录"),
        "弹窗拿得到理由与路径这两行"
    );

    // 审计：两次调用各一条裁决，其中一次是升级。
    let events = fixture.events();
    let asked = events
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::PermissionAsked { .. }))
        .count();
    assert_eq!(asked, 1, "事件流里只看得到那一次升级询问");
    let decided: Vec<String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::PermissionDecided { reason, .. } => reason.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(decided.len(), 2, "{decided:?}");
    assert!(
        decided.iter().any(|reason| reason.contains("沙箱升级")),
        "裁决理由里写明这是一次升级：{decided:?}"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_refused_escalation_is_a_failed_result_not_a_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/echo");
    let granted = dir.path().join("granted");
    std::fs::create_dir_all(&granted).unwrap();

    let asker = ScriptedAsker::new(vec![heng::permissions::Answer::Deny]);
    let mut fixture = fixture_full(
        vec![
            call(
                "call-1",
                "bash",
                serde_json::json!({
                    "command": "echo x > ~/.npm/probe",
                    "escalation": {
                        "justification": "构建产物要写到缓存目录",
                        "writable_paths": [granted.display().to_string()],
                    }
                }),
            ),
            Reply::text("could not write"),
        ],
        sandboxed(&bwrap),
        builtin(false),
        Policy::for_mode(Mode::Auto),
        Some(Arc::new(asker.clone())),
        None,
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (ok, message) = fixture.results().remove(0);
    assert!(!ok, "拒绝即终局：{message}");
    assert!(message.contains("权限拒绝"), "{message}");
    assert!(message.contains("沙箱升级"), "{message}");
    assert_eq!(asker.requests().len(), 1);

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn an_escalation_into_a_mask_is_denied_without_asking() {
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/echo");
    let ssh = dir.path().join("ssh");
    std::fs::create_dir_all(&ssh).unwrap();
    let mut config = sandboxed(&bwrap);
    config.sandbox.masks = vec![ssh.canonicalize().unwrap()];

    let asker = ScriptedAsker::default();
    let mut fixture = fixture_full(
        vec![
            call(
                "call-1",
                "bash",
                serde_json::json!({
                    "command": "echo x >> authorized_keys",
                    "escalation": {
                        "justification": "想加一把钥匙",
                        "writable_paths": [ssh.join("authorized_keys").display().to_string()],
                    }
                }),
            ),
            Reply::text("blocked"),
        ],
        config,
        builtin(false),
        Policy::for_mode(Mode::Auto),
        Some(Arc::new(asker.clone())),
        None,
    )
    .await;

    fixture.harness.run_turn("run it").await.unwrap();

    let (ok, message) = fixture.results().remove(0);
    assert!(!ok, "{message}");
    assert!(message.contains("没有任何通道放宽"), "{message}");
    assert!(
        asker.requests().is_empty(),
        "写死的边界不问：问了就等于给了用户一个能批的错觉"
    );

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_half_written_escalation_is_a_parameter_error() {
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/echo");

    for args in [
        serde_json::json!({
            "command": "echo hi",
            "escalation": { "justification": "有理由没路径" }
        }),
        serde_json::json!({
            "command": "echo hi",
            "escalation": { "justification": "有理由没路径", "writable_paths": [] }
        }),
        serde_json::json!({
            "command": "echo hi",
            "escalation": { "justification": "  ", "writable_paths": ["/tmp/x"] }
        }),
    ] {
        let mut fixture = fixture(
            vec![call("call-1", "bash", args), Reply::text("ack")],
            sandboxed(&bwrap),
            builtin(false),
        )
        .await;
        fixture.harness.run_turn("run it").await.unwrap();

        let (ok, message) = fixture.results().remove(0);
        assert!(!ok, "半截的升级申请是参数错误：{message}");
        assert!(message.contains("escalation"), "{message}");
        fixture.harness.shutdown().await;
    }
}

// --- 模型可见的说明与状态事件（票 04）-------------------------------------

#[test]
fn the_bash_description_states_the_sandbox_terms() {
    let registry = builtin(false);
    let description = registry
        .get("bash")
        .expect("bash 是内置工具")
        .spec()
        .description;

    assert!(
        description.contains(heng::tools::bash::SANDBOX_NOTE),
        "描述里那句沙箱说明是常量的一部分：{description}"
    );
    let note = heng::tools::bash::SANDBOX_NOTE;
    assert!(note.contains("沙箱"), "{note}");
    assert!(note.contains("工作区"), "{note}");
    assert!(note.contains("只读"), "{note}");
    assert!(note.contains("/tmp"), "{note}");
    assert!(note.contains("每次调用"), "{note}");
}

#[test]
fn the_bash_description_states_the_escalation_terms() {
    let registry = builtin(false);
    let description = registry
        .get("bash")
        .expect("bash 是内置工具")
        .spec()
        .description;

    assert!(
        description.contains(heng::tools::bash::ESCALATION_NOTE),
        "描述里那段升级话术是常量的一部分：{description}"
    );
    let note = heng::tools::bash::ESCALATION_NOTE;
    // 四件事一件都不能少：被拒是结论、只有原样重试一次这一条路、不许先绕道去聊天里问、
    // 不许投机性升级（`.scratch/workspace-mode/spec.md` §4）。
    assert!(note.contains("被沙箱拒绝"), "{note}");
    assert!(note.contains("结论"), "{note}");
    assert!(note.contains("原样重试一次"), "{note}");
    assert!(note.contains("绕道"), "{note}");
    assert!(note.contains("没被拒"), "{note}");
}

#[test]
fn the_escalation_argument_is_not_part_of_the_argv() {
    let registry = builtin(false);
    let tool = registry.get("bash").expect("bash 是内置工具");
    let args = serde_json::json!({
        "command": "echo hi",
        "escalation": { "justification": "要写缓存", "writable_paths": ["/tmp/x"] }
    });

    assert_eq!(
        tool.command(&args),
        Some(vec![
            "bash".to_owned(),
            "-lc".to_owned(),
            "echo hi".to_owned()
        ]),
        "`escalation` 不是 argv 的一部分：它请的是放宽沙箱，不是命令本身"
    );
}

/// 一次新会话（脚本化 provider，什么都不做）里的事件流。
async fn session_events(config: SessionConfig) -> Vec<Event> {
    let fixture = fixture(vec![Reply::text("hi")], config, builtin(false)).await;
    let events = fixture.events();
    fixture.harness.shutdown().await;
    events
}

#[tokio::test]
async fn a_new_session_records_exactly_one_sandbox_status_event() {
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/true");
    let events = session_events(sandboxed(&bwrap)).await;

    let statuses: Vec<&EventPayload> = events
        .iter()
        .map(|event| &event.payload)
        .filter(|payload| matches!(payload, EventPayload::SandboxStatus { .. }))
        .collect();
    assert_eq!(statuses.len(), 1, "每条流有且只有一条沙箱状态事件");
    match statuses[0] {
        EventPayload::SandboxStatus {
            mode,
            unavailable_reason,
        } => {
            assert_eq!(mode, "bwrap");
            assert_eq!(*unavailable_reason, None);
        }
        other => panic!("不是沙箱状态事件：{other:?}"),
    }
}

#[tokio::test]
async fn the_status_event_does_not_change_the_projected_messages() {
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/true");
    let events = session_events(sandboxed(&bwrap)).await;
    let speaker = SpeakerId::Debater("kimi".into());
    let caps = caps_for("deepseek-flash").expect("内置模型");

    let without: Vec<Event> = events
        .iter()
        .filter(|event| !matches!(event.payload, EventPayload::SandboxStatus { .. }))
        .cloned()
        .collect();

    assert_eq!(
        project(&events, &speaker, &caps),
        project(&without, &speaker, &caps),
        "log-only 事件不进 messages：前缀因此逐字不变"
    );
}

#[tokio::test]
async fn mode_off_is_recorded_as_off() {
    let events = session_events(SessionConfig::new("fake-model")).await;

    let status = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::SandboxStatus {
                mode,
                unavailable_reason,
            } => Some((mode.clone(), unavailable_reason.clone())),
            _ => None,
        })
        .expect("每条流都记下当时那一档");
    assert_eq!(status.0, "off");
    assert_eq!(status.1, None, "关掉不是「不可用」，没有理由可报");
}

#[tokio::test]
async fn an_unavailable_sandbox_records_why() {
    let mut config = SessionConfig::new("fake-model");
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Unavailable {
        reason: "PATH 上没有 `bwrap`".to_owned(),
    };
    config.sandbox = settings;
    let events = session_events(config).await;

    let reason = events.iter().find_map(|event| match &event.payload {
        EventPayload::SandboxStatus {
            unavailable_reason, ..
        } => unavailable_reason.clone(),
        _ => None,
    });
    assert_eq!(reason.as_deref(), Some("PATH 上没有 `bwrap`"));
}

/// 真机那一层（票 05）：前两层只能验「我们拼对了参数」，这一条验**内核真的拦住了**。
///
/// 没有可用 `bwrap` 的机器上跳过 —— 那台机器上的行为由「不可用就是 fail closed」覆盖。
#[tokio::test]
async fn the_real_bubblewrap_keeps_writes_inside_the_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let availability = probe(std::env::var_os("PATH").as_deref(), &workspace);
    let SandboxAvailability::Available { bwrap } = availability else {
        eprintln!("跳过：这台机器上没有可用的 bwrap（{availability:?}）");
        return;
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("跳过：没有 HOME，区外没有一个稳定的目标");
        return;
    };

    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Available { bwrap };
    let sandbox = Sandbox::new(&settings);

    // 工作区里照常写。
    let inside = process::run(
        &workspace,
        &shell("echo inside > made.txt"),
        Duration::from_secs(30),
        &sandbox,
    )
    .await
    .unwrap();
    assert!(inside.status.success(), "{}", inside.report());
    assert_eq!(
        std::fs::read_to_string(workspace.join("made.txt")).unwrap(),
        "inside\n"
    );

    // 区外写不动，而且宿主上真的没有那个文件。
    let outside = home.join(".heng-sandbox-probe");
    let _ = std::fs::remove_file(&outside);
    let escaped = process::run(
        &workspace,
        &shell(&format!("echo x > {}", outside.display())),
        Duration::from_secs(30),
        &sandbox,
    )
    .await
    .unwrap();
    assert!(
        !escaped.status.success(),
        "区外必须由内核打回：{}",
        escaped.report()
    );
    assert!(!outside.exists(), "沙箱里写的区外文件不该出现在宿主上");
}

/// 一份开着沙箱、但探测结果还没填的配置 —— `cli` 用 `Config::session_config` 造出来的就是
/// 这一份（`availability` 是解析期的 `Untested`）。
fn untested_bwrap(bwrap: &Path) -> SessionConfig {
    let mut settings = SandboxSettings::off();
    settings.mode = SandboxMode::Bwrap;
    settings.availability = SandboxAvailability::Untested;
    settings.writable_roots = vec![bwrap.to_path_buf()];
    let mut config = SessionConfig::new("fake-model");
    config.sandbox = settings;
    config
}

#[tokio::test]
async fn a_discussion_forked_from_a_live_session_keeps_the_sandbox() {
    // `/discuss` 的讨论者是从活会话 fork 出来的，拿的是 `Config::session_config` 造的那份
    // 配置 —— 里面的沙箱还没探过。fork 必须继承父会话已经定下来的结果，否则讨论者一调 shell
    // 就撞「沙箱状态还没有定下来」，而它本该与父会话跑在同一个沙箱里（票 03 第 3 条：
    // 执行者与讨论者同等生效）。
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/echo");
    let mut fixture = fixture(vec![Reply::text("hi")], sandboxed(&bwrap), builtin(false)).await;

    let caps = caps_for("deepseek-flash").expect("内置模型");
    let debater_config = untested_bwrap(&bwrap);
    let debaters = vec![
        DebaterParts {
            speaker: SpeakerId::Debater("kimi-k3".into()),
            config: debater_config.clone(),
            provider: Box::new(FakeProvider::with_caps(
                vec![
                    call(
                        "call-bash",
                        "bash",
                        serde_json::json!({ "command": "echo hi" }),
                    ),
                    Reply::text("结论：进沙箱\nCONCLUSION: 进沙箱"),
                ],
                caps,
            )),
            soul: None,
        },
        DebaterParts {
            speaker: SpeakerId::Debater("kimi-k3#2".into()),
            config: debater_config,
            provider: Box::new(FakeProvider::with_caps(
                vec![Reply::text("结论：进沙箱\nCONCLUSION: 进沙箱")],
                caps,
            )),
            soul: None,
        },
    ];
    let synthesizer = SynthesizerParts {
        config: SessionConfig::new("fake-model"),
        provider: Box::new(FakeProvider::with_caps(
            vec![Reply::text("共识：进沙箱")],
            caps,
        )),
    };

    fixture
        .harness
        .discuss("该不该进沙箱？", debaters, synthesizer, Some(1))
        .await
        .unwrap();

    let (ok, output) = fixture.results().remove(0);
    assert!(ok, "讨论者的 shell 调用必须真的跑起来：{output}");
    assert!(output.contains("-- bash -lc echo hi"), "{output}");

    fixture.harness.shutdown().await;
}

#[tokio::test]
async fn a_resumed_session_records_the_sandbox_status_again() {
    // 续接不再记骨架，但这一刻的沙箱状态是新的：两次运行之间 `bwrap` 可能变得可用、也
    // 可能用不了了。它只进日志，所以补记一条不碰前缀稳定性。
    let dir = tempfile::tempdir().unwrap();
    let bwrap = fake_bwrap(&dir.path().join("bin"), "/bin/true");

    let first = fixture(vec![Reply::text("hi")], sandboxed(&bwrap), builtin(false)).await;
    let log_path = first.log_path.clone();
    let before = first
        .events()
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::SandboxStatus { .. }))
        .count();
    assert_eq!(before, 1, "新会话一条");
    first.harness.shutdown().await;

    let resumed = fixture_at(
        vec![Reply::text("again")],
        sandboxed(&bwrap),
        builtin(false),
        Some(log_path),
    )
    .await;
    let after = resumed
        .events()
        .iter()
        .filter(|event| matches!(event.payload, EventPayload::SandboxStatus { .. }))
        .count();
    assert_eq!(after, 2, "续接再记一条，于是 replay 看到的是这一刻的状态");
    resumed.harness.shutdown().await;
}
