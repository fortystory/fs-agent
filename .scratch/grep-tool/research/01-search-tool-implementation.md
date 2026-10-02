# 一手调研：搜索工具的实现路线

> 这是 `.scratch/grep-tool/seed.md` 那条意向的 implementation research：把「给模型加一个
> grep / 搜索工具」的实现路线查清。**写就于 2026-10-02。** 核实过的源码位置：
> `src/tools/{mod,tool,bash,file,custom,repo_map,registry,paths,process,sandbox}.rs`、
> `src/permissions.rs`、`src/context.rs`、`src/config.rs`、`src/agent.rs`、`Cargo.toml`、
> `Cargo.lock`，以及 `docs/`（`sandbox.md`、`repo-map.md`、`permissions.md`、`research/`）。
> 外部事实来自 crates.io / docs.rs / ripgrep 仓库与 man 页，URL 都列在末尾。
>
> 本文只落在 `.scratch/grep-tool/research/` 下，**没有**改 `.scratch/README.md` 的 feature
> 索引（那一行现在指 `seed.md`）。

## 结论摘要（5 条以内）

1. 一个「只读、只扫工作区」的搜索工具，在今天的代数里落在 `Effect::ReadOnly` 这一类：
   四档权限模式都放行（含 `readonly`），不取工作区级的 `Exclusive` 锁
   （`src/permissions.rs:129-153`、`src/tools/registry.rs:178-182`）。
2. 推荐**路线 A（自带 ripgrep 拆出来的库：`grep-searcher` + `grep-regex` + `ignore`）**：
   与既有权限代数契合、遍历范围由我们的代码决定、输出可结构化。理由与代价见 §4。
3. 路线 B（spawn `rg`）今天就能走通，代价是 **0 个新 crate**，但换来三条运行期依赖：
   `rg` 在不在 PATH、`rg` 自己的配置文件会改行为、以及经 `process::run` 会继承沙箱
   「整机可读」的性质（`docs/sandbox.md:22`）—— 后者与 `[permissions] outside_read`
   默认 `deny` 不是同一个东西。
4. `Effect::ReadOnly` 今天**不**让工具调用并行：一批里只有 `task` 走 `Deferred`，
   其余「原地、按这一批的顺序跑」（`src/agent.rs:1030-1067`）。所以这条路线买到的是
   权限语义与不占锁，不是并发。
5. 最需要人拍板的是**读集合语义**：`Tool::read_paths(&args)` 是调用前的纯函数，声明不了
   「运行时才知道的命中文件」，所以搜索工具要么像 `repo_map` 一样不登记读
   （之后 `edit_file` 仍要求先 `read_file`），要么改接口。见 §7。

---

## 1. Effect 与权限现状

### 1.1 `Effect` 今天有三类

`src/tools/tool.rs:21-33`：

| 变体 | 含义 | 调度后果 |
| --- | --- | --- |
| `ReadOnly` | 只读工作区 | 可与其他只读调用分区；不取任何锁 |
| `WritePaths(Vec<PathBuf>)` | 恰好写这些路径 | 对这些路径取逐路径锁 |
| `Exclusive` | 独占工作区 | 取工作区级锁（`registry.rs:178-182`） |

`effect` 描述的是**工作区**副作用，不是「有没有副作用」：只 spawn 一个 agent 的工具也是
`ReadOnly`（`src/tools/tool.rs:1-5`）。

### 1.2 每个工具今天是什么

| 工具 | `effect()` | 位置 |
| --- | --- | --- |
| `read_file` | `ReadOnly` | `src/tools/file.rs:130-131` |
| `write_file` | `WritePaths` | `src/tools/file.rs:193-194` |
| `edit_file` | `WritePaths` | `src/tools/file.rs:266-267` |
| `bash` | `Exclusive`（恒） | `src/tools/bash.rs:113-115` |
| `skill` | `ReadOnly` | `src/tools/skill.rs:40-41` |
| `repo_map` | `ReadOnly` | `src/tools/repo_map.rs:61-62` |
| `task` | `ReadOnly` | `src/tools/task.rs:55-56` |
| `todo` | `ReadOnly` | `src/tools/todo.rs:260-261` |
| `goal_note` | `ReadOnly` | `src/tools/goal_note.rs:132-133` |
| `ask_user_question` | `ReadOnly` | `src/tools/ask_user.rs:196-197` |
| `custom__*`（动态声明） | `Exclusive`（恒） | `src/tools/custom.rs:75-77` |

工具表在 `src/tools/mod.rs:69-83`（`builtin`）与 `:91-97`（`with_dynamic`），今天没有
检索类工具 —— 与 `seed.md` 的《现状》一致。

### 1.3 一个只读搜索工具落在哪一类

落 `ReadOnly`，与 `repo_map` 同一档，理由是它同时满足两件事：

- **权限门**：`Mode` 四档对 `Effect::ReadOnly` 的立场都是 `Allow`（`readonly` 在
  `src/permissions.rs:129-133`、`ask` 在 139-143、`workspace` 在 149-153、`auto` 在
  178-182）。也就是说搜索在 `readonly` 档可用、在 `ask` 档不问。
- **锁**：`Registry::dispatch` 只对 `Exclusive` 取工作区锁，对 `WritePaths` 取逐路径锁；
  `ReadOnly` 不取（`src/tools/registry.rs:178-185`）。

但要看清 `ReadOnly` 买到的是什么、买不到什么：

- **买不到并发。** `agent.rs` 的批次循环里只有 `task` 返回 `Disposition::Deferred`
  （`src/agent.rs:1030-1062`），其余调用「原地，…，按这一批的顺序跑」
  （`src/agent.rs:1064-1067`）。并行执行只读调用是 `Effect` 的文档里预留的接线
  （`src/tools/tool.rs:5`），不是现状。
- **买不到 `outside_read` 的保护。** 见下。

### 1.4 `outside_read` 这条旋钮，以及它**管不到**什么

- `[permissions] outside_read` 接 `deny`（缺省）/ `ask` / `allow`
  （`src/config.rs:1099-1110`、错误文案在 `:1701-1703`），缺省 `Deny`
  （`src/permissions.rs:363-371`），由 `Policy::with_outside_read` 注入（`:373-377`），
  三个组装点都在 `src/cli.rs`（`:387`、`:703`、`:2216`）。
- 它的判据是**方向**：`registry.facts` 只在 `tool.read_paths(args)` 里的某条路径
  `resolve_read` **失败**时记一条 `PathError::read`（`src/tools/registry.rs:131-142`），
  而 `decide()` 的 ④ 段对 `Direction::Read` 用 `policy.outside_read`、对
  `Direction::Write` 用档位（`src/permissions.rs:567-581`）。`SessionPaths::check_contained`
  按组件做前缀比较，`outside_read` / `outside_write` 两个开关在那一次越界被放行时打开
  （`src/tools/paths.rs:75-88`、`:136-153`）。
- 因此 **`outside_read` 只作用于「工具声明过、且解析失败的读路径」**。一个像 `repo_map`
  那样**不声明读路径、自己走 `ctx.cwd`** 的工具，权限门根本看不见它扫过哪里
  （`src/tools/repo_map.rs:7-8` 的注释就是「不读任何模型给出的工作区路径 —— 它自己走会话
  cwd … 也不声明读路径」）。`Effect::ReadOnly` 在门里只意味着「这不会写」，不意味着
  「它只读 cwd 之内」。
- 规则的 `Scope` 有 `Tool(glob)`、`CommandPrefix`、`Path(glob)`、`PathSet`、`All`
  （`src/permissions.rs:252-264`）；`Scope::Path` 拿**写目标 + 声明过的读目标**去比
  （`:265-277`）。不声明读路径的搜索工具只能被 `Tool("grep")` 或 `All` 覆盖。

**结论**：搜索工具「只扫工作区」这条约束，落 `ReadOnly` 之后**必须由工具自己实现**
（自己从 `ctx.cwd` 出发），权限门不会替你兜。若反过来让它接受模型给的 `path` 参数并走
`resolve_read`，`outside_read` 与 `Scope::Path` 就都生效了 —— 这是一个二选一，见 §7。

---

## 2. 路线 A：自带 Rust 库

### 2.1 五个 crate 各自的用途

| crate | 用途 | 最新稳定版（2026-10-02 查） | 许可 |
| --- | --- | --- | --- |
| `grep` | facade：re-export 下面几个，方便一次引入（自身只有 7 行代码） | 0.4.1（2025-10-22） | `Unlicense OR MIT` |
| `grep-searcher` | 逐行搜索的执行器：上下文行、计数、反转、二进制检测、UTF-16 转码、mmap 决策 | 0.1.17（2026-07-15） | 双许可 MIT / UNLICENSE |
| `grep-regex` | `grep-matcher::Matcher` 的 Rust regex 实现（ripgrep 用的正则后端） | 0.1.14（2025-10-16） | 双许可 MIT / UNLICENSE |
| `ignore` | 递归目录遍历 + `.gitignore` / `.ignore` / glob / 文件类型过滤（`Walk` / `WalkBuilder`） | 0.4.33（2026-08-04） | 双许可 MIT / UNLICENSE |
| `globset` | 单个或多个 glob 的同时匹配（`Glob` / `GlobSet`） | 0.4.20（2026-08-04） | 双许可 MIT / UNLICENSE |

许可一栏的口径：`grep` 来自 crates.io API 的 `license` 字段；其余四个 crate 的
crates.io 页面 README 都写 "Dual-licensed under MIT or the UNLICENSE"
（docs.rs 的 crate 页面正文）。ripgrep 二进制自身的 `Cargo.toml` 也写
`license = "Unlicense OR MIT"`。

### 2.2 体积与编译代价

docs.rs 给的是**包大小**与**它自己构建机上的构建时长**（只是量级，不是本机数字）：

| crate | 源码包大小 | Ø 构建时长（本 release / 近期平均） |
| --- | --- | --- |
| `ignore` | 329.2 kB | 11s / 14s |
| `grep-searcher` | 237.0 kB | 3s / 13s |
| `grep-regex` | 110.9 kB | 14s / 22s |
| `globset` | 127.3 kB | 10s / 18s |
| `grep`（facade） | 6,977 B（`crate_size`） | — |

传递依赖（docs.rs 的依赖列表）：

- `ignore` → `crossbeam-deque`、`globset`、`log`、`memchr`、`regex-automata`、
  `same-file`、`walkdir`（+ Windows 的 `winapi-util`）。
- `globset` → `aho-corasick`、`bstr`、`regex-automata`、`regex-syntax`。
- `grep-searcher` → `bstr`、`encoding_rs`、`encoding_rs_io`、`grep-matcher`、`log`、
  `memchr`、`memmap2`。
- `grep-regex` → `bstr`、`grep-matcher`、`log`、`regex-automata`、`regex-syntax`。

也就是说，最小组合（`grep-searcher` + `grep-regex` + `ignore`）会新拉进来
`grep-matcher`、`globset`、`bstr`、`encoding_rs`、`encoding_rs_io`、`memmap2`、
`same-file`、`walkdir`、`crossbeam-deque`、`crossbeam-epoch`、`crossbeam-utils`
（Windows 另有 `winapi-util`）—— 大约 11~14 个新包；其中 `log`、`memchr`、
`regex-automata`、`regex-syntax`、`aho-corasick`、`bitflags` **已经在** `Cargo.lock` 里。

另一个量级参照：本仓库已经硬依赖 9 个 tree-sitter 文法，「每个文法都要编一个 C parser，
所以首次构建比纯 Rust 依赖慢」（`Cargo.toml` 的注释）。纯 Rust 的这几个 crate 在这个
基线之上不算突出。

### 2.3 是否已是传递依赖（查 `Cargo.lock`）

**不是。** `Cargo.lock`（共 300 个 `[[package]]`）里没有 `grep`、`grep-searcher`、
`grep-regex`、`grep-matcher`、`ignore`、`globset`、`bstr`、`encoding_rs`、`memmap2`、
`walkdir`、`same-file`、`crossbeam-*`。

**在** lock 里的相关包：`regex 1.13.1`、`regex-automata`、`regex-syntax`、
`aho-corasick 1.1.5`、`memchr 2.8.3`、`log`、`bitflags` —— 由 `tree-sitter`、
`tree-sitter-highlight`、`pulldown-cmark`、`serde_json`、`nom`、`winnow`、`fancy-regex`
等引入（反查 `Cargo.lock` 的 `dependencies` 得到）。

### 2.4 仓库 `Cargo.toml` 现有依赖

直接依赖（`[dependencies]`）：`async-trait`、`chrono`、`futures`、`reqwest`
（`default-features = false`，只开 `json`/`stream`/`native-tls`）、`serde`、`serde_json`、
`thiserror`、`tokio`（`rt-multi-thread`/`macros`/`sync`/`net`/`time`/`process`/`io-util`）、
`libc`、`ratatui`、`crossterm`、`tree-sitter-highlight`、`toml`、`tree-sitter`、
`tree-sitter-rust`、`tree-sitter-bash`、`tree-sitter-json`、`tree-sitter-toml-ng`、
`tree-sitter-html`、`tree-sitter-javascript`、`tree-sitter-typescript`、`tree-sitter-php`、
`tree-sitter-sequel`、`tree-sitter-python`、`url`、`pulldown-cmark`。
`[dev-dependencies]`：`tempfile`。

`.gitignore` 的严格程度：按**类目**忽略，没有「忽略一切再白名单」的模式 ——
`debug`、`target`、`.cargo-home/`、`**/*.rs.bk`、`*.pdb`、`**/mutants.out*/`、
`__pycache__/`、两个 DSH 缓存/探针、以及一段被注释掉的 `.idea/` 说明。仓库自己**不**产出
会被搜索工具误伤的大目录（除了 `target/`，而它在 `.gitignore` 里，`ignore` crate 与
`rg` 默认都会跳过它）。

### 2.5 路线 A 的落点（若采纳）

一个 `GrepTool` 需要在三处接线：`src/tools/mod.rs` 的 `builtin()`（`:69-83`）注册、
`Cargo.toml` 加依赖、以及（可选）一个 `SessionConfig` 字段当输出上限（照
`repo_map_tokens` 的样子，`src/config.rs:59-66`、`:1849-1854`）。工具本体是
`spec()` + `effect()` + `read_paths()`（可选）+ `call()` 四个方法，形状与
`src/tools/repo_map.rs:38-82` 一一对应。

---

## 3. 路线 B：spawn `rg`

### 3.1 复用哪条路径

`src/tools/process.rs` 的 `run(cwd, argv, limit, sandbox)`（`:105-219`）是仓库里唯一的
命令 spawn 处（`docs/sandbox.md:9` 明说「它是 `bash` 与动态工具**唯一**的 spawn 处」）。
`bash`（`src/tools/bash.rs:131-143`）与动态工具（`src/tools/custom.rs:86-97`）都这么走。
复用到的既有性质：

- argv **直接** spawn，中间不插 shell，argv 元素替换不展开（`src/tools/process.rs:1-17`、
  `src/tools/custom.rs:41-60`）；
- 墙钟上限 + 整个进程组 `SIGKILL`（`:101-110`、`:150-194`、`:232-267`）；上限来自
  `ToolContext.bash: &BashLimits`（`src/tools/tool.rs:126`、`:80-105`），
  `BashLimits` 又是从 `SessionConfig` 注入的（缺省 120s / 上限 600s，
  `src/config.rs:71-78`）；
- 结果文本由 `CommandOutcome::report()` 组装：退出码 + `--- 标准输出 ---` +
  `--- 标准错误 ---`（`src/tools/process.rs:57-82`），非零退出是**结果**不是
  `ToolError`（`:16-17`）；
- `Tool::command()` 让权限门在进程启动**之前**看到 argv（`src/tools/tool.rs:217-224`），
  于是 `Scope::CommandPrefix(["rg", …])` 规则与 `rm` 断路器都看得见
  （`src/permissions.rs:256-258`、`:662-680`）。

### 3.2 `rg` 不在 PATH 时怎么探测

仓库里**没有**通用的 `which` 助手，也没有探测 `git` 的先例（`render/wording.rs:451` 的
`SUBCOMMAND_PROGRAMS` 只是显示用的名字表）。唯一的「探测外部二进制」先例是 bubblewrap：

- `sandbox::probe(search_path, cwd)`（`src/tools/sandbox.rs:44-76`）：在 PATH 里找候选，
  跑一条**最小 profile**，**以退出码为准**（不用 `--version`）；
- `find_bwrap`（`:86-111`）：**排除落在会话 cwd 之内的候选**，按词法路径比较，不看
  symlink —— 防止工作区里放一个假的 `bwrap`；
- `is_executable`（`:113-130`）：存在、是文件、至少一个执行位；
- 结果作为 `SandboxAvailability` 在**组装期定一次**，随会话配置携带，每条命令不重探
  （`src/config.rs:108-111`）；
- 不可用时给两条出路文案 `WAYS_OUT`（`src/tools/sandbox.rs:22-24`）。

一个 `rg` 探测照这个形状抄即可（找 `rg`、排除 cwd 内候选、跑 `rg --version` 或
`rg --files-with-matches` 之类的最小验证），并且应该沿用「组装期定一次」还是「每次调用
现探」的一致性选择。注意与 `bwrap` 的一处差别：`bwrap` 探测要跑最小 profile（装了不等于
能用），而 `rg` 的探测可以只看 PATH + 执行位 + 一次 `--version`。

### 3.3 经沙箱跑只读子进程会怎样

- `Sandbox::wrap` 拼出来的 argv 第一段是 `--ro-bind / /`（`src/tools/sandbox.rs:283-288`），
  之后才是各条 `--bind`、`--tmpfs /tmp`、遮罩与保护路径。所以**沙箱内整机可读**。
  `docs/sandbox.md:22` 写得很直白：「也不管读（整机可读，除下面那两个遮罩）」。
- 两个遮罩是 `~/.config/fs-agent`（provider key 就在那）与 `~/.ssh`
  （`src/config.rs:106`、`docs/sandbox.md:33-36`），表现为「目录还在，但是空的、且只读」。
- 可写根：会话 cwd、`[sandbox] writable_roots`（缺省 `~/.cargo`、`~/.rustup`、`~/.cache`）、
  一个每次调用全新的 `/tmp`（`docs/sandbox.md:27-31`）。只读搜索用不到这些。
- 沙箱不可用（`mode = "off"` 除外）时，`process::run` 在 `wrap` 那一步直接返回
  `ToolError`（`src/tools/sandbox.rs:213-228`、`src/tools/process.rs:214-217`）：**整个工具
  变成一条错误**，与「命令失败了」不是一回事。

结论有两面：

- **可以**：只读遍历完全没问题，`rg` 在沙箱里能读整机（除遮罩），也能读工作区。
- **但不贴权限代数的意图**：spawn 路线的搜索范围是**沙箱挂载表**决定的，不经过
  `resolve_read`，所以 `[permissions] outside_read = "deny"`（缺省）对它**无效** ——
  它和今天的 `bash` 一样能 `rg /etc`。若要在工具层再收一道 cwd 前缀，那是我们自己的代码，
  与权限门无关。

### 3.4 `rg` 自身的两个行为坑

- **退出码**：`0` 有匹配、`1` 无匹配且无错、`2` 出错（ripgrep 15.2.0
  `crates/core/main.rs` 的 `run()` 末尾；man 页的 EXIT STATUS 一节同口径）。经
  `CommandOutcome::report()` 进模型的是 `退出码：1` 这种文本 —— 要么在工具里翻译成
  「没有匹配」，要么让模型自己懂。
- **配置文件**：`rg` 会读 `RIPGREP_CONFIG_PATH` 指向的文件，「每一行是一个 shell 参数」，
  并把它 prepend 到命令行之前（man 页 CONFIGURATION FILES 一节）。也就是说用户机器上的
  ripgrep 配置会改变这个工具的行为；要关掉得显式传 `--no-config`。
- **输出模式与分页**（man 页）：`--json` 是 JSON Lines、五种消息类型（`begin`/`end`/
  `match`/`context`/`summary`），**不能**与 `-l`/`-c`/`--files` 同用，且隐式打开
  `--stats`；`-m/--max-count` 是**每文件**上限，不是全局上限；`-c/--count`、
  `-l/--files-with-matches`、`-A`/`-B`/`-C`、`-g/--glob`、`--no-ignore`/`-u`、
  `-.`/`--hidden`、`-L/--follow`、`-M/--max-columns` 都在。想让输出在 N 条后停下并
  区分「还有更多」，得自己读流、早停或先要 `--count` —— 而 ripgrep 把
  BrokenPipe 当**优雅退出 0**（`crates/core/main.rs` 的 `main()`），早停时拿到的退出码
  不是一个可靠的「还有更多」信号。

---

## 4. 两条路线的对照

| 维度 | A：自带库（`grep-searcher`+`grep-regex`+`ignore`） | B：spawn `rg` |
| --- | --- | --- |
| 代码量（估算） | 一个 `GrepTool`（spec/effect/call + 遍历与格式化 + 上限），加 `Cargo.toml`、`builtin()`、可能的 `SessionConfig` 字段、测试与文档；量级数百行 | 一个 `GrepTool`（spec/effect/command/call → `process::run` + argv 组装 + 退出码翻译），加一份 `rg` 探测（照 `sandbox::probe` 抄）；量级百行上下，但探测与三个运行期分支会把差距拉小 |
| 新依赖 | `Cargo.toml` 加 3 条（`ignore`、`grep-searcher`、`grep-regex`；`globset` 由 `ignore` 传递，显式声明也可），`Cargo.lock` 约 +11~14 个包，纯 Rust | 0 个新 crate；换来一条**运行期**外部依赖：PATH 上的 `rg` |
| 可移植性 | `cargo build` 即可，跨平台（`ignore` 自己处理 Windows）；与仓库已有的 9 个 tree-sitter C parser 的构建代价同性质 | 依赖用户机器装了 `rg`；沙箱是 bubblewrap（Linux/WSL2）专有，但 `mode = "off"` 时工具仍可用 |
| 与既有权限代数 | `Effect::ReadOnly` 完全贴（四档放行、不取锁）；遍历范围由我们写死在 cwd，不会被沙箱的「整机只读」意外放宽；若接受模型给的 `path` 并走 `resolve_read`，`outside_read` 与 `Scope::Path` 都生效 | `effect` 也可以是 `ReadOnly`，argv 可被 `CommandPrefix` 与断路器看见；但实际可读范围由**沙箱挂载表**决定（整机可读），`outside_read` 管不到它 —— 与 `ReadOnly` 的语义（只读**工作区**）不完全一致 |
| 输出可控性 | 拿到结构化 match（路径、行号、绝对偏移），自己格式化、自己设上限、自己写「还有 N 条」；二进制/UTF-16 交由 searcher 的既有策略 | 文本或 `--json`；`-m` 是每文件上限，全局上限要自己早停，而早停的退出码不可靠；`RIPGREP_CONFIG_PATH` 会改行为（要 `--no-config`）；退出码 1/2 要自己翻译 |

**推荐：路线 A**，按最小组合先落地（`ignore` + `grep-searcher` + `grep-regex`），理由：

1. **权限语义干净。** `ReadOnly` 在四档都放行、不取锁，而遍历范围是**我们代码**里的
   `ctx.cwd`；spawn 路线要么接受「沙箱里整机可读」这个超出 `ReadOnly` 本意的范围，
   要么在工具里再写一道 cwd 过滤（那就等于承认权限门不管这块）。
2. **没有新的运行期依赖。** 现在仓库已经容忍「首次构建慢」这一档代价（9 个 C parser），
   而路线 B 把「工具能不能用」交给用户机器上有没有 `rg`，还要多一份探测 + 一条
   `WAYS_OUT` 式的出路文案 + 「沙箱不可用时整个工具报错」的耦合。
3. **输出可控。** 自己控上限与「还有 N 条」，是 §6 那套 `truncate_result` 之外还能给
   模型一个可操作的收尾（`coding-agent-features.md:687` 的建议：实现分页/过滤/截断并给出
   「请缩小搜索范围」的提示）。

同时承认路线 B 的两个真实优势：代码量小；`rg` 的行为就是用户熟悉的行为。**如果第一版
只要「能用」，路线 B 更快落地** —— 但它得先把「`rg` 不在 PATH」「退出码 1 = 无匹配」
「沙箱不可用 = 工具全废」这三条处理掉。

---

## 5. 上游怎么做

### 5.1 是内建工具还是让模型拼 shell

| 实现 | 形状 | 出处 |
| --- | --- | --- |
| Claude Code | **内建** `Grep`（基于 ripgrep）、`Glob`；`Read`/`Glob`/`Grep` 属只读、可并发 | `docs/research/coding-agent-features.md:56`、`:92-94`；`notes/claude-code-amp.md:40`、`:72-77` |
| Codex | **没有** `read_file`/`write_file`/`grep`/`glob` 核心工具，读与搜索都走 `exec_command` | `coding-agent-features.md:111-112`；`notes/codex-gemini.md:600`、`:1149`、`:1177-1179` |
| aider | 没有工具集（纯文本 edit format），grep 由模型经 shell 自己拼 | `coding-agent-features.md:723`；`notes/aider-openhands.md:65` |
| OpenHands | V1 把 `glob` 与 `grep` 做成一等工具（V0 两个都没有）；plan mode 是一个只带 `glob` + `grep` + planning editor 的预设 agent | `notes/aider-openhands.md:715-717`、`:724`、`:840`、`:1010` |
| Gemini CLI | **内建** `glob`、`grep_search`（legacy alias `search_file_content`）、`list_directory` | `notes/codex-gemini.md:92-96`、`:1149` |
| opencode | 内建 `grep`、`glob`，实现于 ripgrep；权限面把它们当独立 key | `notes/opencode-goose.md:121-123`、`:155-158`、`:374` |
| goose | Developer extension 只有 5 个工具（`shell`/`write`/`edit`/`tree`/`read_image`），**没有 grep**，搜索交给 `shell`/ripgrep | `notes/opencode-goose.md:171-196` |
| Cline | `search` / `search_files`（Ripgrep-powered codebase search） | `notes/cline-continue.md:222`、`:226-228`、`:237-241` |
| Continue | `grep_search` + `file_glob_search` 是一等只读工具；plan mode 只读工具列表里有它们 | `notes/cline-continue.md:1770-1784`、`:1811-1823` |

**「不做也能成功」的证据**在 `coding-agent-features.md:747`：专用 read/grep/glob 工具这一项，
Codex 与 aider 都有成功实现把它省掉 —— 「这是**效率优化**，不是能力前提」。同一份材料把
「专用 grep / glob 工具」标为 🔷 而不是 ✅（`:140`、`:698`）。

### 5.2 参数面

- **Claude Code**：`Grep` 有三种输出模式 `files_with_matches` / `content` / `count`，
  带 `head_limit` / `offset`；`Glob` 按修改时间排序、上限 100 条并返回截断标志、默认
  **不**遵守 `.gitignore`；`Grep` 遵守 `.gitignore`、用的是 ripgrep 的非 POSIX 正则
  （`coding-agent-features.md:92-94`）。权限规则里 `path` 是 `Grep`/`Glob` 的 primary
  content field（`notes/claude-code-amp.md:246`）。
- **Cline**：`search_files` 接 `path`、`regex`（「Uses Rust regex syntax」）与可选
  `file_pattern` glob（`notes/cline-continue.md:280`）。
- **Gemini CLI**：工具表只记了 `glob`（pattern matching）与 `grep_search`（无参数细节）；
  忽略规则由 `.gitignore` + `.geminiignore` + custom paths 合并，`fileFiltering` 下有
  `respectGitIgnore`/`enableRecursiveFileSearch`/`maxFileCount`/`searchTimeout`，工具还接受
  每次调用的 `respect_git_ignore`/`respect_gemini_ignore` 覆盖（`notes/codex-gemini.md:92-96`、
  `:178-186`）。
- **opencode**：`grep`/`glob` 基于 ripgrep、默认遵守 `.gitignore`，项目根的 `.ignore`
  可以反选（`!node_modules/`）；`glob` 按 mtime 排序（`notes/opencode-goose.md:155-158`）。
- Cline subagent 的只读白名单里 `execute_command` 只允许只读命令，`search_files` 是
  「Read-only by construction」的一部分（`notes/cline-continue.md:1281-1285`）。

### 5.3 输出上限与分页

上游材料在这块**普遍很薄**，能摘到的只有：

- Claude Code `Grep` 的 `head_limit`/`offset`；`Glob` 的 100 条上限 + 截断标志
  （`coding-agent-features.md:93-94`）。
- Claude Code `Bash`：> ~30k 字符只回文件路径 + 预览，失败时只回 ~10k head-tail 摘录
  （`coding-agent-features.md:92`）。
- Gemini CLI：`maxFileCount`、`searchTimeout` 设置；`read_file` 的截断常量
  `DEFAULT_MAX_LINES_TEXT_FILE = 2000`（`notes/codex-gemini.md:182`、`:189`）。
- 其余实现（Cline、Continue、OpenHands、opencode、goose、aider）的笔记里**没有**记录
  grep 结果的分页/上限数字。这是本次一手材料的一个空白，不要拿它当「它们没有上限」。

### 5.4 有没有「必须用工具、不许拼 shell」的强制

**基本没有。** 检索全部 `docs/research/`，唯一近似的语句是 Continue 的
`run_terminal_command` 工具描述里那句「use Edit/MultiEdit tools instead of bash commands
(sed, awk, etc)」—— 而且它说的是**编辑**，不是搜索
（`notes/cline-continue.md:1832-1835`）。Claude Code 只是把 Grep/Glob 声明为只读工具、并让
权限规则按工具名写（文档里用 `Read(docs/**)` 取代 `Glob(docs/**)` 的写法，
`notes/claude-code-amp.md:97-101`），没有「禁止用 bash 跑 grep」这一类规则。

---

## 6. 输出上限与形状可复用的东西

### 6.1 `truncate_result` 的具体形状

`src/context.rs`：

- `pub fn truncate_result(text: &str, tool_call_id: &str, outputs_dir: &Path, max_tokens: u64)
  -> SpilledResult`（`:336-371`）。返回值
  `SpilledResult { preview: String, pointer: Option<PathBuf>, truncated: bool }`（`:319-329`）。
- 行为：估算 token 没超上限就**原样返回**（`preview == text`、`pointer == None`、
  `truncated == false`）；超了就整份写进 `<outputs_dir>/<tool_call_id>.txt`（0600，
  `write_owner_only`），然后返回一条预览。写不出去时降级成「只有预览」；预览 + 指针说明
  比正文还长时干脆保留正文（`:351-365`）。
- `preview()`（`:373-396`）：取 `max_tokens / 10` 换算成字符数、下限 200 字符，**头尾各半**，
  中间插 `TRUNCATED_MARKER = "[已截断："` + `共 N 字符，约 M token；全文在 <path>]`
  （`:403`）。
- 调用点**只有一处**：`agent.rs` 的 `emit_completed`（`src/agent.rs:2043-2054`），
  成功与失败正文同样处理；**只有 `preview` 进事件**，`pointer` 从不进事件
  （`.scratch/tui-ux/research/01-collapse-detail-data-sources.md:110`）。
- 上限来自 `SessionConfig.max_tool_result_tokens`，缺省
  `DEFAULT_MAX_TOOL_RESULT_TOKENS = 25_000`（`src/config.rs:57`）。

**它是每个工具结果自动经过的流水线**：搜索工具只要返回字符串，溢出、指针、头尾预览都不用
自己写。

### 6.2 `repo_map` 工具的声明与返回

- `spec()`：名字 `repo_map`（`src/context/repo_map.rs:35` 的 `REPO_MAP_TOOL`），
  参数只有可选 `focus`（`src/tools/repo_map.rs:40-59`）；
- `effect()` 恒 `ReadOnly`，**不声明读路径**，也不接受模型给的路径
  （`src/tools/repo_map.rs:61-63`、`:7-8` 的模块注释）；
- `call()`：从 `ctx.repo_map` 拿排序上下文，预算用 `ctx.repo_map.tokens`，
  模型硬塞的 `tokens` 键被忽略（`:65-81`）；
- 上限是**配置**：`repo_map_tokens` 缺省 1_024、上限 4_096（`src/config.rs:59-66`、
  `:1849-1854`），`SessionConfig` 字段、**不进 `config.toml`**
  （`.scratch/fs-agent-v1/issues/20-bash-tool.md:34`、`09-repo-map.md:28` 都记了这条
  「`max_tool_result_tokens` 同规矩」）。
- 地图「装不下的按整个符号的边界切，末尾一行写出被省掉多少」
  （`docs/repo-map.md:31-33`）—— 这是仓库里「工具自己截断并如实报告省掉多少」的既有先例。

### 6.3 一个 grep 工具最省地复用哪一套

- **路线 A**：工具返回字符串即可，溢出与指针全交给 `truncate_result`；若要「还有 N 条」
  这类收尾，就在工具内先按条数截断并在尾部写清省了多少（同 `repo_map` 的末尾一行）。
  预算若也要「配置说了算」，照 `repo_map_tokens` 加一个 `SessionConfig` 字段。
- **路线 B**：`CommandOutcome::report()` 给出「退出码 + stdout + stderr」三段，仍然经过
  同一个 `truncate_result`；但要把 `退出码：1`（无匹配）与 `退出码：2`（错误）在工具层
  翻译清楚，否则模型读到的是原始退出码。

---

## 7. 需要人拍板的点

1. **工具名与参数面。** 叫 `grep`？`search`？参数要不要 `path` / `glob` / `ignore_case` /
   `context`（`-C`）/ 大小写 / `output_mode`（`files_with_matches` / `content` / `count`）？
   上游没有统一答案（Claude 三模式 + `head_limit`/`offset`；Cline `path`+`regex`+`file_pattern`；
   Gemini 是 `grep_search` + 独立的 `glob`）。工具声明进前缀缓存、一次定死
   （`src/tools/mod.rs:3-5`），改名有成本。
2. **读集合语义（`ReadSet`）。** `Tool::read_paths(&args)` 是调用前的纯函数
   （`src/tools/tool.rs:209-215`），声明不了运行时才命中的文件。搜索命中的文件要不要算
   「读过了」，从而让随后的 `edit_file` 不必先 `read_file`？今天 `repo_map` 的答案是「不登记」
   （`src/tools/repo_map.rs:7-8`），`read-before-write` 在 `registry.rs:271-293`。要登记就得改
   接口（这是结构性改动）。
3. **搜索范围：只限 cwd，还是允许 `path`？** 只限 cwd 就同 `repo_map`、权限门看不见它扫过什么；
   允许 `path` 并走 `resolve_read`，`outside_read` 与 `Scope::Path` 才生效
   （`src/permissions.rs:567-581`、`src/tools/paths.rs:136-153`）。这是 §1.4 的二选一。
4. **路线 B 特有：沙箱的「整机可读」要不要在工具层再收一道？** 若 spawn `rg`，它能读区外
   （`docs/sandbox.md:22`），与 `ReadOnly` 的本意（只读**工作区**）不完全一致。要不要写死
   一条 cwd 前缀过滤？
5. **`rg` 缺失时的行为（路线 B）。** 组装期探测 + 工具错误（同 `bwrap`
   `src/tools/sandbox.rs:44-76`、`WAYS_OUT` 于 `:22-24`），还是每次调用现探并回落到 `bash`？
6. **「鼓励用工具、别拼 shell」的措辞。** 上游没有硬强制（§5.4）。要不要在 `bash` 或新工具
   的描述里写一句 `rg` 相关的话？描述是前缀缓存的一部分、一次定死
   （`src/tools/bash.rs:32-52` 是既有先例）。
7. **输出上限与分页的取舍。** 工具内 `head_limit`/`offset`（Claude 风格）还是只依赖
   `truncate_result` 的头尾 + 指针？后者对「匹配太多」给出的是头若干行 + 尾若干行、中间全进
   `.txt`（`src/context.rs:373-396`）。
8. **是否顺便把动态工具那条路当成试验田。** 用户今天就能在 `config.toml` 里声明一个
   `custom__search__rg`（argv 模板 + JSON Schema，`docs/custom-tools.md`），但 `CustomTool`
   的 `effect` **恒为 `Exclusive`**（`src/tools/custom.rs:73-77`），会在 `ask` 档要批准、
   在 `readonly` 档被拒 —— 与「只读搜索从 `Exclusive` 里救出来」的意向正好相反。要不要先用它
   试参数面？
9. **文档与索引落点。** 新增内建工具后，模型可见的说明该落在哪（`README.md` 的「文档」一节
   是唯一索引）；本文按「只写一个文件」的约束，**没有**动 `.scratch/README.md` 的索引行。
   另外要不要照 `docs/repo-map.md` 给搜索工具写一份逐面文档。

---

## 来源

### 仓库内文件（`:行`）

- `src/tools/tool.rs:1-5`、`:21-33`、`:80-105`、`:126`、`:209-215`、`:217-224` —— `Effect`
  三类、`BashLimits`、`read_paths`/`command` 的契约。
- `src/tools/mod.rs:3-5`、`:69-83`、`:91-97` —— 工具表是缓存前缀的一部分；`builtin()` /
  `with_dynamic()` 今天有什么。
- `src/tools/file.rs:130-141`、`:193-194`、`:266-267` —— `read_file` 的 effect 与
  `read_paths`、两个写工具的 effect。
- `src/tools/bash.rs:32-52`、`:113-115`、`:125-143` —— `bash` 恒 `Exclusive`、argv 契约、
  沙箱与升级说明常量。
- `src/tools/custom.rs:41-60`、`:73-77`、`:86-97` —— 动态工具的 argv 替换与恒 `Exclusive`。
- `src/tools/repo_map.rs:7-8`、`:38-82` —— `repo_map` 的声明、返回与「不声明读路径」。
- `src/tools/registry.rs:19`、`:102-164`、`:178-185`、`:229-240`、`:271-293` —— `facts`
  的路径解析与 `PathError`、`dispatch` 的取锁规则、`guardrails` 的 read-before-write。
- `src/tools/paths.rs:42-88`、`:136-153`、`:181-215` —— cwd 收容按方向、`relaxed_read`、
  逐路径锁。
- `src/tools/process.rs:1-17`、`:57-82`、`:105-219`、`:232-267` —— 唯一的 spawn 处、
  `CommandOutcome::report()`、进程组 kill。
- `src/tools/sandbox.rs:22-24`、`:44-76`、`:86-111`、`:113-130`、`:213-228`、`:273-324` ——
  `bwrap` 探测先例、`is_executable`、不可用即 `ToolError`、`--ro-bind / /` 的挂载表。
- `src/permissions.rs:21-33`、`:56-66`、`:127-184`、`:252-277`、`:355-382`、`:531-639`、
  `:643-680` —— `Effect` 的注释、四档 `Mode`、`stance` 表、`Scope`、`outside_read`、
  `decide()` ④ 段、断路器。
- `src/context.rs:319-403` —— `SpilledResult`、`truncate_result`、`preview`、
  `TRUNCATED_MARKER`。
- `src/config.rs:57`、`:59-66`、`:71-78`、`:106`、`:108-111`、`:1099-1110`、`:1735-1755`、
  `:1849-1854` —— 输出上限常量、repo map 预算与上限、bash 超时、遮罩、沙箱探测、
  `outside_read` 解析、`SessionConfig` 字段。
- `src/agent.rs:1030-1067`、`:1126-1147`、`:2043-2054` —— 只有 `task` 走 `Deferred`、
  其余按批次顺序跑；`run_deferred` 的上限是 `max_parallel_executors`；`truncate_result`
  的唯一调用点。
- `Cargo.toml`（`[dependencies]` / `[dev-dependencies]`）、`Cargo.lock`（300 个
  `[[package]]`；相关包的有无）。
- `.gitignore` —— 按类目忽略，无「忽略一切」模式。
- `.scratch/grep-tool/seed.md` —— 本次意向的原文与它列的四个分叉。
- `.scratch/README.md:48` —— `grep-tool/` 那一行现在只指 `seed.md`。
- `.scratch/fs-agent-v1/issues/09-repo-map.md:28`、`20-bash-tool.md:34` ——
  「上限进 `SessionConfig`、不进 `config.toml`」的既有规矩。
- `.scratch/tui-ux/research/01-collapse-detail-data-sources.md:110` —— 截断调用点与
  「指针不随事件旅行」。
- `docs/sandbox.md:9`、`:22`、`:27-43`、`:63-65` —— 唯一 spawn 处、「不管读（整机可读）」、
  可写根与遮罩、没有沙箱就没有 `workspace` 档。
- `docs/repo-map.md:14-20`、`:31-33` —— 「找名字被用到的每一处」判给 `grep / read`；
  地图的预算与末尾「省掉多少」。
- `docs/research/README.md` —— `docs/research/` 是一手引文、不加维护。

### `docs/research/`（上游正文）

- `docs/research/coding-agent-features.md:56`、`:64`、`:85-95`、`:111-112`、`:121-122`、
  `:140`、`:687`、`:698`、`:723`、`:745-750`、`:761`。
- `docs/research/notes/claude-code-amp.md:40`、`:64-65`、`:72-77`、`:97-101`、`:246`、
  `:306-307`。
- `docs/research/notes/cline-continue.md:222`、`:226-228`、`:237-241`、`:280`、`:492`、
  `:1281-1285`、`:1770-1784`、`:1811-1823`、`:1832-1835`。
- `docs/research/notes/codex-gemini.md:92-96`、`:178-190`、`:600`、`:1149`、`:1177-1179`。
- `docs/research/notes/opencode-goose.md:121-123`、`:155-158`、`:171-196`、`:374`、`:1234`。
- `docs/research/notes/aider-openhands.md:65`、`:715-717`、`:724`、`:840`、`:1010`。

### 外部 URL（2026-10-02 抓取）

crate 版本 / 许可 / 体积 / 依赖：

- <https://crates.io/api/v1/crates/grep> —— `grep` 0.4.1、`license = "Unlicense OR MIT"`、
  `crate_size = 6977`、linecounts 显示 facade 只有 7 行。
- <https://docs.rs/crate/ignore/latest/> —— `ignore 0.4.33`、源码包 329.2 kB、构建时长、
  依赖表（`crossbeam-deque`、`globset`、`regex-automata`、`same-file`、`walkdir`…）、
  双许可 MIT / UNLICENSE。
- <https://docs.rs/crate/grep-searcher/latest/> —— `grep-searcher 0.1.17`、237.0 kB、
  依赖表（`bstr`、`encoding_rs`、`encoding_rs_io`、`grep-matcher`、`memmap2`…）。
- <https://docs.rs/crate/grep-regex/latest/> —— `grep-regex 0.1.14`、110.9 kB、依赖表。
- <https://docs.rs/crate/globset/latest/> —— `globset 0.4.20`、127.3 kB、依赖表。

ripgrep 二进制：

- <https://raw.githubusercontent.com/BurntSushi/ripgrep/master/Cargo.toml> ——
  `version = "15.2.0"`、`license = "Unlicense OR MIT"`、`rust-version = "1.96"`。
- <https://raw.githubusercontent.com/BurntSushi/ripgrep/master/crates/core/main.rs> ——
  退出码：`matched` → 0、有 error → 2、否则 1；BrokenPipe 当优雅退出 0；
  `SearchMode::JSON` 与 grep-printer 的 JSON 输出。
- <https://man.archlinux.org/man/rg.1.en> —— EXIT STATUS（0/1/2，`-q` 的例外）、
  `--json`（五种消息、与 `-l`/`-c`/`--files` 互斥、隐式 `--stats`）、`-m/--max-count`
  （每文件）、`-c`/`-l`/`-A`/`-B`/`-C`/`-g`/`--no-ignore`/`-.`/`-L`/`-M`、
  `RIPGREP_CONFIG_PATH` 的配置文件行为。man 页是 ripgrep 自带文档的镜像。
