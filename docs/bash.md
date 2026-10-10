# `bash`

`bash(command, workdir, timeout_ms?, escalation?)` 在会话工作区里跑一条 shell 命令，返回它的
站位、退出状态、stdout 与 stderr。spec §7 把它列为 v1 内建工具的最后一个；§12 管它要过的那道权限门；
§20 管它**刻意不做**的那件事（进程级隔离）。这份文件是给人看的决策地图，以及这些决策住在代码的
哪里。

## 形状

| 部件 | 值 | 为什么 |
| --- | --- | --- |
| `spec()` | `bash(command: string, workdir: string, timeout_ms?: integer, escalation?: object)` | 一条命令字符串、**必填**的站位、一个可选的上限、一个可选的升级申请 |
| `effect()` | **恒为 `Exclusive`** | shell 什么都能写，所以派发层拿的是全工作区的锁，权限门看到的是一次写 |
| `command(args)` | `["bash", "-lc", command]` | 权限门、`CommandPrefix` 作用域与断路器在进程起来**之前**读 argv；`escalation` 与 `workdir` 都**不进** argv，它们不是命令的一部分 |
| 站位 | `workdir` 解析后落在工作区内 → `current_dir` 取它 | 它换的是进程站在哪，**不是**边界（见下面那一节）；必填，根写 `.` |
| 执行 | 对这个 argv 直接 `spawn` | 模型的命令是**一个元素**，所以它插不进第二层 shell |
| 结果 | **首行 `cwd: <相对工作区的路径>`** + 退出状态 + stdout + stderr，分段 | 站位无条件回显（`cd "$(pwd)"` 的痛点正是「不确定自己站在哪」）；非零退出是数据，不是错误 |

`bash -lc` 让命令拿到用户的登录环境（`-l`），并把命令字符串当作**一个**参数读（`-c`）。
任何东西都不会被拼进一条更大的 shell 命令行，所以模型给的字符串没有地方变成模型自己
没写的 shell 语法。

## 权限门站在哪

`permissions.rs` 里 `bash` 不需要任何特例：

- `readonly` 拒它，因为这一档拒掉每一次非 `ReadOnly` 调用，而 `Exclusive` 不是
  `ReadOnly`。没有豁免可借：这个项目**曾经**有过的唯一一处写豁免（已退场的 plan 模式的
  `PLAN.md`，一种 `WritePaths` 形状、写入集**整体**就是那一个文件）随那一档模式一起
  退了场，何况 shell 根本没有写入集（见 `docs/adr/0003-plan-leaves-the-permission-modes.md`）。
- `ask` 问；`workspace` 与 `auto` 放行，照旧受规则与断路器约束。**这两档才是沙箱真正
  补上的位置**：门判不出区内区外（`Exclusive` 没有路径集），真正的判据在内核。
- `CommandPrefix` 规则匹配的是声明的 argv —— `["bash", "-lc", …]` —— 就按声明的样子。
  为命令自己的 argv 写规则需要把 shell 拆开，这个作用域不做这件事。

### 升级手势：被内核拒之后唯一的一条路

命令被沙箱拒（内核说只读文件系统）之后，模型可以带上

```json
{ "escalation": { "justification": "构建产物要写到 ~/.npm 的缓存目录",
                  "writable_paths": ["/home/ada/.npm"] } }
```

把**同一条命令**原样重试一次。这次调用会弹一次审批；批准之后，`Sandbox` 在构造时把声明
的那几条路径追加进可写根 —— **粒度就是那条路径本身**，文件绑文件、目录绑目录，不做父目录
提升。

四条形状，都是刻意的：

- **只这一次调用**：不进规则、不写 `config.toml`、不进会话状态；`AllowedCall::sandbox_grants`
  是它的全部归宿。
- **只给一次重试**：同一条命令第二次被拒之后照常返回命令结果，运行时不做计数 —— 话术在
  工具描述里（`ESCALATION_NOTE`，模型可见）。
- **`justification` 与 `writable_paths` 都要非空、必须成对**：半截的写法是参数错误，不是
  静默忽略。
- **要放开的路径必须已经存在**：`bwrap` 只能绑已经存在的源，所以批准一个不存在的路径会是
  一条工具错误（「命令没有跑」，理由写明为什么不替它猜）—— 静默跳过等于用户批了一条什么都
  不发生的路径。要新建一个文件，就声明它**存在的那个父目录**。
- **各档都有效，`readonly` 除外**：它请的是放宽沙箱，而沙箱不因权限模式而关。

写死的边界**不给通道**：遮罩目录（`~/.config/heng`、`~/.ssh`）、工作区里的
`.git/config` 与 `.git/hooks`、以及 `.env` 一族，门里直接拒 —— 它们在挂载表里排在可写根
之后，批准等于白批。完整决策地图见 [`permissions.md`](permissions.md)。

### `rm` 断路器看穿这层包装

spec §12 把一条硬约束放在模式与规则**之外**：`rm` 打到 `/`、`~`、或这两者任一的祖先，
一律 `Deny`，不管哪条 allow 或 hook 说什么。藏在 `bash -lc "…"` 后面的 `rm` 只是往里
一层而已，所以断路器看的是 shell 将要跑什么，而不是外面那层包装（spec §7）：

```
["bash", "-lc", "rm -rf /"]            → 这条命令被分词，然后被拒
["bash", "-lc", "cd /tmp && rm -rf /"] → 先按 `&&` 拆分，然后被拒
["bash", "-lc", "(rm -rf /)"]          → 外面那层括号就是分隔符
["bash", "-lc", "if x; then rm -rf /"] → 打头的 `then` 是语法，被丢掉
["bash", "-o", "pipefail", "-c", "rm -rf /"] → 这个选项的参数被跳过
["bash", "-lc", "echo rm -rf /"]       → `echo` 才是命令；普通工作
```

**这次扫描是词法的、尽力而为的，而且是有意的。** spec §12 写明断路器是为了拦住一次
事故，不是为了关住一个对手；[沙箱](sandbox.md)管的是写边界，管不了下面这些间接引用。
它处理引号、shell 的控制操作符、命令前面的语法关键字、以及 `-c` 之前的一个 shell 选项；
它**不**跟着间接引用走。
变量（`rm -rf $HOME`）、改变将要跑什么的包装程序（`sudo rm …`、`env …, rm …`、`eval`、
`xargs`、别名）、命令替换（`$(rm …)`）、here-doc 或写到盘上的脚本、以及换个写法的混淆
拼写，它都看不见。真正的边界是 agent 没有 root，而它手里的 key 也不是你文件系统的钥匙。

## `workdir`：这条命令站在哪

模型今天要换个目录干活，只能把 `cd x && …` 写进每一条命令。`workdir` 把「站在哪」从
命令文本里拿出来交给参数：一条命令的 `current_dir` 由它决定，省掉的是那个重复的前缀。

**必填**（2026-10-10 改）。它上线两天只有 **1 次**调用带它（3175 次里的 0.01%），而同期
**86%** 的调用以 `cd` 开头 —— 可选 + 一句描述的引导完全失效，于是改成字段必填，并**无条件**在
结果首行回显实际站位。

**值域**：一段字符串。相对路径按**工作区**解析，绝对路径也给；**区外一律按解析后的真实
位置**判定，所以 `src/..` 合法、`src/../../..` 越界。`.` **合法**且表示工作区根，空串与省略
都是参数错误（文案里点名「根写 `.`」，否则模型要去猜根怎么表达）。目录**必须已存在** ——
`workdir` 不替你 `mkdir`，那是另一条命令的事。

模型可见的那句是 `WORKDIR_NOTE` 常量，逐字如下（工具声明进请求前缀，所以它一次定死）：

> 必填，这条命令在工作区内的哪个目录跑；相对路径按工作区解析，工作区根写 `.`，目录必须已存在
> 且落在工作区内。用它代替在命令里写 cd；只影响这一条命令的进程，不改变其他工具解析相对路径
> 的基准。

**三条工具错误**（都不是权限裁决：`workdir` 不构成写，所以它自己不引发任何询问）：省略或不是
非空字符串；解析后落在工作区之外；那个目标不是工作区里一个**已存在**的目录（不存在，或者是
文件）。前两条的检查在 `call()` 里，与 `timeout_ms: 0` 同一种形状。区外那一句的原文由
`tools/paths.rs` 的 `SessionPaths::resolve_within()` 产出，其余在 `tools/bash.rs`。

**结果首行固定是 `cwd: <相对工作区的路径>`**，根写作 `.`，**无条件**加：成功、超时、非零退出，
以及跑不起来的那些出口（沙箱不可用、进程起不来）都一样 —— 那种时候模型正需要知道自己以为站在
哪、而实际什么都没跑起来。填根的时候最需要这一行确认。

**`workdir` 与 `escalation` 的旧互斥已取消**（同一天）。站位必填之后两者永远同现，那条规则会
打死**每一次**升级重试；它当初要防的「借站位自选工作区」，在「必填 + 严格解析在会话工作区之内」
之后已经由收容判定收着。

**`cd` 冗余只改描述**：不加结果提示、也不在工具层拒绝 —— 含 `cd` 的命令占 85%，提示会变噪声，
而 `cd` 在 `/tmp`、子 shell、`for` 循环里都是合法用法。

**三条它刻意不动的东西**：

- **边界**。沙箱的可写根与 `.git/config`、`.git/hooks`、`.env` 那一族保护路径**仍绑工作区**。
  所以 `run()` 把边界与站位拆成两个参数，而 `wrap()` 只收前者：可写根与保护路径都由它推出来
  （[`tools/sandbox.rs`](../src/tools/sandbox.rs)），否则子目录会让工作区那几条保护悄悄消失。
- **其他工具的基准**。`read_file`、`grep`、`@` 记号、左栏文件页与改动页的路径全都仍按工作区
  解析：`workdir: ".scratch/x"` 之后 `read_file("spec.md")` 读的是工作区根那份。工具描述把
  这句明写给模型看。
- **门与断路器**。`Call.cwd` 仍是工作区：`rm` 断路器与路径 glob 按它折叠相对参数，在子目录里
  只会**更保守**（更容易拒，不会漏），而 `bash` 本来不声明写目标。

**为什么是这个形状**。[`../.scratch/bash-workdir/research/01-cwd-parameter-precedent.md`](../.scratch/bash-workdir/research/01-cwd-parameter-precedent.md)
那份一手调研里，Codex 的 `workdir`、Gemini CLI 的 `dir_path`、opencode 的 `workdir` 都是
「参数 + 每次新进程」，而 Anthropic 的 API 级 bash 规格与 goose 把 cwd 留给应用侧、
Claude Code 则把 `cd` 记成会话状态。选参数是因为本仓库的进程**每次都是新的**，
没有可沿用的状态；描述里那半句「用它代替在命令里写 cd」照 opencode 的措辞。

## 超时与进程树

一次只把 future drop 掉的超时，会把 shell 的子进程留在后台继续跑。所以 `bash`：

1. 在**它自己的进程组**里 spawn 这个 shell（`Command::process_group(0)`，与 `setsid`
   等价 —— 同样的组语义，不另开会话 id），所以它的 pid 就是它的组 id；
2. 只跑一个有限的循环，盯三件事：shell 退出、stdout 的 EOF、stderr 的 EOF；
3. 到期时把 **`SIGKILL` 打给整个进程组**（`libc::killpg`），然后回收 shell。

这个截止时间限住的是**整次调用**，不只是 shell 的存活期，因为这两者不是同一个时刻：
被放到后台的子进程继承了输出管道，所以 `bash -lc "sleep 300 &"` 立刻退出而管道还开着。
只等 shell 的话，整个回合要陪那个子进程挂到它自己结束。如果一个进程刻意脱离了进程组却
还握着管道，kill 之后会有一段短短的宽限，结果如实报告已知的内容，而不是挂住。

这次 kill 住在一个 `ProcessGroup` 守卫里，它在 **drop** 时同样触发，于是盖住了调用提前
结束的另一条路：一次取消手势（spec §6）让循环把进行中的工具 drop 掉。两条路留下同一条
保证 —— 命令起过的东西，事后一个都不在跑。

| 旋钮 | 默认 | 在哪 |
| --- | --- | --- |
| 默认上限 | 120 s | `config::DEFAULT_BASH_TIMEOUT_MS`, `SessionConfig::bash_timeout_ms` |
| 硬上限 | 600 s | `config::MAX_BASH_TIMEOUT_MS`, `SessionConfig::max_bash_timeout_ms` |

模型可以用 `timeout_ms` 要得**更少**，但从不能要得更多，于是一条命令没法无限期占着
全工作区的 `Exclusive` 锁。`timeout_ms` 为 0（或不是整数）是参数错误，不是静默回退。
两个值都通过 `ToolContext::bash` 以一对 `BashLimits` 到达工具，每次调用从 `SessionConfig`
构造 —— 与仓库地图的预算一个形状。

超时是**结果**，不是 `ToolError`：产物报告
`超时：<n> ms 内没有跑完；整个进程组已被杀掉`、杀掉 shell 的那个信号、以及已经
产出的 stdout/stderr。模型看得见发生了什么；只有 spawn 或回收 shell 本身失败才算错误。

## 它是非交互的

- **stdin 是 `/dev/null`**，并且**不分配 TTY**：交互式程序看到的是 EOF，而不是永远挂着。
- **环境原样继承。** v1 不做环境清理，也不凭空造 `TERM`、`NO_COLOR` 之类（§20）；想要
  不一样环境的命令自己设。`bash -lc` 还会 source 用户的登录文件，所以命令一开始拿到的
  环境与终端会给它的那个一样 —— 只是没有终端。

## 结果与截断

结果文本是三段带名字的节 —— `退出码：…`、`--- 标准输出 ---`、`--- 标准错误 ---` ——
退出码写成一个码，或者写成 `被信号 N 杀掉`。非零退出不是这个工具失败：模型要的是
一个进程，它也拿到了一个。

过大的正文**不在这里**处理。它和每一个别的工具结果走同一条入流前流水线（spec §10、
票 07）：落盘到 `outputs/<tool_call_id>.txt`，事件里带上首尾预览和那个指针。`bash` 既不
知道、也不需要知道这件事在发生。

## `bash` 不包含什么

- **网络隔离。** shell 跑在 **bubblewrap** 沙箱里（工作区可写、区外只读，见
  [`sandbox.md`](sandbox.md)），但那层只管文件：这一版不加 `--unshare-net`，所以 `curl`
  该通还是通。沙箱也不管读（整机可读，除那两个遮罩），不做资源限额。
- **一条自动的越界通道。** 越界之后能走的只有「模型声明 + 批准一次」这一条
  （见上面的升级手势）：没有候选路径推断、没有 `allow-always`、没有授权库。
- **PTY / 交互式程序。** 不分配终端、stdin 按设计是空的。
- **后台作业与作业控制。** 一条命令可以在它自己的 shell 里把进程放到后台，但没有任何
  东西管理或汇报作业；超时或取消杀掉整个进程组。
- **会话中途换工作区。** 工作区由 `--cwd` 或启动时的当前目录定下，一场会话只有一个；
  `workdir` 只在它里面挑站位。要在别处长期干活，是「起一个会话在那儿」或 worktree 那条路。

## 代码住在哪

| 部件 | 模块 |
| --- | --- |
| `BashTool`、argv、超时、进程组 kill、结果格式 | `tools/bash.rs` |
| `escalation` 与 `workdir` 两个参数、模型可见的那段描述 | `tools/bash.rs` |
| `workdir` 的解析与收容判定（不存在的目标按最深已存在祖先解析） | `tools/paths.rs` 的 `SessionPaths`，经 `tools/tool.rs` 交给工具 |
| 升级的裁决（遮罩 / 保护路径的拒绝、其余 `Ask`） | `permissions.rs` |
| `sealed()` 与升级路径的归一化 | `tools/sandbox.rs` |
| 边界与站位两个参数（可写根与保护路径绑前者，`current_dir` 取后者） | `tools/process.rs` 的 `run()`；`tools/sandbox.rs` 的 `wrap()` 按边界拼挂载表 |
| `BashLimits`（交给这个工具的两个上限） | `tools/tool.rs` |
| 默认值与上限 | `config.rs`（`SessionConfig::bash_timeout_ms` / `max_bash_timeout_ms`） |
| `rm` 断路器与它拆 shell 的那部分 | `permissions.rs`（`rm_breaker`、`simple_commands`） |
| 落盘 + 预览指针 | `context.rs`（`truncate_result`），由 `agent.rs` 施加上去 |
| 沙箱（argv 包一层再 spawn） | `tools/sandbox.rs`，由 `tools/process.rs` 的 `run()` 施加（见 [`sandbox.md`](sandbox.md)） |
