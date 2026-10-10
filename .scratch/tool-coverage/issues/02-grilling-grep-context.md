# grep 的上下文参数：`after` / `before` 的形状

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

冻结项 7 定了大方向：`grep` 加 `after` / `before`（**不加 `context`**），并推翻
[`../grep-tool/spec.md:205`](../grep-tool/spec.md) 那条
「`ignore_case` / `context`（`-C`）/ `max_count` 等开关：用 pattern 的内联语法或多次调用解决」。
这条票要把**形状**定死，因为工具声明的每个字段都会进 provider 缓存前缀
（[ADR 0001](../../../docs/adr/0001-chinese-ui-frozen-model-text.md)、
[ADR 0003](../../../docs/adr/0003-plan-leaves-the-permission-modes.md)）。

### 要拍的问题

1. **输出形状**：命中行与上下文行怎么区分？ripgrep 的默认是命中行用 `:` 分隔
   （`path:12:文本`）、上下文行用 `-` 分隔（`path-11-文本`），两个块之间插一行 `--`。
   今天 `grep` 只有一种行形状（[`../../src/tools/grep.rs`](../../../src/tools/grep.rs) 的
   `Collector`）。三种候选：(a) 照抄 rg 的 `:` / `-` / `--`；(b) 上下文行也带真实行号、
   不加额外标记（模型自己数）；(c) 每块前面加一行头（`--- path:12 ---`）。
2. **行号**：上下文行给不给行号？rg 给。给了模型能直接 `read_file` 定位。
3. **`MAX_MATCHES` 怎么算**：500 条那条上限收的是"要列出来的行"——上下文行算不算？
   若算，`after: 30` 下十几个命中就撑满；若不算，输出总量可能爆掉。末尾那句
   「还有 N 条未列出」在两种口径下分别怎么数。
4. **与 `count` 的交互**：`count: true` 时 `after`/`before` 是忽略、报错，还是各自独立
   （计数模式本来一行都不列，忽略更自然）。
5. **`after` 与 `before` 同给、与 `glob` 组合**时的边界：比如 `after: 0` 是否合法、
   上限有没有（`after: 10000` 要不要夹住）、与 `count` 同给时谁优先。
6. **参数面的整体取舍**（顺带定，不许扩大成新工具）：实测 bash 内 grep 里还有
   `-l`（只列文件名）126 次、`-c` 295 次（`count` 已覆盖）、`-v` 236 次、`-o` 70 次、
   `-i` 117 次（`(?i)` 已覆盖）、`-E` 1016 次（`|` 已覆盖）。要不要顺带加
   `files_only`（`-l` 的等价）与 `invert`（`-v`）？还是这一轮只加 `after`/`before`。
7. **`path` 参数**：今天范围写死会话 cwd；实测目标在 cwd 之外的只有 0.9%，但读
   `~/.cargo/registry` 的依赖源码、读 `/tmp` 下载的资料都出现过。加不加 `path`，
   加了要不要接 `outside_read` 那套语义（[`../grep-tool/spec.md`](../grep-tool/spec.md) §3）。
8. **描述怎么改**：描述里那句「不要用 `bash` 拼 `rg` / `grep`」实测无效（八天密度不降），
   留着、改弱、还是换成"要上下文就用 after/before"这种正向说明。
9. **既有决定的推翻记录**：`grep-tool/spec.md` 那条《明确不做》要改成"已推翻"，
   附上这次的新证据（`-A` 903、`-B` 202、`-C` 1；严格可替代只有 24.2%）。

### 输入

- 现有参数面：[`src/tools/grep.rs`](../../../src/tools/grep.rs)（`pattern` / `glob` / `count`）、
  [`docs/grep.md`](../../../docs/grep.md)（参数表、输出与上限、计数模式）。
- 上游对照：[`docs/research/notes/shell-vs-first-class-tools.md`](../../../docs/research/notes/shell-vs-first-class-tools.md)
  启示第 6 条（Gemini 是唯一默认补上下文的一家，只在模型没显式要时补，声称 SWEBench 上
  减少约 10% 轮次）；Claude Code 的 `Grep` 也有 `-A`/`-B`/`-C`。
- 上一轮讨论里已经排除的路：不加 `context`（实测 `-C` 只 1 次），不做 stdin/管道。

### 约束

- 票内不得重开冻结项 7 的"加 `after`/`before`、不加 `context`"。
- 若结论要动 `docs/grep.md`，改完要过
  [`scripts/check-doc-size.py`](../../../scripts/check-doc-size.py) 与
  [`scripts/check-language.py`](../../../scripts/check-language.py)。
- 答案要自足：`/to-tickets` 在别处读它，看不到本图与 charting 对话。

## 作答

四问拍板（Q1 / Q2 / Q3 / Q4 / Q5）：

### 1. 输出形状：照抄 ripgrep（Q1 = a）

- **命中行**：`相对路径:行号:文本` —— 与今天完全一致，解析规则不变。
- **上下文行**：`相对路径-行号-文本`（分隔符是两个 `-`），**行号是真实行号**，模型可据此
  直接 `read_file` 续读。
- **块之间**：插一行 `--`。
- **相邻命中的上下文区间重叠时合并成一个块**（rg 的行为）：不要每个命中各画一遍上下文，
  否则同一段正文会重复出现。
- 路径仍相对会话 cwd；条目仍按路径有序（遍历器排过序，输出的稳定性跟着它）。

例（`after: 2`）：

```text
src/tools/grep.rs:47:pub const MAX_MATCHES: usize = 500;
src/tools/grep.rs-48-/// 一次调用最多列出多少条命中，超出时在末尾如实写出还剩多少条。
src/tools/grep.rs-49-///
--
src/tools/grep.rs:90:    fn effect(&self, _args: &Value) -> Effect {
```

### 2. 参数面：只加两个可选的整数

| 参数 | 必填 | 说明 |
| --- | --- | --- |
| `after` | 否 | 命中行**之后**再带 N 行，等价 `rg -A N` |
| `before` | 否 | 命中行**之前**再带 N 行，等价 `rg -B N` |

- 值域：非负整数；负数与非法类型 → **参数错误**（与 `pattern` 必填同一档，不静默忽略）。
- **不设上限、不加夹取**：输出被下面那条 500 收口，`after: 100000` 只会白扫不会爆。
- `after` 与 `before` 可同给；只给一个时另一侧为 0。
- **不加 `context`**（冻结项 7）、**不加 `files_only` / `invert`**（Q3 决定）：`-l`（126 次）
  会把输出形状从两种变三种，那是 [grep-tool spec](../grep-tool/spec.md) 留了口的
  `output_mode` 枚举，属于另一条线；`invert` 一并留给 05 票逐条判。
- **不加 `path`**（Q4 决定）：范围写死会话 cwd 是 `Effect::ReadOnly` 的支点；开了 `path`
  就要接 `outside_read` 那套"工具声明过的读路径"语义，为 0.9% 的用法不值得。
  若 05 票判「读依赖源码」是常态，再单独开票。

### 3. 上限：一条 500，数的是"列出的行"（Q2 = a）

- `MAX_MATCHES = 500` 的语义从"最多列出多少条命中"改成 **「最多列出多少行」——命中行 +
  上下文行合计**。常量不动、不新增第二把刀、不加夹取逻辑。
- 代价如实记下：`after: 30` 时十几个命中就撑满 500；`after` 再大也只是白扫。
- 末尾那句**改成数未列出的命中行数**（不是"行数"）：写「还有 N 条命中未列出：请缩小搜索范围，
  或用 `glob` 只搜一部分文件」。`N` 必须在整个工作区数完才是诚实的 —— 收集满 500 之后
  仍要继续数命中（今天的 `Collector` 已经是这个形状，只是口径从"命中"变成"列出的行"）。
- `count: true` 与 `after` / `before` 同给时 **忽略后者**（计数模式一行都不列，报错只是多一次
  往返）。与今天"类型不对按没给算"的宽松风格同档。
- 上下文行同样过 `max_tool_result_tokens` 那条流水线（落盘 + 头尾预览 + 指针），
  **不新增第二套截断**。

**实现提示**（省一次重扫）：`grep_searcher` 的 `Sink` 已经有 `context()` 回调，
`SearcherBuilder` 也有 `after_context(n)` / `before_context(n)` —— 不要在 `Collector` 里
自己重扫或补行。

### 4. 描述：把劝阻换成"用它能得到什么"（Q5 按推荐）

描述里那句「不要用 `bash` 拼 `rg` / `grep`」保留一句短的最强形式，其余篇幅用来正面说明：
`after` / `before` 解决"命中行本身没有信息量"，`count: true` 解决"只要数字"，
命中太多时输出会被截断并给出一条落盘路径。实证：劝阻句上线八天，bash 内 grep 的密度一天没降
（冻结项 2 的判据要靠量度而不是口号）。

### 5. 要同步改的既有文本

- [`.scratch/grep-tool/spec.md:205`](../grep-tool/spec.md)《明确不做》里那条
  「`ignore_case` / `context`（`-C`）/ `max_count` 等开关」→ 改成**已推翻**，理由写实测：
  `-A` 903 / `-B` 202 / `-C` 1；带上下文的 grep 占 grep/rg 全切段的 **23.2%**（1310 段），
  形态表里 `grep -A <N> -n <PAT> <PATH>` 是 664 段 / 584 调用（
  [形态底账](../research/01-shape-inventory.md)）；"内联语法或多次调用解决"这条替代方案
  八年份的数据里没有被采用。
- [`docs/grep.md`](../../../docs/grep.md)：参数表加 `after` / `before` 两行；「输出与上限」
  一节写清 `-` 分隔与 `--` 分块、500 的语义（含上下文行）、`N` 数的是命中行；
  「计数模式」一节加一句与 `after`/`before` 互斥（忽略）；描述那节把"劝阻"改写成
  "怎么用"。改完过 `check-doc-size.py` 与 `check-language.py`。
- **一次发布 vs 分两次**：这条改动会废一次所有存量会话的前缀缓存
  （[ADR 0001](../../../docs/adr/0001-chinese-ui-frozen-model-text.md)）。是否与
  [workdir 必传](03-grilling-bash-workdir-required.md) 合并成一次发布，由那条票一起拍
  （它也在改工具声明）。

### 6. 术语

- 「**命中行**」与「**上下文行**」的区分进 `docs/grep.md`；`MAX_MATCHES` 的语义从
  "命中条数"改成"列出的行数"。**不立 `CONTEXT.md` 词条** —— 它是模型可见文本的词汇表，
  而这两个词是工具输出形状的局部描述，住 `docs/grep.md` 就够。

### 留给别的票

- `files_only`（`-l`，126 次）、`invert`（`-v`，236 次）、`-o`（70 次）→
  [收编的逐条判定](05-grilling-intake-verdicts.md)。
- `path` 参数（cwd 之外 0.9%）→ 同上，若判为常态再开票。
- 真 token 影响、`after` 档位的实际分布 → [验收口径](06-grilling-measurement.md)。
