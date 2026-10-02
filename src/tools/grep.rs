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
//! [`crate::context::truncate_result`]。
//!
//! 命中的文件**不算「已读」**：`read_paths()` 是调用前的纯函数，声明不了运行时才知道的命中
//! 文件，而这条工具不值得动 `Tool` 接口（spec §3）。

use std::io;
use std::path::Path;

use async_trait::async_trait;
use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 工具名，只在这里命名一次，好让注册表、描述与测试不互相漂离。
pub const GREP_TOOL: &str = "grep";

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

        let hits = search(ctx.cwd, &matcher)?;
        if hits.is_empty() {
            return Ok(ToolOutput::new(format!(
                "在工作区里没有匹配 `{pattern}` 的行"
            )));
        }
        Ok(ToolOutput::new(hits))
    }
}

/// 走一遍会话 cwd，把命中的行写成 `相对路径:行号:文本`。
///
/// 忽略规则就是 `ignore` 的默认：遵守 `.gitignore`（含 `.ignore` 与 git 的全局忽略）、跳过
/// 隐藏文件与隐藏目录 —— 与 `rg` 的默认一致，所以换工具不改变搜索结果（spec §2）。条目按路径
/// 排序，于是同一个工作区上的输出稳定。
fn search(root: &Path, matcher: &RegexMatcher) -> Result<String, ToolError> {
    let mut text = String::new();
    let mut searcher = SearcherBuilder::new().line_number(true).build();

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
        let mut sink = Collector {
            path: relative,
            text: &mut text,
        };
        // 单个文件读不了（竞态、权限）不该让整次搜索失败 —— 与遍历错误同一档处理。
        let _ = searcher.search_path(matcher, path, &mut sink);
    }
    Ok(text)
}

/// 把命中收进一个字符串的接收端。
struct Collector<'a> {
    path: &'a Path,
    text: &'a mut String,
}

impl Sink for Collector<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
        let line = mat.line_number().unwrap_or(1);
        let matched = String::from_utf8_lossy(mat.bytes());
        self.text.push_str(&format!(
            "{}:{line}:{}\n",
            self.path.display(),
            matched.trim_end_matches(&['\n', '\r'][..])
        ));
        Ok(true)
    }
}
