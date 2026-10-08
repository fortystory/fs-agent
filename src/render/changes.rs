//! 改动页的数据：跑一次只读的 git，把「相对 HEAD 改了什么」折成一行一个文件
//! （`.scratch/diff-page/spec.md` 实现决定 §2、§3）。
//!
//! 这一层只做两件事：**跑 git**（[`status`] 与 [`body`]）与**解析它的输出**（[`parse_status`]
//! 与 [`patch_rows`]）。字形、分组顺序、截断与绘制都在别处 —— 这一层不认 `wording`，也不
//! 知道屏幕上长什么样。
//!
//! 口径：**「改了」= 相对 HEAD** —— 已暂存 + 未暂存 + 未跟踪。`git diff`（工作区 vs index）
//! 那一档被否决：它看不见已暂存的，也看不见未跟踪的。
//!
//! 三条实测结论决定了这里的写法（`.scratch/diff-page/research/02-git-readouts.md`）：
//!
//! * `-z` 是路径**原样**的唯一出路（不用它就是 `core.quotePath` 说了算：引号 + 八进制）；
//! * `git status` **默认会写回 index**，所以每次都要 `--no-optional-locks`；
//! * 失败**只看退出码**，不看 stderr —— 它的文案随 locale 变。

use std::path::Path;
use std::time::Duration;

use super::files;
use super::highlight::DiffTag;
use super::hostproc::{self, Finished, Run};
use ratatui::style::{Color, Modifier, Style};

/// 一次取数的墙钟上限。
///
/// 三个读数实测是百毫秒级：真仓库（10537 个被跟踪文件）23–25 ms，最坏的合成场景（10 万个
/// 未跟踪文件 + `-uall`）约 60 ms。2 秒给现实里的慢磁盘、冷页缓存与别人抢 IO 都留了一大截，
/// 而卡住时读的人也等得起（`.scratch/diff-page/research/02-git-readouts.md` 3.6）。
pub const LIMIT: Duration = Duration::from_secs(2);

/// 跑哪一个程序。写死，不做配置：这一页要的是 git 的口径。
const GIT: &str = "git";

/// 从继承来的环境里清掉的东西。
///
/// 实测污染会改语义：`GIT_DIR` / `GIT_WORK_TREE` 会顶掉「当前目录是不是仓库」的答案，
/// `GIT_INDEX_FILE` 会让读数换成**另一套 index** 的世界。
const UNSET: &[&str] = &["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"];

/// 一次取数的结果，从渲染循环回到状态机。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 取到了：相对 HEAD 的那些改动（可能是空的 —— 工作区干净）。
    Changed(Vec<ChangedFile>),
    /// 这里不是 git 仓库（`status` 退出码 128）。
    ///
    /// 归属不一致（`safe.directory`）落在这一档：git 在这一层**连仓库都不认**。
    NotARepo,
    /// `PATH` 上找不到 `git`。与上一档分开：那是环境问题，不是「没改动」。
    NoGit,
    /// 其它失败：非零退出、超时、IO。页里保留上一次读数，提示行给一句回执。
    Failed,
}

/// 相对 HEAD 改过的一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// 相对 cwd 的路径，**原样**（不引号、不转义）：`-z` 保证的。
    pub path: String,
    pub kind: Kind,
}

/// 一个文件的改动属于哪一档。这四个就是列表上那四个字形
/// （`.scratch/diff-page/issues/04-prototype-layout.md` 作答 A.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Modified,
    Added,
    Deleted,
    Untracked,
}

impl Kind {
    /// 列表上分组的先后：**已修改 → 新增 → 已删除 → 未跟踪**，组内按路径字典序。
    ///
    /// 「已暂存 / 未暂存」不在这一页上分家（§2）：同一个文件两处都有改动时只画一个字形。
    pub const GROUPS: [Kind; 4] = [Kind::Modified, Kind::Added, Kind::Deleted, Kind::Untracked];
}

/// 列表上的一行：一个分组标题，或者一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// 分组标题（`已修改` / `新增` / `已删除` / `未跟踪`）。空组不出现。
    Header(Kind),
    File(ChangedFile),
}

/// 排成屏幕上那一列：**先按状态分组、组内按路径字典序**，空组不画（实现决定 §5）。
///
/// 排序按字节序（`String::cmp`），与文件索引那条「按路径排序」同一种口径；这一层不认识屏幕
/// 宽度，也不知道哪一行画在哪里。
pub fn rows(files: &[ChangedFile]) -> Vec<Item> {
    let mut out = Vec::new();
    for kind in Kind::GROUPS {
        let mut group: Vec<&ChangedFile> = files.iter().filter(|file| file.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by(|left, right| left.path.cmp(&right.path));
        out.push(Item::Header(kind));
        out.extend(group.into_iter().cloned().map(Item::File));
    }
    out
}

/// 跑一次 `git status`，把读数装成 [`Outcome`]。
///
/// 未跟踪的目录按 `-uall` 展开成文件，所以**一行永远是一个文件**。改名与复制在 porcelain
/// 里是 `2` 行（`R` / `C`），折字形时归进「新增」那一档 —— 列表上出现的是新名字。
pub async fn status(root: &Path) -> Outcome {
    let args = status_args();
    match hostproc::run(Run {
        program: GIT,
        args: &args,
        cwd: root,
        stdin: None,
        env: &[],
        unset: UNSET,
        limit: LIMIT,
    })
    .await
    {
        Finished::Ok(stdout) => Outcome::Changed(parse_status(&stdout)),
        // 128 是 git 在「连仓库都不认」那一层的退出码：不在仓库里、以及 dubious ownership
        // 都走它。**不靠 stderr 判定** —— 那句话随 locale 本地化。
        Finished::Exit(Some(128)) => Outcome::NotARepo,
        Finished::NotFound => Outcome::NoGit,
        Finished::Exit(_) | Finished::Timeout => Outcome::Failed,
    }
}

/// `git status` 那一次调用的 argv。
///
/// 逐条都有来路（`.scratch/diff-page/research/02-git-readouts.md` 第二节）：
///
/// * `--no-optional-locks`：**`git status` 默认会写回 index** —— 不锁就是动用户的仓库；
/// * `--no-pager`：保险（stdout 是管道时 git 本来就不分页）；
/// * `-c status.relativePaths=true`：**porcelain v2 并不与用户配置无关**（只有 v1 有那句
///   白纸黑字的保证）。我们要的正是「相对 cwd 的路径」，所以显式写上，而不是随用户配置；
/// * `-z`：路径原样、NUL 结尾。不用它就是 `core.quotePath` 说了算；
/// * `-uall`：未跟踪的目录展开成文件（命令行选项压住 `status.showUntrackedFiles`）；
/// * `--porcelain=v2`：给脚本读的形态，行类型够表达改名与未合并。
///
/// **`--no-color` 不在这里**：porcelain 本来就无色，而 `git status` 根本不接这个选项
/// （它会当场报「未知选项」）。那一行是 `git diff` 的（票 11 那条命令带着它）。
fn status_args() -> Vec<String> {
    [
        "--no-optional-locks",
        "--no-pager",
        "-c",
        "status.relativePaths=true",
        "status",
        "--porcelain=v2",
        "-z",
        "-uall",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// 把 `git status --porcelain=v2 -z` 的输出折成一行一个文件。
///
/// 五种记录（`-z` 时都以 NUL 结尾，同一行内字段之间仍是一个空格）：
///
/// ```text
/// 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
/// 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path> NUL <origPath>
/// u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
/// ? <path>
/// ! <path>
/// ```
///
/// 路径**可以含空格**，所以每一行都按**字段数**切、最后一段才是路径 —— 按空白切分会把它切碎。
/// 认不出的记录直接跳过：一次读不完整的遍历不该是整页的失败。
pub fn parse_status(output: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut fields = output.split('\0');
    while let Some(record) = fields.next() {
        let Some(tag) = record.as_bytes().first() else {
            continue;
        };
        match tag {
            b'1' => {
                if let Some((xy, path)) = fields_of(record, 9) {
                    files.push(ChangedFile {
                        path: path.to_owned(),
                        kind: kind_of(xy),
                    });
                }
            }
            b'2' => {
                let parsed = fields_of(record, 10);
                // 改名那一行的 path 与原名之间**也是 NUL**：不管这一行读不读得懂，下一格都要
                // 消费掉 —— 否则一个叫 `1foo` 的旧名字会被当成下一条记录。
                fields.next();
                if let Some((xy, path)) = parsed {
                    files.push(ChangedFile {
                        path: path.to_owned(),
                        kind: kind_of(xy),
                    });
                }
            }
            b'u' => {
                if let Some((xy, path)) = fields_of(record, 11) {
                    files.push(ChangedFile {
                        path: path.to_owned(),
                        kind: kind_of(xy),
                    });
                }
            }
            b'?' => {
                if let Some(path) = record.strip_prefix("? ") {
                    files.push(ChangedFile {
                        path: path.to_owned(),
                        kind: Kind::Untracked,
                    });
                }
            }
            // `!`（忽略项，不带 `--ignored` 时不会出现）与任何看不懂的东西都跳过。
            _ => {}
        }
    }
    files
}

/// 把一条 porcelain 记录切成 `count` 个**空格分隔**的字段，回答（`XY`，路径）。
///
/// `splitn` 保证路径里那些空格留在最后一段里；字段数对不上（畸形输出）时回答 `None`。
fn fields_of(record: &str, count: usize) -> Option<(&str, &str)> {
    let parts: Vec<&str> = record.splitn(count, ' ').collect();
    if parts.len() != count {
        return None;
    }
    let xy = *parts.get(1)?;
    let path = *parts.last()?;
    if path.is_empty() {
        return None;
    }
    Some((xy, path))
}

/// `XY` 两列**折成一个字形**：优先取**工作区**那一位，它为空时取 index 那一位。
///
/// 于是 `MM` / ` M` / `M ` 都画 `M` —— 已暂存与未暂存在这一页上不分家（§2）。porcelain 用
/// `.` 表示「这两位里空着的那一位」。
///
/// 四档之外的字母（`T` 类型变了、`U` 未合并、以及没见过的）都折进**已修改**：那是保守的一
/// 档，「与 HEAD 不同」，不假装知道更多。改名与复制折进**新增**：列表上出现的是新名字。
fn kind_of(xy: &str) -> Kind {
    let mut letters = xy.chars();
    let index = letters.next().unwrap_or('.');
    let work = letters.next().unwrap_or('.');
    let letter = if work == '.' { index } else { work };
    match letter {
        'D' => Kind::Deleted,
        'A' | 'R' | 'C' => Kind::Added,
        _ => Kind::Modified,
    }
}

/// 一份 diff 的正文（`.scratch/diff-page/spec.md` 实现决定 §6）。
///
/// 「正文点开时才取」（票 01 冻结项 8）：列文件只跑一次 `status`，点开某一个文件才为它单独
/// 跑一次 `git diff`（或者读一次盘）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// 还在读。
    ///
    /// 弹窗**先立起来**、正文后到：一次 `git diff` 是一次子进程，而处理一个按键的路径上不许等
    /// 它 —— 与那三条「一帧里不 await」同一条纪律（§3）。
    Pending,
    /// 一份补丁。
    Patch(String),
    /// 未跟踪的新文件：它**没有 diff 可比**（`git diff HEAD` 对它就是空输出），正文是全文。
    NewFile(files::FileBody),
    /// 外部工具画的那一版（`[ui] diff_viewer`，§8）：它吐的那些行已经解成了带样式的片。
    External {
        /// 画这份 diff 的是谁 —— 标题右端标出它的名字，好让读的人知道为什么两台机器上长得
        /// 不一样。
        tool: String,
        lines: Vec<Vec<Piece>>,
        /// 有界截断省掉的行数。
        skipped: usize,
    },
    /// 二进制：只报一句，不往屏幕上泼乱码。
    Binary,
    /// 读不出来：无 HEAD 的空仓库、跑不成的命令、读不了的盘。
    Unreadable,
}

/// 取一个文件的正文。
///
/// 两条路各走各的（§2）：
///
/// * **未跟踪**：直接读盘给全文 —— `git diff HEAD -- <path>` 对它**是空输出 + 退出码 0**
///   （research 1.3 实测），拿它当正文只会画出一块空白；
/// * **已跟踪**：`git diff HEAD -- <path>`。它对改名文件只给一侧时会退化成「新增」——
///   列表上那个名字正是新名字，所以两侧都读得通（research 1.4）。
///
/// 配了 `[ui] diff_viewer` 时，那份 diff 从 **stdin** 喂给那个命令，正文换成它吐的东西
/// （§8）；它没跑成时**回退内置那一档**，第二个返回值是提示行那句回执。
pub async fn body(
    root: &Path,
    file: &ChangedFile,
    viewer: &crate::config::DiffViewerSettings,
    columns: usize,
) -> (Body, Option<String>) {
    let source = raw(root, file).await;
    let Some(text) = source.text().map(str::to_owned) else {
        // 没有可以喂给谁的一行文本（二进制、读不了）：内置那一档各自有一句话。
        return (source.builtin(), None);
    };
    if let Some(program) = viewer.program.as_deref() {
        match external(program, &viewer.args, root, &text, columns).await {
            Ok(stdout) => {
                let (lines, skipped) = clamp_styled(parse_sgr(&stdout));
                return (
                    Body::External {
                        tool: program.to_owned(),
                        lines,
                        skipped,
                    },
                    None,
                );
            }
            Err(note) => return (source.builtin(), Some(note)),
        }
    }
    (source.builtin(), None)
}

/// 「那份 diff」的原文，以及它是不是一份**补丁**。
enum Raw {
    /// 一份补丁（已跟踪文件那条路）。
    Patch(String),
    /// 一个文件的全文（未跟踪那一档）：它的「diff」就是全文。
    Text { body: files::FileBody },
    /// 二进制。
    Binary,
    /// 读不出来。
    Unreadable,
}

impl Raw {
    /// 能喂给外部工具的那一行文本；二进制与读不了时没有。
    fn text(&self) -> Option<&str> {
        match self {
            Raw::Patch(text) => Some(text),
            Raw::Text {
                body: files::FileBody::Text { text, .. },
            } => Some(text),
            _ => None,
        }
    }

    /// 内置那一档的正文。
    fn builtin(self) -> Body {
        match self {
            Raw::Patch(text) => Body::Patch(text),
            Raw::Text { body } => Body::NewFile(body),
            Raw::Binary => Body::Binary,
            Raw::Unreadable => Body::Unreadable,
        }
    }
}

/// 把「那份 diff」原文拿回来。
async fn raw(root: &Path, file: &ChangedFile) -> Raw {
    if file.kind == Kind::Untracked {
        return Raw::Text {
            body: files::read(root, &file.path),
        };
    }
    let args = diff_args(&file.path);
    match hostproc::run(Run {
        program: GIT,
        args: &args,
        cwd: root,
        stdin: None,
        env: &[],
        unset: UNSET,
        limit: LIMIT,
    })
    .await
    {
        Finished::Ok(stdout) if binary(&stdout) => Raw::Binary,
        Finished::Ok(stdout) => Raw::Patch(stdout),
        // 空仓库（还没有第一次提交）里 `diff HEAD` 是 128；`git` 不在 PATH 是 NotFound。
        // 两种都不该画出一块假空白，所以它们都归「读不出来」那一档。
        Finished::Exit(_) | Finished::NotFound | Finished::Timeout => Raw::Unreadable,
    }
}

/// 把那份 diff 喂给外部工具，收它的 stdout。
///
/// 三条规矩（§8）：
///
/// * **只给环境变量**：`PAGER=cat` / `GIT_PAGER=cat`（不让它去起自己的分页器把我们卡住）、
///   `COLUMNS=<正文宽>`（宽度与我们那个弹窗对齐）。**一条命令行参数都不注入** —— 工具那么
///   多，猜谁认什么是白猜，而认不出的会当场因未知参数报错；
/// * **argv 按整元素给**（绝不过 shell）；
/// * 失败与超时（2 秒）都回退内置那一档，回执里说清是谁没跑成。
async fn external(
    program: &str,
    args: &[String],
    root: &Path,
    input: &str,
    columns: usize,
) -> Result<String, String> {
    let columns = columns.to_string();
    let env = [
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("COLUMNS", columns.as_str()),
    ];
    match hostproc::run(Run {
        program,
        args,
        cwd: root,
        stdin: Some(input),
        env: &env,
        unset: &[],
        limit: LIMIT,
    })
    .await
    {
        Finished::Ok(stdout) => Ok(stdout),
        Finished::NotFound => Err(crate::render::wording::changes_viewer_missing(program)),
        Finished::Timeout => Err(crate::render::wording::changes_viewer_timeout(program)),
        // 非零退出：**不把它的 stderr 画进正文** —— 读的人会把它当成 diff。
        Finished::Exit(_) => Err(crate::render::wording::changes_viewer_failed(program)),
    }
}

/// `git diff` 那一次调用的 argv。
///
/// 与 `status` 同一族卫生，逐条都有来路：
///
/// * `--no-color`：`color.ui=always` 时 git 会往输出里塞 ANSI（实测）；
/// * `--no-ext-diff`：`diff.external` / `GIT_EXTERNAL_DIFF` 会把输出**整个换成**外部程序的
///   输出 —— 那不是补丁，解析它只会画错；
/// * `--src-prefix=a/ --dst-prefix=b/`：压住 `diff.noPrefix` / `diff.mnemonicPrefix` /
///   `diff.srcPrefix`（它们都会改头行的前缀）；
/// * `--unified=3`：压住 `diff.context`（实测 `--context=0` 会让 hunk 头变成 `@@ -2 +2 @@`）；
/// * `--no-optional-locks` / `--no-pager`：与 `status` 同样的两件；
/// * `HEAD -- <path>`：**相对 HEAD**，一个路径的 pathspec。
fn diff_args(path: &str) -> Vec<String> {
    [
        "--no-optional-locks",
        "--no-pager",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--unified=3",
        "HEAD",
        "--",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(std::iter::once(path.to_owned()))
    .collect()
}

/// 这份输出是不是「这是个二进制文件」那一句。
///
/// 判据是 **git 自己的输出**（`Binary files a/x and b/x differ`，或者 `--binary` 那种
/// `GIT binary patch`），不是 stderr —— 后者随 locale 本地化，不能当判据。
fn binary(text: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with("Binary files ") || line.starts_with("GIT binary patch"))
}

/// 补丁里的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchLine {
    /// 它在**文件里**的号：新增与上下文取新文件的号，删除取旧文件的号。hunk 头与
    /// `\ No newline at end of file` 那一行留空。
    pub number: Option<usize>,
    /// 这一行在补丁里算什么（票 12 的上色用它）。
    pub tag: DiffTag,
    /// 原文**整行**（含行首那个标记字符）：画出来的是补丁本身的样子。
    pub text: String,
}

/// 把一份补丁排成弹窗里的那些行。
///
/// 两条规矩（§6）：
///
/// * **头行不留**：`diff --git` / `index` / `---` / `+++`，以及 `new file mode` /
///   `similarity index` / `rename from` 那一族 —— 弹窗标题已经说明是哪个文件，人是来看代码
///   怎么变的。**`@@` 那行留**：它带着函数上下文，也是读的人定位的锚。
/// * **行号**：hunk 头给出两个区间的起点，此后新增与上下文走新文件的号、删除走旧文件的号。
///
/// 头行只在**进入第一个 hunk 之前**过滤：进了 hunk 之后，一行以 `---` 开头的可能是删掉的
/// 内容（`-- foo` 这种行），那是正文，不是文件头。
pub fn patch_rows(lines: &[String]) -> Vec<PatchLine> {
    let mut rows = Vec::new();
    let (mut old, mut new) = (0usize, 0usize);
    let mut in_hunk = false;
    for line in lines {
        if !in_hunk {
            if let Some((old_start, new_start)) = hunk_header(line) {
                old = old_start;
                new = new_start;
                in_hunk = true;
                rows.push(PatchLine {
                    number: None,
                    tag: DiffTag::Hunk,
                    text: line.clone(),
                });
                continue;
            }
            // 还没进 hunk：这些都是文件头。
            continue;
        }
        if let Some((old_start, new_start)) = hunk_header(line) {
            old = old_start;
            new = new_start;
            rows.push(PatchLine {
                number: None,
                tag: DiffTag::Hunk,
                text: line.clone(),
            });
        } else if line.starts_with('\\') {
            // `\ No newline at end of file`：它说的是上一行的事，自己不是一个行号。
            rows.push(PatchLine {
                number: None,
                tag: DiffTag::Context,
                text: line.clone(),
            });
        } else if line.starts_with('+') {
            rows.push(PatchLine {
                number: Some(new),
                tag: DiffTag::Added,
                text: line.clone(),
            });
            new += 1;
        } else if line.starts_with('-') {
            rows.push(PatchLine {
                number: Some(old),
                tag: DiffTag::Removed,
                text: line.clone(),
            });
            old += 1;
        } else if line.is_empty() {
            // 一个空的上下文行在 git 的输出里是**一个空格**，所以真正的空串只可能是末尾那
            // 一条（`split('\n')` 切出来的）：跳过它。
        } else {
            rows.push(PatchLine {
                number: Some(new),
                tag: DiffTag::Context,
                text: line.clone(),
            });
            old += 1;
            new += 1;
        }
    }
    rows
}

/// `@@ -a,b +c,d @@ 函数上下文` 那两个区间的起点。
///
/// 数量那一半（`b` / `d`）缺省是 1，我们不看它 —— 行号靠的是逐行往前走。
fn hunk_header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ -")?;
    let mut parts = rest.split(' ');
    let old = range_start(parts.next()?)?;
    let new = range_start(parts.next()?.strip_prefix('+')?)?;
    Some((old, new))
}

/// `12,3` 里的那个 `12`。
fn range_start(range: &str) -> Option<usize> {
    range.split(',').next()?.parse().ok()
}

/// 一份正文的有界截断：**行宽、行数、字节**三档，照文件页那三条
/// （`.scratch/files-page/spec.md` §6）。
///
/// 行宽那一档先做：一行特别长时，它不该先把字节预算吃光、让后面几百行都看不到。回答（留下
/// 的那些行，省掉的行数）—— 省掉的数要说给读的人听，而不是悄悄少一段。
pub fn clamp(text: &str) -> (Vec<String>, usize) {
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.chars().take(files::MAX_LINE_CHARS).collect())
        .collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let total = lines.len();
    let mut kept: Vec<String> = Vec::new();
    let mut bytes = 0usize;
    for line in lines {
        let cost = line.len() + 1;
        if kept.len() >= files::MAX_LINES || bytes + cost > files::MAX_BYTES {
            break;
        }
        bytes += cost;
        kept.push(line);
    }
    let skipped = total.saturating_sub(kept.len());
    (kept, skipped)
}

/// 我们认下来的那几档样式：**只认 SGR**（`ESC[…m`）那一族里的重置、前景、背景、粗体、
/// 暗淡（`.scratch/diff-page/spec.md` §8 与票 06 作答第 5 条）。
///
/// 认不出的颜色档（256 色之后的写法、truecolor）**保持现状**；其它 CSI 序列（光标移动、清屏、
/// `K`）整段丢掉 —— 我们不搬一台终端模拟器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sgr {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
}

impl Sgr {
    /// 画它用的样式。没认下任何一样时是默认样式 —— 也就是**正文档**那一档。
    pub fn style(self) -> Style {
        let mut style = Style::default();
        if let Some(fg) = self.fg {
            style = style.fg(fg);
        }
        if let Some(bg) = self.bg {
            style = style.bg(bg);
        }
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.dim {
            style = style.add_modifier(Modifier::DIM);
        }
        style
    }
}

/// 外部工具吐出的一小片文字，带着它认下来的样式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub style: Sgr,
}

/// 把一段带 ANSI 的输出解成逐行的片。
///
/// 解出来的行**仍走我们自己的正文管线**（折行、有界截断、可滚）：外部工具只决定「文本与
/// 颜色长什么样」。而它吐的转义序列**不会留在文本里**，所以拖选复制出来的仍是干净的文本
/// （§8）—— 这是「解成 span」比「原样透传」更值的地方。
pub fn parse_sgr(text: &str) -> Vec<Vec<Piece>> {
    let mut lines: Vec<Vec<Piece>> = vec![Vec::new()];
    let mut style = Sgr::default();
    let mut run = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => {
                flush(&mut lines, &mut run, style);
                if chars.peek() == Some(&'[') {
                    chars.next();
                    let mut params = String::new();
                    let mut final_byte = None;
                    for next in chars.by_ref() {
                        // CSI 的终结符是 0x40–0x7E；参数与中间字节在它之前。
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            final_byte = Some(next);
                            break;
                        }
                        params.push(next);
                    }
                    if final_byte == Some('m') {
                        apply_sgr(&mut style, &params);
                    }
                    // 别的 CSI（`K`、`2J`、`1A`…）整段丢掉。
                } else if chars.peek() == Some(&']') {
                    // OSC（`ESC ] … BEL` 或 `ESC ] … ESC \`）：它不是 CSI，但也不是能画的东西。
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // 别的转义序列：丢掉那个 ESC 就够了（后面那一个字符照常当文本走）。
            }
            '\n' => {
                flush(&mut lines, &mut run, style);
                lines.push(Vec::new());
            }
            // 回车与别的控制字符都不画：我们按行排，光标停在哪儿不关这一层的事。
            ch if ch.is_control() => {}
            ch => run.push(ch),
        }
    }
    flush(&mut lines, &mut run, style);
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

/// 把手上那一小段文字收成一片。
fn flush(lines: &mut [Vec<Piece>], run: &mut String, style: Sgr) {
    if run.is_empty() {
        return;
    }
    let text = std::mem::take(run);
    if let Some(line) = lines.last_mut() {
        line.push(Piece { text, style });
    }
}

/// 把一条 SGR 参数串（`1;31`）铺到当前样式上。
///
/// 认得的那些见 [`Sgr`]；`38;5;n` / `38;2;r;g;b` 这类带参数的颜色写法**整组跳掉**（认不出就
/// 保持现状），其余参数忽略。
fn apply_sgr(style: &mut Sgr, params: &str) {
    // `ESC[m`（空参数）就是 `ESC[0m`。
    let parts: Vec<&str> = if params.is_empty() {
        vec!["0"]
    } else {
        params.split(';').collect()
    };
    let mut index = 0;
    while index < parts.len() {
        match parts[index] {
            "" | "0" => *style = Sgr::default(),
            // 认不出的写法（含 `?25` 这种私有参数）什么都不改。
            other => match other.parse::<u32>() {
                Ok(1) => style.bold = true,
                Ok(2) => style.dim = true,
                Ok(22) => {
                    style.bold = false;
                    style.dim = false;
                }
                Ok(39) => style.fg = None,
                Ok(49) => style.bg = None,
                Ok(code @ 30..=37) => style.fg = Some(basic_colour(code)),
                Ok(code @ 90..=97) => style.fg = Some(bright_colour(code)),
                Ok(code @ 40..=47) => style.bg = Some(basic_colour(code)),
                // 8 位 / 24 位的颜色写法：整组跳掉。
                Ok(38 | 48) => index += extended_skip(&parts, index),
                _ => {}
            },
        }
        index += 1;
    }
}

/// `38` / `48` 后面还要跳几个参数：`5;n` 是两格，`2;r;g;b` 是四格，别的当一格。
fn extended_skip(parts: &[&str], index: usize) -> usize {
    match parts.get(index + 1) {
        Some(&"5") => 2,
        Some(&"2") => 4,
        _ => 1,
    }
}

/// 30–37 那一档前景色。背景（40–47）用同一族值。
fn basic_colour(code: u32) -> Color {
    match code {
        31 | 41 => Color::Red,
        32 | 42 => Color::Green,
        33 | 43 => Color::Yellow,
        34 | 44 => Color::Blue,
        35 | 45 => Color::Magenta,
        36 | 46 => Color::Cyan,
        37 | 47 => Color::Gray,
        _ => Color::Black,
    }
}

/// 90–97 那一档亮前景色。
fn bright_colour(code: u32) -> Color {
    match code {
        91 => Color::LightRed,
        92 => Color::LightGreen,
        93 => Color::LightYellow,
        94 => Color::LightBlue,
        95 => Color::LightMagenta,
        96 => Color::LightCyan,
        97 => Color::White,
        _ => Color::DarkGray,
    }
}

/// 外部工具那一档的有界截断：**行数与字节**两档。
///
/// 行宽那一档留给折行 —— 它已经把一条长行折成好几条显示行，不必再砍掉文字；而砍在这个位置
/// 还会把一条 SGR 序列切两半。回答（留下的那些行，省掉的行数）。
fn clamp_styled(lines: Vec<Vec<Piece>>) -> (Vec<Vec<Piece>>, usize) {
    let total = lines.len();
    let mut kept: Vec<Vec<Piece>> = Vec::new();
    let mut bytes = 0usize;
    for line in lines {
        let cost: usize = line.iter().map(|piece| piece.text.len() + 1).sum();
        if kept.len() >= files::MAX_LINES || bytes + cost > files::MAX_BYTES {
            break;
        }
        bytes += cost;
        kept.push(line);
    }
    let skipped = total.saturating_sub(kept.len());
    (kept, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一行 `1` 记录（最常见的那一种）。
    fn one(xy: &str, path: &str) -> String {
        format!("1 {xy} N... 100644 100644 100644 aaa bbb {path}\0")
    }

    #[test]
    fn a_tracked_change_carries_its_path_and_a_folded_glyph() {
        let files = parse_status(&one(".M", "src/render/tui.rs"));
        assert_eq!(
            files,
            vec![ChangedFile {
                path: "src/render/tui.rs".to_owned(),
                kind: Kind::Modified,
            }]
        );
    }

    #[test]
    fn the_worktree_side_wins_and_both_sides_are_one_glyph() {
        // `MM` / ` M` / `M ` 都画 `M`：已暂存与未暂存在这一页上不分家。
        for xy in ["MM", " M", "M "] {
            let files = parse_status(&one(xy, "a.rs"));
            assert_eq!(files[0].kind, Kind::Modified, "`{xy}` 折成已修改");
        }
        // 只有 index 那一侧动了（已暂存的新文件、已暂存的删除）：取那一位。
        assert_eq!(parse_status(&one("A.", "new.rs"))[0].kind, Kind::Added);
        assert_eq!(parse_status(&one("D.", "gone.rs"))[0].kind, Kind::Deleted);
        // 工作区那一位优先：已暂存新增、工作区又改成别的样子时，说的是工作区那一档。
        assert_eq!(parse_status(&one("AD", "new.rs"))[0].kind, Kind::Deleted);
    }

    #[test]
    fn untracked_files_are_their_own_kind_and_directories_arrive_expanded() {
        // `-uall` 之后未跟踪目录里的文件各占一行，于是这里读到的**每一个**都直接是一个文件。
        let files = parse_status("? brand-new.txt\0? d/a.txt\0? d/b.txt\0");
        assert_eq!(files.len(), 3);
        assert!(files.iter().all(|file| file.kind == Kind::Untracked));
        assert_eq!(files[0].path, "brand-new.txt");
    }

    #[test]
    fn a_path_with_spaces_survives_the_fields() {
        // 未 `-z` 时这里就是那个会切错的地方（research 1.1 的第 4 条）。
        let files = parse_status(&one(".M", "sp ace.txt"));
        assert_eq!(files[0].path, "sp ace.txt");
    }

    #[test]
    fn a_rename_consumes_its_original_path_and_lands_in_added() {
        // `2` 行：path 与原名之间也是 NUL。原名那一格必须消费掉，否则一个以 `1` 开头的旧名字
        // 会被当成下一条记录。
        let output = format!(
            "2 R. N... 100644 100644 100644 aaa bbb R100 b.txt\0a.txt\0{}",
            one(".M", "c.rs")
        );
        let files = parse_status(&output);
        assert_eq!(
            files,
            vec![
                ChangedFile {
                    path: "b.txt".to_owned(),
                    kind: Kind::Added,
                },
                ChangedFile {
                    path: "c.rs".to_owned(),
                    kind: Kind::Modified,
                },
            ]
        );
        // 旧名字恰好长得像一条记录时也不误读。
        let nasty = "2 R. N... 100644 100644 100644 aaa bbb R100 b.txt\01 leading\0";
        assert_eq!(parse_status(nasty).len(), 1);
    }

    #[test]
    fn an_unmerged_file_folds_into_modified_and_unknown_records_are_skipped() {
        let unmerged = "u UU N... 100644 100644 100644 100644 a b c c.txt\0";
        assert_eq!(parse_status(unmerged)[0].kind, Kind::Modified);
        // `!` 与任何看不懂的记录都不该让整页失败。
        assert!(parse_status("! ignored.txt\0???\0").is_empty());
    }

    #[test]
    fn a_record_with_too_few_fields_is_dropped_rather_than_guessed() {
        assert!(parse_status("1 .M N... 100644\0").is_empty());
    }

    #[test]
    fn the_list_groups_by_kind_and_sorts_by_path_inside_a_group() {
        let files = vec![
            ChangedFile {
                path: "b.rs".to_owned(),
                kind: Kind::Modified,
            },
            ChangedFile {
                path: "new.txt".to_owned(),
                kind: Kind::Untracked,
            },
            ChangedFile {
                path: "a.rs".to_owned(),
                kind: Kind::Modified,
            },
        ];
        assert_eq!(
            rows(&files),
            vec![
                Item::Header(Kind::Modified),
                Item::File(files[2].clone()),
                Item::File(files[0].clone()),
                Item::Header(Kind::Untracked),
                Item::File(files[1].clone()),
            ],
            "组按已修改 → 新增 → 已删除 → 未跟踪，组内按路径字典序，空组不出现"
        );
    }

    #[test]
    fn an_empty_readout_has_no_rows_at_all() {
        assert!(rows(&[]).is_empty());
    }

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_owned).collect()
    }

    #[test]
    fn the_head_lines_go_and_the_hunk_header_stays() {
        let patch = lines(
            "diff --git a/a.rs b/a.rs\n\
             index 1111111..2222222 100644\n\
             --- a/a.rs\n\
             +++ b/a.rs\n\
             @@ -1,3 +1,4 @@ fn main\n\
             \x20context\n\
             -removed\n\
             +added\n\
             +added2\n",
        );
        let rows = patch_rows(&patch);
        assert_eq!(rows.len(), 5, "四行头都走了：{rows:#?}");
        assert_eq!(rows[0].tag, DiffTag::Hunk);
        assert_eq!(rows[0].text, "@@ -1,3 +1,4 @@ fn main");
        assert_eq!(rows[0].number, None, "hunk 头没有行号");
        assert_eq!(rows[1].number, Some(1), "上下文行取新文件的号");
        assert_eq!(rows[2].number, Some(2), "删除行取旧文件的号");
        assert_eq!(rows[3].number, Some(2), "新增行取新文件的号");
        assert_eq!(rows[4].number, Some(3));
    }

    #[test]
    fn a_second_hunk_moves_the_numbers_with_it() {
        let patch = lines(
            "@@ -1,2 +1,2 @@\n\
             a\n\
             b\n\
             @@ -10,2 +20,2 @@\n\
             c\n\
             +d\n",
        );
        let rows = patch_rows(&patch);
        assert_eq!(rows[0].text, "@@ -1,2 +1,2 @@");
        assert_eq!(rows[3].text, "@@ -10,2 +20,2 @@");
        assert_eq!(rows[4].number, Some(20), "第二个 hunk 从它自己的新号起");
        assert_eq!(rows[5].number, Some(21));
    }

    #[test]
    fn a_removed_line_that_looks_like_a_header_is_body_once_the_hunk_started() {
        // `-- foo` 这样一行在补丁里是 `--- foo`（三个减号）：它出现在 hunk 里就是正文，
        // 不是文件头 —— 头行只在第一个 hunk 之前过滤。
        let patch = lines("@@ -1,2 +1,2 @@\n--- foo\n+bar\n");
        let rows = patch_rows(&patch);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].tag, DiffTag::Removed);
        assert_eq!(rows[1].text, "--- foo");
    }

    #[test]
    fn a_binary_readout_is_recognised_from_gits_own_line() {
        assert!(binary(
            "diff --git a/x.bin b/x.bin\nBinary files a/x.bin and b/x.bin differ\n"
        ));
        assert!(binary("GIT binary patch\nliteral 12\n"));
        assert!(!binary("diff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n"));
    }

    #[test]
    fn the_clamp_keeps_the_row_cap_and_says_how_many_it_left_out() {
        let body: Vec<String> = (0..files::MAX_LINES + 7).map(|n| format!("L{n}")).collect();
        let (kept, skipped) = clamp(&body.join("\n"));
        assert_eq!(kept.len(), files::MAX_LINES);
        assert_eq!(skipped, 7);
        assert_eq!(kept[0], "L0", "从头上留");
    }

    #[test]
    fn the_clamp_cuts_a_line_width_first_so_the_row_budget_is_not_eaten() {
        let long = "x".repeat(files::MAX_LINE_CHARS + 100);
        let (kept, skipped) = clamp(&format!("ok\n{long}\n"));
        assert_eq!(skipped, 0);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1].chars().count(), files::MAX_LINE_CHARS);
    }

    #[test]
    fn the_clamp_stops_at_the_byte_budget() {
        // 每条都是两字节的字符：按字节算得住的那些行才会留下。
        let line = "终".repeat(500);
        let body = std::iter::repeat_n(line, 1_000)
            .collect::<Vec<_>>()
            .join("\n");
        let (kept, skipped) = clamp(&body);
        assert!(kept.len() < 1_000, "字节那一档先拦住了");
        assert_eq!(kept.len() + skipped, 1_000);
    }

    #[test]
    fn a_patch_with_no_hunk_at_all_has_no_rows() {
        // 只有头行的输出（比如空 diff）不该画出任何东西。
        assert!(patch_rows(&lines("diff --git a/a.rs b/a.rs\nindex 111..222 100644\n")).is_empty());
    }

    /// 一段带 ANSI 的文本里，那一行解出来的文字。
    fn shown(pieces: &[Piece]) -> String {
        pieces.iter().map(|piece| piece.text.as_str()).collect()
    }

    #[test]
    fn sgr_colours_bold_and_dim_are_recognised() {
        let lines = parse_sgr("\x1b[1;32madded\x1b[0m plain\n\x1b[2;31mremoved\x1b[0m");
        assert_eq!(lines.len(), 2);
        assert_eq!(shown(&lines[0]), "added plain");
        assert_eq!(
            lines[0][0].style,
            Sgr {
                fg: Some(Color::Green),
                bold: true,
                ..Sgr::default()
            }
        );
        // 重置之后那一段回到正文档。
        assert_eq!(lines[0][1].style, Sgr::default());
        assert_eq!(
            lines[1][0].style,
            Sgr {
                fg: Some(Color::Red),
                dim: true,
                ..Sgr::default()
            }
        );
    }

    #[test]
    fn a_background_colour_and_a_reset_are_recognised() {
        let lines = parse_sgr("\x1b[47;30mlight\x1b[49m back\n");
        assert_eq!(lines[0][0].style.bg, Some(Color::Gray));
        assert_eq!(lines[0][0].style.fg, Some(Color::Black));
        assert_eq!(lines[0].len(), 2, "换回来的那一段没有背景");
        assert_eq!(lines[0][1].style.bg, None);
    }

    #[test]
    fn a_bright_foreground_is_its_own_slot() {
        let lines = parse_sgr("\x1b[91mhot\x1b[0m");
        assert_eq!(lines[0][0].style.fg, Some(Color::LightRed));
    }

    #[test]
    fn a_colour_we_do_not_recognise_leaves_the_style_alone() {
        // 256 色与 truecolor 那两种写法：认不出就**保持现状**，不把颜色画成别的样子。
        let lines = parse_sgr("\x1b[31mred\x1b[38;5;208m still red\x1b[38;2;1;2;3m and red\x1b[0m");
        assert_eq!(shown(&lines[0]), "red still red and red");
        assert!(
            lines[0]
                .iter()
                .all(|piece| piece.style.fg == Some(Color::Red))
        );
    }

    #[test]
    fn every_csi_that_is_not_sgr_is_dropped_whole() {
        // 光标移动、清屏、`K`：我们不搬一台终端模拟器，所以它们整段丢掉 —— 一个字符都不留在
        // 文本里（拖选复制出来的因此仍是干净的）。
        let lines = parse_sgr("\x1b[2K\x1b[1Akeep\x1b[?25l me\n");
        assert_eq!(shown(&lines[0]), "keep me");
        assert!(
            lines[0].iter().all(|piece| piece.style == Sgr::default()),
            "丢掉的那些序列也没有留下样式"
        );
    }

    #[test]
    fn an_osc_sequence_is_dropped_too() {
        // `ESC ] 8 ; ; https://example.com BEL 链接文字`：OSC 不是能画的东西。
        let lines = parse_sgr("\x1b]8;;https://example.com\x07link\n");
        assert_eq!(shown(&lines[0]), "link");
    }

    #[test]
    fn a_trailing_newline_does_not_leave_an_empty_line() {
        assert_eq!(parse_sgr("one\n").len(), 1);
        assert_eq!(parse_sgr("one\ntwo\n").len(), 2);
        assert!(parse_sgr("").is_empty());
    }

    #[test]
    fn the_external_readout_is_clamped_by_rows_and_bytes() {
        let long: Vec<Vec<Piece>> = (0..files::MAX_LINES + 3)
            .map(|index| {
                vec![Piece {
                    text: format!("L{index}"),
                    style: Sgr::default(),
                }]
            })
            .collect();
        let (kept, skipped) = clamp_styled(long);
        assert_eq!(kept.len(), files::MAX_LINES);
        assert_eq!(skipped, 3);
    }
}
