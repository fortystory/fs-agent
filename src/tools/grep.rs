//! 内建的 `grep(pattern, glob?, count?, after?, before?)` 工具：在工作区里按行搜索，只读、
//! 只扫会话 cwd。
//!
//! 与 `repo_map` 同一个形状（`.scratch/grep-tool/spec.md` §2）：一个**只读**工具，自己走
//! `ctx.cwd`，不声明读路径 —— 于是它绕开 `outside_read` 那套「工具声明过的读路径」语义，
//! 在四档权限模式下都可用（`Effect::ReadOnly` 的门裁决全是 `Allow`），也不取工作区锁。
//!
//! 搜索内核是 ripgrep 拆出来的三个库（`ignore` + `grep-searcher` + `grep-regex`），而不是
//! spawn `rg`：`rg` 经 `process::run` 跑起来继承的是沙箱的「整机可读」，与「只读工作区」不是
//! 一回事，而且它换来三条运行期依赖（spec §4）。
//!
//! 上限只有一条流水线：`call()` 返回字符串，溢出、指针与头尾预览全交给
//! [`crate::context::truncate_result`]。工具内另按**列出来的行**先收一刀（[`MAX_MATCHES`]），
//! 并在末尾如实写出省掉了多少条命中 —— token 那条界与它无关，也不新增第二套截断。
//!
//! `after` / `before` 把命中行前后各若干行一并列出来，形状照抄 ripgrep：命中行 `路径:行号:文本`、
//! 上下文行 `路径-行号-文本`、两个命中块之间一行 `--`。重叠的上下文由搜索器自己合成一块，
//! 收集端不重扫补行，于是同一段正文不会在结果里出现两遍。
//!
//! `count: true` 换一种输出：每个文件一行命中**行数**，末尾一个总数。它**不受 [`MAX_MATCHES`]
//! 影响** —— 那条界收的是「要列出来的行」，而计数模式一行都不列，于是那个总数是整个工作区的
//! 真实值，而不是一个模型无从察觉的残数。
//!
//! 命中的文件**不算「已读」**：`read_paths()` 是调用前的纯函数，声明不了运行时才知道的命中
//! 文件，而这条工具不值得动 `Tool` 接口（spec §3）。
//!
//! 可选的 `glob` 只影响「搜哪些文件」，由 `globset` 在遍历之后过滤 —— 它缩小的范围，不放宽的是
//! 忽略规则（spec §3、票 02）。它与 `count` 正交：计数同样只扫被 `glob` 收窄过的那批文件。

use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use globset::{Glob, GlobSet, GlobSetBuilder};
use grep_regex::RegexMatcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};
use super::walk;

/// 工具名，只在这里命名一次，好让注册表、描述与测试不互相漂离。
pub const GREP_TOOL: &str = "grep";

/// 一次调用最多列出多少**行**（命中行与上下文行合计），超出时在末尾如实写出还剩多少条命中。
///
/// 是常量、不是配置：先跑一段看真实用量，再决定要不要照 `repo_map_tokens` 开一个按工具的
/// 预算字段（spec §4）。上下文行占同一份额度 —— 否则「带上 `after: 100` 就把额度翻倍」
/// 会让那条界形同虚设。token 那条界仍由 `max_tool_result_tokens` 与那条唯一的截断流水线管。
pub const MAX_MATCHES: usize = 500;

/// 在工作区里搜一个正则，逐行报出命中。
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: GREP_TOOL.to_owned(),
            description: "在工作区里按行搜索正则（rg 语法）。这是搜代码的首选方式 —— \
                          别用 `bash` 拼 `rg` / `grep`：这个工具是只读的，在每一档权限模式\
                          下都放行。范围是会话工作区，遵守 `.gitignore` 并跳过隐藏文件；\
                          结果形如 `path:line:文本`，命中行前后还能带上下文行\
                          （`after` / `before`，上下文行形如 `path-line-文本`，两个命中块\
                          之间一行 `--`），列出的行太多时会被截断并给出一条落盘路径。\
                          命中行本身没有信息量时（例如只想看一个函数体）用 `after` / `before`，\
                          只要数字时用 `count: true`。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "要搜的正则（rg 语法）。大小写这类需求用内联语法解决，\
                                        例如 `(?i)`"
                    },
                    "glob": {
                        "type": "string",
                        "description": "可选，只搜匹配这个 glob 的文件，例如 `*.rs`。它只缩小\
                                        搜的范围，不会让被 `.gitignore` 忽略的文件重新被搜到"
                    },
                    "count": {
                        "type": "boolean",
                        "description": "可选，默认 `false`。为 `true` 时不列命中行，改为报每个文件\
                                        的命中行数与一个总数 —— 数的是**行**（同一行里出现多次\
                                        只算一条，与 `rg -c` 同义）"
                    },
                    "after": {
                        "type": "integer",
                        "description": "可选，非负整数：每条命中行后面再列多少行上下文，等价 `rg -A N`"
                    },
                    "before": {
                        "type": "integer",
                        "description": "可选，非负整数：每条命中行前面再列多少行上下文，等价 `rg -B N`"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty())
            .ok_or_else(|| ToolError::message(format!("{GREP_TOOL}：`pattern` 是必填的")))?;
        let matcher = RegexMatcher::new_line_matcher(pattern).map_err(|error| {
            ToolError::message(format!("{GREP_TOOL}：`pattern` 不是合法的正则：{error}"))
        })?;
        let glob_text = args
            .get("glob")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|glob| !glob.is_empty());
        let glob = glob_text.map(compile_glob).transpose()?;
        // 类型不对（`1` 而不是 `true`）按没给算，而不是报错：这一位错了最多让模型拿到
        // 列行的那份输出、重问一次，而报错只会让它卡在这儿。与 `glob` 同一档宽松。
        let counting = args.get("count").and_then(Value::as_bool).unwrap_or(false);
        // 计数模式一行不列，于是两个上下文参数在这里就被丢掉 —— 不是报错，是各走各的
        // （spec §1）。报错会让模型以为两个模式不能共存，于是先学一条「什么时候不能一起用」。
        let (after, before) = if counting {
            (0, 0)
        } else {
            (
                context_lines(&args, "after")?,
                context_lines(&args, "before")?,
            )
        };

        let hits = search(
            ctx.cwd,
            &matcher,
            glob.as_ref(),
            counting,
            Context { after, before },
        )?;
        // 两种模式的「空」不是同一处：行模式看有没有列出行，计数模式看有没有命中过文件。
        let empty = if counting {
            hits.per_file.is_empty()
        } else {
            hits.text.is_empty()
        };
        if empty {
            // 「glob 什么都没选上」与「选上了但这些文件里没有匹配」是两件事，
            // 而模型看到的必须是能据以行动的那一句。两种输出模式共用这一处：0 处匹配在
            // 计数模式里也就是「没有匹配的行」，所以措辞一个字都不必分岔。
            if let (Some(glob), 0) = (glob_text, hits.scanned) {
                return Ok(ToolOutput::new(format!(
                    "工作区里没有匹配 glob `{glob}` 的文件"
                )));
            }
            return Ok(ToolOutput::new(format!(
                "在工作区里没有匹配 `{pattern}` 的行"
            )));
        }
        if counting {
            return Ok(ToolOutput::new(hits.count_text()));
        }
        Ok(ToolOutput::new(hits.text))
    }
}

/// 把模型给的 `glob` 编译成一个只用于「搜哪些文件」的集合。
///
/// 用 `globset` 而不是 `ignore` 的 `OverrideBuilder`：overrides 的优先级高于
/// `.gitignore`，于是 `glob` 会变成一条绕过忽略规则的逃生口（`glob: "ignored.txt"` 能让本该
/// 被忽略的文件重新被搜到）。两条路 spec §3 都允许，这里取不动忽略规则的那条。
fn compile_glob(glob: &str) -> Result<GlobSet, ToolError> {
    let glob = Glob::new(glob).map_err(|error| {
        ToolError::message(format!("{GREP_TOOL}：`glob` 不是合法的 glob：{error}"))
    })?;
    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    builder
        .build()
        .map_err(|error| ToolError::message(format!("{GREP_TOOL}：`glob` 无法编译：{error}")))
}

/// `after` / `before`：命中行前后各列多少行上下文。非负整数，缺省 `0`（不列）。
///
/// 负数与非法类型是**参数错误**，而不是悄悄取绝对值或按 0 算 —— 半截的写法会变成一条模型
/// 无从察觉的错结果。不设上限、不夹取：真正兜住输出的是下面那条 [`MAX_MATCHES`]。
fn context_lines(args: &Value, name: &str) -> Result<usize, ToolError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(0),
        Some(value) => value
            .as_u64()
            .and_then(|lines| usize::try_from(lines).ok())
            .ok_or_else(|| {
                ToolError::message(format!(
                    "{GREP_TOOL}：`{name}` 是非负整数（每条命中行{name}列多少行上下文）；\
                     不给就整段别给"
                ))
            }),
    }
}

/// 一次遍历的产物：要交给模型的文本，每个命中文件的命中行数，经 `glob` 过滤后**实际搜过**的
/// 文件数，以及超出的条数。
///
/// `scanned` 只为一件事存在：把「`glob` 什么都没选上」与「选上了但这些文件里没有匹配」分开。
/// 文本已经含末尾那句「还有 N 条命中未列出」，所以调用方不必再拼一次。
///
/// `text` 与 `per_file` 是**两个模式的产物**，不会同时有内容：行模式只有 `text`，计数模式
/// 只有 `per_file`（`text` 恒空，因为那种模式一行命中都不列）。`per_file` 天然按路径有序 ——
/// 遍历器排过序，所以这里不必再排一次，输出稳定性也就跟着它。
struct Search {
    text: String,
    per_file: Vec<(PathBuf, usize)>,
    scanned: usize,
}

impl Search {
    /// 计数模式的输出：每个命中文件一行 `路径:条数`，末尾一个总数。
    ///
    /// 总数是各项之和，**没有第二处存它**：[`MAX_MATCHES`] 那条界在这里不生效（计数模式一行
    /// 都不列），所以这个和就是整个工作区的真实命中行数。收尾句的位置与行模式那句
    /// 「还有 N 条命中未列出」一致 —— 都在末尾。
    fn count_text(&self) -> String {
        let mut text = String::new();
        let mut total = 0usize;
        for (path, lines) in &self.per_file {
            text.push_str(&format!("{}:{lines}\n", path.display()));
            total += *lines;
        }
        text.push_str(&format!(
            "共 {total} 条匹配（{} 个文件）\n",
            self.per_file.len()
        ));
        text
    }
}

/// 一行搜索要带多少上下文行：命中行之后与之前各多少行。
///
/// 成对出现、且总是同一副用途，所以捆成一个类型而不是两个位置参数 —— `after` 与 `before` 摆成
/// 两个 `usize` 时，调用点写反了编译器不会吭声，而写反的那一种只会安静地少列几行上下文。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Context {
    after: usize,
    before: usize,
}

/// 走一遍会话 cwd，把命中的写成 `相对路径:行号:文本`（计数模式写成 `相对路径:条数`）。
///
/// 忽略规则与排序由 [`walk::files`] 给出 —— `glob` 工具看见的是同一个世界，两条工具不各自
/// 发明一套（spec §2）。
///
/// `glob` 在遍历之后过滤：它只决定「搜哪些文件」，绝不参与 pattern 的匹配，也不放宽忽略规则
/// （spec §3）。
///
/// `counting` 只换产出、不换扫描：同一条遍历、同一批文件、同一套忽略规则，区别落在
/// [`Collector`] 把命中写成行还是计进那个文件的数上。
///
/// `after` / `before` 交给**搜索器自己**的前后上下文能力，不是收集端再扫一遍补行 —— 后者要在
/// 已列出的行里重排去重，而重叠合并（两个相邻命中的上下文区间叠在一起）本来就是搜索器内部
/// 的状态机做对的事。
fn search(
    root: &Path,
    matcher: &RegexMatcher,
    glob: Option<&GlobSet>,
    counting: bool,
    context: Context,
) -> Result<Search, ToolError> {
    let mut text = String::new();
    let mut per_file: Vec<(PathBuf, usize)> = Vec::new();
    let mut scanned = 0usize;
    let mut listed = 0usize;
    let mut skipped = 0usize;
    // 二进制检测要显式打开：`grep_searcher` 自己的缺省是 `BinaryDetection::None`（照单全收），
    // 而这里要的是 rg 的默认那一档 —— 一个文件里见到 NUL 就放弃它，免得把二进制倒进上下文。
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .after_context(context.after)
        .before_context(context.before)
        .binary_detection(BinaryDetection::quit(0))
        .build();

    for file in walk::files(root) {
        if glob.is_some_and(|glob| !glob.is_match(&file.relative)) {
            continue;
        }
        scanned += 1;
        let mut sink = Collector {
            path: &file.relative,
            text: &mut text,
            listed: &mut listed,
            skipped: &mut skipped,
            local: 0,
            counting,
        };
        // 单个文件读不了（竞态、权限）不该让整次搜索失败 —— 与遍历错误同一档处理。
        let _ = searcher.search_path(matcher, &file.path, &mut sink);
        // 这个文件的行数只在 `sink` 里活着，搜完就得取出来；零命中的文件不进 `per_file`
        // —— 输出里不该有一行 `0`，那是在替模型多写一句「这个文件没有」。
        if counting && sink.local > 0 {
            per_file.push((file.relative.clone(), sink.local));
        }
    }

    if skipped > 0 {
        // `repo_map` 末尾那一行是同一种收尾：说清省掉了多少，并给一句能照做的事。
        // 数的是**未列出的命中行数**（不是未列出的行数）—— 上下文行不算命中，说「还有 N 条」
        // 时那个 N 必须是命中条数，否则模型会以为漏掉的是那么多处地方。
        text.push_str(&format!(
            "还有 {skipped} 条命中未列出：请缩小搜索范围，或用 `glob` 只搜一部分文件\n"
        ));
    }
    Ok(Search {
        text,
        per_file,
        scanned,
    })
}

/// 把命中收进一个字符串的接收端。
///
/// 列满 [`MAX_MATCHES`] 之后不再收集文本，但**继续数**：末尾那句「还有 N 条命中未列出」只有把整个
/// 工作区数完才是诚实的。额度数的是**列出来的行**，命中行与上下文行共用它 —— 上下文行不是
/// 赠品，超限时它们和命中行一起被省掉。
///
/// 计数模式（`counting`）是同一个接收端的另一种写法：只累 `local`，既不写文本也不受那条界约束
/// —— 换来的那个总数因此不受截断影响。此时搜索器那一档上下文也一并关掉，于是这里根本收不到
/// 上下文回调。
struct Collector<'a> {
    path: &'a Path,
    text: &'a mut String,
    listed: &'a mut usize,
    skipped: &'a mut usize,
    /// 这个文件自己的命中行数。行模式没人读它（行已经在 `text` 里了），计数模式下它是这个
    /// 文件唯一被留下的东西 —— [`search`] 搜完就把它取走。
    local: usize,
    counting: bool,
}

impl Sink for Collector<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
        // 计数模式一行都不列，于是 [`MAX_MATCHES`] 那条界在这里**不生效**：它收的是「要列出来
        // 的行」，而这里没有要列的行。先分岔，于是总数是全工作区的真实值，不是一个残数。
        if self.counting {
            self.local += 1;
            return Ok(true);
        }
        if *self.listed >= MAX_MATCHES {
            *self.skipped += 1;
            return Ok(true);
        }
        let line = mat.line_number().unwrap_or(1);
        let matched = String::from_utf8_lossy(mat.bytes());
        self.text.push_str(&format!(
            "{}:{line}:{}\n",
            self.path.display(),
            matched.trim_end_matches(&['\n', '\r'][..])
        ));
        *self.listed += 1;
        Ok(true)
    }

    /// 一行上下文：形状与命中行只差那个分隔符（`-` 而不是 `:`），行号是真的，于是模型可以拿
    /// 它接 `read_file` 的 `offset`。它与命中行共用 [`MAX_MATCHES`] 那份额度。
    fn context(&mut self, _searcher: &Searcher, line: &SinkContext<'_>) -> Result<bool, io::Error> {
        // 收尾句数的是**命中**，所以上下文行满了只是不再列，不进 `skipped`。
        if *self.listed >= MAX_MATCHES {
            return Ok(true);
        }
        let number = line.line_number().unwrap_or(1);
        let text = String::from_utf8_lossy(line.bytes());
        self.text.push_str(&format!(
            "{}-{number}-{}\n",
            self.path.display(),
            text.trim_end_matches(&['\n', '\r'][..])
        ));
        *self.listed += 1;
        Ok(true)
    }

    /// 两个命中块之间的断口，插一行 `--`（ripgrep 的默认分隔符）。
    ///
    /// 用搜索器的这个回调，而不是拿行号自己判「离上一行列出的那行远不远」：重叠的上下文区间
    /// 已经由搜索器合成一块，它才知道块从哪儿开始、到哪儿结束。
    fn context_break(&mut self, _searcher: &Searcher) -> Result<bool, io::Error> {
        // 分隔符不占额度：它不是一条内容，而额度收的是「列出来的行」。
        if *self.listed < MAX_MATCHES {
            self.text.push_str("--\n");
        }
        Ok(true)
    }
}
