//! 内建的 `grep(pattern)` 工具：在工作区里按行搜索，只读、只扫会话 cwd。
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
//! [`crate::context::truncate_result`]。工具内另按**条数**先收一刀（[`MAX_MATCHES`]），并在
//! 末尾如实写出省掉了多少 —— token 那条界与它无关，也不新增第二套截断。
//!
//! 命中的文件**不算「已读」**：`read_paths()` 是调用前的纯函数，声明不了运行时才知道的命中
//! 文件，而这条工具不值得动 `Tool` 接口（spec §3）。
//!
//! 可选的 `glob` 只影响「搜哪些文件」，由 `globset` 在遍历之后过滤 —— 它缩小的范围，不放宽的是
//! 忽略规则（spec §3、票 02）。

use std::io;
use std::path::Path;

use async_trait::async_trait;
use globset::{Glob, GlobSet, GlobSetBuilder};
use grep_regex::RegexMatcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、描述与测试不互相漂离。
pub const GREP_TOOL: &str = "grep";

/// 一次调用最多列出多少条命中，超出时在末尾如实写出还剩多少条。
///
/// 是常量、不是配置：先跑一段看真实用量，再决定要不要照 `repo_map_tokens` 开一个按工具的
/// 预算字段（spec §4）。行有长有短，所以这里数的是**条数**；token 那条界仍由
/// `max_tool_result_tokens` 与那条唯一的截断流水线管。
pub const MAX_MATCHES: usize = 500;

/// 在工作区里搜一个正则，逐行报出命中。
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: GREP_TOOL.to_owned(),
            description: "在工作区里按行搜索正则（rg 语法）。这是搜代码的首选方式 —— \
                          不要用 `bash` 拼 `rg` / `grep`：这个工具是只读的，在每一档权限模式\
                          下都放行。范围是会话工作区，遵守 `.gitignore` 并跳过隐藏文件；\
                          结果形如 `path:line:文本`，命中太多时会被截断并给出一条落盘路径。"
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

        let hits = search(ctx.cwd, &matcher, glob.as_ref())?;
        if hits.text.is_empty() {
            // 「glob 什么都没选上」与「选上了但这些文件里没有匹配」是两件事，
            // 而模型看到的必须是能据以行动的那一句。
            if let (Some(glob), 0) = (glob_text, hits.scanned) {
                return Ok(ToolOutput::new(format!(
                    "工作区里没有匹配 glob `{glob}` 的文件"
                )));
            }
            return Ok(ToolOutput::new(format!(
                "在工作区里没有匹配 `{pattern}` 的行"
            )));
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

/// 一次遍历的产物：要交给模型的文本，经 `glob` 过滤后**实际搜过**的文件数，以及超出的条数。
///
/// `scanned` 只为一件事存在：把「`glob` 什么都没选上」与「选上了但这些文件里没有匹配」分开。
/// 文本已经含末尾那句「还有 N 条未列出」，所以调用方不必再拼一次。
struct Search {
    text: String,
    scanned: usize,
}

/// 走一遍会话 cwd，把命中的行写成 `相对路径:行号:文本`。
///
/// 忽略规则就是 `ignore` 的默认：遵守 `.gitignore`（含 `.ignore` 与 git 的全局忽略）、跳过
/// 隐藏文件与隐藏目录 —— 与 `rg` 的默认一致，所以换工具不改变搜索结果（spec §2）。条目按路径
/// 排序，于是同一个工作区上的输出稳定。
///
/// `glob` 在遍历之后过滤：它只决定「搜哪些文件」，绝不参与 pattern 的匹配，也不放宽忽略规则
/// （spec §3）。
fn search(
    root: &Path,
    matcher: &RegexMatcher,
    glob: Option<&GlobSet>,
) -> Result<Search, ToolError> {
    let mut text = String::new();
    let mut scanned = 0usize;
    let mut listed = 0usize;
    let mut skipped = 0usize;
    // 二进制检测要显式打开：`grep_searcher` 自己的缺省是 `BinaryDetection::None`（照单全收），
    // 而这里要的是 rg 的默认那一档 —— 一个文件里见到 NUL 就放弃它，免得把二进制倒进上下文。
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(0))
        .build();

    let mut builder = WalkBuilder::new(root);
    builder.sort_by_file_path(|left, right| left.cmp(right));
    for entry in builder.build() {
        let Ok(entry) = entry else {
            // 走不进去的目录（权限、竞态）：这不是这次搜索的失败，跳过它继续。
            continue;
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(path);
        if glob.is_some_and(|glob| !glob.is_match(relative)) {
            continue;
        }
        scanned += 1;
        let mut sink = Collector {
            path: relative,
            text: &mut text,
            listed: &mut listed,
            skipped: &mut skipped,
        };
        // 单个文件读不了（竞态、权限）不该让整次搜索失败 —— 与遍历错误同一档处理。
        let _ = searcher.search_path(matcher, path, &mut sink);
    }

    if skipped > 0 {
        // `repo_map` 末尾那一行是同一种收尾：说清省掉了多少，并给一句能照做的事。
        text.push_str(&format!(
            "还有 {skipped} 条未列出：请缩小搜索范围，或用 `glob` 只搜一部分文件\n"
        ));
    }
    Ok(Search { text, scanned })
}

/// 把命中收进一个字符串的接收端。
///
/// 列满 [`MAX_MATCHES`] 之后不再收集文本，但**继续数**：末尾那句「还有 N 条未列出」只有把整个
/// 工作区数完才是诚实的。
struct Collector<'a> {
    path: &'a Path,
    text: &'a mut String,
    listed: &'a mut usize,
    skipped: &'a mut usize,
}

impl Sink for Collector<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
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
}
