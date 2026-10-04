# 17 — 提示词模板接进 `/` 菜单

Type: implement
Status: ready-for-walkthrough
Part of: ../map.md
Blocked by: 10, 12

> 规格：[`../spec.md`](../spec.md) §8（第三阶段）。发起者是**人**，不是模型。

## 目标

人在 `/` 菜单里看到某台 server 的模板、挑一个、填上参数，它变成这次会话的一条消息。

## 具体行为

1. **`/` 菜单支持运行时条目**：数据源从常量变成「常量 + 运行时」（模板清单从 server 拉）。
2. **填参数的界面**：选中模板后用 `prompts/get` 带 arguments，把结果变成一条消息。
3. **server 不可用时它的模板条目不出现**（不是出现后点开报错）—— 菜单本来就是运行时拉的。
4. **菜单的动态部分不进工具表、不进前缀缓存**：它是界面层的东西，与四个元工具无关。
5. **模型不自发调用模板**：工具表里没有它。

## 验证

`cargo test` + `cargo clippy --all-targets` + `python3 scripts/check-language.py`：

1. **条目来源**：假连接声明两个模板，菜单里出现两项；参数填完变成一条消息。
2. **降级**：server 不可用时那台的条目不出现。
3. **常量条目回归**：`/discuss` / `/goal-new` / `/loop` / `/clear` / `/undo` 照旧可用。
4. **不进表**：工具表里没有模板相关的工具（模型看不到）。

## 不做什么

- 不让模型自发调用模板。
- 不把模板清单塞进工具描述或系统提示词（那是缓存前缀）。

## 评论

- 2026-10-03 落地（`Status: done`）。落点：`src/mcp/mod.rs`（`PromptSummary` / `PromptArgument` /
  `PromptListing`、`McpConnection::list_prompts` 与 `get_prompt`、`McpService::list_prompts` /
  `prompt_entries` / `get_prompt`）、`src/mcp/rmcp_client.rs`（`prompts/list` 与 `prompts/get`，
  后者走会自动驱动 MRTR 的那个入口）、`src/tools/mod.rs`（`with_mcp` 收 `impl Into<Arc<..>>`，
  于是工具表与菜单共用同一次连接）、`src/cli.rs`（`McpPromptEntry`、`mcp_prompt_entries`、
  `slash_catalog` 的第三参数、`Submission::McpPrompt`、`run_mcp_prompt` 与
  `prompt_inline_arguments`）、`src/render/wording.rs`（五条新短语）、
  `tests/mcp_prompts.rs`（新，4 条）与 `src/cli.rs` 的单元测试（+4 条）。
- **菜单那一半怎么接的**：`/` 菜单的条目从「常量 + 技能」变成「常量 + 技能 + 运行时模板」，
  条目名是 `/<server>:<模板名>`（冒号是它与技能名的分界）。清单在组装之后现问一次，
  **server 不可用时它的条目根本不出现**（`McpService::prompt_entries` 直接跳过失败的 server）。
- **填参数的界面**：命令行上跟的位置参数按模板声明的顺序填；还缺的（含必填）用
  `ask_user_question` 那条问询端口逐项问 —— **不新开第二套界面**。参数齐了才发 `prompts/get`，
  渲染出来的文本用既有的 `TurnStart::Prompt` 变成这一轮的一条消息。
- 三条降级都是如实的：清单里没有这个名字是「未知命令」；参数比声明的多是给人看的一句中文；
  必填空着不发调用；人在询问里按掉（输入结束 / 取消）就什么都不发生。
- **验证 1 的自动化程度**：菜单条目与解析、位置参数与实参形状、`prompts/get` 的往返都由单元 /
  集成测试钉住（`the_menu_shows_the_builtin_commands_skills_and_mcp_prompts`、
  `inline_arguments_fill_the_declared_parameters_in_order`、`templates_are_listed_per_server_and_
  rendered_with_the_arguments`）；「在真终端里点一次菜单、填一遍参数」那一步**没有**自动化
  （问卷的 TUI 交互本来就走真机走查那条线），留给真机看一眼。
- 验证：`cargo test`（全绿）· `cargo clippy --all-targets` · `cargo fmt` ·
  `python3 scripts/check-language.py`。

### 审查后的修正与收尾（2026-10-03）

- 两轴审查（Standards / Spec）跑过这一份 diff，修掉的：`slash_catalog` 的 rustdoc 被新函数切断
  （文档归属已还原）；`submission` 原来收两个同类型的 `impl Fn` 闭包（写反了编译器不管），现在
  直接收 `&[McpPromptEntry]`；措辞层里两条同格式的函数合成 `mcp_prompt_description`；补上票面
  验证 1 要的**两个模板**（现在是三台 server、四条拍平断言）；
- 补的测试：`missing_required_arguments`（必填缺那条路径）与「两台好的拍平、坏的那台整台不出现」。
- **一条口径写准**：清单是在**组装期、`assemble()` 之前**现问的那一次（不是「组装之后」）。
  因此会话中途某台 server 挂掉时，菜单里它的条目会留到本次会话结束 —— 与票面「不出现（不是出现后
  点开报错）」的差别在于「启动那一刻的可用性」，这条差别记在这里，不在实现里假装解决。
- **状态记 `ready-for-walkthrough`**：能自动化的部分都落地了（解析、参数、往返、降级都有测试），
  剩下的是**在真终端里点一次菜单、填一遍参数**看那一眼 —— 问卷的 TUI 交互本来就走真机走查那条线
  （`docs/agents/issue-tracker.md` 的约定）。
