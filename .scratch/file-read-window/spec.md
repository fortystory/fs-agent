# `read_file` 的读窗口：`offset` / `limit`

Status: 1/1 done（2026-10-06 一轮 grilling 折成；一张实现票
[`issues/01`](issues/01-read-file-window.md)）

模型的 `read_file` 今天只有一种读法：**整个文件**。大文件下这条路两头都堵。

一头在模型这侧：它没有别的读法。文件 10 万行、只想看第 4 万行附近，它只能整份拉进来。
另一头在流水线上：真拉进来之后，[`context::truncate_result`](../../src/context.rs) 把它折成
头尾各半的预览加一条落盘指针。模型于是花了整整一次读的预算，换来一份**中段全看不见**的东西，
而它连「这文件到底多长、该从哪续读」都答不上来。

这份 spec 给 `read_file` 加一个**读窗口**：`offset`（1-based 行号）与 `limit`（行数），
默认封顶 2000 行，并在还有未读行时如实给出续读的下一句。

来源：2026-10-06 维护者的需求（「优化下 `read_file`，添加 `offset`/`limit` 参数，这样在遇到
大文件时可以读取其中一部分」）+ 一轮三问的 grilling。三个落点：默认封顶；越界报可行动的错；
按 issue tracker 约定落一份 spec 与一张票。

## 问题陈述

1. **没有「读一部分」这条线。** `ReadFile::call` 读全文、按 `content.lines()` 逐行编号
   （[`src/tools/file.rs`](../../src/tools/file.rs)）。模型面只有 `file_path` 一个参数，
   所以「只想看某一段」在工具上无法表达。
2. **大文件的默认路径对模型没用。** 超过 `max_tool_result_tokens`（默认 25k 估算 token）的
   结果被 [`truncate_result`](../../src/context.rs) 切成头 + 标记 + 尾，全文落盘。头尾预览
   是给「被人读的一眼」设计的，不是给「模型继续干活」设计的：中段既看不见，也不可寻址。
3. **先例把数字摆在那儿。** codex 的 `DEFAULT_MAX_LINES_TEXT_FILE = 2000`
   （[`docs/research/notes/codex-gemini.md`](../../docs/research/notes/codex-gemini.md)），
   Gemini 的 `read_file` 收 `start_line` / `end_line`。行式窗口是这一层的共识，不需要发明。

## 方案

- **两个可选参数**：`offset`（1-based 行号，默认 1）、`limit`（行数，默认 2000）。
- **行号是文件里的真实行号**，不是窗口内的相对序号。模型的图景、它写给 `edit_file` 的
  `old_string`、以及 `truncate_result` 的落盘全文三者靠这一个坐标系对齐。
- **还有未读行就在末尾说清下一句**：`（第 X–Y 行，共 N 行；续读 offset=Z）`。全部读完时
  不加这一行，于是小文件的输出与今天**一字不差**。
- **越界是可行动的错**：显式 `offset` 越过末尾报「文件只有 N 行」；畸形参数报参数错。
- **读集仍按路径登记**：读窗口与整读在 `read_paths()` 上同形，所以 `edit_file` 照常放行。
- **不新增第二套截断**：窗口再大也由既有的 `truncate_result` 兜底。

## 实现决定

### §1 参数：名字、单位、默认值

`read_file(file_path, offset?, limit?)`。`offset` 是 1-based 的**行号**，`limit` 是**行数**；
`offset` 默认 1、`limit` 默认 [`DEFAULT_READ_LINES`](../../src/tools/file.rs) = 2000。
`required` 仍只有 `file_path`。

按行不按字节：`edit_file` 的坐标、`grep` 输出的 `path:line:文本`、以及今天 `read_file`
自己打印的行号全是行。字节偏移在这里没有任何一个下游能对上。

2000 这个数字照抄 codex 的 `DEFAULT_MAX_LINES_TEXT_FILE`（引文见问题陈述第 3 条）。
它落在 25k token 的结果上限之下：一屏 2000 行的普通源码约 2 万字符，离截断还远。

### §2 行号是绝对行号

窗口里的第 i 行显示它在文件里的真实行号（`40001\t…`），也就是今天的同一个格式。
窗口内重新从 1 数，会让「模型从结果里抄给 `edit_file` 的上下文」与「文件真实内容」错位，
而那正是 `edit_file` 唯一依赖的东西（§8 的降级梯按文本匹配，不按行号，但模型的判断按行号）。

### §3 续读提示：一行，只在还有未读行时出现

形状：`（第 1–2000 行，共 12345 行；续读 offset=2001）`。最后一个窗口读到底时不出现——
那时「读完」这件事本身已经由结果自证。这一行不参与 `truncate_result` 的判定，它只是结果
正文的一部分。

它与截断标记（`[已截断：…`，[`context::TRUNCATED_MARKER`](../../src/context.rs)）是两回事：
截断说的是「这条结果被流水线裁过、全文在某个文件里」，续读提示说的是「这个文件还有你没读到
的行，换个 `offset` 就能读」。一个窗口大到自己也撞上截断时，两条会同时出现在结果里，各说
各的，不冲突。

### §4 越界与畸形参数

- 显式给了 `offset`，而 `offset > 总行数` ⇒ `ToolError::Message`：`文件只有 N 行`。
- 空文件（0 行）+ 没给 `offset` ⇒ 返回空正文，与今天一致（不报错）。
- `offset` / `limit` 存在但不是正整数（0、负数、小数、字符串）⇒ 参数错误，不静默回退。
  照 [`bash` 的 `requested_timeout_ms`](../../src/tools/bash.rs) 那条先例：畸形就是错，
  不是回落到默认值。
- 越界**不**失效该路径的读集：这不是「agent 的图景陈旧」的信号（那是 `edit_file` 的
  `EditError::NoMatch` 的语义），而是一次参数错，读权限没有理由被收回。

### §5 读集仍按路径登记，`edit_file` 照常放行

`read_paths(args)` 的契约是「这次调用看过哪个路径」，它**没有行区间**的概念，
[`ReadSet`](../../src/tools/tool.rs) 也就是一组路径。读窗口不改这条：读过一段算读过这个文件，
随后 `edit_file` 与 `write_file` 的 `read-before-edit` 闸照常放行。

**已知边界（本次不动）**：模型可能改了窗口外的行却「以为自己读过」。这是既有的路径级粒度
——今天整读也一样可能改到自己没细看的地方。要收紧就得给读集加区间，那会同时改变
`grep` 命中与执行者继承的语义，属于另一次决定。

### §6 权限与副作用一字不动

`effect()` 仍是 `ReadOnly`：四档权限模式全放行、不取锁、`read_paths` 的发放在
`guardrails` 里走原路。窗口是参数，不是新能力。

### §7 `limit` 不设天花板

模型显式给的 `limit` 一律尊重，工具内不夹。理由：夹一个数字是第二套上限，而这一层已经有一条
统一的上限流水线（§8）。模型给 `limit=100000` 是显式请求，撞上 25k token 的界时由
`truncate_result` 落盘并给指针——与今天读全文撞界完全同一条路。

## 明确不做

- **字节偏移**：`offset` 一律行号。
- **TUI 的调用描述带上窗口**：那一行仍是 `调用 read_file src/lib.rs`（描述从参数推出，
  [`wording::tool_description`](../../src/render/wording.rs) 只取路径）。窗口的价值在结果里；
  要显示 `src/lib.rs:1200-1400` 是渲染层的一次独立改动。
- **批量读 / 读目录 / glob**：`read_file` 还是读一个文件。
- **给 `grep` 命中算「已读」**：那条边界在 [`grep` 的 spec](../grep-tool/spec.md) 里已经定过，
  本次不动。
- **`edit_file` 的窗口内匹配**：`old_string` 照旧在全文件里匹配（那是它的契约）。
- **不给 `read_file` 新开一份逐面文档**：它今天没有，而 `docs/` 的逐面文档是给有独立机制的面
  （`grep` / `repo_map` 那种）。窗口的行为写进工具描述与这份 spec 就够。

## 决定速查

| 问 | 落点 |
| --- | --- |
| 不给 `limit` 时读多少 | 默认封顶 2000 行，末尾告知续读（§1、§3） |
| `offset` 越界 | 明确报「文件只有 N 行」，不失效读集（§4） |
| 畸形参数 | 参数错误，不静默回退（§4） |
| 读一段之后还能不能改 | 能：读集按路径登记（§5） |
| `limit` 上限 | 不夹，交给既有的截断流水线（§7） |
| 行号坐标 | 文件里的绝对行号（§2） |
