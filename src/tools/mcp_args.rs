//! 四个元工具共用的参数读取（`.scratch/mcp-support/spec.md` §2）。
//!
//! 三个工具的参数面是同一套规则：`server` 可选、`tool` / `uri` 必填非空、`arguments` 是一个
//! 自由对象。规则只写一处，好让「哪个参数必填」不会在四个声明之间漂。

use serde_json::Value;

use super::tool::ToolError;

/// `server` 是可选参数：缺席、`null` 或空白字符串都表示「全部已配置的 server」。
pub(super) fn optional_server(tool: &str, args: &Value) -> Result<Option<String>, ToolError> {
    match args.get("server") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => {
            let name = name.trim();
            Ok((!name.is_empty()).then(|| name.to_owned()))
        }
        Some(_) => Err(ToolError::message(format!(
            "{tool}：`server` 是一个字符串（server 的名字）"
        ))),
    }
}

/// 一个必填的非空字符串参数。
pub(super) fn required_str(tool: &str, key: &str, args: &Value) -> Result<String, ToolError> {
    match args.get(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        _ => Err(ToolError::message(format!(
            "{tool}：`{key}` 是必填的非空字符串"
        ))),
    }
}

/// 把一段外部文本压成一行：换行与制表都变成空格，首尾空白去掉；空的当作没有。
pub(super) fn flatten(text: Option<&str>) -> Option<String> {
    let text = text?;
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!flat.is_empty()).then_some(flat)
}
