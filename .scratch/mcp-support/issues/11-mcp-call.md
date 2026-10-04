# 11 — `mcp_call`：转发一次外部调用

Type: implement
Status: done
Part of: ../map.md
Blocked by: 10

> 规格：[`../spec.md`](../spec.md) §2、§6、§7。

## 目标

模型能调 `mcp_call(["github"], "create_issue", {…})`，参数**原样透传**给 server，结果带不可信标记
回到流上；这次调用**默认按最严的副作用**处理（每次都要过权限门）。

## 具体行为

1. **参数面**：`mcp_call(server, tool, arguments)` —— `arguments` 是自由 JSON 对象，**不校验**
   （schema 在 server 那侧，我们只透传）。
2. **`effect()` 恒 `Effect::Exclusive`**（缺省）：`readonly` 档拒、`ask` 档问、`workspace` / `auto`
   放行；调度器为它取工作区锁。放宽归 [票 14](14-effect-and-trust.md)。
3. **结果带不可信标记**（除非那台 server 开了 `trust_results`，归票 14）。
4. **上限走既有流水线**：`call()` 返回字符串，`context::truncate_result` + `emit_completed` 负责
   落盘与指针；不新增按工具的预算字段。
5. **失败一律结构化**：未知 server / 未知工具 / server 侧失败都回一条可读结果（code 英文、句子中文）。

## 验证

`cargo test` + `cargo clippy --all-targets` + `python3 scripts/check-language.py`：

1. **透传**：假连接断言收到的 `arguments` 与模型给的逐字相同（含嵌套对象与空对象）。
2. **`Effect`**：`readonly` 档拒、`ask` 档问；反向锚是 `bash` 的同类行为。
3. **标记**：结果以那句中文标记开头。
4. **超限**：造一个超 `max_tool_result_tokens` 的结果，断言落盘 + 指针。
5. **失败**：server 侧报错时是一条可读的结构化错误，且流上恰好一条结果。
6. **执行者**：执行者表里有 `mcp_call`，且它跑在派发者的模式之下。

## 不做什么

- 不做信任放宽（票 14）、不接真连接（票 12）、不做资源与提示词（票 16 / 17）。

## 评论

- 2026-10-03 落地（`Status: done`）。落点：`src/tools/mcp_call.rs`（新）、`src/mcp/mod.rs`
  （`McpService::call_tool`）、`src/tools/mod.rs`（注册与 re-export）、`tests/mcp_call.rs`（新，9 条）。
- 两处实现里定的细节：
  - `arguments` 缺席或 `null` 当成 `{}` 透传；给了非对象（数组、字符串）是**参数错误**
    （`Err`，不是结果）—— 「自由」指的是内部形状不校验，不是可以给个数组。
  - 失败结果不带那句外部内容标记（web 那条工具的错误结果同样不带）：错误消息是我们自己写的
    中文，不是外部内容；只有成功结果才以标记开头。
- 验证 6 的后半句「跑在派发者的模式之下」不由本票新写：执行者拿的是派发者工具表的
  `for_executor()`，而「委派向下不继承允许」那条纪律住在 `src/agent/executor.rs`，由
  `tests/executor.rs` 的 `a_readonly_dispatcher_keeps_its_executor_read_only` 与
  `a_denial_travels_down_to_the_executor` 覆盖；`mcp_call` 的 `effect()` 是 `Exclusive`，
  与 `bash` 同类，所以那些覆盖原样适用。本票断言的是执行者表里有 `mcp_call`。
- 验证：`cargo test`（新增 9 条全绿）· `cargo clippy --all-targets`（无 warning）·
  `cargo fmt` · `python3 scripts/check-language.py` 全通过。
