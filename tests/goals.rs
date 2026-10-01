//! 目标清单与 `/goal new`（`.scratch/goal-loop/spec.md` §1、§2）。
//!
//! 这一票不碰 agent、不碰 `/loop`：全是纯函数与文件断言，所以每一个分支都能在这里断言，
//! 不需要假 provider。

use std::path::Path;

use fs_agent::goals::{self, GoalError};

/// 一个真实存在的 feature 目录：五张票，编号连着。
const SANDBOX: &str = ".scratch/sandbox";

/// `.scratch/sandbox/issues/` 里那五张票的标题，按编号。
const SANDBOX_TITLES: [&str; 5] = [
    "沙箱的 argv 拼装：`wrap()` 纯函数",
    "探测、配置与 fail closed",
    "接进 `process::run`，并区分「沙箱坏了」与「命令失败」",
    "模型可见的说明与状态事件",
    "真机验收清单与收尾",
];

#[test]
fn a_feature_directory_generates_one_entry_per_ticket_with_the_ticket_number_as_its_id() {
    let manifest = goals::generate("sandbox", Path::new(SANDBOX)).expect("来源里五张票都在");

    assert_eq!(manifest.name, "sandbox");
    assert_eq!(
        manifest.ids().collect::<Vec<_>>(),
        ["01", "02", "03", "04", "05"],
        "票号就是条目 id，两位十进制"
    );
    let contents: Vec<&str> = manifest
        .entries
        .iter()
        .map(|entry| entry.content.as_str())
        .collect();
    assert_eq!(contents, SANDBOX_TITLES, "条目内容就是各票的标题");
}

#[test]
fn a_single_ticket_file_generates_one_entry() {
    let ticket = Path::new(SANDBOX).join("issues/03-wire-into-process-run.md");
    let manifest = goals::generate("sandbox", &ticket).expect("来源是一份票文件");

    assert_eq!(manifest.entries.len(), 1);
    assert_eq!(manifest.entries[0].id, "03");
    assert_eq!(manifest.entries[0].content, SANDBOX_TITLES[2]);
}

#[test]
fn rendering_and_parsing_a_manifest_round_trips() {
    let manifest = goals::generate("sandbox", Path::new(SANDBOX)).unwrap();
    let path = Path::new("/tmp/sandbox.md");

    let parsed = goals::Manifest::parse(path, &manifest.render()).expect("自己渲染的自己读得回来");
    assert_eq!(parsed, manifest);
}

#[test]
fn a_hand_written_manifest_is_read_back() {
    let text = "# grep-tool\n\n- 01 看 bash 工具怎么注册\n- 02 写 grep 工具骨架\n\n- 03 补测试\n";
    let manifest = goals::Manifest::parse(Path::new("grep-tool.md"), text).unwrap();

    assert_eq!(manifest.name, "grep-tool");
    assert_eq!(
        manifest.entries,
        vec![
            goals::Entry {
                id: "01".to_owned(),
                content: "看 bash 工具怎么注册".to_owned(),
            },
            goals::Entry {
                id: "02".to_owned(),
                content: "写 grep 工具骨架".to_owned(),
            },
            goals::Entry {
                id: "03".to_owned(),
                content: "补测试".to_owned(),
            },
        ]
    );
}

#[test]
fn a_duplicate_id_is_a_file_level_error_pointing_at_the_line() {
    let text = "# grep-tool\n\n- 01 第一件\n- 02 第二件\n- 01 又是第一件\n";
    let error = goals::Manifest::parse(Path::new("grep-tool.md"), text).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("第 5 行"), "报错要指向那一行：{message}");
    assert!(message.contains("01"), "报错要点名那个 id：{message}");
    assert!(message.contains("重复"), "报错要说清是重复：{message}");
}

#[test]
fn malformed_entry_lines_are_rejected() {
    // 没有 id：`- 看 bash` 是两句里的第一句，后面的 `- 01` 只是被误读的内容。
    for text in [
        "# g\n\n- 看 bash 工具怎么注册\n",
        "# g\n\n- 3 一位数\n",
        "# g\n\n- 003 三位数\n",
        "# g\n\n- ab 不是数字\n",
    ] {
        let error = goals::Manifest::parse(Path::new("g.md"), text)
            .expect_err(&format!("这一行读不成条目：{text:?}"));
        assert!(
            matches!(error, GoalError::Malformed { line: 3, .. }),
            "错误要指向第 3 行：{error}"
        );
    }
}

#[test]
fn an_entry_without_content_is_rejected() {
    let text = "# g\n\n- 01 \n";
    let error = goals::Manifest::parse(Path::new("g.md"), text).unwrap_err();
    assert!(error.to_string().contains("第 3 行"));
    assert!(error.to_string().contains("没有内容"));
}

#[test]
fn a_file_without_a_heading_is_rejected_rather_than_read_as_empty() {
    let text = "- 01 有内容，却没有标题\n";
    let error = goals::Manifest::parse(Path::new("g.md"), text).unwrap_err();
    assert!(error.to_string().contains("标题"), "{error}");

    let error = goals::Manifest::parse(Path::new("g.md"), "").unwrap_err();
    assert!(error.to_string().contains("标题"), "{error}");
}

#[test]
fn a_name_must_be_one_unbroken_word_without_a_path_separator() {
    for name in ["", "两个 词", "a/b", "a\\b", ".", ".."] {
        assert!(!goals::is_valid_name(name), "`{name}` 不该被当成目标名");
        assert!(matches!(
            goals::validate_name(name),
            Err(GoalError::InvalidName { .. })
        ));
    }
    assert!(goals::is_valid_name("grep-tool"));
    assert!(goals::is_valid_name("目标一"));
}

#[test]
fn creating_a_goal_writes_the_manifest_and_refuses_to_overwrite_it_silently() {
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(SANDBOX);

    let sandbox = [source.to_path_buf()];
    let (manifest, path) = goals::create("sandbox", &sandbox, dir.path(), false).unwrap();
    assert_eq!(manifest.entries.len(), 5);
    assert_eq!(path, dir.path().join("sandbox.md"));
    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written, manifest.render());
    assert!(goals::exists(dir.path(), "sandbox"));

    // 重名一律拒绝：默默覆盖会抹掉一份已生成的目标定义。
    let error = goals::create("sandbox", &sandbox, dir.path(), false).unwrap_err();
    assert!(matches!(error, GoalError::AlreadyExists { .. }), "{error}");
    assert!(error.to_string().contains("--force"), "报错要说怎么覆盖");

    // 加 --force 才覆盖。
    let (again, _) = goals::create("sandbox", &sandbox, dir.path(), true).unwrap();
    assert_eq!(again, manifest);
}

#[test]
fn a_source_that_is_missing_or_has_no_tickets_is_refused_with_its_own_message() {
    let dir = tempfile::tempdir().unwrap();

    let missing = goals::generate("goal", &dir.path().join("nope")).unwrap_err();
    assert!(
        matches!(missing, GoalError::SourceNotFound { .. }),
        "{missing}"
    );

    let empty = dir.path().join("feature");
    std::fs::create_dir_all(&empty).unwrap();
    let no_tickets = goals::generate("goal", &empty).unwrap_err();
    assert!(
        matches!(no_tickets, GoalError::NoTickets { .. }),
        "{no_tickets}"
    );

    // 有一个 `issues/`，但里面一张票都没有 —— 与「来源根本不存在」是两种不同的话。
    std::fs::create_dir_all(empty.join("issues")).unwrap();
    std::fs::write(empty.join("issues/readme.txt"), "不是票\n").unwrap();
    let no_tickets = goals::generate("goal", &empty).unwrap_err();
    assert!(
        matches!(no_tickets, GoalError::NoTickets { .. }),
        "{no_tickets}"
    );
}

#[test]
fn several_sources_become_one_manifest_with_fresh_ids_and_a_source_on_every_entry() {
    // 一个目标装三个 feature：11 条票、id 按顺序重排（票号在来源之间必然撞号），而每条内容前面
    // 带着 `<来源标签>/<票号>` —— 模型读到标题之后要能直接找到那张票。
    let sources = [
        std::path::PathBuf::from(".scratch/usage-stats-format"),
        std::path::PathBuf::from(".scratch/terminal-title"),
        std::path::PathBuf::from(".scratch/exit-gesture"),
    ];
    let manifest = goals::generate_from("three-seeds", &sources).unwrap();

    assert_eq!(manifest.name, "three-seeds");
    assert_eq!(manifest.entries.len(), 11, "3 + 3 + 5 张票");
    let ids: Vec<String> = manifest
        .entries
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    assert_eq!(
        ids,
        (1..=11)
            .map(|number| format!("{number:02}"))
            .collect::<Vec<_>>(),
        "多个来源时 id 按最终顺序分配"
    );
    assert!(
        manifest.entries[0]
            .content
            .starts_with("usage-stats-format/01 "),
        "{:?}",
        manifest.entries[0]
    );
    assert!(
        manifest.entries[3]
            .content
            .starts_with("terminal-title/01 "),
        "第二个来源从第 4 条接着排：{:?}",
        manifest.entries[3]
    );
    assert!(
        manifest.entries[6].content.starts_with("exit-gesture/01 "),
        "第三个来源从第 7 条接着排：{:?}",
        manifest.entries[6]
    );

    // 每一条的凭证都真的点到一张存在的票 —— 这才是「模型找得到」。
    for entry in &manifest.entries {
        let (ticket, _) = entry
            .content
            .split_once(' ')
            .expect("`<标签>/<票号> <标题>`");
        let (feature, number) = ticket.split_once('/').expect("`<标签>/<票号>`");
        let dir = std::path::Path::new(".scratch")
            .join(feature)
            .join("issues");
        let prefix = format!("{number}-");
        assert!(
            std::fs::read_dir(&dir).unwrap().any(|file| file
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(&prefix)),
            "清单第 {} 条点不到票：{ticket}",
            entry.id
        );
    }

    // 渲染之后读得回来。
    let text = manifest.render();
    assert_eq!(
        goals::Manifest::parse(std::path::Path::new("three-seeds.md"), &text).unwrap(),
        manifest
    );
}

#[test]
fn several_sources_are_checked_one_by_one() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();

    // 一个来源都没给。
    let error = goals::generate_from("goal", &[]).unwrap_err();
    assert!(matches!(error, GoalError::NoSource), "{error}");

    // 其中一个来源不存在、或者一张票都没有：指名道姓地拒掉，不是把其余的悄悄收下。
    let missing = [std::path::PathBuf::from(SANDBOX), dir.path().join("nope")];
    let error = goals::generate_from("goal", &missing).unwrap_err();
    assert!(matches!(error, GoalError::SourceNotFound { .. }), "{error}");

    let no_tickets = [std::path::PathBuf::from(SANDBOX), empty];
    let error = goals::generate_from("goal", &no_tickets).unwrap_err();
    assert!(matches!(error, GoalError::NoTickets { .. }), "{error}");
    assert!(error.to_string().contains("empty"), "{error}");
}

#[test]
fn loading_a_manifest_goes_through_the_name_and_reports_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    goals::create(
        "sandbox",
        &[std::path::PathBuf::from(SANDBOX)],
        dir.path(),
        false,
    )
    .unwrap();

    let manifest = goals::load(dir.path(), "sandbox").unwrap();
    assert_eq!(manifest.entries.len(), 5);

    let missing = goals::load(dir.path(), "nothing").unwrap_err();
    assert!(matches!(missing, GoalError::Read { .. }), "{missing}");
}
