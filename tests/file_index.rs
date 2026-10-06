//! 会话级文件索引：一次遍历加上一个查询接口（`input-tokens` 票 01）。
//!
//! 接缝是 [`heng::render::file_index`] 的公开 API。索引刻意只是一个**值**
//! （`Idle | Loading | Ready`），遍历与查询分开，于是「什么算工作区文件」这条规则
//! 不用终端、也不用真键盘就能钉死 —— 它就是 `grep` 工具那条规则的第二份使用者。

use std::fs;
use std::path::PathBuf;

use heng::render::file_index::{self, FileIndex};
use tempfile::TempDir;

/// 一个工作区：普通文件、子目录、空目录、藏起来的文件，以及一条 `.gitignore`。
///
/// `.git` 目录是给 `ignore` 的 `require_git` 缺省值准备的：不在 git 仓库里时它不读
/// `.gitignore`，这正是 `grep` 工具上的同一条行为。
fn workspace() -> TempDir {
    let root = TempDir::new().expect("临时工作区");
    let at = root.path();
    fs::create_dir(at.join(".git")).expect(".git");
    fs::write(at.join(".gitignore"), "ignored.txt\n").expect(".gitignore");
    fs::write(at.join("a.txt"), "a").expect("a.txt");
    fs::write(at.join("b.txt"), "b").expect("b.txt");
    fs::write(at.join(".hidden"), "藏").expect(".hidden");
    fs::write(at.join("ignored.txt"), "被忽略").expect("ignored.txt");
    fs::create_dir_all(at.join("src/nested")).expect("src/nested");
    fs::write(at.join("src/main.rs"), "fn main() {}").expect("src/main.rs");
    fs::write(at.join("src/cli.rs"), "// 一个只被名字找到的文件").expect("src/cli.rs");
    fs::write(at.join("src/nested/deep.rs"), "").expect("src/nested/deep.rs");
    fs::create_dir(at.join("empty")).expect("empty");
    root
}

/// 一次遍历的结果，作为相对路径的文本 —— 断言里读起来就是候选菜单的样子。
fn names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn the_scan_keeps_hidden_and_ignored_files_out_and_fixes_the_order() {
    // 与 `grep` 工具逐字同一条规则（`src/tools/grep.rs` 的 `search()`）：遵守
    // `.gitignore`、跳过隐藏文件、非 git 仓库也能走、按路径排序。目录也进索引 ——
    // `@` 的候选要能钻下去，所以目录以**尾随斜杠**的形式列出来。
    let root = workspace();
    assert_eq!(
        names(&file_index::scan(root.path())),
        vec![
            "a.txt",
            "b.txt",
            "empty/",
            "src/",
            "src/cli.rs",
            "src/main.rs",
            "src/nested/",
            "src/nested/deep.rs",
        ],
        "隐藏文件、被忽略的文件与 .git 都不在，目录带尾随斜杠"
    );
}

#[test]
fn paths_come_back_relative_to_the_root_and_an_empty_workspace_does_not_blow_up() {
    let root = workspace();
    let paths = file_index::scan(root.path());
    assert!(
        paths.iter().all(|path| path.is_relative()),
        "全是相对会话 cwd 的路径"
    );
    assert!(
        !paths.iter().any(|path| path.starts_with(root.path())),
        "没有哪一条带着工作区的绝对前缀"
    );

    let empty = TempDir::new().expect("空工作区");
    assert!(file_index::scan(empty.path()).is_empty(), "空目录不炸");
}

#[test]
fn a_workspace_that_is_not_a_git_repository_still_scans() {
    // `require_git` 让 `.gitignore` 只在 git 仓库里生效，但遍历本身从不要求仓库 ——
    // 非 git 的工作区照样列出它的文件（`grep` 工具就是这么用的）。
    let root = TempDir::new().expect("非 git 工作区");
    fs::write(root.path().join("plain.txt"), "").expect("plain.txt");
    fs::create_dir(root.path().join("dir")).expect("dir");
    fs::write(root.path().join("dir/inner.txt"), "").expect("dir/inner.txt");
    assert_eq!(
        names(&file_index::scan(root.path())),
        vec!["dir/", "dir/inner.txt", "plain.txt"]
    );
}

#[test]
fn a_scan_that_is_already_running_is_not_started_twice() {
    // `Loading` 就是那个守卫：它让「提交后重扫」与还没回来的预热遍历不会各起一份、
    // 再把互相的结果盖掉。
    let mut index = FileIndex::new();
    assert!(index.begin(), "第一次开始遍历");
    assert!(!index.begin(), "还在跑就不重复发");
    index.loaded(vec![PathBuf::from("a.txt")]);
    assert!(
        index.begin(),
        "结果回来之后还可以再扫一次 —— 那正是提交后的重扫"
    );
}

#[test]
fn candidates_filter_by_prefix_without_regard_to_case_and_stop_at_the_limit() {
    let mut index = FileIndex::new();
    assert!(
        index.candidates("a", 10).is_empty(),
        "索引未就绪时什么都不显示"
    );
    index.loaded(vec![
        PathBuf::from("README.md"),
        PathBuf::from("src/"),
        PathBuf::from("src/main.rs"),
    ]);
    // 大小写不敏感：打 `r` 找得到 `README.md`。返回的是索引里那个拼法。
    assert_eq!(index.candidates("r", 10), vec!["README.md"]);
    assert_eq!(index.candidates("SRC/", 10), vec!["src/", "src/main.rs"]);
    assert_eq!(index.candidates("s", 1), vec!["src/"], "行数上限先收一刀");
    assert!(index.candidates("zzz", 10).is_empty(), "没指到就是空");
}

#[test]
fn candidates_match_a_path_segment_from_any_depth() {
    // 打 `cli` 要配得到 `src/cli.rs`：判据从整条路径的前缀换成**分段前缀、可从任意一段起**
    // （`.scratch/tui-feedback/spec.md` §4）。候选的用处正是「我不记得它在哪一层」。
    let root = workspace();
    let mut index = FileIndex::new();
    index.loaded(file_index::scan(root.path()));

    assert_eq!(
        index.candidates("cli", 5),
        vec!["src/cli.rs"],
        "从第二段起也命中"
    );
    assert_eq!(
        index.candidates("nested/deep", 5),
        vec!["src/nested/deep.rs"],
        "跨段，段序保持"
    );
    assert_eq!(
        index.candidates("CLI", 5),
        vec!["src/cli.rs"],
        "大小写不敏感"
    );
    // 前缀那一条不退化：`src` 与 `src/` 同一串，而且 `src/` 自己排在最前。
    let plain = index.candidates("src", 10);
    assert_eq!(plain, index.candidates("src/", 10));
    assert_eq!(plain.first().map(String::as_str), Some("src/"));
    assert!(plain.contains(&"src/main.rs".to_owned()), "{plain:?}");
}

#[test]
fn candidates_rank_the_earlier_matching_segment_first() {
    // 匹配起点更靠前的排在前面（`src/cli.rs` 的起点是第 1 段，`vendor/x/cli-tool` 的是第 2
    // 段）；同分的保持索引里的路径序。
    let mut index = FileIndex::new();
    index.loaded(vec![
        PathBuf::from("vendor/x/cli-tool"),
        PathBuf::from("docs/cli.md"),
        PathBuf::from("src/cli.rs"),
    ]);
    assert_eq!(
        index.candidates("cli", 10),
        vec!["docs/cli.md", "src/cli.rs", "vendor/x/cli-tool"],
        "起点 1 的两条保持索引顺序在前，起点 2 的在后"
    );
}

#[test]
fn a_path_with_a_space_never_becomes_a_candidate() {
    // `@` 的记号按空白结束，插进去会当场断掉，所以这类路径干脆不进候选
    // （spec §3 明写的那条边界）。
    let mut index = FileIndex::new();
    index.loaded(vec![
        PathBuf::from("my file.txt"),
        PathBuf::from("plain.txt"),
    ]);
    assert_eq!(index.candidates("m", 10), Vec::<String>::new());
    assert_eq!(index.candidates("p", 10), vec!["plain.txt"]);
}

#[test]
fn only_a_path_that_is_in_the_index_can_be_honoured() {
    // 「能兑现」这条判据是上色与 chip 共用的那一条（票 03），所以它得有地方问。
    let mut index = FileIndex::new();
    index.loaded(vec![PathBuf::from("src/main.rs"), PathBuf::from("src/")]);
    assert!(index.contains("src/main.rs"));
    assert!(index.contains("src/"), "目录也算兑现得了");
    assert!(!index.contains("src/reneder/tui.rs"), "拼错的就是兑现不了");
    assert!(!index.contains("src"));
}

#[test]
fn a_dot_directory_can_be_reached_with_at_but_dot_files_still_cannot() {
    // 真机反馈：`@.scratch/` 选不到。`@` 的候选来自这份索引，而 `ignore` 的缺省把隐藏条目
    // 整个跳过 —— 修法是**放行隐藏目录、仍挡隐藏文件与 `.git/`**：`.scratch/` 这类工作区
    // 材料要能一路钻下去，而 `.env` 一类的名字不进候选。
    let root = TempDir::new().expect("临时工作区");
    fs::create_dir(root.path().join(".scratch")).expect(".scratch");
    fs::write(root.path().join(".scratch/ticket.md"), "").expect("票");
    fs::create_dir(root.path().join(".git")).expect(".git");
    fs::write(root.path().join(".git/HEAD"), "").expect("HEAD");
    fs::write(root.path().join(".env"), "SECRET=1").expect(".env");
    fs::write(root.path().join("README.md"), "").expect("README");

    let mut index = FileIndex::new();
    index.loaded(file_index::scan(root.path()));

    // 隐藏目录进候选（前缀匹配按字符串来，所以钻进去之后那一层也一起匹配 —— 与
    // `@src` 同时列出 `src/` 和 `src/main.rs` 是同一条规则）。
    let dotted = index.candidates(".", 10);
    assert!(
        dotted.contains(&".scratch/".to_owned()),
        "点目录进候选：{dotted:?}"
    );
    assert_eq!(
        index.candidates(".scratch/", 10),
        vec![".scratch/", ".scratch/ticket.md"],
        "钻进去之后那一层也看得见"
    );

    // `.git/` 是版本库的内部结构，照旧不列（整棵子树一起剪掉）。
    assert!(
        index
            .candidates(".", 10)
            .iter()
            .all(|path| !path.starts_with(".git")),
        "`.git/` 不进候选"
    );
    // 隐藏**文件**仍不进候选 —— 那正是「跳过隐藏」要防的暴露面。
    assert_eq!(index.candidates(".e", 10), Vec::<String>::new());
    assert!(!index.contains(".env"), "`.env` 兑现不了，也就上不了色");
}
