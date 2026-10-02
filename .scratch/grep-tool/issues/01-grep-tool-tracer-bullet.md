# 01 — `grep(pattern)`：内建的只读搜索（tracer bullet）

Type: implement
Status: ready-for-agent
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1–§6。一手调研：
> [`../research/01-search-tool-implementation.md`](../research/01-search-tool-implementation.md)
> —— `Effect` 的分类表在 §1.2、路线 A 的落点在 §2.5、`truncate_result` 的形状在 §6.1。
> 本票打通工具本身；`glob` 归 [票 02](02-glob-filter.md)，上限与落盘收尾归
> [票 03](03-limit-and-spill.md)，文档归 [票 04](04-docs-and-index.md)。

## 目标

模型给一个 `pattern`，就能在工作区里搜到匹配的行，结果形如 `相对路径:行号:文本`。这次调用
在 `readonly` 档**放行**、在 `ask` 档**不打断人** —— 从工具声明到事件流一条通路打通，
而且它进的是内建工具表，不是 `config.toml` 里的动态声明。

## 现状（2026-10-02 核实，改前先复核）

- `Effect` 三类在 `src/tools/tool.rs:21-33`；`bash` 的 `effect()` 恒 `Exclusive`
  （`src/tools/bash.rs:113-115`），理由写在同一处 `src/tools/tool.rs:29-31`。
- 四档对 `Effect::ReadOnly` 的裁决都是 `Allow`：`readonly` / `ask` / `workspace` 在
  `src/permissions.rs:129-153`，`auto` 在 `:178-182`。`Registry::dispatch` 只对 `Exclusive`
  取工作区锁、对 `WritePaths` 取逐路径锁（`src/tools/registry.rs:178-185`）。
- 工具表在 `src/tools/mod.rs:69-83`（`builtin`）与 `:91-97`（`with_dynamic`）；顶部 `:1-10`
  写明工具表在前缀缓存的整个生命周期里保持不变。
- **现成模板是 `repo_map`**（“只读、自己走 cwd、不声明读路径”）：整个 `src/tools/repo_map.rs`
  可以照抄形状 —— 模块注释在 `:1-8`、`spec()` 在 `:40-59`、`effect()` 在 `:61-63`、
  `call()` 在 `:65-81`。
- `Tool` trait 的默认 `read_paths` 返回空（`src/tools/tool.rs:213-215`）；read-before-write
  在 `src/tools/registry.rs:271-293`。
- 读路径的解析与 `outside_read` 在 `src/tools/registry.rs:131-142` 与
  `src/permissions.rs:567-581` —— 不声明读路径、自己走 cwd 的工具**完全绕过**它。
- 截断流水线：`context::truncate_result`（`src/context.rs:336-371`），唯一调用点是
  `agent.rs` 的 `emit_completed`（`src/agent.rs:2043-2054`），上限来自
  `SessionConfig.max_tool_result_tokens`（缺省 25_000，`src/config.rs:57`）。

## 落点

`Cargo.toml`、`src/tools/grep.rs`（新）、`src/tools/mod.rs`、`tests/`（新增一个搜索工具的
测试文件，或并入既有的工具测试 —— 按邻居怎么组织照做）。

## 具体行为

1. **依赖**：加 `ignore`、`grep-searcher`、`grep-regex`。调研记的版本是 0.4.33 / 0.1.17 /
   0.1.14（三者都是 `Unlicense OR MIT`），落地时取当时的稳定版并如实记进 `Cargo.lock`。
2. **`GrepTool`**：`spec()` 声明工具名 `grep` 与一个必填 `pattern`（描述见第 4 条）；
   `effect()` 恒 `Effect::ReadOnly`；`call()` 自己走 `ctx.cwd`，**不**读 `ctx.read_paths`、
   **不**重写 `read_paths`。
3. **遍历与搜索**：用 `ignore` 的默认规则（遵守 `.gitignore`，跳过隐藏文件与二进制），
   逐文件搜索，输出 `相对路径:行号:文本`（路径相对 `ctx.cwd`）。没有匹配时返回一句如实的
   说明（例如「在工作区里没有匹配 `<pattern>` 的行」），**不返回空串** —— 空结果与「工具坏了」
   在流上必须分得开。
4. **描述**：写明三件事 —— 这是搜代码的首选方式（**不要用 `bash` 拼 `rg` / `grep`**）；
   范围是会话工作区、遵守 `.gitignore`；结果形如 `path:line:文本`。**这句话是这张票一半的
   收益来源**：不把模型引过来，权限账一分也拿不到。
5. **注册**：加进 `builtin()`，不走 `with_dynamic()` —— 动态声明工具的 effect 恒为
   `Exclusive`（`src/tools/custom.rs:73-77`），正好是要修的那一类。
6. **不改别的**：`bash`、`repo_map`、调度器、权限门、事件 schema 一个字不动。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt` + `python3 scripts/check-language.py`：

1. **只读的门裁决**：同一个 `grep` 调用在 `readonly` 档放行、在 `ask` 档不产生询问。
   反向锚：`bash` 的一次纯搜索在 `ask` 档仍要审批 —— 两条一起才说明白这条工具修的是什么。
2. **不取工作区锁**：断言 `effect()` 返回 `Effect::ReadOnly`（或一次 `grep` 与一次 `read_file`
   同批时不被串行化）。
3. **命中与形状**：往工作区写一个文件与一行已知文本，断言结果含 `相对路径:行号:文本`。
4. **忽略规则三条一起**：写进 `.gitignore` 的文件搜不到；隐藏文件（如 `.hidden.txt`）
   搜不到；未被忽略的普通文件搜得到。它们钉的是「与 rg 默认一致」。
5. **grep 之后仍要先读才能改**：`grep` 命中一个文件后直接 `edit_file` 被拒，补一次
   `read_file` 后成功 —— 钉住「不登记命中文件」这条决定。
6. **一次调用一条结果**：每次调用在流上恰好一条 `ToolCallCompleted`（那是三条既有不变量
   之一，不是新规则）。
7. **执行者也拿到它**：执行者的工具表里有 `grep`（`task` 那种 `delegable() == false`
   不适用于它）。

## 不做什么

- 不做 `glob` 过滤（票 02）、不做条数上限与落盘的收尾（票 03）、不写逐面文档（票 04）。
- 不加 `path` 参数、不读工作区外、不把命中文件登记进读集合。
- 不加任何配置项（尤其不照 `repo_map_tokens` 开一个按工具的预算字段）。
- 不承诺并发：`Effect::ReadOnly` 今天不带来工具调用的并行，本票不碰调度器。
- 不在描述里禁用 `bash` 里的 `rg` —— 只劝阻，不拦。
