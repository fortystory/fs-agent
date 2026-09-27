//! 内建的文件工具：`read_file`、`write_file`、`edit_file`（spec §7、§8）。
//!
//! `edit_file` 的线级契约是固定的：绝对的 `file_path`、一个 `old_string`、一个 `new_string`，
//! 以及可选的 `replace_all`。此外不再预留任何东西，因为新编辑格式插进来的地方是按 profile 组装
//! 的那个注册表，而不是这里的一个字段。

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::edit::{find_matches, MatchLevel};
use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 注释形状占位符的检查与匹配等级都作为约定文本落进工具结果，于是渲染与诊断共用一种格式。
pub const MATCH_LEVEL_PREFIX: &str = "edit match level: ";

/// 每一次成功写开头的那一行，点名它落到了哪个文件上。
///
/// `ToolCallStarted` 事件记录的是**模型**发的参数，而后一个 `hook.pre` 可能把它们改写过；结果
/// 才是「实际写到哪个文件」的唯一记录。所以任何从流上派生的、关于「哪些文件变了」的东西
/// （spec §16）都读这一行，而产生它和解析它共用这一个常量 —— 与 [`MATCH_LEVEL_PREFIX`] 同一条
/// 规矩（spec §18）。
pub const WROTE_PATH_PREFIX: &str = "wrote: ";

/// `read_file`：读会话工作区里的一个文件。
pub struct ReadFile;

/// 每个内建文件工具声明的工具名，只在这里命名一次，好让注册表、参数解析器与测试不会互相漂离。
pub const READ_FILE: &str = "read_file";
pub const WRITE_FILE: &str = "write_file";
pub const EDIT_FILE: &str = "edit_file";

/// `/undo` 为一次编辑读的那个文件名：这次编辑实际替换掉的字节（spec §11）。
///
/// 产生这个名字与找到它共用这个函数，所以这条例会不会像一对 `format!` 调用那样漂掉。
pub fn before_artifact(tool_call_id: &str) -> String {
    format!("{tool_call_id}.before")
}

#[derive(Debug, Deserialize)]
struct ReadFileArgs {
    #[serde(default, deserialize_with = "nullable_string")]
    file_path: String,
}

impl ReadFile {
    fn path(args: &Value) -> PathBuf {
        declared_path::<ReadFileArgs>(args)
    }
}

/// 一个很小的取值口，好让 `declared_path` 从任何文件工具已解析的参数里取出 `file_path`，而不
/// 必让这三种参数类型变成同一个类型。
macro_rules! file_path_arg {
    ($($args:ident),+) => {
        $(
            impl FilePathArg for $args {
                fn file_path(&self) -> &str {
                    &self.file_path
                }
            }
        )+
    };
}

file_path_arg!(ReadFileArgs, WriteFileArgs, EditCall);

/// 每个文件工具都收的 `file_path` 字段。
trait FilePathArg {
    fn file_path(&self) -> &str;
}

/// 一次调用声明的路径，经该工具自己的参数类型解析。解析不了或为空的调用什么都没声明 —— 一次
/// 畸形调用就是这样先拿到空的写集，然后在工具内部带着一条真消息失败。
fn declared_path<T>(args: &Value) -> PathBuf
where
    T: for<'de> Deserialize<'de> + FilePathArg,
{
    parse::<T>(args)
        .map(|parsed| PathBuf::from(parsed.file_path()))
        .unwrap_or_default()
}

/// 一次调用声明的写目标。
fn write_targets<T>(args: &Value) -> Vec<PathBuf>
where
    T: for<'de> Deserialize<'de> + FilePathArg,
{
    let path = declared_path::<T>(args);
    if path.as_os_str().is_empty() {
        Vec::new()
    } else {
        vec![path]
    }
}

/// 每个文件工具都要求的 `file_path`，或者说每个工具收到的都是同一句「必填」错误。此后每个文件
/// 工具都经与它方向相符的那个解析器解析它：读经 `read_paths`，写经 `write_paths`。
fn required_path(tool: &str, parsed_file_path: &str) -> Result<PathBuf, ToolError> {
    if parsed_file_path.is_empty() {
        return Err(ToolError::message(format!(
            "{tool}: `file_path` is required"
        )));
    }
    Ok(PathBuf::from(parsed_file_path))
}

#[async_trait]
impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: READ_FILE.to_owned(),
            description: "Read a file from the workspace. Returns its contents with line numbers."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file: absolute, or relative to the workspace root. \
                                          Paths outside the workspace are refused."
                    }
                },
                "required": ["file_path"]
            }),
        }
    }

    fn effect(&self, _args: &Value) -> Effect {
        Effect::ReadOnly
    }

    fn read_paths(&self, args: &Value) -> Vec<PathBuf> {
        let path = Self::path(args);
        if path.as_os_str().is_empty() {
            Vec::new()
        } else {
            vec![path]
        }
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let parsed: ReadFileArgs = parse(&args)?;
        let requested = required_path(READ_FILE, &parsed.file_path)?;
        let path = ctx.read_paths.resolve_read(&requested)?;
        let content = std::fs::read_to_string(&path).map_err(|error| {
            ToolError::message(format!("cannot read {}: {error}", path.display()))
        })?;

        let mut text = format!("{}\n", path.display());
        for (index, line) in content.lines().enumerate() {
            text.push_str(&format!("{}\t{line}\n", index + 1));
        }
        Ok(ToolOutput::new(text))
    }
}

/// `write_file`：在会话工作区里新建或覆写一个文件。
pub struct WriteFile;

#[derive(Debug, Deserialize)]
struct WriteFileArgs {
    #[serde(default, deserialize_with = "nullable_string")]
    file_path: String,
    #[serde(default, deserialize_with = "nullable_string")]
    content: String,
}

#[async_trait]
impl Tool for WriteFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: WRITE_FILE.to_owned(),
            description: "Write a file in the workspace, creating it or replacing its contents."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file: absolute, or relative to the workspace root. \
                                          Paths outside the workspace are refused."
                    },
                    "content": {
                        "type": "string",
                        "description": "The complete new contents of the file"
                    }
                },
                "required": ["file_path", "content"]
            }),
        }
    }

    fn effect(&self, args: &Value) -> Effect {
        Effect::WritePaths(write_targets::<WriteFileArgs>(args))
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let parsed: WriteFileArgs = parse(&args)?;
        let requested = required_path(WRITE_FILE, &parsed.file_path)?;
        let path = ctx.write_paths.resolve_write(&requested)?;
        let existed = path.exists();
        std::fs::write(&path, parsed.content.as_bytes()).map_err(|error| {
            ToolError::message(format!("cannot write {}: {error}", path.display()))
        })?;
        let verb = if existed { "replaced" } else { "created" };
        Ok(ToolOutput::new(format!(
            "{WROTE_PATH_PREFIX}{}\nwrite_file: {verb} ({} bytes)",
            path.display(),
            parsed.content.len()
        )))
    }
}

/// `edit_file`：替换一段匹配上的区域，并报告匹配到了哪一档。
pub struct EditFile;

/// 一次 `edit_file` 调用的参数。
///
/// 是公开的，因为 `/undo` 从流上（`ToolCallStarted.args`）重读它们：工具接受的形状与 undo 解析
/// 的形状是同一个类型，所以两者不可能漂掉。
#[derive(Debug, Clone, Deserialize)]
pub struct EditCall {
    #[serde(default, deserialize_with = "nullable_string")]
    pub file_path: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub old_string: String,
    #[serde(default, deserialize_with = "nullable_string")]
    pub new_string: String,
    #[serde(default)]
    pub replace_all: bool,
}

#[async_trait]
impl Tool for EditFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: EDIT_FILE.to_owned(),
            description: "Replace `old_string` with `new_string` in a workspace file. The match \
                          must be unique unless `replace_all` is set."
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file: absolute, or relative to the workspace root. \
                                          Paths outside the workspace are refused."
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact text to replace"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The text to put in its place"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace every occurrence instead of requiring a unique match"
                    }
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
        }
    }

    fn effect(&self, args: &Value) -> Effect {
        Effect::WritePaths(write_targets::<EditCall>(args))
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let parsed: EditCall = parse(&args)?;
        let requested = required_path(EDIT_FILE, &parsed.file_path)?;
        let path = ctx.write_paths.resolve_write(&requested)?;
        let content = std::fs::read_to_string(&path).map_err(|error| {
            ToolError::message(format!("cannot read {}: {error}", path.display()))
        })?;

        let edits = find_matches(
            &content,
            &parsed.old_string,
            &parsed.new_string,
            parsed.replace_all,
        )
        .map_err(|error| match error {
            // 阶梯失败就是「agent 对那个文件的图景已陈旧」的信号，所以派发器收回该路径的读权限。
            error @ super::edit::EditError::NoMatch => ToolError::InvalidatesReads {
                message: format!("{}: {error}", path.display()),
                path: path.clone(),
            },
            other => ToolError::message(format!("{}: {other}", path.display())),
        })?;

        // 实际被替换掉的那些字节，不是调用方给的 `old_string`：降档后的匹配会匹配上不同的字节，
        // 而 `.before` 是 `/undo` 用来做字节级还原的源。
        let replaced: String = edits.iter().map(|edit| edit.old_text.clone()).collect();
        let updated = apply_edits(&content, &edits, &parsed.new_string)?;

        let snapshot = ctx.outputs_dir.join(before_artifact(ctx.tool_call_id));
        std::fs::create_dir_all(ctx.outputs_dir).map_err(|error| {
            ToolError::message(format!(
                "cannot create {}: {error}",
                ctx.outputs_dir.display()
            ))
        })?;
        // 快照**先于**目标落盘：如果它写不进去，那就什么都还没变，这次调用干净地失败。一个活得比
        // 失败的目标写更久的快照是无害的 —— `/undo` 只会考虑那些已记录结果为成功的编辑。
        super::paths::write_owner_only(&snapshot, replaced.as_bytes()).map_err(|error| {
            ToolError::message(format!("cannot write {}: {error}", snapshot.display()))
        })?;
        std::fs::write(&path, updated.as_bytes()).map_err(|error| {
            ToolError::message(format!("cannot write {}: {error}", path.display()))
        })?;

        let level = edits
            .first()
            .map(|edit| edit.level)
            .unwrap_or(MatchLevel::Exact);
        Ok(ToolOutput::new(format!(
            "{WROTE_PATH_PREFIX}{}\n{} {}: {} replacement{} ({} bytes -> {} bytes)",
            path.display(),
            MATCH_LEVEL_PREFIX,
            level.as_str(),
            edits.len(),
            if edits.len() == 1 { "" } else { "s" },
            content.len(),
            updated.len(),
        )))
    }
}

/// 施加每一处已规划的编辑，从后往前，好让前面的 span 保持有效。
fn apply_edits(
    content: &str,
    edits: &[super::edit::EditMatch],
    new_string: &str,
) -> Result<String, ToolError> {
    if edits.is_empty() {
        return Err(ToolError::message("edit_file: no edit to apply"));
    }
    let mut updated = content.to_owned();
    for edit in edits.iter().rev() {
        if !updated.is_char_boundary(edit.span.start) || !updated.is_char_boundary(edit.span.end) {
            return Err(ToolError::message(
                "edit_file: matched region is not on character boundaries",
            ));
        }
        updated.replace_range(edit.span.clone(), new_string);
    }
    Ok(updated)
}

fn parse<T: for<'de> Deserialize<'de>>(args: &Value) -> Result<T, ToolError> {
    serde_json::from_value(args.clone())
        .map_err(|error| ToolError::message(format!("invalid tool arguments: {error}")))
}

/// 一个想表达「没有值」的模型可能会发 `null`；把它当作缺席。
fn nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}
