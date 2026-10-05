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
pub const MATCH_LEVEL_PREFIX: &str = "编辑匹配等级：";

/// 每一次成功写开头的那一行，点名它落到了哪个文件上。
///
/// `ToolCallStarted` 事件记录的是**模型**发的参数，而后一个 `hook.pre` 可能把它们改写过；结果
/// 才是「实际写到哪个文件」的唯一记录。所以任何从流上派生的、关于「哪些文件变了」的东西
/// （spec §16）都读这一行，而产生它和解析它共用这一个常量 —— 与 [`MATCH_LEVEL_PREFIX`] 同一条
/// 规矩（spec §18）。
pub const WROTE_PATH_PREFIX: &str = "已写入：";

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
        return Err(ToolError::message(format!("{tool}：`file_path` 是必填的")));
    }
    Ok(PathBuf::from(parsed_file_path))
}

#[async_trait]
impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: READ_FILE.to_owned(),
            description: "读取工作区里的一个文件，返回带行号的内容".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "文件路径：绝对路径，或相对于工作区根目录；工作区之外的\
                                          路径会被拒绝"
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
        let content = std::fs::read_to_string(&path)
            .map_err(|error| ToolError::message(format!("无法读取 {}：{error}", path.display())))?;

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
            description: "在工作区里写一个文件：新建它，或替换它的内容；缺失的父目录会被\
                           一并建出来"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "文件路径：绝对路径，或相对于工作区根目录；工作区之外的\
                                          路径会被拒绝"
                    },
                    "content": {
                        "type": "string",
                        "description": "要写入文件的完整内容"
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
        // 第一次往一个新目录里写（一份新 spec）要把缺失的父目录建出来。这里的 `path` 已经
        // 过了收容检查，所以建的是工作区里的目录。
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ToolError::message(format!("无法创建 {}：{error}", parent.display()))
            })?;
        }
        let existed = path.exists();
        std::fs::write(&path, parsed.content.as_bytes())
            .map_err(|error| ToolError::message(format!("无法写入 {}：{error}", path.display())))?;
        let verb = if existed { "替换" } else { "新建" };
        Ok(ToolOutput::new(format!(
            "{WROTE_PATH_PREFIX}{}\nwrite_file：{verb}（{} 字节）",
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
            description: "把工作区文件里的 `old_string` 替换成 `new_string`。除非设了 \
                          `replace_all`，匹配必须唯一"
                .to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "文件路径：绝对路径，或相对于工作区根目录；工作区之外的\
                                          路径会被拒绝"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "要被替换掉的原文，必须与文件里的文本一字不差"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "替换上去的新文本"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "替换每一处，而不是要求匹配唯一"
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
        let content = std::fs::read_to_string(&path)
            .map_err(|error| ToolError::message(format!("无法读取 {}：{error}", path.display())))?;

        let edits = find_matches(
            &content,
            &parsed.old_string,
            &parsed.new_string,
            parsed.replace_all,
        )
        .map_err(|error| match error {
            // 阶梯失败就是「agent 对那个文件的图景已陈旧」的信号，所以派发器收回该路径的读权限。
            error @ super::edit::EditError::NoMatch => ToolError::InvalidatesReads {
                message: format!("{}：{error}", path.display()),
                path: path.clone(),
            },
            other => ToolError::message(format!("{}：{other}", path.display())),
        })?;

        // 实际被替换掉的那些字节，不是调用方给的 `old_string`：降档后的匹配会匹配上不同的字节，
        // 而 `.before` 是 `/undo` 用来做字节级还原的源。
        let replaced: String = edits.iter().map(|edit| edit.old_text.clone()).collect();
        let updated = apply_edits(&content, &edits, &parsed.new_string)?;

        let snapshot = ctx.outputs_dir.join(before_artifact(ctx.tool_call_id));
        std::fs::create_dir_all(ctx.outputs_dir).map_err(|error| {
            ToolError::message(format!("无法创建 {}：{error}", ctx.outputs_dir.display()))
        })?;
        // 快照**先于**目标落盘：如果它写不进去，那就什么都还没变，这次调用干净地失败。一个活得比
        // 失败的目标写更久的快照是无害的 —— `/undo` 只会考虑那些已记录结果为成功的编辑。
        super::paths::write_owner_only(&snapshot, replaced.as_bytes()).map_err(|error| {
            ToolError::message(format!("无法写入 {}：{error}", snapshot.display()))
        })?;
        std::fs::write(&path, updated.as_bytes())
            .map_err(|error| ToolError::message(format!("无法写入 {}：{error}", path.display())))?;

        let level = edits
            .first()
            .map(|edit| edit.level)
            .unwrap_or(MatchLevel::Exact);
        Ok(ToolOutput::new(format!(
            "{WROTE_PATH_PREFIX}{}\n{} {}: {} 处替换（{} 字节 -> {} 字节）",
            path.display(),
            MATCH_LEVEL_PREFIX,
            level.as_str(),
            edits.len(),
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
        return Err(ToolError::message("edit_file：没有可施加的编辑"));
    }
    let mut updated = content.to_owned();
    for edit in edits.iter().rev() {
        if !updated.is_char_boundary(edit.span.start) || !updated.is_char_boundary(edit.span.end) {
            return Err(ToolError::message("edit_file：匹配上的区域不在字符边界上"));
        }
        updated.replace_range(edit.span.clone(), new_string);
    }
    Ok(updated)
}

fn parse<T: for<'de> Deserialize<'de>>(args: &Value) -> Result<T, ToolError> {
    serde_json::from_value(args.clone())
        .map_err(|error| ToolError::message(format!("工具参数无效：{error}")))
}

/// 一个想表达「没有值」的模型可能会发 `null`；把它当作缺席。
fn nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}
