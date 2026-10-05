# 01 — `read_file` 的读窗口

Type: implement
Status: done
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1–§7。本票是这份 spec 的全部：参数、默认封顶、续读提示、
> 越界与畸形、读集不变、权限不变、`limit` 不夹。工具只有这一个，不再拆第二张票。

## 目标

模型对 `read_file` 能说「读第 40001 行起的 200 行」，拿到的是**带文件真实行号**的那一段；
还有没读到的行时，结果末尾告诉它下一段从哪读。不传这两个参数时，2000 行以内的小文件与今天
**一字不差**。

## 现状（2026-10-06 核实，改前先复核）

- `ReadFile` 在 [`src/tools/file.rs`](../../../src/tools/file.rs)：`spec()` 在 `:112-128`
  （只有 `file_path`，`required: ["file_path"]`）、`effect()` 恒 `ReadOnly` 在 `:130-132`、
  `read_paths()` 在 `:134-141`、`call()` 在 `:143-155`（读全文、`content.lines()` 逐行
  打印 `{行号}\t{行}`）。
- 参数解析的风格先例是 [`bash` 的 `requested_timeout_ms`](../../../src/tools/bash.rs)：
  `args.get(...)` + `as_u64()`，存在但不是正整数 = 参数错误（不静默回退）；`nullable_string`
  在 `src/tools/file.rs:359-363`，管的是「模型发 `null` 当缺席」。
- 读集是**一组路径**（[`ReadSet`](../../../src/tools/tool.rs)，`src/tools/tool.rs:168-200`），
  由 `read_paths()` 声明、由 `guardrails`/`dispatch` 在工具之外施加；
  `read-before-edit` 那道闸在 `src/tools/registry.rs`。`tests/tools_dispatch.rs` 的
  `a_write_to_an_unread_file_is_refused_before_the_tool_runs` 钉着它。
- 结果上限流水线是 [`context::truncate_result`](../../../src/context.rs)（`src/context.rs:336`），
  唯一调用点在 `agent.rs` 的 `emit_completed`；上限 `max_tool_result_tokens` 缺省 25k token。
- codex 的先例数字：`DEFAULT_MAX_LINES_TEXT_FILE = 2000`
  （[`docs/research/notes/codex-gemini.md`](../../../docs/research/notes/codex-gemini.md)）。

## 落点

`src/tools/file.rs`（`ReadFileArgs`、`ReadFile::spec`、`ReadFile::call`、新的默认值与提示常量）、
`tests/tools_dispatch.rs` 或按邻居的组织新开一个测试文件。

## 具体行为

1. **参数**：`offset`（`integer`，1-based 行号，可选，默认 1）、`limit`（`integer`，行数，
   可选，默认 `DEFAULT_READ_LINES = 2000`）。`required` 仍是 `["file_path"]`。
2. **窗口**：`lines()` 的第 `offset-1` 到 `offset-1+limit` 行（右开），逐行打印
   `{文件里的真实行号}\t{行}`，编号不从 1 重来。
3. **续读提示**：还有未读行时，正文最后加一行
   `（第 X–Y 行，共 N 行；续读 offset=Z）`（X/Y 是这次实际读到的首末行号，Z = Y + 1）。
   读到末尾时不加这一行 —— 2000 行以内的小文件输出与今天完全一致。
4. **越界**：显式给了 `offset` 且 `offset > N` ⇒ `ToolError::Message`，消息里带
   `文件只有 N 行`；**不**返回 `InvalidatesReads`（那是「图景陈旧」，这是一次参数错）。
5. **空文件**（N = 0）不传 `offset` ⇒ 空正文，与今天一致；显式 `offset` 对 0 行文件 ⇒
   报「文件只有 0 行」。
6. **畸形参数**：`offset` / `limit` 存在但不是正整数 ⇒ 参数错误（`ToolError::Message`），
   不静默回退到默认值。
7. **`limit` 不夹**：模型给多大就是多大；撞上 25k token 的界时由既有的 `truncate_result`
   落盘 + 指针兜底，不新增第二套上限。
8. **读集不变**：`read_paths()` 一字不动（仍声明整条路径），于是窗口读之后 `edit_file` 与
   `write_file` 的覆写照常放行。
9. **描述**：`spec()` 的 description 里把两个参数的语义与默认值写清（模型要知道默认封顶与
   怎么续读），并保留今天那句「返回带行号的内容」。

## 验证

`cargo test` + `cargo clippy --all-targets` + `cargo fmt --check` + 两道文档护栏：

1. **窗口取的是那一段，行号是绝对行号**：造一个 5 行文件，读 `offset=2, limit=2`，断言
   正文恰是第 2、3 行，且带 `2` / `3` 两个行号。
2. **默认封顶与续读提示**：造一个 `DEFAULT_READ_LINES + k` 行的文件，不传参数，断言读到
   第 2000 行、末尾那行的 `offset` 是 2001、并含总行数。
3. **读完时没有提示**：小文件整读的结果不含续读那一行（钉住「小文件输出一字不差」）。
4. **越界报可行动的错**：`offset` 落在末尾之后，断言失败消息含「文件只有」与真实行数；
   并断言这次失败**没有**失效读集（随后 `edit_file` 仍放行）。
5. **畸形参数**：`offset=0`、`limit=0`、非整数各自报参数错误，且**没有**碰文件内容。
6. **窗口读之后能改**：读 `limit=1` 的一段，随后 `edit_file` 改窗口外的另一行 —— 成功。
   这条钉的是 spec §5 那条边界被有意接受：读集是路径级的。
7. **空文件**：不传 `offset` 读空文件成功且正文为空；`offset=1` 报「文件只有 0 行」。
8. **只读性质不变**：`effect()` 仍返回 `Effect::ReadOnly`（既有断言在
   `tests/tools_dispatch.rs` 的 `the_scheduler_partitions_calls_by_declared_effect`）。

## 不做什么

- 不加字节偏移、不加 `end_line`、不改 `edit_file` 的匹配范围（spec §8）。
- 不让 TUI 的调用描述带上窗口（spec「明确不做」）。
- 不给读集加区间概念（spec §5 的已知边界）。
- 不动 `grep`、`write_file`、截断流水线、权限四档。

## 评论

实现落点：`src/tools/file.rs`（`ReadWindow` / `read_window` / `positive_line_arg` /
`resume_note` / `DEFAULT_READ_LINES`）、`src/tools/mod.rs`（导出那个常量）、
`tests/tools_dispatch.rs`（11 条新测试，用现成的派发 fixture）。

实现期收口的三处细节（票面留白写实，spec 未改）：

1. **窗口参数不走 serde 类型化。** `ReadFileArgs` 只留 `file_path`；`offset` / `limit` 用
   `args.get()` + `as_u64()` 显式校验（照 `bash` 的 `requested_timeout_ms`）。走 serde 也能
   拒掉畸形值，但交出来的是一句 serde 的泛化消息，点不到是哪个参数。
2. **`offset` 留成 `Option<usize>`。** 空文件上「没给」与「给了 1」判得不一样：不给我返回空
   正文（与今天一致），给了我就是「没有第 1 行，文件只有 0 行」。一个 `usize` 表达不了这个
   差别。
3. **续读提示只在 `end < total` 时追加**，所以整读一个小文件的输出与加窗口之前一字不差；
   `tests/tools_dispatch.rs` 的 `a_small_file_still_reads_exactly_like_before` 用精确相等
   钉住它，而不是 `contains`。

没动的：`read_paths()`、`effect()`、`edit_file` 的匹配梯、截断流水线、权限四档、事件 schema。
`DEFAULT_READ_LINES` 是这次唯一新增的公开名字（schema 描述与测试共用它一处）。

4. **两端都不溢出**：窗口末行的算法是 `first - 1 + limit` 的**饱和**相加 —— 模型给
   `limit = u64::MAX` 时它顶到 `usize` 的天花板，而 debug 构建下的一次 panic 比一个荒唐的
   数字糟；`an_absurd_limit_reads_to_the_end_instead_of_overflowing` 钉住这条（它同时断言
   这种情况下读到末尾且不带续读提示）。
