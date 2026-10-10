//! 会话工作区里那一遍文件遍历：`grep` 与 `glob` 共用的那半。
//!
//! 两条只读工具都要「走一遍会话 cwd，遵守 `.gitignore`，跳过隐藏文件，按路径排好序」
//! （`.scratch/grep-tool/spec.md` §2、`.scratch/tool-coverage` §4）。规则只有一份、也**必须**
//! 只有一份：同一张工具表里的两个工具看见的世界如果不一样，模型就得先弄清「为什么这条找得到
//! 那条找不到」才敢信任何一条。
//!
//! 遍历器在这里，**过滤**留给各自的工具：`grep` 按 `glob` 收窄搜哪些文件，`glob` 按模式选
//! 要枚举哪些文件。两条路都在遍历之后过滤，于是没有任何一条能放宽忽略规则。

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

/// 走一遍工作区里的一个文件：绝对路径与相对会话 cwd 的那一份。
///
/// 相对的那一份是给模型看的（输出形状），绝对的那一份是给 [`std::fs`] 与搜索器用的。
#[derive(Debug, Clone)]
pub struct File {
    pub path: PathBuf,
    pub relative: PathBuf,
}

/// 按路径排序的文件遍历。
///
/// 忽略规则就是 `ignore` 的默认：遵守 `.gitignore`（含 `.ignore` 与 git 的全局忽略）、跳过隐藏
/// 文件与隐藏目录 —— 与 `rg` 的默认一致，所以换工具不改变看见的世界。注意 `ignore` 的
/// `require_git` 默认开着：**不在 git 仓库里的目录，`.gitignore` 不生效**，这一点也与 `rg` 一致。
///
/// 条目按路径排序，于是同一个工作区上的输出稳定，模型与测试都少一件要猜的事。
///
/// 走不进去的目录与读不了的文件**不是失败**，跳过就行 —— 一次搜索不该因为一个竞态而整个失败。
pub fn files(root: &Path) -> impl Iterator<Item = File> {
    let mut builder = WalkBuilder::new(root);
    builder.sort_by_file_path(|left, right| left.cmp(right));
    builder.build().filter_map(move |entry| {
        let entry = entry.ok()?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            return None;
        }
        let path = entry.into_path();
        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        Some(File { path, relative })
    })
}
