# 文件枚举工具：只读的 `ls` / `find` 替代

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

[收编的逐条判定](05-grilling-intake-verdicts.md) 判「做」的**唯一一条**：把 `ls`（各种形态合计
728 段）与 `find`（104 段）合成一条**只读**的文件枚举工具。写侧（`mkdir` 78 / `rm` 27 / `mv` /
`cp`）在同票判了**不做**。本票把形状定死；工具声明进 provider 缓存前缀
（[ADR 0001](../../../docs/adr/0001-chinese-ui-frozen-model-text.md)）。

### 要拍的问题

1. **语义：一条工具还是两件事**。"列一个目录里有什么"（`ls`）与"按模式找工作区里的文件"
   （`find` / glob）是不是同一条工具？上游三家各不相同：Claude Code 是 `Glob`（按模式、只回路径）、
   Gemini 是 `list_directory`（列目录）、opencode 是 `glob`。
2. **名字**：`glob` / `list_dir` / `find` / `files`？名字进前缀，一次定死。
3. **忽略规则**：遵守 `.gitignore` 与隐藏文件规则（与 [`grep`](../../../src/tools/grep.rs) /
   `repo_map` 同档）吗？注意上游有一个反例：Claude Code 的 `Glob` **默认不遵守** `.gitignore`
   （与它自己的 `Grep` 相反）。heng 该不该让两个工具用同一套规则。
4. **范围与深度**：固定会话 cwd（与 `grep` 同例，不声明读路径、不接 `outside_read`）？要不要
   `depth` 参数（实测 `find` 的用法里 `-maxdepth` 常见）。
5. **输出形状**：一行一条相对路径？带文件大小 / mtime？排序按路径还是按修改时间（Claude Code
   是按 mtime、新到旧）？上限多少（Claude Code 是 100）？超限时照例给一句可操作的收尾。
6. **与 `grep` / `repo_map` 的分工**：`repo_map` 答"有哪些符号"、`grep` 答"某个名字在哪"、
   这条答"有哪些文件"。三者的边界要一句话写清，免得模型在两条路之间来回试。
7. **`tree` 那一档**：层级显示要不要（实测该形态很小）。
8. **`effect()`**：`ReadOnly`（这一条没有争议，零权限成本）。`path` 参数要不要（与
   [grep 的判例](02-grilling-grep-context.md)一致：不加，保住"只扫工作区"这个支点）。

### 输入

- 形态数据（[形态底账](../research/01-shape-inventory.md)）：`ls <PATH>` 227 段 / 200 调用、
  `ls <PATH> 2> <DEVNULL>` 62 段、`ls -la` 52 段，`ls` 全体 728 段；`find` 104 段。
- 同类实现的现成形状：[`grep.rs`](../../../src/tools/grep.rs)（只扫 cwd、遵守 `ignore` 的默认、
  输出稳定排序、`MAX_MATCHES = 500` 与末尾收尾）、[`repo_map.rs`](../../../src/tools/repo_map.rs)。
- 上游对照：[`docs/research/notes/shell-vs-first-class-tools.md`](../../../docs/research/notes/shell-vs-first-class-tools.md)
  （Claude Code `Glob` 与 `Grep` 的忽略规则相反、Gemini `list_directory`、opencode `glob`）。
- 冻结项 6：优先改参数，新工具只开给既有工具**结构上接不住**的类别 —— 本票的依据是
  "枚举既不是搜索（`grep`）也不是读文件（`read_file`）"。

### 约束

- 不得重开 [05](05-grilling-intake-verdicts.md) 的判定（只做只读枚举、写侧不做）。
- 答案自足：`/to-tickets` 在别处读它。
- 若新增 `docs/<工具名>.md`，改完过
  [`scripts/check-doc-size.py`](../../../scripts/check-doc-size.py) 与
  [`scripts/check-language.py`](../../../scripts/check-language.py)。

## 作答

形状由折 spec 那一步定下（本票是 frontier 上最后一张，`/to-spec` 直接把答案写进了
[`spec.md`](../spec.md) 的实现决定 §4）。落点：

- **一条工具、一个字段**：`pattern`（必填，相对会话 cwd 的 glob，支持 `**`）；
  **不加 `path`** —— 路径前缀就写在 pattern 里，"只扫工作区"那个支点原样保留。
- **名字 `glob`**（上游同名，模型最熟；目录语义由 `src/*` 这类 pattern 覆盖）。
- **忽略规则与 `grep` 同一套**：遵守 `.gitignore`、跳过隐藏文件与隐藏目录。上游有一家让
  `Glob` 与 `Grep` 用两套规则，本仓库不跟 —— 同一张工具表里的两个工具看见的世界必须一致。
- **输出**：一行一条相对路径，**按路径排序**（与 `grep` / `repo_map` 同一个稳定性口径；
  不按 mtime——那要么多一次 stat、要么让输出不可预期）；不带大小与时间。
- **上限 `MAX_PATHS = 500`**（与 `grep` 的 `MAX_MATCHES` 同值，少一个新常量）+ 末尾一句可操作的收尾。
- **`effect()` = `ReadOnly`**；**不做** `tree` 那档层级缩进。
- 描述里一句话写清三方分工：符号地图答"有哪些符号"、`grep` 答"某个名字在哪"、本工具答"有哪些文件"。

**三处是按已知惯例推断、不是逐条拍过的**：工具名取 `glob`、忽略规则与 `grep` 同一套、
排序按路径而非 mtime。改起来都便宜 —— 只需动 [`spec.md`](../spec.md) 的 §4 与《补记》。
