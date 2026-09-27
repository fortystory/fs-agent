//! 动态声明的工具（spec §14）。
//!
//! 用户在 `config.toml` 里把一个工具声明成一条 argv 模板加一份 JSON Schema；此后它就像内建工具
//! 一样干活。有三条性质是结构性的：
//!
//! * **它不能自称只读。** 声明语法里没有副作用字段，而 [`Tool::effect`] 对每次调用都是
//!   `Exclusive`。所以一个真正只读的动态工具也会被工作区级地串行化 —— 这是「不信声明」要付的
//!   代价，写在文档里 —— 而剩下的那道约束是权限门，因为 `read-before-edit` 看不见工具根本
//!   没声明过的 `WritePaths`。
//! * **它注入不了 shell。** argv 是直接 spawn 的（[`super::process`]）；一个参数替换掉一整个
//!   argv 元素，而数组或对象会变成单个元素，而不是被展开。
//! * **它的名字在词法上可辨认。** 每个声明出来的工具都是 `custom__<命名空间>__<工具>`，而
//!   内建名都不含 `__`，所以 [`is_custom_tool`] 是只针对这个字符串的谓词。

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::config::{parameter_placeholder, ToolDeclaration, CUSTOM_TOOL_SEPARATOR};
use crate::provider::ToolSpec;

use super::process;
use super::tool::{Effect, Tool, ToolContext, ToolError, ToolOutput};

/// 一个工具名是来自配置、还是来自内建表。内建名都不含 `__`，所以这个判定是词法的（spec §14）。
pub fn is_custom_tool(name: &str) -> bool {
    name.contains(CUSTOM_TOOL_SEPARATOR)
}

/// 一个动态声明的工具，裹着它那条已解析的声明。
pub struct CustomTool {
    declaration: ToolDeclaration,
}

impl CustomTool {
    pub fn new(declaration: ToolDeclaration) -> Self {
        Self { declaration }
    }

    /// 一次调用的 argv：每个 `{参数}` 元素被对应的参数替换掉，参数缺席时整个元素省掉。
    ///
    /// 替换以**整个 argv 元素**为单位。数组或对象参数会被序列化进一个元素，绝不展开成好几个，
    /// 所以没有哪个参数能改变命令的形状。
    pub fn argv(&self, args: &Value) -> Vec<String> {
        let mut argv = Vec::with_capacity(self.declaration.command.len());
        for element in &self.declaration.command {
            let Some(name) = parameter_placeholder(element) else {
                // 字面元素原样使用。
                argv.push(element.clone());
                continue;
            };
            match args.get(name) {
                // 缺席或为 null 就把这个元素整个省掉。
                None | Some(Value::Null) => {}
                Some(value) => argv.push(render_argument(value)),
            }
        }
        argv
    }
}

#[async_trait]
impl Tool for CustomTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.declaration.name.clone(),
            description: self.declaration.description.clone(),
            parameters: self.declaration.parameters.clone(),
        }
    }

    /// 永远是 `Exclusive`（spec §14）：声明里没有效果类别的字段，所以「这个其实只读」没有地方
    /// 可说。调度器的保守路径不需要特例。
    fn effect(&self, _args: &Value) -> Effect {
        Effect::Exclusive
    }

    /// 权限门的 `CommandPrefix` 范围读的那条 argv，按 [`Tool::call`] 造它的同一种方式构造，
    /// 好让门与进程不可能有分歧。
    fn command(&self, args: &Value) -> Option<Vec<String>> {
        let argv = self.argv(args);
        (!argv.is_empty()).then_some(argv)
    }

    async fn call(&self, ctx: &ToolContext<'_>, args: Value) -> Result<ToolOutput, ToolError> {
        let argv = self.argv(&args);
        if argv.is_empty() {
            return Err(ToolError::message(format!(
                "{}: the call produced no argv to run",
                self.declaration.name
            )));
        }
        let limit = Duration::from_millis(self.declaration.timeout_ms);
        let outcome = process::run(ctx.cwd, &argv, limit).await?;
        Ok(ToolOutput::new(outcome.report()))
    }
}

/// 一个参数值，作为单个 argv 元素。
///
/// 字符串原样使用（不存在要做转义的引号层）；其他每个 JSON 值都用它的紧凑序列化，所以数组或对
/// 象是一个元素，而不是一次展开。
fn render_argument(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
