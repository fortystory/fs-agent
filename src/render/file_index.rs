//! 会话级的文件索引：工作区里有哪些文件与目录（`input-tokens` 票 01）。
//!
//! 它是**一个值加一个查询接口**，不是渲染器私有的缓存：`Idle | Loading | Ready` 三态由
//! 渲染循环推着走，而遍历本身（[`scan`]）是一个同步函数，跑在 `spawn_blocking` 里。索引
//! 从不阻塞键盘，也从不进任何一次按键的处理路径。
//!
//! 忽略规则**照搬** `grep` 工具（`src/tools/grep.rs` 的 `search()`）：遵守 `.gitignore`、
//! 非 git 仓库也能走、条目按路径排序。仓库里只该有一个「什么算工作区文件」的答案 —— 否则
//! `grep` 找不到的文件却能在补全里选出来，而把 `.env` 一类列进候选是实打实的暴露面。
//!
//! **与 `grep` 有意分家的一处**（2026-10-05，来自真机反馈「`@.scratch/` 选不到」）：
//! `grep` 跳过整棵隐藏子树，而这里放行隐藏**目录**、只挡隐藏**文件**与 `.git/`。理由：
//! `.scratch/`、`.github/` 这样的目录是工作区里正常的材料，`@` 选不到它们等于让人手抄路径；
//! 而那条暴露面说的是**文件**（`.env` 的内容），目录名本身不泄露什么。分家的代价是
//! 「`grep` 搜不到的路径却能在补全里选出来」—— 这一点如实记在 `docs/render.md` 里。
//!
//! 目录也进索引，并以**尾随斜杠**的形式列出来（`src/`）：那既是菜单里看得见的形状，也是
//! `@` 插进草稿的文本，于是「选一个目录接着往下打」不必再看文件系统第二眼。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::WalkBuilder;

/// 索引此刻处在哪一态。
#[derive(Debug, Default)]
pub enum FileIndex {
    /// 还没扫过。
    #[default]
    Idle,
    /// 一次遍历正在跑（结果还没回来）。
    Loading,
    /// 扫完了：相对会话 `cwd` 的路径，文件不带尾斜杠、目录带一个。
    Ready(Arc<Vec<PathBuf>>),
}

impl FileIndex {
    pub fn new() -> Self {
        Self::Idle
    }

    /// 该不该现在发一次遍历：不在跑就置位并回答 `true`，已经在跑就回答 `false`。
    ///
    /// 这就是「预热一次、提交后再扫一次」共用的那个开关 —— 它保证结果永远不会被两次并发
    /// 的遍历互相盖掉。
    pub fn begin(&mut self) -> bool {
        match self {
            Self::Loading => false,
            _ => {
                *self = Self::Loading;
                true
            }
        }
    }

    /// 一次遍历的结果回来了。顺序就是遍历给的顺序（与 `grep` 工具同一条）。
    pub fn loaded(&mut self, paths: Vec<PathBuf>) {
        *self = Self::Ready(Arc::new(paths));
    }

    /// 按 `query` 过滤的候选，大小写不敏感，最多 `limit` 条。
    ///
    /// 匹配是**分段**的（`.scratch/tui-feedback/spec.md` §4）：query 与路径都按 `/` 切段、空段
    /// 丢掉（`src/` 与 `src` 因此等价），query 的每一段按前缀落在路径的某一段上、段序保持，
    /// 而**第一段可以从路径的任意一段起** —— 于是 `cli` 配得到 `src/cli.rs`，`src/cli` 与
    /// `render/tui` 也各配得到它们的那一份。匹配起点更靠前段者排在前面，同分保持索引里的
    /// 路径序，所以 `src/cli.rs`（起点 1）排在 `vendor/x/cli-tool`（起点 2）之前。
    ///
    /// 返回的是索引里那个拼法（`/Ask` 找得到 `ask-matt`，而按 `Tab` 写出的是循环会认的
    /// 那个名字）。含空白的路径一律不进候选：`@` 的记号按空白结束，插进去会当场断掉。
    /// 索引未就绪时是空的 —— 那是一段极短的时间，预热让它几乎不可能被看见。
    pub fn candidates(&self, query: &str, limit: usize) -> Vec<String> {
        let Self::Ready(paths) = self else {
            return Vec::new();
        };
        let mut matched: Vec<(usize, String)> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .filter(|text| !text.contains(char::is_whitespace))
            .filter_map(|text| match_start(&text, query).map(|start| (start, text)))
            .collect();
        // 稳定排序：同分那些保持索引里的路径序（`scan` 已经按路径排过）。
        matched.sort_by_key(|(start, _)| *start);
        matched
            .into_iter()
            .take(limit)
            .map(|(_, text)| text)
            .collect()
    }

    /// 「能兑现」的判据：这个相对路径在索引里**精确**存在。
    ///
    /// 上色与 chip 共用这一条（票 03），所以它得有一个地方被问到。线性扫描足够：文件是
    /// 几千条，而这只在一次按键时发生，不在每一帧。
    pub fn contains(&self, path: &str) -> bool {
        let Self::Ready(paths) = self else {
            return false;
        };
        paths
            .iter()
            .any(|candidate| candidate.to_string_lossy() == path)
    }
}

/// `query` 按段落在 `path` 上的匹配起点：第一段匹配到的那个段号，不匹配是 `None`。
///
/// 段按 `/` 切、空段丢掉，每段按前缀匹配（大小写不敏感），且后一段必须落在前一段**之后**。
/// query 一段都没有时（空串、光一个 `/`）算作匹配、起点 0 —— 那是「还没打字」的样子，`@` 的
/// 菜单在那时本来就不开，但这个函数不必为此编一个假的判据。
fn match_start(path: &str, query: &str) -> Option<usize> {
    let segments = |text: &str| -> Vec<String> {
        text.split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let needle = segments(query);
    if needle.is_empty() {
        return Some(0);
    }
    let hay = segments(path);
    let start = hay
        .iter()
        .position(|segment| segment.starts_with(&needle[0]))?;
    let mut cursor = start;
    for segment in &needle[1..] {
        let rest = &hay[cursor + 1..];
        let offset = rest
            .iter()
            .position(|candidate| candidate.starts_with(segment.as_str()))?;
        cursor += 1 + offset;
    }
    Some(start)
}

/// 走一遍 `root`，把工作区里的文件与目录列出来，路径相对 `root`。
///
/// 这是同步的：调用方负责把它放进 `tokio::task::spawn_blocking`，好让一次大遍历永远不
/// 挡住渲染循环。走不进去的目录（权限、竞态）与读不了的条目直接跳过 —— 与 `grep` 同一条
/// 处理，一次不完整的遍历不该是整个索引的失败。
pub fn scan(root: &Path) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(root);
    builder.sort_by_file_path(|left, right| left.cmp(right));
    // `ignore` 的缺省是把隐藏条目整个跳过。那个缺省要挡的是 `.env` 一类的**文件**（内容
    // 泄露），但 `.scratch/`、`.github/` 这样的**目录**是工作区里正常的材料 —— 所以这里
    // 放开隐藏，再用 [`visible_entry`] 把隐藏文件与 `.git/` 挡回去。
    builder.hidden(false);
    builder.filter_entry(visible_entry);
    let mut paths = Vec::new();
    for entry in builder.build() {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        // 遍历的第一个条目是根自己；它不是工作区里的一个候选。
        if path == root {
            continue;
        }
        let Some(kind) = entry.file_type() else {
            continue;
        };
        let relative = path.strip_prefix(root).unwrap_or(path);
        if kind.is_dir() {
            paths.push(PathBuf::from(format!("{}/", relative.display())));
        } else if kind.is_file() {
            // 符号链接不跟随（`follow_links` 的缺省），于是它与 `grep` 一样不会被搜到，
            // 也不会被补全出来。
            paths.push(relative.to_path_buf());
        }
    }
    paths
}

/// 放开隐藏之后，这一条重新挡回**不进候选**的东西。
///
/// - `.git/`：版本库的内部结构，列出来没有用处，而且很大；
/// - 隐藏**文件**（`.env`、`.gitignore`、`.hidden` …）：那正是「跳过隐藏」要防的暴露面；
/// - 其余一切（含隐藏**目录**）放行，于是 `@.scratch/` 能一路钻下去。
///
/// 它是 `filter_entry` 的剪枝判据：对一个目录返回 `false` 会连带它的整棵子树一起剪掉。
fn visible_entry(entry: &ignore::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    if !name.starts_with('.') {
        return true;
    }
    entry.file_type().is_some_and(|kind| kind.is_dir()) && name != ".git"
}
