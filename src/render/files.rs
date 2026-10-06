//! 文件页那棵树的排版：把会话级索引那份平坦的路径列表排成一层层的行
//! （`.scratch/files-page/spec.md` §1、§3）。
//!
//! 索引（[`super::file_index`]）列的是工作区里的文件与目录，**目录带尾斜杠**、按路径排序
//! —— 那是 `@` 补全与 `grep` 共用的契约，一个字不改。这一页只**消费**它：把路径按 `/`
//! 切段拼回一棵树，只列出「祖先目录都展开着」的那些行，并把同一层里的目录排到文件前面。
//!
//! **同层排序是这次唯一一处与索引顺序有意分家**：`@` 候选与 `grep` 都按索引序，而树要目录
//! 在前。两处都写在文档里，免得后来者以为是漏改。
//!
//! 这里全是纯函数：展开状态由调用方（`TuiState`）拿着，遍历规则、重绘与命中都不在这一层。

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// 一个文件内容弹窗最多读多少字节。
///
/// 这三个上限是渲染层自己写的：权限门约束的是**模型**的读写，而这是人在自己的终端里点开
/// 自己的文件 —— 与 `@` 补全列出路径同一性质，所以读盘不经过那道门，限额也就没有现成件
/// （`.scratch/files-page/spec.md` §6）。
pub const MAX_BYTES: usize = 200_000;

/// 一个文件内容弹窗最多读多少行。行数封顶之后，字节上限通常已经先拦住了 ——
/// 两笔账各自独立，因为一行可以极长。
pub const MAX_LINES: usize = 2_000;

/// 一个文件内容弹窗里，**一行**最多读多少字符。
///
/// 字节与行数那两档管的是整个文件，这一档管的是「一行特别长」：正文按行画、超宽的行折成好几
/// 条显示行，所以一行的长度也要有它自己的上限（`.scratch/files-page/spec.md` §6 的三档限额）。
pub const MAX_LINE_CHARS: usize = 4_000;

/// 判定二进制时看开头多少字节：这范围内没有 NUL 就不当它是二进制。
///
/// 与 `grep` 工具那条 `BinaryDetection::quit(0)` 同一条判据（见到 NUL 就走开）。
const SNIFF_BYTES: usize = 8 * 1024;

/// 按扩展名认语言，交给 [`super::highlight`] 那一层去上色。
///
/// 名字取 `canonical_language` 认的那一份（`rs` 与 `rust` 是同一份文法这件事归它管），所以
/// 这里只回答「后缀是什么语言」，不回答「怎么画」。名单之外一律 `None`：认不出就按纯文本
/// 画，不报错、也不画错色（`.scratch/files-page/spec.md` §6）。
pub fn language_for(path: &str) -> Option<&'static str> {
    // 目录带尾斜杠，先摘掉；`Makefile` 一类没有后缀的路径自然落在 `None` 那一支。
    let name = path.strip_suffix('/').unwrap_or(path);
    let (_, extension) = name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "rs" => Some("rust"),
        "sh" | "bash" => Some("bash"),
        "json" => Some("json"),
        "toml" => Some("toml"),
        "html" | "htm" => Some("html"),
        "js" | "mjs" | "cjs" => Some("javascript"),
        "ts" | "mts" | "cts" => Some("typescript"),
        "php" => Some("php"),
        "sql" => Some("sql"),
        "py" => Some("python"),
        _ => None,
    }
}

/// 弹窗里那个文件读出来是什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileBody {
    /// 读到了文本；`truncated` 说三个上限里的字节或行数截过它 —— 屏幕上要把这件事说出来，
    /// 而不是悄悄少一段。
    Text { text: String, truncated: bool },
    /// 二进制：只该报一句，不往屏幕上泼乱码。
    Binary,
    /// 读不了：不存在、没有权限、或者根本不是能读的东西。
    Unreadable,
    /// 读到的那些字节不是 UTF-8 文本（而且没有 NUL）。
    NotText,
}

/// 按工作区相对路径读一个文件，有界截断。
///
/// 同步读盘，在打开弹窗的那一刻调用一次 —— 与轨迹页的详情同一个形状：此后滚动是纯算术，
/// 不再碰磁盘（`.scratch/files-page/spec.md` §6）。
pub fn read(root: &Path, path: &str) -> FileBody {
    let full = root.join(path.strip_suffix('/').unwrap_or(path));
    let Ok(file) = std::fs::File::open(&full) else {
        return FileBody::Unreadable;
    };
    // 多读一个字节：它回来就说明还有更多，而读满上限的文件不必再问第二次。
    let mut bytes = Vec::new();
    if file
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return FileBody::Unreadable;
    }
    if bytes[..bytes.len().min(SNIFF_BYTES)].contains(&0) {
        return FileBody::Binary;
    }
    let mut truncated = bytes.len() > MAX_BYTES;
    if truncated {
        // 退到字符边界：截断不该在末尾留下半个字符，那会把它读成「不是文本」。
        let mut cut = MAX_BYTES;
        while cut > 0 && (bytes[cut] & 0xC0) == 0x80 {
            cut -= 1;
        }
        bytes.truncate(cut);
    }
    let Ok(mut text) = String::from_utf8(bytes) else {
        return FileBody::NotText;
    };
    if text
        .split('\n')
        .any(|line| line.chars().count() > MAX_LINE_CHARS)
    {
        text = text
            .split('\n')
            .map(|line| {
                if line.chars().count() > MAX_LINE_CHARS {
                    line.chars().take(MAX_LINE_CHARS).collect()
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        truncated = true;
    }
    if text.split('\n').count() > MAX_LINES {
        // 截到第 `MAX_LINES` 行末尾那一个换行之后。
        let cut = text
            .match_indices('\n')
            .nth(MAX_LINES - 1)
            .map(|(at, _)| at + 1)
            .unwrap_or(text.len());
        text.truncate(cut);
        truncated = true;
    }
    FileBody::Text { text, truncated }
}

/// 文件页上的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 索引里的那个拼法（目录带尾斜杠）。`@` 记号和弹窗读盘都用它。
    pub path: String,
    /// 显示的名字：路径的最后一段，目录不带那个尾斜杠。
    pub name: String,
    /// 缩进几层。顶层是 0，每往下一层多一档。
    pub depth: usize,
    /// 这一行是不是目录。
    pub dir: bool,
    /// 目录是不是展开着；文件恒为假。
    pub expanded: bool,
}

/// 一屏看得见的那些行：按树的顺序，收起的目录里什么都不出现。
///
/// `expanded` 装的是展开着的目录在索引里的拼法（`src/`）—— 与 [`Row::path`] 同一套写法，
/// 所以两处不必做任何转换。
pub fn rows(paths: &[PathBuf], expanded: &HashSet<String>) -> Vec<Row> {
    // 每个目录挂着它的直接子项，**按索引给的顺序**：顶层挂在空串下面。
    let mut buckets: HashMap<String, Vec<Row>> = HashMap::new();
    for path in paths {
        let path = path.to_string_lossy().into_owned();
        let (parent, name) = split(&path);
        let dir = path.ends_with('/');
        buckets.entry(parent).or_default().push(Row {
            name: name.to_owned(),
            depth: 0,
            dir,
            expanded: dir && expanded.contains(&path),
            path,
        });
    }
    // 同层里目录在前，同类之内保持索引序（稳定排序）。
    for bucket in buckets.values_mut() {
        bucket.sort_by_key(|row| !row.dir);
    }
    let mut out = Vec::new();
    walk(&buckets, "", 0, &mut out);
    out
}

/// 摊开一层，再摊开那些展开着的目录。
fn walk(buckets: &HashMap<String, Vec<Row>>, parent: &str, depth: usize, out: &mut Vec<Row>) {
    let Some(children) = buckets.get(parent) else {
        return;
    };
    for child in children {
        let mut row = child.clone();
        row.depth = depth;
        let descend = row.expanded;
        let key = row.path.clone();
        out.push(row);
        if descend {
            walk(buckets, &key, depth + 1, out);
        }
    }
}

/// 一条索引路径的（父目录，名字）。
///
/// 父目录按索引里的拼法写（`src/` 这样带尾斜杠），顶层那一层是空串；于是「谁的子项」这个
/// 问题在两边是同一个字符串，不需要第二套键。目录自己那个名字**不带**尾斜杠 —— 那是显示
/// 的事，路径照旧带着它。
fn split(path: &str) -> (String, &str) {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    match trimmed.rfind('/') {
        Some(at) => (format!("{}/", &trimmed[..at]), &trimmed[at + 1..]),
        None => (String::new(), trimmed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn expanded(list: &[&str]) -> HashSet<String> {
        list.iter().map(|path| (*path).to_owned()).collect()
    }

    /// 树按屏幕上读它的样子：缩进加路径。
    fn shown(tree: &[Row]) -> Vec<String> {
        tree.iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), row.path))
            .collect()
    }

    #[test]
    fn directories_come_first_and_the_index_order_holds_inside_each_kind() {
        // 索引序是 `README.md` < `src/` < `z.rs`，而树要目录在前。
        let tree = rows(
            &paths(&["README.md", "src/", "src/a.rs", "z.rs"]),
            &expanded(&[]),
        );
        assert_eq!(shown(&tree), ["src/", "README.md", "z.rs"]);
    }

    #[test]
    fn a_collapsed_directory_hides_its_contents() {
        let tree = rows(
            &paths(&["src/", "src/render/", "src/render/tui.rs", "top.rs"]),
            &expanded(&[]),
        );
        assert_eq!(shown(&tree), ["src/", "top.rs"]);
    }

    #[test]
    fn expanding_a_directory_lays_its_children_one_level_deeper() {
        let tree = rows(
            &paths(&["src/", "src/a.rs", "src/render/", "src/render/tui.rs"]),
            &expanded(&["src/", "src/render/"]),
        );
        // `src/` 下面同样是目录在前：`render/` 排在 `a.rs` 之前。
        assert_eq!(
            shown(&tree),
            [
                "src/",
                "  src/render/",
                "    src/render/tui.rs",
                "  src/a.rs"
            ]
        );
    }

    #[test]
    fn a_directory_that_left_the_index_takes_its_expansion_with_it() {
        // 一次重扫之后不在索引里的路径不出现，展开状态也在那一层自然失效。
        let tree = rows(&paths(&["src/", "src/a.rs"]), &expanded(&["src/", "gone/"]));
        assert_eq!(shown(&tree), ["src/", "  src/a.rs"]);
    }

    #[test]
    fn names_are_the_last_segment_and_directories_lose_their_slash() {
        let tree = rows(
            &paths(&["src/", "src/render/", "src/render/tui.rs"]),
            &expanded(&[]),
        );
        assert_eq!(tree.len(), 1, "子项还在收起着的目录里");
        assert_eq!(tree[0].name, "src");
        assert_eq!(tree[0].path, "src/");
        assert!(tree[0].dir);
    }

    #[test]
    fn the_extension_picks_the_language_the_highlighter_knows() {
        assert_eq!(language_for("src/render/tui.rs"), Some("rust"));
        assert_eq!(language_for("Cargo.toml"), Some("toml"));
        assert_eq!(language_for("scripts/run.sh"), Some("bash"));
        assert_eq!(language_for("src/cli.PY"), Some("python"), "后缀不分大小写");
        // 认不出的一律退纯文本：没有后缀的、名单外的、以及一个藏在目录名里的点。
        assert_eq!(language_for("Makefile"), None);
        assert_eq!(language_for("notes.yaml"), None);
        assert_eq!(language_for(".gitignore"), None);
        assert_eq!(language_for("a.dir/name"), None);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("heng-files-read-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录");
        dir
    }

    #[test]
    fn a_plain_file_comes_back_as_text() {
        let dir = scratch("plain");
        std::fs::write(dir.join("a.txt"), "hello\nworld\n").expect("写一个文件");
        assert_eq!(
            read(&dir, "a.txt"),
            FileBody::Text {
                text: "hello\nworld\n".to_owned(),
                truncated: false
            }
        );
    }

    #[test]
    fn nul_bytes_make_it_binary_and_missing_files_are_unreadable() {
        let dir = scratch("binary");
        std::fs::write(dir.join("blob"), [b'a', 0u8, b'b']).expect("写一个二进制");
        assert_eq!(read(&dir, "blob"), FileBody::Binary);
        assert_eq!(read(&dir, "gone"), FileBody::Unreadable);
    }

    #[test]
    fn bytes_that_are_not_utf8_are_not_text() {
        let dir = scratch("not-text");
        // 一个不完整的序列，而且没有 NUL —— 它既不是 UTF-8，也不是二进制。
        std::fs::write(dir.join("latin"), [0xE4, 0xBD, 0x20]).expect("写一段坏字节");
        assert_eq!(read(&dir, "latin"), FileBody::NotText);
    }

    #[test]
    fn the_line_cap_truncates_and_says_so() {
        let dir = scratch("lines");
        let body: Vec<String> = (0..MAX_LINES + 10).map(|line| format!("L{line}")).collect();
        std::fs::write(dir.join("long.txt"), body.join("\n")).expect("写一个长文件");
        let FileBody::Text { text, truncated } = read(&dir, "long.txt") else {
            panic!("这是一个文本文件");
        };
        assert!(truncated, "行数封顶之后它说出来了");
        assert_eq!(text.lines().count(), MAX_LINES, "恰好留下前 {MAX_LINES} 行");
        assert!(text.starts_with("L0\n"), "从头上留");
    }

    #[test]
    fn the_line_width_cap_truncates_and_says_so() {
        let dir = scratch("line-width");
        let long = "x".repeat(MAX_LINE_CHARS + 10);
        std::fs::write(dir.join("wide.txt"), format!("ok\n{long}\n")).expect("写一条极长的行");
        let FileBody::Text { text, truncated } = read(&dir, "wide.txt") else {
            panic!("这是一个文本文件");
        };
        assert!(truncated, "行宽封顶之后它说出来了");
        let wide = text.split('\n').nth(1).expect("第二条逻辑行");
        assert_eq!(wide.chars().count(), MAX_LINE_CHARS);
        assert_eq!(text.split('\n').next(), Some("ok"), "别的行原样留着");
    }

    #[test]
    fn the_byte_cap_cuts_on_a_character_boundary() {
        let dir = scratch("bytes");
        // 每条都是两字节的字符：按字节截会正好落在字符中间。
        let body = "终".repeat(MAX_BYTES);
        std::fs::write(dir.join("wide.txt"), &body).expect("写一个大文件");
        let FileBody::Text { text, truncated } = read(&dir, "wide.txt") else {
            panic!("这是一个文本文件");
        };
        assert!(truncated, "字节封顶之后它说出来了");
        assert!(
            text.chars().all(|ch| ch == '终'),
            "截断没有留下半个字符，也没有把它读成「不是文本」"
        );
    }
}
