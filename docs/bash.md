# `bash`

`bash(command, timeout_ms?)` 在会话工作区里跑一条 shell 命令，返回它的退出状态、stdout
与 stderr。spec §7 把它列为 v1 内建工具的最后一个；§12 管它要过的那道权限门；§20 管它
**刻意不做**的那件事（进程级隔离）。这份文件是给人看的决策地图，以及这些决策住在代码的
哪里。

## 形状

| 部件 | 值 | 为什么 |
| --- | --- | --- |
| `spec()` | `bash(command: string, timeout_ms?: integer)` | 一条命令字符串、一个可选的上限 |
| `effect()` | **恒为 `Exclusive`** | shell 什么都能写，所以派发层拿的是全工作区的锁，权限门看到的是一次写 |
| `command(args)` | `["bash", "-lc", command]` | 权限门、`CommandPrefix` 作用域与断路器在进程起来**之前**读 argv |
| 执行 | 对这个 argv 直接 `spawn` | 模型的命令是**一个元素**，所以它插不进第二层 shell |
| 结果 | 退出状态 + stdout + stderr，分段 | 与别的工具结果一样的普通工具结果；非零退出是数据，不是错误 |

`bash -lc` 让命令拿到用户的登录环境（`-l`），并把命令字符串当作**一个**参数读（`-c`）。
任何东西都不会被拼进一条更大的 shell 命令行，所以模型给的字符串没有地方变成模型自己
没写的 shell 语法。

## 权限门站在哪

`permissions.rs` 里 `bash` 不需要任何特例：

- `readonly` 拒它，因为这一档拒掉每一次非 `ReadOnly` 调用，而 `Exclusive` 不是
  `ReadOnly`。没有豁免可借：这个项目**曾经**有过的唯一一处写豁免（已退场的 plan 模式的
  `PLAN.md`，一种 `WritePaths` 形状、写入集**整体**就是那一个文件）随那一档模式一起
  退了场，何况 shell 根本没有写入集（见 `docs/adr/0003-plan-leaves-the-permission-modes.md`）。
- `ask` 问；`auto` 放行，照旧受规则与断路器约束。
- `CommandPrefix` 规则匹配的是声明的 argv —— `["bash", "-lc", …]` —— 就按声明的样子。
  为命令自己的 argv 写规则需要把 shell 拆开，这个作用域不做这件事。

### `rm` 断路器看穿这层包装

spec §12 把一条硬约束放在模式与规则**之外**：`rm` 打到 `/`、`~`、或这两者任一的祖先，
一律 `Deny`，不管哪条 allow 或 hook 说什么。藏在 `bash -lc "…"` 后面的 `rm` 只是往里
一层而已，所以断路器看的是 shell 将要跑什么，而不是外面那层包装（spec §7）：

```
["bash", "-lc", "rm -rf /"]            → the command is tokenized and refused
["bash", "-lc", "cd /tmp && rm -rf /"] → split on `&&`, then refused
["bash", "-lc", "(rm -rf /)"]          → the wrapping parens are separators
["bash", "-lc", "if x; then rm -rf /"] → the leading `then` is grammar, dropped
["bash", "-o", "pipefail", "-c", "rm -rf /"] → the option's argument is skipped
["bash", "-lc", "echo rm -rf /"]       → `echo` is the command; ordinary work
```

**这次扫描是词法的、尽力而为的，而且是有意的。** spec §12 写明断路器是为了拦住一次
事故，不是为了关住一个对手，§20 则说 v1 不发布任何进程级沙箱。它处理引号、shell 的控制
操作符、命令前面的语法关键字、以及 `-c` 之前的一个 shell 选项；它**不**跟着间接引用走。
变量（`rm -rf $HOME`）、改变将要跑什么的包装程序（`sudo rm …`、`env …, rm …`、`eval`、
`xargs`、别名）、命令替换（`$(rm …)`）、here-doc 或写到盘上的脚本、以及换个写法的混淆
拼写，它都看不见。真正的边界是 agent 没有 root，而它手里的 key 也不是你文件系统的钥匙。

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
结束的另一条路：一次取消手势（spec §6）让循环把在飞的工具 drop 掉。两条路留下同一条
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

- **进程级沙箱。** spec §20：v1 靠写串行化、权限门与断路器，升级路径写明是「只做 Linux
  的 bubblewrap」，且不预做抽象。
- **PTY / 交互式程序。** 不分配终端、stdin 按设计是空的。
- **后台作业与作业控制。** 一条命令可以在它自己的 shell 里把进程放到后台，但没有任何
  东西管理或汇报作业；超时或取消杀掉整个进程组。

## 代码住在哪

| 部件 | 模块 |
| --- | --- |
| `BashTool`、argv、超时、进程组 kill、结果格式 | `tools/bash.rs` |
| `BashLimits`（交给这个工具的两个上限） | `tools/tool.rs` |
| 默认值与上限 | `config.rs`（`SessionConfig::bash_timeout_ms` / `max_bash_timeout_ms`） |
| `rm` 断路器与它拆 shell 的那部分 | `permissions.rs`（`rm_breaker`、`simple_commands`） |
| 落盘 + 预览指针 | `context.rs`（`truncate_result`），由 `agent.rs` 施加上去 |
