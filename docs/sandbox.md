# 沙箱

`bash` 与 `config.toml` 里声明的动态工具跑在一个 **bubblewrap** 沙箱里：整台机器只读挂进来，只有会话工作区、`/tmp` 与一列工具缓存目录可写。**判据在内核**（只读挂载上的 `EROFS`），不在我们的代码里——我们只负责把 argv 拼对。

这份文件是给人看的决策地图：这一层管什么、不管什么、代码住在哪。决定与理由在 [ADR 0006](adr/0006-sandbox-by-bubblewrap.md)，完整规格在 [`.scratch/sandbox/spec.md`](../.scratch/sandbox/spec.md)，一手材料（含本机实测）在 [`.scratch/sandbox/research/`](../.scratch/sandbox/research/)。

## 形状

| 部件 | 值 | 为什么 |
| --- | --- | --- |
| 包裹点 | `tools/process.rs` 的 `run()` | 它是 `bash` 与动态工具**唯一**的 spawn 处，超时与进程组 kill 也都在那，包一层不影响它们 |
| 拼装 | `tools/sandbox.rs` 的 `wrap()`，**纯函数** | 输出可以逐字断言，不需要真有 bubblewrap 就能测 |
| 探测 | 组装期跑一次最小 profile，看退出码 | 装了 bubblewrap 不等于它在这个环境里能用 |
| 失败态 | **fail closed**：工具错误 + 两条出路 | 静默放行是谎话，降级成「问」则判据已经消失 |
| 拒绝信号 | `bwrap: ` 前缀 = 沙箱没起来；其余原样给模型 | 被沙箱拒绝的消息随 locale 变，不能拿来做判据 |

## 它管什么、不管什么

| 这一层 | 说明 |
| --- | --- |
| **管** | shell 与动态工具的**写**边界：能写哪些目录 |
| **不管** | **网络**——`curl` 带着 key 出去这一层拦不住（见 [credentials.md](credentials.md)）。也不管读（整机可读，除下面那两个遮罩），不管进程数、不做资源限额 |
| **不替代** | 权限门（跑不跑、要不要问）、断路器（`rm -rf /` 那类）、打码。它们是**三件不同的事**，沙箱是又一层 |

## 边界

**可写**（每条一个 `--bind`，都在 `--ro-bind / /` 之后）：

- 会话 cwd；
- `[sandbox] writable_roots` 里的每一项，默认 `~/.cargo`、`~/.rustup`、`~/.cache`。**这是可用性决定，不是安全决定**：没有它们，`cargo build` 会因为写不了 `~/.cargo/.package-cache` 就失败；
- `/tmp`——但它是一条 `--tmpfs`：**每一次工具调用都是一个新的空 `/tmp`**，同一条命令内可用，跨命令不保留，也不写到宿主上。它挂在**所有 `--bind` 之前**：挂载是叠上去的，反过来的话，一个落在 `/tmp` 下的会话工作区会被这条 tmpfs 整个盖掉（真机验证过：那样写自己的工作区也会报「只读文件系统」）。

**遮住**（`--tmpfs` + `--remount-ro`，表现是「目录还在，但是空的、且只读」）：

- `~/.config/fs-agent`（provider key 就在那儿）；
- `~/.ssh`。

**压回只读**（在可写根的 `--bind` **之后**追加 `--ro-bind`，顺序反了等于没保护）：

- 工作区里的 **`.git/config` 与 `.git/hooks`**——不是整个 `.git`：那样会把 `git add` / `git commit` 一起挡死（它们写的第一样东西是 `.git/index.lock`），而地板真正要防的是改 hooks（下次 commit 执行任意代码）与改 remote；
- 工作区里**存在的** `.env` 家族文件（`.example` / `.sample` / `.template` 除外，与 `permissions.rs` 同一份口径）。

这三条清单里，**可写根是可配的，遮罩与保护路径写死**。

`.env` 家族只扫**工作区顶层**：每次 `bash` 调用都要现拼一次挂载表，而递归遍历一个可能很大的
工作区不在那一步的预算里。于是 `sub/.env` 这类嵌套文件 shell 仍然写得进去 —— 这是这一层
比权限门的拒绝地板**窄**的一处，与「只保护 `.git` 的两个入口而不是整个 `.git`」是同一类
取舍。

## 与权限门的关系

沙箱**不改变**任何一档的裁决：

| 档位 | 沙箱的角色 |
| --- | --- |
| `readonly` | 用不上——非只读调用在门口就被拒了，命令根本没跑 |
| `ask` | 照旧问；被批准之后才在沙箱里跑 |
| `auto` | **这一档才是沙箱真正补上的位置**：以前不问就直接跑，现在跑了也写不出去 |

两者回答的是不同的问题：**权限门答「跑不跑」**，**沙箱答「跑起来能碰到什么」**。

## 失败时长什么样

- bubblewrap 用不了（没装、user namespace 建不起来、`/proc` 挂不上）→ `bash` 返回**工具错误**，文本写明「命令没有跑」以及两条出路：装 bubblewrap，或把 `[sandbox] mode` 设成 `"off"`。
- 命令自己越界 → **不是工具错误**，是命令失败。模型看到内核给的「只读文件系统」（中文环境）或 `Permission denied`（`LC_ALL=C`），照常决定下一步。

**状态在流里、在转录里、也在复盘视图里。** `SandboxStatus` 只进日志（不进 `messages`），所以它不到模型那里；转录里它占一行叙述，与 `[上下文注入：…]` 同一档（不进 TUI 左栏——那里已经很挤，而沙箱在一个会话内基本不变）；`fs-agent sessions show <id>` 打出同一行（不可用时带上原因），`--json` 里就是那条 entry。于是「某条命令当时有没有被关着」在会话当中和事后都读得到。

## 代码住在哪

| 部件 | 模块 |
| --- | --- |
| `wrap()`、探测、可写根与遮罩的清单 | `tools/sandbox.rs` |
| 包裹 argv 的那一处 spawn | `tools/process.rs` 的 `run()` |
| 两个调用点（`bash`、动态工具） | `tools/bash.rs`、`tools/custom.rs` |
| `ToolContext::sandbox` | `tools/tool.rs` |
| `[sandbox]` 的解析与默认值 | `config.rs` |
| 工具描述里那句「你在沙箱里」 | `tools/bash.rs` 的 `SANDBOX_NOTE`（模型可见，走中文） |
| 状态事件（log-only） | `events.rs` 的 payload、`agent.rs` 的写入口、`lib.rs` 的组装期调用 |

## 这一版不包含什么

- **越界之后的一次性审批**（被拒 → 原样重试 + 一句理由 → 批一次）。形状已经调研清楚，但它依赖「沙箱能稳定给出拒绝信号」，是下一个 effort。
- **网络隔离**：`--unshare-net`、域名白名单代理都不做。
- **Linux 之外的平台**：macOS 可行（Seatbelt 能表达同样的语义）但要另写 SBPL 生成；Windows 的三条路线都要求改动宿主 ACL；鸿蒙第一层就没有可用机制。
- **provider 抽象**：没有平台候选链、没有后端 trait、没有 `enforcement` 上报。
- **自动降级与 `--no-proc` 之类的退路**：不可用就是拒绝。
