# 事件接缝与权限门的控制流次序（prototype）

Type: prototype
Status: closed
Blocked by: 01, 03

## Question

用**粗糙可跑的代码**回答一个问题：**工具调用前后的事件挂载点与权限门，谁拥有控制流？**

背景：用户已选扩展档位 (iii)——工具调用前后各留一个挂载点（`PreToolUse` / `PostToolUse`），v1 只挂内建消费者。综述对此的定位是：

> **唯一「应该做但可以晚做」的一条**：`PreToolUse` hook。……**如果只保留一个扩展机制，保留它，而不是 MCP。**

必须回答的具体问题：

1. **次序。** 综述引 Claude Code 的规则：`PreToolUse` **在任何权限检查之前触发，且在所有模式下都触发（包括 `bypassPermissions`）**。原文是 "Hooks can tighten restrictions but not loosen them."——即 hook 只能收紧、不能放松权限。这个"谁在外层"在代码里怎么表达？
2. **`PostToolUse` 回灌。** 综述把它和 aider 的 lint/test 反省循环（`max_reflections = 3`）并列，标为 ROI 很高："把 verify 从模型的自觉变成 harness 的保证"。它的产物是**注入一条新消息**，还是**改写工具结果**？
3. **权限门的返回值有几态。** 综述最小可用版是 `readonly` / `ask` / `auto` 三档，外加危险命令黑名单与 cwd 路径限制。三态够吗，还是要预留 `allow`/`deny`/`ask` 的更细表达？
4. **边界情况**：hook 失败、hook 超时、hook 返回"放松"的意图——各自怎么处理？

**为什么用 prototype 而不是纸上争论**：这个次序很微妙，且错了要等到实现阶段才暴露。一个能跑的最小骨架（一两个 stub 工具 + 一个 stub 权限门 + 两个挂载点）就能看出接缝对不对。

**产物形态**：丢弃式原型，**不是**正式代码。按综述/wayfinder 的约定，原型留在 `prototype/<name>` 分支上作为原始资料，并由本票链接；结论折回真实代码。
