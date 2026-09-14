//! The built-in file tools: `read_file`, `write_file`, `edit_file` (spec §7, §8).
//!
//! The wire contract of `edit_file` is fixed: an absolute `file_path`, an
//! `old_string`, a `new_string`, and an optional `replace_all`. Nothing else is
//! reserved, because the place a new edit format plugs in is the registry being
//! assembled per profile, not a field here.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::provider::ToolSpec;

use super::edit::{find_matches, MatchLevel};
use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// The comment-shaped placeholder check and the match level both land in the
/// tool result as convention text, so rendering and diagnosis share one format.
pub const MATCH_LEVEL_PREFIX: &str = "edit match level: ";

/// `read_file`: read a file inside the session workspace.
pub struct ReadFile;

/// The tool name each built-in file tool declares, named once so the registry,
/// the args parsers and the tests cannot drift apart.
pub const READ_FILE: &str = "read_file";
pub const WRITE_FILE: &str = "write_file";
pub const EDIT_FILE: &str = "edit_file";

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

/// A tiny accessor so `declared_path` can pull `file_path` out of any file
/// tool's parsed arguments without the three argument types being one type.
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

file_path_arg!(ReadFileArgs, WriteFileArgs, EditFileArgs);

/// The `file_path` field every file tool takes.
trait FilePathArg {
    fn file_path(&self) -> &str;
}

/// The path a call declares, parsed through that tool's own argument type. An
/// unparsable or empty call declares nothing, which is how a malformed call ends
/// up with an empty write set and then fails inside the tool with a real message.
fn declared_path<T>(args: &Value) -> PathBuf
where
    T: for<'de> Deserialize<'de> + FilePathArg,
{
    parse::<T>(args)
        .map(|parsed| PathBuf::from(parsed.file_path()))
        .unwrap_or_default()
}

/// The declared write targets of a call.
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

/// The `file_path` every file tool requires, or the same "required" error for
/// each. Every file tool then resolves it through the resolver that matches its
/// direction: reads through `read_paths`, writes through `write_paths`.
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

/// `write_file`: create or overwrite a file inside the session workspace.
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
            "write_file: {verb} {} ({} bytes)",
            path.display(),
            parsed.content.len()
        )))
    }
}

/// `edit_file`: replace a matched region, reporting which level matched.
pub struct EditFile;

#[derive(Debug, Deserialize)]
struct EditFileArgs {
    #[serde(default, deserialize_with = "nullable_string")]
    file_path: String,
    #[serde(default, deserialize_with = "nullable_string")]
    old_string: String,
    #[serde(default, deserialize_with = "nullable_string")]
    new_string: String,
    #[serde(default)]
    replace_all: bool,
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
        Effect::WritePaths(write_targets::<EditFileArgs>(args))
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let parsed: EditFileArgs = parse(&args)?;
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
            // A failed ladder is the signal that the agent's picture of the file
            // is stale, so the dispatcher withdraws the path's read permission.
            error @ super::edit::EditError::NoMatch => ToolError::InvalidatesReads {
                message: format!("{}: {error}", path.display()),
                path: path.clone(),
            },
            other => ToolError::message(format!("{}: {other}", path.display())),
        })?;

        // The bytes actually replaced, not the caller's `old_string`: a
        // downgraded level matches different bytes, and `.before` is the
        // byte-for-byte source `/undo` restores from.
        let replaced: String = edits.iter().map(|edit| edit.old_text.clone()).collect();
        let updated = apply_edits(&content, &edits, &parsed.new_string)?;

        // The target changes first: if that write fails there is no edit to undo,
        // so no snapshot may claim there is one.
        std::fs::write(&path, updated.as_bytes()).map_err(|error| {
            ToolError::message(format!("cannot write {}: {error}", path.display()))
        })?;

        let snapshot = ctx.outputs_dir.join(format!("{}.before", ctx.tool_call_id));
        std::fs::create_dir_all(ctx.outputs_dir).map_err(|error| {
            ToolError::message(format!(
                "cannot create {}: {error}",
                ctx.outputs_dir.display()
            ))
        })?;
        std::fs::write(&snapshot, replaced.as_bytes()).map_err(|error| {
            ToolError::message(format!("cannot write {}: {error}", snapshot.display()))
        })?;

        let level = edits
            .first()
            .map(|edit| edit.level)
            .unwrap_or(MatchLevel::Exact);
        Ok(ToolOutput::new(format!(
            "{} {}: {} replacement{} in {} ({} bytes -> {} bytes)",
            MATCH_LEVEL_PREFIX,
            level.as_str(),
            edits.len(),
            if edits.len() == 1 { "" } else { "s" },
            path.display(),
            content.len(),
            updated.len(),
        )))
    }
}

/// Apply every planned edit, back to front so earlier spans stay valid.
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

/// A model that means "no value" may send `null`; treat it as absent.
fn nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}
