# `grep`：一个只读、只扫工作区的搜索工具

Status: 4 ready-for-agent（2026-10-02 由 [`seed.md`](seed.md) 折成 spec，同日拆出
[`issues/01`](issues/01-grep-tool-tracer-bullet.md)–[`04`](issues/04-docs-and-index.md)；
blocking edges 是 `01 → {02, 03} → 04`，每张票抬头写着自己被谁 block）

模型今天要搜代码，只有一条路：经 `bash` 拼一条 `rg` / `grep`。那条路上的两件事都不对。
**权限代数**里，一次纯读取的搜索长得像一次可能写盘的操作（`bash` 的 `effect()` 恒为
`Exclusive`），于是它在默认的 `ask` 档下每次都要打断人。**结果**则全凭模型自己拼参数、
自己决定截到哪儿 —— 仓库里那套「超限落盘 + 头尾预览 + 指针」的约定它拿不到。

这份 spec 给模型一个内建的 `grep(pattern, glob?)`：只读、只扫会话 cwd、遵守 `.gitignore` 与
隐藏文件规则，输出形如 `path:line:文本`，溢出交给既有的截断与指针流水线。

来源：2026-10-02 一轮 `/wayfinder` 会话 —— 从「需求池里哪条优先做」出发，选定 `grep-tool`
之后经五轮 grilling 折成这份 spec。**路线判定落在「不建决策图」**：destination 钉完（把这条
折成 spec）之后没有 fog —— seed 的四条分叉里两条已有仓库约定（与 `repo_map` 的分工、输出上限
复用 `context.rs`）、一条是纯加法（输出整形），剩下一条（依赖路线：自带库还是 spawn `rg`）
由一份一手调研关掉。调研产物在
[`research/01-search-tool-implementation.md`](research/01-search-tool-implementation.md)：
它记了 `Effect` 三类与每个工具的分类、两条路线的五维对照、上游八个实现的做法、以及九条待拍板点。
本次的每一问与落点见文末《决定速查》。

## 问题陈述

1. **一次纯读取的搜索，在权限代数里是 `Exclusive`。** `bash` 的 `effect()` 恒为
   [`Effect::Exclusive`](../../src/tools/bash.rs)（注释在 [`src/tools/tool.rs:29-31`](../../src/tools/tool.rs)：
   「一个 shell 什么都能写」）。于是模型搜一次代码，在权限门那里付的是一次「可能写盘」的账：
   默认的 `ask` 档要问你一次，`readonly` 档直接拒。
2. **`workspace` 档已经把这条动机补掉了一半 —— 但只补了一半。** 那一档下 `Exclusive` 的门裁决
   已经是 `Allow`（同 `auto`，[`src/permissions.rs:173-176`](../../src/permissions.rs)、
   [`workspace-mode/spec.md`](../workspace-mode/spec.md) §3），所以「让区内自动对搜索生效」这句话
   在 `workspace` 档不再成立。可是交互式的**默认档是 `ask`**（[`README.md:207`](../../README.md)），
   默认档下每一次纯搜索仍然要过审批 —— 工具本身还是缺。
3. **结果的形状与上限没人管。** 模型自己拼的 `rg` 输出可能是一次几千行的转储；它可以被
   `truncate_result` 兜住（任何工具结果都过那条流水线），但兜住的方式是「头尾各半 + 中间进
   `.txt`」，模型既不知道被省掉了多少，也没有一句可操作的收尾。

## 方案

- **一个内建工具**：`grep(pattern, glob?)`，进 `builtin()` 的表（[`src/tools/mod.rs`](../../src/tools/mod.rs)），
  不走 `config.toml` 的动态声明。理由见 §1。
- **`effect()` 恒为 `ReadOnly`**：四档权限模式全放行（含 `readonly`）、不取工作区锁。这是这条
  工具存在的第一理由。
- **只扫会话 cwd，不接受 `path` 参数**：遍历范围写死在实现里，与 `repo_map` 同例（自己走 cwd、
  不声明读路径），于是它绕开 `outside_read` 那套「工具声明过的读路径」语义，也就不必为「读区外」
  新开一条口径。
- **遵守 `.gitignore`、跳过隐藏文件**：与 rg 的默认一致 —— 换工具不该改变搜索结果。
- **输出 `path:line:文本`，一行一条**：rg 的默认形状，省 token、模型熟。
- **上限不做第二套**：工具只返回字符串，溢出、指针、头尾预览全交给
  [`context::truncate_result`](../../src/context.rs)；匹配太多时在末尾如实写清省掉了多少（同
  `repo_map` 的收尾）。
- **描述里把模型从 `bash` 那边引过来**：不写这一句，模型很可能继续拼 shell，上面那条权限收益
  一分也拿不到。
- **实现路线取自带 ripgrep 拆出的库**：`ignore` + `grep-searcher` + `grep-regex`，见 §4。

## 实现决定

### §1 位置与分类：内建，且是 `ReadOnly`

- **进 `builtin()`，不走 `with_dynamic()`。** 工具表是缓存前缀的一部分、建完不再变化
  （[`src/tools/mod.rs:1-10`](../../src/tools/mod.rs)）—— 一个每次会话都在的内建工具只是让前缀
  变一次；而动态声明工具的 `effect()` 恒为 `Exclusive`
  （[`src/tools/custom.rs:73-77`](../../src/tools/custom.rs)），正好是要修的那一类。
- **`effect(&self, _args) -> Effect::ReadOnly`**，与 `read_file` / `skill` / `repo_map` 同档
  （对照表见调研 §1.2）。它买到的是两件事，都不是并发：
  - **权限门**：四档对 `Effect::ReadOnly` 的立场都是 `Allow`（`readonly` / `ask` / `workspace`
    在 [`src/permissions.rs:129-153`](../../src/permissions.rs)，`auto` 在 `:178-182`），
    所以 `readonly` 档可用、`ask` 档不问；
  - **锁**：`Registry::dispatch` 只对 `Exclusive` 取工作区锁、对 `WritePaths` 取逐路径锁，
    `ReadOnly` 不取（[`src/tools/registry.rs:178-185`](../../src/tools/registry.rs)）。
- **不承诺并发。** `ReadOnly` 今天**不**让工具调用并行：一批里只有 `task` 走 `Deferred`，
  其余按这一批的顺序跑（[`src/agent.rs:1030-1067`](../../src/agent.rs)）。这条工具不改变那件事，
  spec 也不拿它当卖点。

### §2 遍历范围：cwd + `.gitignore` + 隐藏文件

- **只扫 `ctx.cwd`，schema 里没有 `path`。** `outside_read` 那条旋钮只作用于「工具声明过、
  且 `resolve_read` 失败的读路径」（[`src/tools/registry.rs:131-142`](../../src/tools/registry.rs)）；
  像 `repo_map` 那样不声明读路径、自己走 cwd 的工具完全绕过它
  （[`src/tools/repo_map.rs:7-8`](../../src/tools/repo_map.rs)）。本工具照此办理：范围由实现写死，
  不由权限门兜。
- **忽略规则 = `ignore` crate 的默认**：遵守 `.gitignore`（含 `.ignore` 与 git 全局忽略）、
  跳过隐藏文件与隐藏目录。这正好也避开 `target/` 与 `.git/`。
- **不做「全扫」的逃生口**：这一版没有 `include_ignored` / `include_hidden` 开关（见《明确不做》）。
- **二进制文件**：交给 searcher 的既有策略（二进制里不打印匹配行），本工具不自己判。

### §3 参数面与工具声明

线级声明（进前缀缓存，一次定死）：

```jsonc
{
  "type": "object",
  "properties": {
    "pattern": { "type": "string", "description": "要搜的正则（rg 语法）。" },
    "glob":    { "type": "string", "description": "可选，只搜匹配这个 glob 的文件，例如 `*.rs`。" }
  },
  "required": ["pattern"]
}
```

- **只有 `pattern` 必填、`glob` 可选。** 大小写这类需求用 pattern 的内联语法解决（`(?i)`），
  不为它开参数：工具声明是缓存前缀的一部分，每多一个参数都是永久成本。
- **描述里写清楚三件事**：这是搜索代码的首选方式（**不要用 `bash` 拼 `rg` / `grep`**）；
  范围是会话工作区、遵守 `.gitignore`；结果形如 `path:line:文本`，太多时会被截断并给出一条
  落盘路径。
- **`glob` 只影响「搜哪些文件」，不影响 pattern。** 它交给 `ignore` 的 `OverrideBuilder`
  （或 `globset`）做，不是把 glob 拼进正则。
- **工具名 `grep`。** 模型对这个词最熟，也与意向同名；改名有缓存前缀成本，一次定死。
- **`read_paths()` 不重写**（用 trait 的默认空实现，[`src/tools/tool.rs:213-215`](../../src/tools/tool.rs)）：
  命中的文件**不算「已读」**，所以随后 `edit_file` 仍要求先 `read_file`
  （read-before-write 在 [`src/tools/registry.rs:271-293`](../../src/tools/registry.rs)）。
  这是刻意的：`read_paths(&args)` 是调用前的纯函数，声明不了运行时才知道的命中文件；要登记就
  得动 `Tool` 接口，而这条工具不值得那次结构性改动。

### §4 实现路线与依赖

**路线 A：自带 ripgrep 拆出来的库。**

| crate | 用途 | 记录的版本 |
| --- | --- | --- |
| `ignore` | 目录遍历 + `.gitignore` / 隐藏文件规则 + glob 过滤 | 0.4.33 |
| `grep-searcher` | 逐文件搜索、行号与偏移 | 0.1.17 |
| `grep-regex` | 把 pattern 编译成 searcher 用的匹配器 | 0.1.14 |

- 三者都是 `Unlicense OR MIT`，纯 Rust，最小组合让 `Cargo.lock` 约 +11~14 个包；版本以调研
  记录的为准，落地时取当时的稳定版。
- **为什么不是 spawn `rg`**（调研 §4 的五维对照，摘要）：`rg` 经 `process::run` 跑起来会继承
  沙箱的「整机可读」（[`docs/sandbox.md:22`](../../docs/sandbox.md)），与 `Effect::ReadOnly`
  的本意（只读**工作区**）不是一回事；而且它换来三条运行期依赖 —— `rg` 在不在 PATH、`rg` 自己
  的配置文件会改行为（要 `--no-config`）、退出码 1 与 2 要自己翻译。路线 B 的真实优势只有
  「0 新 crate、代码量小」；仓库已经容忍「首次构建慢」这一档代价（9 个 tree-sitter C parser），
  所以这里选范围干净的那条。
- **不新增配置项。** 默认就是内建工具的行为；`max_tool_result_tokens` 照旧由 `SessionConfig`
  决定（缺省 25_000，[`src/config.rs:57`](../../src/config.rs)）、不进 `config.toml`。
  这一版**不**照 `repo_map_tokens` 再开一个按工具的预算字段 —— 先用统一上限，不够再说。

### §5 输出形状与上限

- **一行一条：`相对路径:行号:文本`**（相对 `ctx.cwd`）。没有匹配时返回一句如实的说明
  （「在工作区里没有匹配 `<pattern>` 的行」），而不是空字符串 —— 空结果与「工具坏了」在流上
  必须分得开。
- **上限只有一条流水线**：`call()` 返回字符串，`emit_completed` 里过
  [`context::truncate_result`](../../src/context.rs)（[`src/agent.rs:2043-2054`](../../src/agent.rs)）：
  没超限原样进事件；超了整份写进 `<outputs_dir>/<tool_call_id>.txt`（0600），事件里留头尾预览
  与指针（`[已截断：共 N 字符，约 M token；全文在 <path>]`）。
- **工具内先按条数收一刀。** 命中极多时先在工具里截到一个上限，并在末尾如实写「还有 N 条未列出，
  请缩小搜索范围」——这是 `repo_map` 末尾那行「被省掉多少」的同一种收尾
  （[`docs/repo-map.md:31-33`](../../docs/repo-map.md)），也让模型有一个可操作的动作。
  条数上限是一个常量，不进配置。
- **不做工具内分页**（`head_limit` / `offset`，Claude Code 那种）：分页状态是模型要自己维持的
  一份额外记账，而这里已有「落盘 + 指针」这条更简单的退路（见《明确不做》）。

### §6 与既有工具的分工

- **与 `repo_map`**：地图回答「有什么」（这个工作区里定义了哪些符号，按需取、不注入），grep
  回答「在哪」（某个名字/模式出现在哪里）。[`docs/repo-map.md:16`](../../docs/repo-map.md) 已经
  把「找名字被用到的每一处」判给了 `grep / read` —— 本 spec 把那句话兑现成一条真工具，不改地图。
- **与 `bash`**：`bash` 保持原样（`Exclusive` 不动，它确实什么都能写）；本工具**不**禁用
  `bash` 里的 `rg`，只是在描述里把模型引过来。模型仍可能拼 shell，那是它的选择。
- **与 `task`**：执行者的工具表也照常拿到 `grep`（它不是 `delegable() == false` 的那一类）。

### §7 文档与索引

- **新增一份逐面文档 `docs/grep.md`**，照 [`docs/repo-map.md`](../../docs/repo-map.md) 先例写：
  工具声明的形状、范围与忽略规则、输出与上限、以及「为什么不 spawn `rg`」。
- **在 [`README.md`](../../README.md) 的「文档」一节加一行**（那一节是全仓库唯一的文档索引）。
- **`.scratch/README.md`** 的 feature 索引在本次已经更新（形态从 `seed` 改成 `spec`）。
- **`CONTEXT.md` 不加词条**：`grep` 是一个工具名，不是领域概念；词汇表不收通用词（见
  [`CONTEXT.md:8`](../../CONTEXT.md) 的「不收两类东西」）。

## 测试决定

测试照仓库规矩只断言**外部行为**：事件流的 payload 与工作区副作用；不断言 `at` 时间戳。
接缝仍是库的组装入口（假 provider），所以没有网络依赖。

新增用例：

1. **只读的门裁决**：同一个 `grep` 调用在 `readonly` 档**放行**、在 `ask` 档**不产生询问**
   —— 这两条是这条工具的第一验收面。反向锚：`bash` 同样一次纯搜索仍在 `ask` 档要审批。
2. **不取工作区锁**：一次 `grep` 与一次 `read_file` 同批时不被串行化成两段（或直接断言
   `effect()` 返回 `Effect::ReadOnly`）。
3. **命中与形状**：工作区里放一个文件、写一行已知文本，断言结果含 `相对路径:行号:文本`。
4. **忽略规则**：写进 `.gitignore` 的文件**搜不到**；隐藏文件（如 `.hidden.txt`）**搜不到**；
   未被忽略的普通文件**搜得到** —— 三条一起才有意义，它们钉的是「与 rg 默认一致」。
5. **`glob` 过滤**：同一个 pattern 在两个后缀的文件里都命中时，带 `glob: "*.rs"` 只回 `.rs` 那条。
6. **grep 之后仍要先读才能改**：`grep` 命中一个文件后直接 `edit_file` 被拒（read-before-write），
   补一次 `read_file` 后成功 —— 钉住「不登记命中文件」这条决定。
7. **超限走落盘**：造一个超 `max_tool_result_tokens` 的结果，断言事件里的文本是头尾预览 +
   指针，且 `outputs/<tool_call_id>.txt` 存在。
8. **一次调用一条结果**：`grep` 的每次调用在流上恰好一条 `ToolCallCompleted`（那是三条不变量
   之一，不是新规则）。
9. **描述里有那句劝阻**：断言工具声明的 description 含「不要用 `bash` 拼」那一句的语义
   （前缀缓存的前提是声明稳定，这条断言同时防止它被顺手改掉）。

## 明确不做

- **文件名 / 目录名搜索（glob 或 find 工具）**：那是另一条意向，不混进这张票的面。
- **结构 / 符号检索**：`repo_map` 已经占了「有什么」，本工具只做文本模式。
- **`path` 参数与读工作区外**：范围写死 cwd；要放开就是一次新的决定（要重开 `outside_read`
  的口径）。
- **工具内分页（`head_limit` / `offset`）与 `output_mode`（`files_with_matches` / `count`）**：
  只做 `content` 一种输出。
- **`ignore_case` / `context`（`-C`）/ `max_count` 等开关**：用 pattern 的内联语法或多次调用解决。
- **登记命中文件进读集合**：不动 `Tool::read_paths` 的接口。
- **禁掉 `bash` 里的 `rg` / `grep`**：只劝阻，不拦。
- **并发承诺**：`ReadOnly` 今天不带来并行，本 spec 不改变调度器。
- **新增配置项**：不加按工具的预算字段、不加「是否用内建库」的开关。
- **替换 `bash` 或 `repo_map`**：两个工具一个字不动。

## 补充说明

### 决定速查

| # | 问题 | 决定 | 落在 |
| --- | --- | --- | --- |
| Q1 | 需求池里先做哪条 | `grep-tool`（判据是「对 fs-agent 的帮助大」，第一轴是模型侧能力） | 本轮对话 |
| Q2 | 这条工具主修哪一刀 | 权限代数为主，输出整形顺带 | §1、§5 |
| Q3 | 工具面多大 | 只做内容搜索 | §3、§6 |
| Q4 | 实现路线 | A：自带 `ignore` + `grep-searcher` + `grep-regex` | §4 |
| Q5 | 搜索范围 | 只扫 cwd，不接受 `path` | §2 |
| Q6 | 忽略规则 | 遵守 `.gitignore`、跳过隐藏 | §2 |
| Q7 | 要不要劝阻拼 shell | 要，写进工具描述 | §3 |
| Q8 | 命中文件算不算已读 | 不算（不登记读集合） | §3 |
| Q9 | 参数面 | `pattern` 必填 + `glob` 可选 | §3 |

### 我在实现层面替你定的（写在这里以便否决）

- **工具名 `grep`**（不是 `search` / `grep_search`）：模型最熟、与意向同名；改名有缓存前缀成本。
- **输出形状 `path:line:文本`**、无匹配时返回一句说明：与 rg 默认一致，且让「空结果」与
  「工具坏了」在流上分得开。
- **不做工具内分页**：`truncate_result` 已经给了退路，分页是模型要多维持的一份记账。
- **条数上限是常量、不进配置**：先跑一段看真实用量，再决定要不要照 `repo_map_tokens` 开字段。
- **不先用 `custom__*` 动态工具试参数面**：动态工具的 `effect()` 恒为 `Exclusive`，正好是要修的
  那一类 —— 拿它试等于把验收面本身丢掉。
- **不给搜索另开一条权限旋钮**：`ReadOnly` 的含义已经够了（只读工作区）。
- **文档写 `docs/grep.md` 并在 README 索引加行**，不进 `CONTEXT.md`（工具名不是领域词）。
