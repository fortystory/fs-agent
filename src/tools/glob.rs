//! 内建的 `glob(pattern)` 工具：按一个模式列出工作区里的文件，只读、只扫会话 cwd
//! （`.scratch/tool-coverage` §4；票 11）。
//!
//! 它答的是「**这儿有哪些文件**」，而同一张工具表里的另两条各答一问：**符号地图**答「有哪些
//! 符号」、[`super::grep`] 答「某个名字在哪」。三者在同一个工作区上看见的是**同一个**世界 ——
//! 忽略规则那一套由 [`super::walk`] 给出，两条工具都不自己发明第二套。
//!
//! 只有一个字段、只有一条上限、输出按路径稳定排序：不带大小与时间（那要么多一次 `stat`、要么
//! 让输出不可预期），上限 `MAX_PATHS` 与 `grep` 的 `MAX_MATCHES` 同值。

use async_trait::async_trait;
use globset::{GlobBuilder, GlobSetBuilder};
use serde_json::Value;

use crate::provider::ToolSpec;

use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};
use super::walk;

/// 工具名，只在这里命名一次，好让注册表、描述与测试不互相漂离。
pub const GLOB_TOOL: &str = "glob";

/// 一次调用最多列出多少条路径，超出时在末尾如实写出还剩多少条。
///
/// 与 `grep` 的 [`super::MAX_MATCHES`] 同值、少一个新常量：两处各写一个数字，迟早会漂成
/// 「为什么搜索限 500 条、枚举限 501 条」。
pub const MAX_PATHS: usize = super::grep::MAX_MATCHES;

/// 按一个模式列出工作区里的文件路径。
pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: GLOB_TOOL.to_owned(),
            description: "在工作区里按一个 glob 模式列出文件路径，一行一条、相对工作区、按路径\
                          排序。这是「这儿有哪些文件」的那一条 —— **符号地图**答「有哪些符号」、\
                          **`grep`** 答「某个名字在哪」；别用 `bash` 拼 `ls` / `find`。范围是会话\
                          工作区，遵守 `.gitignore` 并跳过隐藏文件；`src/render/*.rs` 是一个层次、\
                          `src/**` 整棵子树、`**/*.md` 任意深度。列出的路径太多时会被截断并给出一\
                          条落盘路径。"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "要列哪些文件，相对会话工作区的 glob：`src/render/*.rs`、\
                                        `**/*.md`、`.scratch/*/spec.md`。`*` 不跨 `/`，\
                                        `**` 跨"
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
            .ok_or_else(|| ToolError::message(format!("{GLOB_TOOL}：`pattern` 是必填的")))?;
        let matcher = compile(pattern)?;

        // 遍历器已经排过序，所以这里收集到的顺序就是输出顺序 —— 不必再排一次，输出的稳定性
        // 也就跟着它。
        let mut listed: Vec<String> = Vec::new();
        let mut total = 0usize;
        for file in walk::files(ctx.cwd) {
            if !matcher.is_match(&file.relative) {
                continue;
            }
            total += 1;
            if listed.len() < MAX_PATHS {
                listed.push(file.relative.display().to_string());
            }
        }

        if listed.is_empty() {
            return Ok(ToolOutput::new(format!(
                "工作区里没有匹配 `{pattern}` 的文件"
            )));
        }
        let mut text = listed.join("\n");
        text.push('\n');
        if total > listed.len() {
            text.push_str(&format!(
                "还有 {} 条未列出：请把模式收窄一点\n",
                total - listed.len()
            ));
        }
        Ok(ToolOutput::new(text))
    }
}

/// 把模型给的 `pattern` 编成一个匹配器。
///
/// `literal_separator(true)` 是这一处形状的全部：`*` 不跨 `/`、只有 `**` 跨，于是
/// `src/render/*.rs` 恰好是一个目录下一层的文件，而 `src/*` 表达的是「`src` 下有什么」。
/// 关掉它的话 `src/*` 会一路吃到 `src/a/b/c.rs`，那个模式就不再是模型写下的那个意思了。
///
/// **它不放宽忽略规则**：过滤发生在 [`walk::files`] 遍历之后，而那一遍已经按 `.gitignore` 与
/// 隐藏文件收过了 —— 所以 `pattern` 永远选不出本该看不见的文件。
fn compile(pattern: &str) -> Result<globset::GlobSet, ToolError> {
    let glob = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|error| {
            ToolError::message(format!("{GLOB_TOOL}：`pattern` 不是合法的 glob：{error}"))
        })?;
    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    builder
        .build()
        .map_err(|error| ToolError::message(format!("{GLOB_TOOL}：`pattern` 无法编译：{error}")))
}
