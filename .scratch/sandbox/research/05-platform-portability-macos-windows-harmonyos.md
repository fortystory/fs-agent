# research：把文件沙箱扩展到 macOS / Windows / 鸿蒙的困难，与「留出位置」的工程先例

## 这份文件是什么

`fs-agent` 已经拍定在 Linux 上用 **bubblewrap**（`bwrap`）做进程级文件沙箱：`--ro-bind / /` 整机只读 + `--bind <cwd> <cwd>` 工作区可写 + `--tmpfs /tmp`。这份文件回答两个问题：

1. **如果将来把这层沙箱扩展到 macOS、Windows、乃至鸿蒙（HarmonyOS），各自的困难是什么。**
2. **「现在就留出位置」这件事，业界是怎么做的、代价是什么。**

姊妹篇已经覆盖的部分，这份文件**不重做**：

- [`01`](01-dsh-workspace-permissions-and-shell.md) 查清了 DSH（`@deepseek-ai/dsh`）的权限 / 审批模型、平台 rung 链与失败关闭；
- [`02`](02-linux-sandbox-primitives-and-tools.md) 查清了 Linux 机制、现成工具与 Rust 生态；
- [`03`](03-agent-sandbox-precedents.md) 查清了同类 agent 产品（Codex、Claude Code/srt、Cursor、Gemini、Devin、Goose、Pi）的先例；
- [`04`](04-local-probe-bwrap-and-landlock.md) 是这台机器上的实测。

**这份文件不是设计建议**，不替本仓库拍任何决定。②③④ 清点三个平台的困难（④ 是重点），⑤ 清点「统一抽象」的工程先例与代价，⑥ 是一张对照表，⑦ 写清楚哪些问题这份文件回答不了。

读者是接下来要拍「要不要现在留位置、留成什么形状」的人。

## 来源与版本与观察时点

**观察时点：2026-10-01（Asia/Shanghai）**。下面每节各自钉版本；版本相关的都写在节内。

| 对象 | 版本 / 时点 | 取证方式 |
| --- | --- | --- |
| `openai/codex` | commit `67727e7cf114cf3e1b71db368d74b24e32f6cb12`（2026-10-01 sparse clone 的 HEAD） | `git clone --depth 1 --filter=blob:none --sparse`（取 `codex-rs/{sandboxing,protocol,linux-sandbox,mxc-sandbox,windows-sandbox-rs}`）与 `raw.githubusercontent.com` 按 commit 取单文件；行号即该 commit 的行号 |
| HarmonyOS NEXT 安全技术白皮书 | 文档版本 **V1.0（2024-08-13）**，82 页 PDF | 华为消费者业务官网 `consumer.huawei.com` 下载，`pdftotext -enc UTF-8` 提取后检索 |
| 鸿蒙内核（HongMeng Kernel） | OSDI'24 论文（2024-07-10–12，Santa Clara） | [`osdi24-chen-haibo.pdf`](https://www.usenix.org/system/files/osdi24-chen-haibo.pdf) 下载后 `pdftotext` 提取 |
| OpenHarmony 文档 | GitHub 镜像 `openharmony/docs`，`master` 与 `OpenHarmony-6.0-Release`（未钉 commit） | `raw.githubusercontent.com/openharmony/docs`；`gitcode.com/openharmony/docs` |
| OpenHarmony 内核仓库 | `gitee.com/openharmony/kernel_liteos_a`、`kernel_liteos_m`、`kernel_linux_5.10`、`kernel_linux_6.6`（README 未钉 commit） | `raw.giteeusercontent.com` |
| Microsoft MXC | `github.com/microsoft/mxc` 的 `main` 分支（未钉 commit）；README 自称 early preview | `raw.githubusercontent.com/microsoft/mxc/main/...` |
| Microsoft Learn | `Restricted Tokens`（页面标注 Last updated 2021-01-07）、`AppContainer isolation`（Last updated 2025-07-08） | `learn.microsoft.com` 页面 |
| Claude Code 沙箱文档 | 抓取于 2026-10-01；正文提到 `v2.1.212`、`v2.1.216`、`v2.1.246`、`v2.1.257`、`v2.1.260`、`v2.1.271` 等版本点 | [`Configure the sandboxed Bash tool`](https://code.claude.com/docs/en/sandboxing)（Mintlify 的 Markdown 版本） |
| Devin CLI 沙箱文档 | 抓取于 2026-10-01 | [`Sandbox`](https://docs.devin.ai/cli/sandbox)（文档 Markdown 版本） |
| DSH 的 Windows rung 笔记 | `deepseek-ai/deepseek-harness` 的 `master` 分支，`2026-08-08` 日期，状态 `implemented`（**未钉 commit**） | `raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/.agents/notes/implemented/feature/2026-08-08-windows-acl-restricted-token-sandbox.md` |
| `birdcage` / `extrasafe` / `cross-sandbox` | crates.io 官方 API（2026-10-01 抓取） | `https://crates.io/api/v1/crates/<name>` |
| macOS man pages | `sandbox-exec(1)` 日期 March 9, 2017；`sandbox_init(3)`（Mac OS X 10.8）；`sandbox(7)`（Mac OS X 10.7） | `keith.github.io/xcode-man-pages`、`manpagez.com` 两个 Apple man page 镜像 |

**引用格式**：源码写 `owner/repo@commit:路径:行号`；官方文档给 `[标题](URL)` 并指出小节名；crates.io 数据标 `updated_at` 与下载量。凡是我没有实际读到原文的，一律写「**未读**」或「**一手未验证**」，不拿二手内容顶替。

**两条环境事实，先说清楚，免得误读**：

- **这份会话自己跑在一个 `workspace-write` 的内核沙箱里**（[`04`](04-local-probe-bwrap-and-landlock.md) 已记录），每次 `bash` 调用是**独立的 mount namespace**，`/tmp` 不跨调用保留。所以下面凡是「下载 → 解析」的动作都在**同一条命令内**完成；所有临时文件都在宿主的 `/tmp`，**没有在仓库里建任何文件**。
- **`03` 钉的 Codex commit 是 `a5cce889`（2026-09-30），本文件钉的是 `67727e7`（2026-10-01）**。两个 commit 之间 `codex-rs/` 的内容可能已经漂过；引用时请连同各自的 commit 一起引。

---

## ① 结论先行

1. **macOS 的一手现状补上了**：[`03`](03-agent-sandbox-precedents.md) ⑦ 第 5 条标的「`sandbox-exec` 的官方废弃声明一手未验证」，现在找到了 —— **Apple 自己的 man page 就写着 DEPRECATED**。`sandbox-exec(1)` 的 NAME 行是 `sandbox-exec — execute within a sandbox (DEPRECATED)`，DESCRIPTION 里写「The `sandbox-exec` command is DEPRECATED. Developers who wish to sandbox an app should instead adopt the App Sandbox feature...」（日期 March 9, 2017）；`sandbox_init(3)` 的 NAME 行同样是 `(DEPRECATED)`。**但四家 agent 产品今天全部依赖它**，而且 `sandbox-exec` 这条命令今天仍然存在于 macOS 上、仍然被用。

2. **Seatbelt 与 bwrap 不是同一类东西，但能表达同一件事。** 它是「白名单 + 求值时匹配」：`(deny default)` 起手，然后逐条 allow。Codex 的实际写法是 `(allow file-read*)` 表达「读整机」、`(allow file-write* (subpath (param "WRITABLE_ROOT")) (require-not (subpath ...)))` 表达「只写这个目录」，所以**「工作区可写、区外只读」是可表达的**。它比 bwrap 多出整整一层维度：mach port（`(deny mach-lookup (xpc-service-name-prefix ""))`）、sysctl 白名单、`fcntl` 命令、Apple Events —— 这些都是文件系统挂载表管不到的。代价在另一边：`sandbox(7)` 明说限制「generally enforced upon **acquisition** of operating system resources only」，**已经打开的 fd 不受约束**；它对空目录/子路径的删除还要额外写 `file-write-unlink` 规则，规则求值顺序本身我没有拿到 Apple 一手规范。

3. **Windows 的三条路线，没有一条能不动宿主 ACL 就表达「工作区可写、区外只读」。** 受限令牌（restricted token）是一个**减权**原语：去掉特权、把 SID 标成 deny-only、加 restricting SID 列表 —— 它**只能收窄，不能授予某个目录写权限**；要「开一个写窗口」必须往那个目录的 DACL 里写 ACE。AppContainer 要按路径**逐条开 ACE**（Microsoft 官方页面说 read-write 可以授予特定文件与注册表键、read-only「less restricted」，**它没有直接说「读也要预授权」**；说这一步的是 DSH 的笔记，转引见 5.1）。MXC 在 Windows 上的 ProcessContainer 后备（T3）也是 AppContainer + 宿主侧 DACL 增强（**转引** 5.1 的 DSH 笔记；MXC 官方的 OS-version 页我未读）。**这是「产品为什么不做」的机制背景**，但要注意：Claude Code 与 Devin 的官方文档都**只写了「不支持」，没有写理由**。

4. **鸿蒙是本次的重点，结论最硬：HarmonyOS NEXT 的内核不是 Linux。** 华为自己的 OSDI'24 论文摘要是：「This paper presents the design and implementation of HongMeng kernel (HM), **a commercialized general-purpose microkernel**」，并且「we design HM to be compatible with the **Linux API and ABI** to reuse its rich applications and driver ecosystems」，实现方式是「an **ABI-compliant shim** that identifies and **redirects Linux syscalls to IPCs**」。也就是说：用户态**看起来**是 Linux（musl、POSIX、Linux ABI 二进制能跑），但内核**不是** Linux。bwrap 与 Landlock 依赖的 mount/user namespace、LSM 在这一层**没有对应物**（**推论**，依据是论文对 HM 架构的描述：core kernel 只留线程调度、串/定时器驱动、访问控制，其余是 IPC 连接的 OS service；全文没有把 namespace / LSM / seccomp 列为可用给用户态的机制）。

5. **鸿蒙有等价的隔离能力，但都是系统侧、启动时定死、第三方程序无法施加。** 白皮书原文：SE Harmony 的强制访问控制策略「在设备启动时加载到内核中，**无法被动态更改**」；系统调用过滤「基于**只读文件系统中的规则文件**」。应用沙箱在官方文档里的定义是「系统会在内部存储空间映射出一个专属的『应用沙箱目录』」「应用默认仅能看到自己的应用文件以及少量的系统文件」「**所有应用的目录可见范围均经过权限隔离与文件路径挂载隔离**」——路径视图由**系统**给，不是应用能设的边界。

6. **鸿蒙上跑 CLI（含 DSH 自己）是有真机先例的，但每一步都要额外工程。** Harmonybrew 生态的第三方 tap 在鸿蒙 PC（OHOS aarch64）上真机跑通了 `opencode`、`claude-code`、`bun`，而且 `deepseek-harness` 已被移植（含 OHOS 补丁集，其中一条明写「**无沙箱放行**」）。前提是三条：①过 HiShell 的**强制代码签名**（自研/自签工具，否则 `Permission denied`）；②带一个 `ohos-compat-shim` 兜 syscall 差异；③架构必须是 aarch64（x86_64 要另走 box64 路线）。

7. **「留出位置」业界有三种形状**：**(a) 平台链 + 运行时探测**（DSH 的 rung 链，只有多候选时才探测）；**(b) 一个枚举装多后端 + 粘性选择**（Codex 的 `SandboxType`，`WindowsMxc` 一旦被选中就不会被旧的 level 改回去）；**(c) 一个 trait 后面挂后端**（`birdcage` 的 `Sandbox`，`cross-sandbox` 的 `PlatformSandbox`）。前两种是同一家内部做的；第三种在 crates.io 上的采纳度极低（见 ⑤）。

8. **代价是真实的、有具体账目，而且写这些账的人自己承认它。** DSH 的 Windows 笔记逐条否掉了 MXC、AppContainer、landstrip 三个候选，理由不是「抽象不好」，而是「这两条路线要求**宿主侧 DACL 大改**」和「MXC 的 OS 地板在 Windows 11 24H2/25H2」；最后 Windows rung 直接写在 raw ACL + 受限令牌 + Low integrity 上，**自报 `enforcement: partial`**，并把代价列了一长串（硬链接别名无法路径约束、`SetNamedSecurityInfoW` 全树传播在大工作区上「tens of seconds」、命名管道仍然打不开）。微软自己的 MXC 是官方版的「跨平台统一抽象」，但 README 顶部挂着「early preview」和「**no MXC profiles should be treated as security boundaries currently**」。

9. **我没找到一篇专门反对「过早抽象多平台沙箱」的公开文章。** 能拿到的最接近一手材料是上面第 8 条那些项目自陈，加上本仓库 README 自己写的「不预做抽象」（转引 [`01`](01-dsh-workspace-permissions-and-shell.md) §⑦）。这一点我写进 ⑦ 的边界，**不把它包装成「业界共识」**。

---

## ② macOS：`sandbox-exec` + Seatbelt 的一手现状

### 2.1 废弃这件事，Apple 自己写了

| man page | 版本日期 | 原文 | 来源 |
| --- | --- | --- | --- |
| `sandbox-exec(1)` | March 9, 2017 | NAME：`sandbox-exec — execute within a sandbox (DEPRECATED)`；DESCRIPTION：「The `sandbox-exec` command is **DEPRECATED**. Developers who wish to sandbox an app should instead adopt the **App Sandbox** feature described in the App Sandbox Design Guide.」 | [xcode-man-pages: sandbox-exec(1)](https://keith.github.io/xcode-man-pages/sandbox-exec.1.html) |
| `sandbox_init(3)` | Mac OS X 10.8 | NAME：`sandbox_init, sandbox_free_error -- set process sandbox (DEPRECATED)`；DESCRIPTION：「The `sandbox_init()` and `sandbox_free_error()` functions are **DEPRECATED**. Developers who wish to sandbox an app should instead adopt the **App Sandbox** feature described in the App Sandbox Design Guide.」 | [manpagez: sandbox_init(3)](https://www.manpagez.com/man/3/sandbox_init/) |
| `sandbox-exec(1)` | Mac OS X 10.6（**旧版**） | 同样的 SYNOPSIS，但**没有** DEPRECATED 字样 | [manpagez: sandbox-exec(1)](https://www.manpagez.com/man/1/sandbox-exec/) |

**事实**：`sandbox-exec` 与 `sandbox_init` 的废弃声明出现在 Apple 随系统发布的 man page 里，两份 man page 都把用户指向 **App Sandbox**（这是给 App Store / 签名的 `.app` 用的机制，不是给任意 CLI 的）。

**取证限界（必须写明）**：这两份 man page 我是通过**第三方镜像**读到的（`keith.github.io/xcode-man-pages`、`manpagez.com`），不是从 `developer.apple.com` 或本机 `man` 读到的。**我没有在 `apple.com` 域内找到一份「sandbox-exec 已废弃、将在 X 版移除」的独立官方公告页**。社区侧的对应物是 [`03`](03-agent-sandbox-precedents.md) ⑦ 第 5 条引的 [openai/codex#215](https://github.com/openai/codex/issues/215)。

**推论**：man page 的措辞是「建议改用 App Sandbox」，**没有说 `sandbox-exec` 会被移除**。今天四家 agent 产品全在用它（下节），这条命令也仍然存在 —— 所以「废弃」在这里的含义是**「不再推荐的私有接口」**，不是「即将消失」。

### 2.2 Seatbelt 的模型：自愿、获取时检查、继承

`sandbox(7)`（Mac OS X 10.7，经 [manpagez 镜像](https://www.manpagez.com/man/7/sandbox/)）逐字：

> The **sandbox** facility allows applications to **voluntarily restrict** their access to operating system resources. This safety mechanism is intended to limit potential damage in the event that a vulnerability is exploited. **It is not a replacement for other operating system access controls.**
>
> **New processes inherit the sandbox of their parent.** Restrictions are generally enforced upon **acquisition** of operating system resources only. For example, if file system writes are restricted, an application will not be able to `open(2)` a file for writing. However, **if the application already has a file descriptor opened for writing, it may use that file descriptor regardless of restrictions.**

三条从这里读得出来：

- **继承**：子进程继承父的 sandbox —— 与 bwrap 的「新 namespace 里 exec」在效果上一致（都能覆盖整棵子进程树），但机制不同（Seatbelt 是进程属性，bwrap 是 namespace + mount 表）。
- **获取时检查**：已经打开的 fd 可以继续用。bwrap 的只读挂载对已打开的 fd 同样没有追溯力（挂载表是后续解析路径时的视图），这一点两者相近。
- **它是「自愿自我限制」且「不是其他访问控制的替代品」**：用它给**别人的**进程加约束，靠的是 `sandbox-exec` 在 exec 前把 profile 应用到子进程，而不是靠内核强制某个主体。

`03` ⑦ 第 5 条还记了四家的 macOS 路径**全部**依赖 `/usr/bin/sandbox-exec`（Codex、Claude Code/srt、Cursor、Gemini），本条不重做。

### 2.3 SBPL profile 的工程形状：Codex 的实际写法

`openai/codex@67727e7cf114cf3e1b71db368d74b24e32f6cb12` 的 Seatbelt 后端是**基础的 `.sbpl` 文件 + 运行时拼字符串**：

- 四个 profile 用 `include_str!` 编进二进制：基础策略、网络策略、preferences 策略、只读平台默认（`codex-rs/sandboxing/src/seatbelt.rs:21-28`）。
- 基础策略 `seatbelt_base_policy.sbpl` 只有 **116 行**，第 1 行 `(version 1)`，第 7–8 行是注释 `; start with closed-by-default` + **`(deny default)`**（`:1-8`）。之后逐条 allow：`process-exec` / `process-fork` / `signal (target same-sandbox)`（`:11-13`）、`process-info* (target same-sandbox)`（`:16`）、给 `/dev/null` 的 `file-write-data`（`:18-21`，用 `require-all` 同时限定 path 与 vnode 类型）、一长串 `sysctl-read (sysctl-name "...")`（`:24-` 起）。
- 生成命令参数是 `create_seatbelt_command_args()`（`seatbelt.rs:871`），核心在 `build_seatbelt_access_policy()`（`:490` 起）：读用 `("file-read*", "READABLE_ROOT")`、写用 `("file-write*", "WRITABLE_ROOT")`（`:499-500`），路径谓词是 `(subpath (param "..."))`（`:531`）。
- **「只写这个目录」的写法**：`(allow file-write* (subpath <可写根>) (require-not (subpath <排除项>)))`（`:542-572`）；另外可写根下面的只读子路径会被重新压成只读，理由是「否则把一个可写目录改名就能把它的子孙搬出策略」（`:919-940`，转引 [`03`](03-agent-sandbox-precedents.md) ②，我在本 commit 读到的同族代码是 `:925` 的 `read_only_subpaths` 循环）。
- **「读整机」的写法**：`"; allow read-only file operations\n(allow file-read*)"`（`:988`）—— 一条不带谓词的 allow。
- 有一条注释暴露了对 SBPL 语义的不确定：`:949-951` 处写 `(allow file-write* (regex #"^/"))`，注释是 `// Allegedly, this is more permissive than (allow file-write*)`（**「据说是」** —— 连 Codex 的作者也只能靠社区经验判断这条规则的强度）。

**事实**：Codex 的 Seatbelt 后端是**运行时拼 profile 字符串 + `sandbox-exec -p`**，不是「写一份静态 profile」。这与 [`03`](03-agent-sandbox-precedents.md) ② 的记载一致。

### 2.4 表达力：白名单式 Seatbelt 与「挂载表」bwrap

这是任务要求回答的关键差异。分四层看。

**第一层：能不能表达「这个目录可写、其余只读」。**

**能。** 而且是**从「默认拒绝」往上加**：`(deny default)` + `(allow file-read*)` 得到「读整机」+ `(allow file-write* (subpath <工作区>) (require-not ...))` 得到「只写工作区」。这比 bwrap 的挂载表在**表达力上更宽**：bwrap 要「整机只读 + 一个目录可写」，必须在挂载表里造出树的形状（`--ro-bind / /` 再 `--bind <cwd> <cwd>`），而 Seatbelt 只需要两条写谓词。

**第二层：谓词的形状不同。**

bwrap 的模型是**视图 / 挂载点替位**：一个路径在树的哪个位置被换成什么（ro-bind、bind、tmpfs、dev、proc），语义是「解析这个路径时看到什么」。Seatbelt 的模型是**操作 × 路径谓词**：每条规则说「哪一类操作（`file-read*` / `file-write*` / `file-write-unlink` / `mach-lookup` / `network-outbound` / `sysctl-read` …）× 哪个路径模式（`literal` / `subpath` / `regex` / `require-all` / `require-not`）」。

**推论**：两种模型**不互相包含**。挂载表能自然表达「这个路径在沙箱里指向别处」（bind 一个别的目录、`--tmpfs` 造一个空目录），Seatbelt 没有等价物（它是「允许/拒绝这个操作」，不能把路径重定向到别的 inode）。反过来 Seatbelt 能表达「这条路径只允许 unlink 不允许 create」（`(deny file-write-unlink ...)`，`seatbelt.rs:526`、`:678`、`:1077`），挂载表表达不了。

**第三层：Seatbelt 多管一大片 bwrap 完全不管的维度。**

同一个文件里能读到（`seatbelt.rs`）：

- `(deny mach-lookup (xpc-service-name-prefix ""))`（`:1069`）—— 挡 mach port / XPC 服务查找；
- `(deny system-fcntl (fcntl-command 80 110))`（`:1083-1086`，注释说「Even deny-default needs this explicit deny」）；
- `sysctl-read` 的一长串白名单（`seatbelt_base_policy.sbpl:24-`）；
- Claude Code 文档里的 `allowAppleEvents`：默认挡 Apple Events，打开它会「**removes code-execution isolation**」（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Security limitations”）。

**对照**：bwrap 是一个 mount namespace 工具，它对 mach port、sysctl、Apple Events 一无所知（Linux 上没有这些；它有 `--unshare-ipc` / `--unshare-net` / `--unshare-pid` 这类 namespace 开关，维度并不重叠）。

**第四层：求值顺序我没有一手依据。**

SBPL 的规则冲突（`(deny default)` 与后来的 `allow`、更宽的 `allow` 与更窄的 `deny`、last-match-wins 还是 deny 优先）我**没有拿到 Apple 的规范文档**：`sandbox(7)` 只讲「获取时强制」和继承，不讲规则代数；`sandbox-exec(1)` 只讲 `-f/-n/-p/-D` 四个参数。能拿到的**证据级材料是使用方式本身**：Codex 用「先 `(deny default)`、再逐条 allow、用 `require-not` 表达排除」，以及代码里那句 `// Allegedly, this is more permissive than (allow file-write*)`。**这一条标「一手未验证」。**

对照参考：Claude Code 在**它自己的配置层**（不是 SBPL 层）给了重叠规则的处理表，原文是「When read rules overlap, the **narrower path applies**」，并且 `allowRead: ["~/"]` + `denyRead: ["~/.env"]` 的结果是 `.env` **仍然被挡**（「the deny holds inside a wider allow, so a broad allow can't silently re-expose a secret」，[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Configure sandboxing”）。**这是 Claude Code 的语义，不能当作 SBPL 的语义**。

### 2.5 有没有 Apple 官方的替代机制

- `sandbox_init(3)` 的 man page 把开发者指向 **App Sandbox**（`com.apple.security.app-sandbox` entitlement）。那是给**签名并启用 App Sandbox 的 `.app`** 用的；**它对「未签名 / 自用 / 非 App Store 的 CLI」不适用**（**推论**，依据是 man page 的措辞「Developers who wish to sandbox **an app**」，以及 App Sandbox 是 entitlement 驱动、需要签名这一事实；我**没有**找到一份 Apple 一手文档明确写「App Sandbox 不适用于命令行工具」）。
- **未读 / 一手未验证**：Endpoint Security framework、`com.apple.security.app-sandbox` 之外的 entitlement 组合、MDM 下发，我都没有读 Apple 一手文档。**这份文件不对「非 App Store 的 CLI 在 macOS 上有什么官方沙箱通道」下结论。**

### 2.6 Claude Code 在 macOS 上踩过的坑（作为表达力/兼容性的代价样本）

同一份官方文档（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Troubleshooting”）逐条列了：

- `watchman` 与沙箱不兼容 → `jest --no-watchman`；
- Go 写的 CLI（`gh` / `gcloud` / `terraform`）在 Seatbelt 下 TLS 校验失败 → 加进 `excludedCommands`，或者打开 `enableWeakerNetworkIsolation`；
- `open` / `osascript` / 浏览器授权流报 `-600` → `allowAppleEvents`，而打开它「removes code-execution isolation」；
- `pbcopy` / `xclip` / `wl-copy` 拿不到系统剪贴板。

**事实**：这些不是 Seatbelt 的缺陷，而是「白名单式的进程级策略会把所有依赖系统服务的工具都挡在外面」这一模型特征在 macOS 上的具体表现。

---

## ③ Windows：三条路线与「为什么大家不干」

### 3.1 受限令牌（restricted token）：只能收窄，不能授予

[Restricted Tokens - Win32 apps](https://learn.microsoft.com/en-us/windows/win32/secauthz/restricted-tokens)（Microsoft Learn，页面标注 Last updated 2021-01-07）逐字：

> A restricted token is a *primary* or impersonation *access token* that has been modified by the **CreateRestrictedToken** function. ... The **CreateRestrictedToken** function can restrict a token in the following ways:
>
> - Remove **privileges** from the token.
> - Apply the **deny-only attribute** to SIDs in the token so that they cannot be used to access secured objects.
> - Specify a list of **restricting SIDs**, which can limit access to securable objects.
>
> The system uses the list of restricting SIDs when it checks the token's access to a securable object. When a restricted process or thread tries to access a securable object, the system performs **two access checks**: one using the token's enabled SIDs, and another using the list of restricting SIDs. **Access is granted only if both access checks allow the requested access rights.**

同一页还有两条关键：

- 「if the `CreateProcessAsUser` call specifies a restricted version of the caller's primary token, this privilege [SE_ASSIGNPRIMARYTOKEN_NAME] is not required. **This enables ordinary applications to create restricted processes.**」——不需要管理员特权就能起受限进程，这是它能被 agent 产品用的原因。
- Note：「Applications that use restricted tokens should run the restricted application on **desktops other than the default desktop**. This is necessary to prevent an attack by a restricted application, using **SendMessage** or **PostMessage**, to unrestricted applications on the default desktop.」——窗口消息是一条要单独处理的逃逸路径。

**推论（这条是本文件对 Windows 的核心判断）**：受限令牌的三个手段都是**减法**（去特权、把 SID 变 deny-only、加 restricting SID）。它能表达「不许写」，**不能表达「这个目录可以写」** —— 因为「可以写」需要那个对象的 DACL 里有一条允许受限 SID 的 ACE，而写 ACE 是**改宿主文件系统的持久状态**。DSH 的 Windows 笔记把这件事说得更直白（见 5.1）。

### 3.2 AppContainer：连读都要预授权

[AppContainer isolation - Win32 apps](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)（Last updated 2025-07-08）把隔离分六个维度：Credential / Device / **File** / Network / Process / Window isolation。File isolation 一节逐字：

> Controlling file and registry access, the AppContainer environment prevents the application from modifying files that it should not. **Read-write access can be granted to specific persistent files and registry keys. Read-only access is less restricted.** An application always has access to the memory resident files created specifically for that AppContainer.

**事实（Microsoft 页面的原话只到这里）**：官方页面说 read-write 可以授予特定文件与注册表键、read-only「less restricted」，并说应用对「memory resident files created specifically for that AppContainer」总是有访问权 —— **它没有直接说「读也要逐路径预授权」**。

**更直接的一手依据来自 DSH 的笔记（转引，见 5.1）**：它把这条写成了明确的机制陈述 ——「An AppContainer token carries **no ambient read access**: every readable path must be pre-granted through capabilities or explicit ACEs」。**推论**：若这句成立，AppContainer 的默认就是什么都拿不到，要用就得为它逐路径开 ACE；这与「读整机、只写工作区」这个需求正面冲突 —— 要「读整机」就得给整机的每个路径开读 ACE。

### 3.3 Microsoft MXC：微软自己在做的「跨平台统一沙箱」

[MXC README](https://github.com/microsoft/mxc)（`main` 分支，2026-10-01 抓取）自述：

> MXC is a **sandboxed code execution system** for running untrusted code (model output, plugins, tools) on **Windows, Linux, and macOS**. It provides multiple containment backends — from OS-native process sandboxes to full VMs — behind a **unified JSON configuration schema and TypeScript SDK**.

后端清单（同页 Features）：`ProcessContainer`、`Windows Sandbox`、`LXC`、**`Bubblewrap`**、**`Seatbelt (macOS)`**、`MicroVM (NanVix)`、`Hyperlight`、`IsolationSession`、`WSLC`。平台默认表：

| 平台 | 默认后端 | 最低构建 |
| --- | --- | --- |
| Windows 11 24H2+（verified on 25H2） | `processcontainer` | `processcontainer`: 26100 (24H2)；`isolation_session`: 26340.9212（Insider Preview） |
| Linux x64 / ARM64 | `bubblewrap` | — |
| macOS ARM64 / x64（schema `0.7.0-alpha`+） | `seatbelt` | — |

**它对 fs-agent 这类需求最相关的一条限制**（同页 Features，逐字）：**「Filesystem Policy: Read-only and read-write path lists（denied paths not yet supported on Windows）」**。

**README 顶部的警告必须原样引**：

> This repository contains an **early preview** ... The underlying sandboxes in this early preview are expected to change ... There are known cases where **the current policies generated by the MXC SDK in this repository are overly permissive** and will be addressed before this is made more generally available. Security researcher partnership while MXC matures is welcome, however **no MXC profiles should be treated as security boundaries currently**.

Windows 侧的执行链（[process-container guide](https://github.com/microsoft/mxc/blob/main/docs/process-container/guide.md)，`main` 分支）：

```
SandboxPolicy -> SDK: createConfigFromPolicy() -> ContainerConfig JSON
  -> wxc-exec: parses ContainerConfig
    -> BaseContainerRunner: builds a PSEC FlatBuffer
      -> CreateProcessSecurityEnvironment (processmodel.dll)
        -> CreateProcessW with PROC_THREAD_ATTRIBUTE_SECURITY_ENVIRONMENT
          -> OS applies restrictions
```

同页两条关键事实：**「The source-of-truth schema and processmodel implementation live in the internal Windows OS source tree.」**；以及 **「If PSEC cannot represent the request, selection must continue to an AppContainer tier that can fully enforce it. Never silently omit a requested restriction.」**——也就是 **PSEC（Process Security Environment）是 OS 侧的能力，MXC 只是它的封装**，而且**表达力不足时会降级到 AppContainer tier**。

这也解释了 Codex 的 MXC 后端为什么自述依赖 PSEC：`codex-rs/mxc-sandbox/README.md:1-7` 逐字「It requires a working Windows **process security environment (PSEC)**. It never invokes MXC's AppContainer dispatcher, edits host ACLs, creates sandbox users, runs setup, or requests elevation.」；`:13-17` 补充「A requested deny path additionally requires the native **`PSE_SUPPORT_FS_DENY`** capability; otherwise the command fails before launch.」；`:31-34` 说 `windows.sandbox = "mxc"` 是 **strict**，且「Command failures never trigger backend fallback」。

**未读**：[MXC 的 Windows OS-version policy support 页](https://github.com/microsoft/mxc/blob/main/docs/process-container/os-version-support.md)我没有读，25H2 上具体哪些策略项能强制、哪些会掉到 T3，我是**转引 DSH 笔记的记载**（见 5.1）。

### 3.4 为什么产品不干：官方只写了「不支持」

**Claude Code**（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，2026-10-01 抓取）逐字：

> The sandbox is built into Claude Code and runs on macOS, Linux, and WSL2. **Native Windows is not supported.** On Windows, run Claude Code inside a WSL2 distribution.

小节 “Platform and tool compatibility” 再写一次：「**Platform support**: supports macOS, Linux, and WSL2. WSL1 and native Windows are not supported.」组织级配置那节还写：「The sandbox does not run on native Windows, so if your fleet includes Windows hosts, scope this configuration to macOS and Linux or have those users run Claude Code inside WSL2 or a container.」

**Devin**（[Sandbox](https://docs.devin.ai/cli/sandbox)，2026-10-01 抓取）逐字：

> **Windows**: OS-level sandboxing is not currently supported on Windows. Sessions on Windows will **hard-fail** when `--sandbox` is passed or when sandbox enforcement is **Required**, including when the CLI runs as an ACP server inside an IDE (e.g., Devin Desktop).

同页还写：「If sandbox resolution fails ..., the CLI will **refuse to start** rather than running unsandboxed.」以及企业强制那节：「If any users are on Windows, they will be **unable to run the CLI** until OS-level sandboxing is supported on Windows or the policy is relaxed to **Optional**.」

**事实（重要）**：**两家的官方文档都没有给理由**。它们只说「不支持 native Windows」，没有解释「为什么」。任何「因为 Windows 的沙箱机制不够」的说法都是**读者自己的推断**，不是这两家的官方口径。机制层面的解释只能从 3.1–3.3 的一手材料推出来（**那段推论已在上文单独标注**）。

**Devin 的「工作区可写、区外只读」表述有一手原文**（同页 “How the sandbox works”）：

> * **Writable paths** are derived from granted `Write(...)` permission scopes plus the workspace directory; **everything else is read-only**
> * **Readable paths** are everything except paths covered by `Read(...)` rules in the `deny` list, which are hidden from sandboxed commands entirely

这条值得单列，因为它与 fs-agent 已拍定的形状逐字对应，而且它是**产品文档层面的承诺**，不是内核机制描述。

### 3.5 Codex 的 Windows 双后端

`openai/codex@67727e7cf114cf3e1b71db368d74b24e32f6cb12` 的 `codex-rs/protocol/src/sandbox.rs`（全 42 行）给出枚举与选择逻辑：

- `SandboxType::{None, MacosSeatbelt, LinuxSeccomp, WindowsRestrictedToken, WindowsMxc}`（`:8-16`）；
- `effective_windows_sandbox_type()` 的注释是 `/// Preserves explicit MXC selection while honoring legacy runtime level updates.`（`:30`）：`(WindowsMxc, _) => WindowsMxc`（粘性）、`(_, Disabled) => None`、`(_, RestrictedToken | Elevated) => WindowsRestrictedToken`（`:35-40`）。

**受限令牌那条后端在代码里是存在的**：`codex-rs/windows-sandbox-rs/Cargo.toml` 的包名是 `codex-windows-sandbox`，产出三个二进制 —— `codex-windows-sandbox-setup`、`codex-command-runner`、`codex-windows-managed-deny-probe`；`src/lib.rs:1-2` 有 `#![allow(unsafe_op_in_unsafe_fn)]` 与注释「Rust 2024 surfaces this lint across the crate; keep the edition bump separate from the eventual unsafe cleanup.」**我读到「有 setup 步骤 + 有独立的 command runner」，这是「Windows 受限令牌那条路需要一个准备阶段、而且限制最终要落在宿主的 ACL / 完整性标签上」这条说法的代码侧印证**（[`01`](01-dsh-workspace-permissions-and-shell.md) §④ 记的 DSH 侧也是这个形状：随机私有 temp 目录 + `--write-sid`/`--temp-write-sid` + ACE 生命周期）。**注意别把它读成「创建 sandbox 用户」**：DSH 的笔记明确说它走的不是身份路线（见 5.1），Codex 的 MXC 后端也把「creates sandbox users」列为它**不做**的事（`codex-rs/mxc-sandbox/README.md:5-6`）。

**未读**：`codex-rs/windows-sandbox-rs/README.md` 在该 commit 上**不存在**（404），所以我**没有**读到 Codex 对 Windows 后端的自述文档；`codex-rs/mxc-sandbox/README.md` 读到了（见 3.3）。

---

## ④ 鸿蒙（HarmonyOS / OpenHarmony）—— 本文件的重点

`01`–`04` 四份都没有覆盖这一节。下面每一小节的**一手来源**都尽量是华为/OpenHarmony 官方材料或华为作者署名的论文；**二手材料（维基、社区 tap）只用来指路或作为「存在性证据」，不当事实依据**。

### 4.1 内核是什么

**最强的一手来源是华为自己的论文。** [Microkernel Goes General: Performance and Compatibility in the HongMeng Production Microkernel](https://www.usenix.org/system/files/osdi24-chen-haibo.pdf)（OSDI'24，2024-07-10–12；作者全部署名 Huawei Central Software Institute，第一作者 Haibo Chen 兼上海交通大学）。摘要逐字：

> This paper presents the design and implementation of **HongMeng kernel (HM)**, a **commercialized general-purpose microkernel** that preserves most of the virtues of microkernels while addressing the above challenges. For the sake of commercial practicality, we design HM to be **compatible with the Linux API and ABI** to reuse its rich applications and driver ecosystems. ... **HM consists of a minimal core kernel and a set of least-privileged OS services**, and it can run complex frameworks like AOSP and OpenHarmony. HM has been deployed in production on **tens of millions of devices** in emerging scenarios, including smart routers, smart vehicles and smartphones ...

正文里关于架构与兼容层的逐字表述：

- 「To be practical for production deployment, HM achieves **full Linux API/ABI compatibility** and is capable of reusing the Linux applications and driver ecosystems such that it can run complex frameworks like AOSP [42] and OpenHarmony [35] with rich peripherals.」
- 「**Minimal microkernel with least-privileged and well-isolated OS services.** HM retains the minimality principle by keeping only the necessary functionality in the core kernel, including **thread scheduler, serial/timer drivers, and access control**, and leaving all other components as isolated OS services (multi-server) outside the core kernel. In addition, HM adopts **fine-grained access control** to preserve the principle of least privilege for better security.」
- 「Maximizing compatibility... HM integrates existing software ecosystems by achieving full Linux API/ABI compatibility through **ABI-compliant shim that identifies and redirects Linux syscalls to IPCs**. Moreover, HM reuses unmodified Linux drivers via a **driver container** that provides Linux runtime atop HM with minor engineering effort...」
- 设计总览表里两条：`Extended: Linux API/ABI compatible via an ABI-compliant shim.`、`Enhanced: Reusing Linux drivers efficiently via driver container with twin drivers.`；图 3 的标注：「**ABI-compliant shim ❸ enables binary compatibility.**」「AOSP/OpenHarmony App — **Binary Compatible**」。
- 第 5.1 节（syscall 重定向）：「HM achieves Linux ABI compatibility by placing an ABI-compliant shim in **IC0 (kernel space)**, which redirects Linux syscalls into IPCs towards appropriate OS services (identified by syscall number...)」；另有 `Vectored Syscalls` 一节处理不能在 shim 里单独完成的翻译。
- 安全模型用的是 **capability + address token**（「HM adopts fine-grained access control」「supplementing capabilities with address tokens」），**不是** LSM / namespace 这一族。

**华为官网的一手新闻稿**：[Huawei obtains highest-level security certification for smart device OSs](https://www.huawei.com/en/news/2023/8/cybersecurity-hongmengkernel-cceal6)（2023-08-15，Delft）—— 确认「Huawei's **HongMeng Kernel** was awarded the industry's first **Evaluation Assurance Level 6 Augmented (EAL6+)** certificate as part of Common Criteria」，评估方是 SGS Brightsight，引用了华为消费者 BG 软件工程部总裁龚体的话。**这篇稿子只说安全认证，不谈架构。**

**华为官方白皮书对内核的描述**：我下载并全文提取了《HarmonyOS NEXT 安全技术白皮书》V1.0（2024-08-13，82 页）。检索结果（**这是事实，不是推测**）：

| 关键词 | 在白皮书全文中的命中 |
| --- | --- |
| `Linux` | **0 次** |
| `AOSP` | **0 次** |
| `微内核` | **0 次** |
| `宏内核` | **0 次** |
| `鸿蒙内核` | **0 次** |
| `命名空间` / `namespace` | **0 次** |
| `SELinux` | **0 次** |
| `命令行` | **0 次** |
| `POSIX` | 1 次（讲 TEE 里支持 C 库与 POSIX API） |

**事实**：**这份官方白皮书没有描述 HarmonyOS NEXT 的内核形态**，也没有出现「微内核」字样。它的 5.3 节「隔离和访问控制」讲的是 Access Token / SE Harmony / 系统调用过滤 / TEE（见 4.4）。

**关于 OpenHarmony（开源侧）**，一手材料明确是**多内核**。[OpenHarmony 内核子系统文档](https://raw.giteeusercontent.com/openharmony/docs/raw/master/zh-cn/readme/%E5%86%85%E6%A0%B8%E5%AD%90%E7%B3%BB%E7%BB%9F.md)逐字：

> OpenHarmony 针对不同量级的系统，分别使用了不同形态的内核，分别为 **LiteOS 和 Linux**。在轻量系统、小型系统可以选用 LiteOS；在小型系统和标准系统上可以选用 Linux。

同页给出仓库矩阵（`kernel_liteos_a` 面向小型/标准系统、`kernel_liteos_m` 面向轻量系统、Linux 基于 **4.19.y / 5.10.y / 6.6.y** LTS 分支演进）。[`kernel_liteos_a/README_zh.md`](https://raw.giteeusercontent.com/openharmony/kernel_liteos_a/raw/master/README_zh.md)补充它是「基于 Huawei LiteOS 内核演进发展的新一代内核」，目录里有 `security`（「安全特性相关的代码，包括进程权限管理和虚拟 id 映射管理」）与 `compat/posix`。

**必须分清的两件事（这是本节最容易出错的地方）**：

- **OpenHarmony 是开源项目**，它的内核形态是官方文档写明的「LiteOS + Linux 多形态」。
- **HarmonyOS NEXT 是华为的商业发行版**，它用的内核是 **HM（鸿蒙内核）**，即上面 OSDI 论文那个微内核。**HM 是闭源的**（**推论**：论文自称「commercialized」，我没有找到 HM 的开源仓库；OpenHarmony 的 `kernel_*` 仓库里没有它）。
- **两者不能互相证明**。「HarmonyOS NEXT 的内核是 Linux」是**错的**（OSDI 论文 + 白皮书 0 次命中「Linux」这件事在方向上一致）；「OpenHarmony 标准系统用 Linux 内核」是**对的**，但它说的是开源项目。

**二手交叉印证（只作指路，不作依据）**：中文维基百科[「鸿蒙内核」](https://zh.wikipedia.org/zh-cn/%E9%B8%BF%E8%92%99%E5%86%85%E6%A0%B8)条目写「鸿蒙内核采用微内核架构」，并把它的一手引用指向 OSDI 论文、华为官网的 EAL6+ 新闻稿、以及 [HDC 2025 的官方演讲页【OS核心技术】全栈协同内核与通信技术](https://live.huawei.com/hdc2025/meeting/cn/15548.html)。**HDC 2025 那个页面我没有读**（未读），所以不引它的内容。

### 4.2 有没有开发者能用的命令行 / shell / 终端

**有，但要分清两条完全不同的路径。**

**(a) `hdc` 通道：电脑端 → 设备端 shell。** [OpenHarmony 官方 `hdc` 文档](https://raw.githubusercontent.com/openharmony/docs/refs/heads/master/zh-cn/application-dev/dfx/hdc.md)（`master` 分支，2026-10-01 抓取）逐字：「hdc（OpenHarmony Device Connector）是提供给开发人员的**命令行调试工具**，用于与设备进行交互调试、数据传输、日志查看以及应用安装等操作。**该工具支持在 Windows/Linux/MacOS 系统上运行**」。关键事实：

- `hdc shell` 「在设备端执行单次命令，例如 `hdc shell ls`。**无命令参数可进入设备端终端执行命令**」；
- 设备端命令集主要来自 **toybox**：「当前大多数命令都是由 toybox 提供，可通过 `hdc shell toybox --help` 获取命令帮助」；
- 环境准备要求：**设备侧**在「设置 > 系统 > 开发者选项」开启调试开关，电脑侧装 SDK / Command Line Tools；
- **`hdc shell -b <bundlename>` 可以进入某个可调试应用的应用沙箱目录**（文档链接到「命令行方式访问应用沙箱」）；使用条件是「该包名对应的已安装应用必须满足：使用**调试证书签名**，并且已在设备上启动」；
- 报错文本之一是 `/bin/sh: XXX : inaccessible or not found.`；
- **PC/2in1 形态设备上，hdc 的 client/server 调试功能可以被组织禁用** —— 错误码 `E00C001`「Operation restricted by the organization.」「**PC/2in1 形态设备的 hdc client/server 调试功能被组织禁用**」；
- 文档提到文件传输的 `-m` 参数会同步「DAC 权限，uid，gid，**MAC 权限**」。

**推论**：`hdc shell` 是**开发者调试通道**，不是「设备本机给终端用户的 shell」。它需要设备侧开开发者选项、电脑侧装工具链；在 PC/2in1 上还可能是组织策略禁止的。它**不是**给 fs-agent 这种「在设备上跑一个 agent、agent 再调 bash」的场景用的通道。

**(b) HiShell：鸿蒙 PC 自带的终端。** 我拿到的一手材料只有**第三方 tap 的转述**：[`social4hyq/homebrew-core`](https://github.com/social4hyq/homebrew-core)（Harmonybrew 的第三方 tap）README 里两次出现 —— 「鸿蒙 PC 终端（HiShell）**强制代码签名**——自行编译或直接下载的 Linux 程序一律 `Permission denied`」，以及 `hishell-font` 这个 formula 的说明「鸿蒙 PC 自带终端（HiShell）的 Nerd Font 图标字体」。

**一手未验证**：**我没有读到华为官方对 HiShell 的文档**。`developer.huawei.com` 的文档页在我这里抓到的只有 `文档中心` 四个字（页面是 JS 渲染的，`web_fetch` 拿不到正文）；搜索结果里出现的「在鸿蒙 pc 第三应用拉起 hishell」「HarmonyOS 鸿蒙 Next 中 MatePadEdge 不支持终端 HiShell 吗？」都是**华为开发者论坛的社区帖**（华为运营的论坛，但内容是用户发的），**不作为依据**。

### 4.3 能不能跑 Linux ELF 二进制

答案分三层，一层比一层具体。

**第一层（内核侧）：HM 声称兼容 Linux API/ABI，并且「enables binary compatibility」。** 依据就是 4.1 的 OSDI 论文：ABI-compliant shim 把 Linux syscall 重定向到 IPC，图 3 标注 `ABI-compliant shim ❸ enables binary compatibility`，并画了 `AOSP/OpenHarmony App — Binary Compatible`。

**第二层（用户态）：要在鸿蒙 PC 上跑一个 aarch64 Linux CLI，实测路径是「自签名 + 兼容 shim」。** 一手证据是 Harmonybrew 生态：

- [`social4hyq/homebrew-core`](https://github.com/social4hyq/homebrew-core)（第三方 tap，面向「鸿蒙 PC（HarmonyOS，OHOS aarch64）」）README 逐字：「**鸿蒙 PC 终端（HiShell）强制代码签名——自行编译或直接下载的 Linux 程序一律 `Permission denied`**，且不少常用工具还没适配鸿蒙。本 tap 逐一移植、**签名**、**真机验证**后打包成 bottle」。
- 同一个 README 的 “已知限制” 一节逐字：

  > HarmonyOS 与 Linux 存在**少量系统调用差异**，本 tap 通过 `ohos-compat-shim`（**预加载兼容层**，已内嵌进 bun 及所有 bun 编译产物）自动处理，使用者一般无需关心。极端场景下可能感知到：
  >
  > - **性能**：`close_range`/`fchmodat2` 等**缺失的 syscall** 由 shim 替换为兼容实现，高并发 IO 吞吐略低于 Linux 基线
  > - **临时文件**：**沙箱内 `/tmp` 只读**，`tmpfile()` 类调用由 shim 改走 `$TMPDIR`——请确保 `$TMPDIR` 指向可写分区
  > - **用户信息**：`getpwuid_r()` 由 shim 经 HarmonyOS 账号 API 兜底
  > - **文件系统**：**硬链接当前未向三方应用开放（`linkat` 返回 EPERM）**，未加载 shim 的进程直接失败；加载 `ohos-compat-shim` 的进程由 shim 自动降级为原子复制
  > - **管道 I/O**：`splice()` 的 EOF 语义与 poll/epoll 唤醒问题已由 shim 修复

- 同一 README 的 formula 表里，`claude-code` 的记录是「安装时**拉取官方 musl 二进制**，**自签名**后经 `ohos-compat-shim` 运行」；`qemu-aarch64` 是「用户态 QEMU：直接运行/调试 Linux aarch64 程序，自带系统调用跟踪（`-strace`），是**鸿蒙无 root strace** 环境下的排障替代品」。
- 同一 README 的「已下线 / 已迁移」表里有一条 **`deepseek-harness`**：「已由 Harmonybrew 官方 core 原生提供（**含 OHOS 补丁集：link 兜底 / 凭据模式 / ripgrep 回退 / crypto polyfill / 无沙箱放行**）」。

**这三条合起来给出一手事实**：**aarch64 的 Linux 用户态程序在鸿蒙 PC 上跑得起来**，前提是①二进制要过系统的强制代码签名（社区用自签工具解决）②syscall 差异要由用户态 shim 兜住（否则 `close_range` / `fchmodat2` / `linkat` 这类调用直接失败）③**`/tmp` 在应用沙箱里是只读的**。

**第三层（架构）：x86_64 要另走指令翻译。** [`BA4892/HarmonyBox`](https://github.com/BA4892/HarmonyBox)（第三方，README 自陈「This project relies heavily on AI assistance (Vibe Coding)」「The port is still in its early stages」）的标题就是 `Box64 for HarmonyOS NEXT — Run x86_64 Linux programs on HarmonyOS PCs`，README 的平台限制逐字：「Only supports HarmonyOS NEXT-powered **HarmonyOS PCs** (requires a kernel with a **39-bit address space** + LSE/ASIMDDP instruction set)」「Not yet tested on phones or tablets」「Due to limitations in the **HAP self-signing process**, this repository does not provide pre-compiled HAPs」。状态表里 musl/dynamic 与 glibc/dynamic 各 14/14 通过，musl/static 有 signal handler 不兼容，glibc/static 已知上游限制。

**推论**：HarmonyBox 用的是**用户态指令翻译**（box64 是 x86_64 → 宿主指令的模拟器），**不是内核级 Linux 兼容层**。它存在这件事本身说明：**鸿蒙 PC 上没有「WSL 式」的东西** —— 如果有，就不需要 box64 了。（这条是推论：我没有找到华为官方说「鸿蒙没有 WSL 类兼容层」的文档；我只是**没有找到任何**关于 Linux 兼容层 / 容器 / WSL 类子系统的官方一手材料。）

**一手未验证**：**「鸿蒙 PC 上能直接跑 aarch64 Linux 原生二进制、不需要任何翻译」这件事，我没有找到华为官方文档**。我拿到的证据全部来自社区 tap 与 HarmonyBox（都是第三方），它们证明的是「这条路被社区走通了」，不是「华为官方支持这么用」。

### 4.4 有没有类似 namespace / LSM / seccomp 的进程隔离机制可供第三方程序使用

**鸿蒙有等价的机制，但它们都是系统侧的、启动时定死的，第三方程序不能施加。**

**白皮书 5.3「隔离和访问控制」的逐字原文**（《HarmonyOS NEXT 安全技术白皮书》V1.0，2024-08-13，第 27–29 页）：

> **Access Token**：HarmonyOS 构建基于洋葱模型的分级安全机制… HarmonyOS 应用层的权限框架为 **Access Token**，HarmonyOS 将应用分为三个 **APL**（Ability Privilege Level）：normal，system basic 和 system core。**应用各自运行在独立的沙盒化环境中，默认仅允许访问自身的文件**，如需访问其他应用或者系统的信息，则需要通过权限来实现。

> **强制访问控制**：HarmonyOS 支持强制访问控制特性 **SE Harmony**，强制访问控制策略**在设备启动时加载到内核中，无法被动态更改**。该特性对所有进程访问目录、文件、设备节点等操作资源实施强制访问控制，对具有高权限权限的本地进程实施基于权能的强制访问控制，阻止恶意进程读、写受保护数据或者攻击其他进程，把被恶意篡改的进程对系统的影响限制在一个局部范围内，支撑上层应用实现各种安全防护。

> HarmonyOS 同时也支持**系统调用过滤**，基于**只读文件系统中的规则文件**，对进程能够调用的系统调用进行限制，避免恶意应用通过使用敏感的系统调用对系统造成危害。

> **可信执行环境**：华为自研的可信执行环境技术 **iTrustee** 基于 **TrustZone** 技术实现… 通过特殊指令 SMC 在 CPU 的 TEE 和 REE 之间切换来提供硬件隔离。

**应用沙箱的官方定义**（[OpenHarmony 应用沙箱目录](https://raw.githubusercontent.com/openharmony/docs/refs/heads/master/zh-cn/application-dev/file-management/app-sandbox-directory.md)，`master` 分支）逐字：

> 应用沙箱是一种以安全防护为目的的隔离机制，避免数据受到恶意路径穿越访问。在这种沙箱的保护机制下，**应用可见的目录范围即为「应用沙箱目录」**。
>
> - 对于每个应用，**系统**会在内部存储空间映射出一个专属的「应用沙箱目录」…
> - 应用沙箱限制了应用可见的数据范围。在「应用沙箱目录」中，**应用默认仅能看到自己的应用文件以及少量的系统文件**… **系统文件及其目录对于应用是只读的**
> - …所有应用的目录可见范围均经过**权限隔离与文件路径挂载隔离**，形成了**独立的路径视图，屏蔽了实际物理路径**
> - **开发者的 hdc shell 环境等效于系统进程视角**，因此「应用沙箱路径」与使用 hdc 工具调试时看到的真实物理路径不同

同页还给出沙箱路径 ↔ 物理路径的映射表（`/data/storage/el2/base` ↔ `/data/app/el2/<USERID>/base/<PACKAGENAME>`），以及 el1–el5 五档加密等级。

**把这三份材料放在一起，能读出的结论（前两条是事实，第三条是推论）**：

1. **鸿蒙确实有「文件路径挂载隔离」和「独立的路径视图」**（官方文档原话），即在机制层面存在与 mount namespace 同类的东西 —— 但它的使用者是**系统**，不是应用。
2. **鸿蒙确实有 MAC（SE Harmony）和系统调用过滤**，定位与 LSM + seccomp 对应；但白皮书明确写了策略**在启动时加载、无法动态更改**、规则文件在**只读文件系统**里。
3. **推论**：因此第三方程序**没有**「给自己或自己的子进程施加一个新边界」的接口。应用能拿到的是系统分配好的那个沙箱目录视图；它既不能重新定义一个「工作区可写、区外只读」的视图，也不能给自己装一条 seccomp 过滤器（即便它能直接发系统调用，那也只会被系统的规则文件拦掉，而不是给它自己设规则）。**这个推论我没有找到反例，但也没有找到华为一句「应用不能自定义沙箱」的原文**，所以标为推论。

**顺带一条会影响实现的实测事实**：Harmonybrew 的 “已知限制” 里写着「**沙箱内 `/tmp` 只读**」（见 4.3）。也就是说跑在鸿蒙应用沙箱里的 CLI，**默认连 `/tmp` 都不能写**，要由 shim 把临时文件改道到 `$TMPDIR`。

### 4.5 有没有 agent CLI / 开发工具跑在鸿蒙上的先例

**有，而且是「跑在鸿蒙 PC 本机上」的先例，不是「鸿蒙当遥控器」。** 但要区分这两类，搜索结果里两类都有。

**A 类：工具本机跑在鸿蒙上** —— [`social4hyq/homebrew-core`](https://github.com/social4hyq/homebrew-core)（第三方 tap，面向 HarmonyOS / OHOS aarch64）。README 里带版本号的一手表：

| Formula | 版本（README 记录） | 说明（README 原文要点） |
| --- | --- | --- |
| `opencode-v1` | 1.18.33 | 开源的终端 AI 编程助手；自带 75+ 模型提供商接入 |
| `social4hyq/core/opencode` | 2.0.20 | opencode v2 稳定版 |
| `claude-code` | 2.1.274 | 「Anthropic 官方 AI 编程助手 Claude Code 的终端版…**安装时拉取官方 musl 二进制，自签名后经 `ohos-compat-shim` 运行**」 |
| `claude-code.latest` | 2.1.285 | 同一 CLI 的滚动频道 |
| `zcode` | 3.14.3 | 「AI 编程工作台：终端 agent（TUI）与 Web IDE 双形态」 |
| `bun` | 1.4.2 | 「本 tap 多数工具的底座」 |
| `ohos-compat-shim` | 0.6.2 | 「系统兼容层：自动兜底鸿蒙与标准 Linux 的底层行为差异」 |
| `qemu-aarch64` | 11.0.3-r0 | 用户态 QEMU + `-strace` |

「已下线 / 已迁移」表里另有：**`codex`（2026-07-23 下线，已由 Harmonybrew 官方 core 原生提供）**、**`deepseek-harness`（2026-08-15 下线，已由官方 core 原生提供，含 OHOS 补丁集：「link 兜底 / 凭据模式 / ripgrep 回退 / crypto polyfill / **无沙箱放行**」）**、`uv`、`nvm`、`zellij`、`herdr`、`starship`、`libsecret`、`node` 等。

**这条对本仓库直接相关**：`deepseek-harness`（DSH 本体）**已经被移植到鸿蒙 PC**，而且它的 OHOS 补丁集里有一条明写「**无沙箱放行**」。**推论**：这是「在鸿蒙上这层沙箱无法成立，于是移植者选择放行」的一个现成样本；但这是第三方 tap 的补丁说明，**我没有读到那个补丁的实现与理由**。

**B 类：鸿蒙只当遥控器/审批端** —— [`liznee/serein`](https://github.com/liznee/serein)（第三方，README 自述状态为 V1.0 RC，PolyForm Noncommercial 许可）。它的架构逐字：

```
HarmonyOS App (ArkTS)
        | HTTPS / WSS
Go Backend + SQLite ---- ntfy / optional Push Kit
        | WSS
PC Relay (Node.js + node-pty)
        | PTY / JSONL
Claude Code CLI or Codex CLI
```

**事实**：在 Serein 里，**Claude Code / Codex 跑在 PC 上**（Node.js + `node-pty` 的 PC Relay），鸿蒙 App 只是「远程终端 + 风险审批」。**它不是「agent 跑在鸿蒙上」的先例**，不要混用。

**未读 / 一手未验证**：华为官方有没有任何「AI agent 在鸿蒙本机运行」的支持或限制文档，我没有找到。社区侧的 hqzing 系列文章（《鸿蒙 PC 底层开发技术详解》《鸿蒙 PC 上可用的 AI Agent 工具汇总》等，CSDN）我是从 Harmonybrew README 的致谢里看到的**标题与链接**，**没有读正文**。

### 4.6 结论：如果 `fs-agent` 要支持鸿蒙，卡在哪一步

按依赖顺序列，每一条都标明依据来自 4.1–4.5 的哪一段。**这里只清点卡点，不给出路。**

**卡点 0：先要能在设备上执行自己的二进制。**
- 需要过**强制代码签名**（否则 `Permission denied`）：依据是 Harmonybrew README（4.3，第三方转述，**华为官方一手未验证**）。
- 架构只能是 **aarch64**（鸿蒙 PC）；x86_64 要另走 box64 一类用户态翻译，而那条路自己还在 early stage：依据是 HarmonyBox README（4.3，第三方）。
- 需要设备侧给到可执行权限的通道（HiShell？hdc？）—— **HiShell 的官方文档我没有拿到**（4.2）。

**卡点 1（最硬的一条）：内核不是 Linux，bwrap 与 Landlock 依赖的东西不存在。**
- HM 是微内核，core kernel 只留线程调度、串/定时器驱动、访问控制（OSDI'24 论文，4.1）。
- Linux syscall 由 **shim 重定向到 IPC**（同上）。
- **推论**：mount/user namespace 与 Landlock LSM 是 Linux 内核特性，在这个架构里没有对应物；论文把访问控制描述为 **capability + address token**，不是 LSM/namespace。论文全文**没有**把 namespace / LSM / seccomp 列为可供用户态使用的机制（我检索过 `namespace` 等词）。**这条是推论而非论文原话。**

**卡点 2：鸿蒙提供的等价机制，第三方程序不能施加。**
- SE Harmony 的策略「在**设备启动时**加载到内核中，**无法被动态更改**」；系统调用过滤「基于**只读文件系统**中的规则文件」：白皮书 5.3（4.4）。
- 应用沙箱的路径视图是**系统**映射的、「经过权限隔离与**文件路径挂载隔离**」、`hdc shell` 看到的是「系统进程视角」：OpenHarmony 官方文档（4.4）。
- **推论**：第三方程序没有「给自己的子进程重新定义一个文件系统视图」的接口。

**卡点 3：应用沙箱本身对 fs-agent 的形状是反的。**
- 应用默认只能看到自己的目录 + 少量系统文件，**系统文件只读**（4.4）；
- **`/tmp` 在沙箱里只读**，要 shim 改道 `$TMPDIR`（Harmonybrew README，4.3）；
- **硬链接未向三方应用开放（`linkat` 返回 EPERM）**（同上）；
- 而 fs-agent 已拍定的形状是「整机只读 + 工作区可写 + `/tmp` 可写」——**沙箱里连 `/tmp` 可写这一条都不成立**。

**卡点 4：即使跑起来，syscall 层还要一层 shim。**
- `close_range` / `fchmodat2` 缺失、`splice` 的 EOF 与 poll 语义不同、`getpwuid_r` 要另找兜底（Harmonybrew README，4.3）。fs-agent 的 `bash` 工具要跑的是**任意命令**，shim 的覆盖面直接决定哪些命令会在这里莫名其妙地失败。

**卡点 5：测试/使用通道不是产品化的。**
- `hdc shell` 是开发者调试通道，需要在设备上开开发者选项、在电脑上装 SDK；PC/2in1 上还可能被组织策略整体禁用（错误码 `E00C001`）（4.2）。
- **一手未验证**：设备本机有没有一个「给第三方程序用、不是调试通道」的 shell 环境，我没有拿到华为官方材料。

**一句话版本**：**不是「差一个后端」，而是第一层就没有可用的机制** —— Linux 的进程级文件沙箱（bwrap / Landlock）在 HM 上不存在；HM 提供的等价机制（SE Harmony、系统调用过滤）是**系统启动时定死、第三方程序无法施加**的；而应用沙箱给到程序的那个视图，恰恰是「自己的目录可读写、其余受限」，与「整机只读 + 工作区可写」这个形状不同构。在这之上还要先解决**自签名**与**syscall shim** 两个额外的工程层。

**已存在的反例（要一起记）**：`deepseek-harness` 已经被移植到鸿蒙 PC，补丁集里有「**无沙箱放行**」一条（4.5）。这说明**「在鸿蒙上跑 DSH」已经发生过，代价是这层沙箱不成立**。

---

## ⑤ 「留出位置」的工程先例：谁抽象了、抽象成什么、代价是什么

### 5.1 DSH 的 rung 链（回 [`01`](01-dsh-workspace-permissions-and-shell.md) 核对，不重做）

[`01`](01-dsh-workspace-permissions-and-shell.md) §④ 已核对过：平台链是 `linux: ["bwrap","landlock"]`、`darwin: ["seatbelt"]`、`win32: ["windows-acl"]`，**只有多候选时才做功能探测**，单候选直接选（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:163-176,482-499`）；探测结果缓存在 provider 生命周期内，改换 runner 要重载插件；每个 runner 自带两类方言（`denialSignatures` 与 `runnerFailureRules`）；`enforcement` 是**上报的事实**（`full` / `partial`），不是承诺。

**形状小结（不重做调研，只归纳）**：**数组形式的候选链 + 惰性探测 + 能力上报**。它不是 trait，也不是枚举 —— 它把「同一平台可能有多个后端」当成一等公民（`linux` 有两个），而 `darwin` / `win32` 各只有一个。

**这条链现在补上了一手的设计依据。** DSH 仓库里有一份标注 `Status: implemented` 的笔记：[`Agent Note: Windows sandbox rung: raw ACL restricted tokens over mxc and AppContainer`](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/.agents/notes/implemented/feature/2026-08-08-windows-acl-restricted-token-sandbox.md)（`deepseek-ai/deepseek-harness` 的 `master` 分支，2026-08-08，**未钉 commit**）。这份笔记直接回答了「为什么 win32 rung 是现在这个样子」：

> Implement the rung directly on the raw ACL mechanism: duplicate the caller's token into a `WRITE_RESTRICTED` token (`CreateRestrictedToken` with `WRITE_RESTRICTED` + `DISABLE_MAX_PRIVILEGE` + `LUA_TOKEN`) whose restricting SIDs carry distinct workspace and private-temp capabilities. `WRITE_RESTRICTED` intersects write accesses only, so reads keep the caller's ambient access while a write must also match one of these capability ACEs.

同文里「Alternatives considered」一节逐字给出了三个候选被否的理由：

> **Why not mxc (Microsoft xContainer)?** Two disqualifiers. First, the OS floor is too new: the mxc OS-version policy sets the product floor at **Windows 11 24H2 (build 26100)**, and the BaseContainer tier (T1, `Experimental_CreateProcessInSandbox`) exists only on **25H2+ (build 26600+)** with the OS feature enabled — on every supported release at or below 25H2 the filesystem policy **falls back to T3, AppContainer plus host-side DACL ACE augmentation**. Second, supporting arbitrary-path reads under either tier means **granting read access by writing ACLs over every path the child may read**: a model that reads the whole workspace and arbitrary files would require **wholesale host DACL mutation** — a standing side effect and a cost a write-only restriction does not need.
>
> **Why not AppContainer?** An AppContainer token **carries no ambient read access**: every readable path must be pre-granted through capabilities or explicit ACEs, so arbitrary-path reads — the harness's read model — are unsupported without the same wholesale grants. The restricted token needs no read grants at all: it intersects write access only.
>
> **Why not landstrip?** The landstrip evaluation was rejected before implementation (not battle-tested; the in-house launcher plan won), and its **Windows backend is AppContainer-shaped, inheriting the same arbitrary-read problem**.

**代价那一段同样逐字可引**（同文 “Consequences”）：

> Cost: enforcement is **structurally partial** because NTFS **hard-link aliases cannot be path-confined by this token shape** and because a Low-integrity token cannot open a file whose DACL carries another sandbox's AppContainer package SID ...; **no read-side or network isolation; console isolation unavailable** (children created with `CREATE_NO_WINDOW` or `CREATE_NEW_CONSOLE` die with `STATUS_DLL_INIT_FAILED` ...); standing workspace security-descriptor mutations (the reuse cache, plus inert residue when a workspace is renamed) and random temp litter after an unclean shutdown ...; **EAGER full-tree workspace propagation (`SetNamedSecurityInfoW` walks every descendant immediately — tens of seconds on large workspaces), paid once per workspace per machine**; CIM unavailable in both confined modes ...; named-pipe opens remaining denied, so libuv piped-stdio grandchildren fail with EPERM ...

还有两条会在移植时咬人的细节：「`Logon SID + Everyone` are keep-alive invariants (**early DLL init dies with `0xC0000142` and CNG crashes pwsh with `0xE0434352` without them**)」；「**Rejecting all hard links would reject ordinary pnpm workspaces**, so the provider reports `enforcement: 'partial'` and the native suite pins both gaps」。

**事实**：DSH 的 Windows rung **不是**抽象出来的，它是**直接写在机制上**的；而它之所以这么写，是因为两个「更抽象」的候选（MXC 的统一 schema、AppContainer 的容器身份）在**「读整机」这个需求**上都要付「宿主 DACL 大改」的代价。

### 5.2 Codex 的 `SandboxType` 枚举与 feature 开关（引 [`03`](03-agent-sandbox-precedents.md)，补本 commit 的行号）

[`03`](03-agent-sandbox-precedents.md) ② 已记：`SandboxType::{None, MacosSeatbelt, LinuxSeccomp, WindowsRestrictedToken, WindowsMxc}`，`WindowsMxc` 在被显式选中后是**粘性**的，`Disabled` 会归零成 `None`；`--use-legacy-landlock` 是 opt-in，且 README 明写 legacy Landlock「cannot isolate app-server Unix sockets」而被拒绝用于这些策略。

**本 commit（`67727e7`）核到的形状**：`codex-rs/protocol/src/sandbox.rs` 只有 42 行，枚举在 `:8-16`，粘性逻辑是 `effective_windows_sandbox_type()`（`:30-42`），注释 `/// Preserves explicit MXC selection while honoring legacy runtime level updates.`。**它既不是编译期 `cfg` 也不是纯运行时探测：枚举是运行时值，选择规则是一条小函数。**

**「保留旧后端」的代价在代码里能直接看到**：Linux 那一档的枚举名仍叫 `LinuxSeccomp`（今天文件系统隔离者是 bubblewrap）、landlock 的实现留在 `codex-rs/linux-sandbox/src/landlock.rs:138-170` 且带注释「currently unused because filesystem sandboxing is performed via bubblewrap. **It is kept for reference and potential fallback use.**」（转引 [`03`](03-agent-sandbox-precedents.md) ②）。

### 5.3 `birdcage`：真的做了「一个 trait + 两个后端」，然后停了

[`02`](02-linux-sandbox-primitives-and-tools.md) §⑦ 已经查过 crate 层面的数据（0.8.1，`updated_at` 2024-04-19，仓库从 `phoenixkahlo/birdcage` 转到 `phylum-dev/birdcage`）。本文件补充**README 原文**与 crates.io 复核：

- 定位逐字：「Birdcage is a **cross-platform embeddable sandboxing library** allowing restrictions to Filesystem and Network operations using native operating system APIs.」
- **Supported Platforms 只有两行**逐字：「Linux via **namespaces**」「macOS via **`sandbox_init()`** (aka Seatbelt)」。**注意 macOS 那一条用的是 `sandbox_init()`** —— 也就是 Apple 自己在 man page 里标了 DEPRECATED 的那个函数（见 2.1）。
- 自陈局限逐字：「Birdcage focuses **only** on Filesystem and Network operations. It **is not** a complete sandbox preventing all side-effects or permanent damage. **Applications can still execute most system calls**, which is especially dangerous when execution is performed as root. Birdcage should be combined with other security mechanisms, especially if you are executing known-malicious code.」
- README 的示例显示**默认全拒、要显式给例外**：不带 exception 跑 `echo` 会得到 `Error: Os { code: 13, kind: PermissionDenied, message: "Permission denied" }`，要写成 `-e /usr/bin/echo -e /usr/lib` 才跑得起来。
- crates.io API 复核（2026-10-01）：`max_version 0.8.1`、`updated_at 2024-04-19T20:51:11Z`、`downloads 144650`。

**事实**：`birdcage` 是「一个 trait（`Sandbox`）后面挂两个平台后端」这条路的**真实先例**，**最后一次发布在 2024-04，到今天（2026-10-01）已经约 2.5 年没有新版本**。

**一手未验证（重要）**：**我没有找到任何 `birdcage` 的停更说明** —— README 里没有 archive/停更声明，GitHub 仓库页我也没有读到 archived 标记（GitHub API 当时已被限流）。所以「它为什么停了」**我答不了**；能说的只有「它停了」这个可观测事实，以及「它的 macOS 后端建立在 Apple 已标 DEPRECATED 的 `sandbox_init()` 上」这个并存的事实。**两者之间是否有因果关系，我没有证据。**

### 5.4 `extrasafe`：只做 Linux，且自己写明要叠别的机制

[`02`](02-linux-sandbox-primitives-and-tools.md) §⑦ 已引 README 逐字：「extrasafe is an easy-to-use wrapper around various **Linux** security tools, including seccomp filters ... the Landlock Linux Security Module ... and user namespaces for broader isolation.」以及「you should continue to use Linux Security Modules like AppArmor and SELinux!」。

crates.io API 复核（2026-10-01）：`max_version 0.5.1`、`updated_at 2024-04-16T22:08:36Z`、`downloads 36127`。**它不宣称跨平台，所以不构成「跨平台抽象」的先例，但它构成「单平台包装器」的对照**：同样是 2024-04 停更的节奏。

### 5.5 `cross-sandbox`：2026 年的新尝试，形状是 trait + 枚举 + 统一配置

在 crates.io 上检索到一个 2026 年的新项目，是**本文件唯一读到的、明确宣称「统一跨平台沙箱」的 Rust crate**：

| crate | `max_version` | `updated_at` | `downloads` | 描述（crates.io 原文） |
| --- | --- | --- | --- | --- |
| `cross-sandbox` | 0.2.0 | 2026-06-17T10:19:11Z | **67** | `Unified cross-platform sandbox library — single API for Windows/Linux/macOS` |
| `cross-sandbox-core` | 0.1.1 | 2026-06-17T10:10:09Z | **221** | `Cross-platform sandbox core types (ACL rules, config, errors)` |

（crates.io API，2026-10-01 抓取；作者 `WeiChens`，仓库 `github.com/wei/cross-sandbox`，MIT。）

`cross-sandbox-core` 的 docs.rs 页面（0.1.1）给出了**抽象的精确形状**：

- 模块：`acl`（「ACL 规则模型 — 文件权限 + 网络权限」）、`config`（「沙箱配置模型 — **跨平台统一配置**」）、`error`、`platform`（「平台标记与检测」）、`sandbox_trait`（「**`PlatformSandbox` trait — 跨平台沙箱抽象接口**」）、`stream`（「流式沙箱执行接收器」）。
- 重新导出的类型：`FilePermission`、`FileRule`、`NetAction`、`NetProtocol`、`NetRule`、`SandboxConfig`、`SandboxError`、`SandboxResult`、`StreamEvent`、**`Platform`（枚举）**、**`PlatformSandbox`（trait）**。
- 自述逐字：「提供所有平台通用的 ACL 规则、配置模型、错误类型、**沙箱 trait** 等核心类型。**平台特定的功能通过条件编译（`#[cfg]`）隔离。**」
- `docs.rs` 显示文档覆盖率 `87.96%`；`docs.rs` 页面上的构建日期写作 `09 September 2026`，而 **crates.io API 的 `updated_at` 是 `2026-06-17`**（引用时以 crates.io 为准，两者不是一个概念）。

**事实**：它的抽象形状是 **`PlatformSandbox` trait + `Platform` 枚举 + 统一的 `SandboxConfig` / ACL 规则模型 + `#[cfg]` 隔离平台实现**，即任务问的「一个 trait 后面挂多个后端」。**下载量 67 / 221**，是这个形状在 crates.io 上的实际采纳度。

**未读**：`cross-sandbox` 的 README 我在 `main` 与 `master` 两个分支上都拿到 404，**没有读到它的自述、支持矩阵、限制清单**。所以**这个项目「做得怎么样、限在哪里」我答不了**，只报了形状与采纳度两个可核事实。

### 5.6 MXC：微软官方的「跨平台统一抽象」

`microsoft/mxc` 就是这条路的**官方版本**：一个 **unified JSON configuration schema** 后面挂 9 个后端（Windows ProcessContainer / Windows Sandbox / LXC / Bubblewrap / Seatbelt / MicroVM / Hyperlight / IsolationSession / WSLC），跨 Windows / Linux / macOS（见 3.3）。

它给自己的定性（README 顶部警告）是 **early preview**，并且「**no MXC profiles should be treated as security boundaries currently**」「known cases where the current policies generated by the MXC SDK in this repository are **overly permissive**」。

**这条对「抽象代价」的意义**：MXC 把「统一配置 → 各平台机制」这条链做成了产品，而它的**统一 schema 表面**上有一处具体的表达力缺口 —— README Features 里写着 **「denied paths not yet supported on Windows」**，而同一个 schema 在 Linux/macOS 上是支持的。这是「抽象只能取交集、缺口要写在文档里」的一手样本。

### 5.7 代价的可引用清单

把 5.1–5.6 里能直接引用的代价条目汇总（**每条都有出处，不是我的归纳**）：

| 代价形态 | 具体证据 | 出处 |
| --- | --- | --- |
| 抽象后的 API 只能取交集，缺口要单列 | MXC 的 filesystem policy 在 schema 层支持 read-only / read-write path lists，但 **“denied paths not yet supported on Windows”** | [MXC README](https://github.com/microsoft/mxc)（Features） |
| 统一抽象做出来了也不能当安全边界用 | 「no MXC profiles should be treated as security boundaries currently」 | 同上（顶部 WARNING） |
| 后端行为差异必须泄漏给调用方 | DSH 的 `enforcement: 'full' \| 'partial'` 是**上报的事实**；Windows rung 自报 `partial` | [`01`](01-dsh-workspace-permissions-and-shell.md) §④；[DSH Windows 笔记](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/.agents/notes/implemented/feature/2026-08-08-windows-acl-restricted-token-sandbox.md) |
| 保留旧后端要付维护与文档成本 | Codex 的 `LinuxSeccomp` 枚举名与 bwrap 实现并存；landlock 代码带注释「It is kept for reference and potential fallback use.」；`--use-legacy-landlock` 是 opt-in 且被 README 明确不建议 | [`03`](03-agent-sandbox-precedents.md) ② |
| 抽象的**内核**差异会变成上层工程 | 同一个「文件沙箱」需求在 Linux 是挂载表、在 macOS 是路径谓词、在 Windows 是 token+ACL、在鸿蒙是系统策略 | ②③④ 各节 |
| 「统一」不等于「等价」，具体机制要单独写 | DSH 的 Windows rung 不是抽象，而是逐条写 raw ACL + Low integrity + deny `FILE_DELETE_CHILD` | [DSH Windows 笔记](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/.agents/notes/implemented/feature/2026-08-08-windows-acl-restricted-token-sandbox.md) |
| 撤掉整个平台支持的先例 | Goose 的 macOS Seatbelt 沙箱从 v1.25.0 的实验特性到被移除 | [`03`](03-agent-sandbox-precedents.md) ④、⑦ 第 13 条 |

### 5.8 有没有人反对「过早抽象多平台沙箱」

**结论：我找不到一篇明确反对的公开文章。** 我搜了 `premature abstraction sandbox`、`platform abstraction leaky`、`sandbox cross-platform abstraction`、`"do not abstract"` 等方向，命中的是通用软件工程文章（例如 [The Cost of Abstraction for Humans and AI Agents](https://ondrejvelisek.github.io/the-cost-of-abstraction-for-humans-and-ai-agents/)、CMU 的作业材料 [The Leaky Sandbox](https://15316-cmu.github.io/2023/homework/03-sfi/03-sfi.pdf)），**它们不专门讨论「多平台进程沙箱」这个话题**，我不把它们包装成「业界反对意见」。

**能拿到的最接近「反对」的一手材料，性质各不相同，要分清**：

1. **项目自陈的武器级警告**：MXC README 的「no MXC profiles should be treated as security boundaries currently」—— 这不是反对抽象，而是**做抽象的人自己说这个东西现在不能信**。
2. **不做抽象、直接写机制的实例**：DSH 的 Windows rung 逐条否掉 mxc / AppContainer / landstrip（5.1）—— 这是**取舍的证据**，不是「反对抽象」的论述。
3. **跨平台库自己写下的边界**：`birdcage` 的「It is not a complete sandbox」「Applications can still execute most system calls」（5.3）—— 这是**局限声明**。
4. **撤掉支持的实例**：Goose 移除 macOS seatbelt 沙箱（[`03`](03-agent-sandbox-precedents.md) ⑦ 第 13 条）—— **官方没有给撤销理由**，`03` 已把「维护成本是原因之一」标为推论。
5. **本仓库自己的立场**：README 写明 v1 不做进程级隔离、升级路径是 Linux 的 bubblewrap，**「不预做抽象」**（转引 [`01`](01-dsh-workspace-permissions-and-shell.md) §⑦）。

**我读到的范围内，没有一篇「署名作者 + 明确论证 + 专门针对多平台沙箱抽象」的反对文章。** 这一条写进 ⑦ 的边界。

---

## ⑥ 对照表

平台 × 可用机制 × 表达力 × 需要什么权限 × 成熟度。**「表达力」一栏问的是：这个机制能不能表达「工作区可写 + 整机可读 + 其余不可写」**（fs-agent 已拍定的形状）。**事实**列；带 **推论** 的单元格另行标注。

| 平台 | 可用机制 | 能否表达「工作区可写、区外只读」 | 需要什么权限 / 前置 | 成熟度（2026-10-01） |
| --- | --- | --- | --- | --- |
| **Linux**（已拍定） | `bwrap`：`--ro-bind / /` + `--bind <cwd> <cwd>` + `--tmpfs /tmp` | **能**，而且是「造一个视图」的直接形状（[`04`](04-local-probe-bwrap-and-landlock.md) 有本机实测） | 无特权，但要能建 user namespace；Ubuntu 24.04+ 需 AppArmor profile 或 sysctl | bubblewrap 0.13.0（本机）；四家 agent 产品主力机制（[`03`](03-agent-sandbox-precedents.md)） |
| **Linux**（备选） | Landlock LSM：`/` 读+执行、工作区全权限 | **能**，allow-list 语义（[`04`](04-local-probe-bwrap-and-landlock.md) 实测 ABI 10） | 无特权（`PR_SET_NO_NEW_PRIVS`）；内核需 `CONFIG_SECURITY_LANDLOCK` 且在 `lsm=` 列表里 | 本机内核 7.2.6 报 ABI 10；`landlock` crate 0.4.7（2026-07-27） |
| **macOS** | `/usr/bin/sandbox-exec -p <SBPL>`；`(deny default)` + `(allow file-read*)` + `(allow file-write* (subpath <root>) (require-not ...))` | **能**（Codex 的实际写法，`seatbelt.rs:490-572`、`:988`）；而且多出 mach port / sysctl / fcntl / Apple Events 这些维度 | 无特权；**但依赖 Apple 自己标了 DEPRECATED 的私有接口**（`sandbox-exec(1)` / `sandbox_init(3)` man page） | 命令今天仍存在、四家产品全在用；**没有公开的移除时间表** |
| **macOS** | App Sandbox（`com.apple.security.app-sandbox` entitlement） | **对未签名/自用的 CLI 不适用**（**推论**，依据是 man page 措辞与 entitlement 需签名；**一手未验证**） | 签名 + entitlement（App Store 路线） | Apple 官方推荐（man page 把开发者指向它） |
| **Windows** | 受限令牌（`CreateRestrictedToken` + `WRITE_RESTRICTED` / deny-only SID / restricting SID） | **不能单独表达**：它只做减法；「这个目录可写」必须由**宿主侧 DACL** 上的 ACE 提供（Microsoft Learn 原文 + DSH 笔记） | 普通用户即可（受限版自身 token 不需要 `SE_ASSIGNPRIMARYTOKEN_NAME`）；但要**改宿主 ACL**、还要处理桌面消息与继承 | DSH 的 `win32` rung 在用，**自报 `enforcement: partial`**；Codex 也有该后端（`codex-windows-sandbox` crate） |
| **Windows** | AppContainer（capability SID / LPAC） | **不能**（对「读整机」这个需求）：官方文档写 read-write 要逐个授权、**读也要授权** | 给每个可读路径开 ACE；或用 MXC/工具的容器身份 | Microsoft Learn 有官方页（2025-07-08 更新）；DSH 笔记把它作为「要求 wholesale DACL mutation」而否掉 |
| **Windows** | Microsoft MXC（`processcontainer` / PSEC；更旧版本掉到 T3 = AppContainer + DACL） | **不能按需表达**：MXC 的 schema 有 read-only / read-write path lists，但**「denied paths not yet supported on Windows」**；且 T3 需要宿主 DACL | Windows 11 **24H2（build 26100）**起；BaseContainer tier（T1）要 **25H2+（build 26600+）** 且开启 OS 功能；PSEC 来自内部 Windows 源码树 | **early preview**；README 明写「**no MXC profiles should be treated as security boundaries currently**」；Codex 有 `WindowsMxc` 后端（strict，失败不回退） |
| **Windows** | Low integrity（Gemini 用 `icacls` 打 Low Mandatory Level） | 只能表达「不许向上写」，不能表达「这个目录可写」 | 改宿主文件的完整性标签 | [`03`](03-agent-sandbox-precedents.md) ④：`icacls` 的改动**在文件系统上持久**，要手工复位 |
| **鸿蒙** | 内核侧：HM 的 Linux ABI shim（用户态看起来是 Linux） | **不能表达**（**推论**）：shim 转发的是 Linux **syscall**，而 namespace / LSM 是内核特性；HM 的访问控制是 capability + address token，不是 LSM/namespace | 无（且这是内核内部机制，不是给用户态的接口） | HM 已在「数千万台设备」上量产（OSDI'24 论文自述）；**闭源** |
| **鸿蒙** | SE Harmony（强制访问控制）+ 系统调用过滤 | **第三方不能施加**：白皮书原文「策略**在设备启动时加载到内核中，无法被动态更改**」「基于**只读文件系统中的规则文件**」 | 系统/厂商侧；策略烧在镜像里 | 白皮书 V1.0（2024-08-13）为官方描述；OpenHarmony 侧另有 SELinux（**本文件未取证**） |
| **鸿蒙** | 应用沙箱（「应用沙箱目录」+ 路径挂载隔离） | **形状是反的**：应用只能看到自己的目录 + 少量系统文件，**系统文件只读**；沙箱内 **`/tmp` 只读**（Harmonybrew 记录）；**硬链接未开放（`linkat` EPERM）** | 系统分配；应用不能自定义视图（**推论**） | OpenHarmony 官方文档有完整定义；Harmonybrew 有真机记录 |
| **鸿蒙 PC**（执行通道） | HiShell（自带终端）+ 二进制自签名 + `ohos-compat-shim` | 与上两行同（沙箱语义由系统定）；**这里说的是「能不能跑起来」** | **强制代码签名**（否则 `Permission denied`）；架构必须 aarch64 | 第三方 tap 真机验证过 `opencode` / `claude-code` / `bun` / `deepseek-harness`；**华为官方对 HiShell 的一手文档我未拿到** |
| **鸿蒙 PC**（执行通道之二） | `hdc shell`（电脑端 → 设备端 toybox 命令） | 同上 | 设备侧开开发者选项 + 电脑侧装 SDK；PC/2in1 上可能被组织策略整体禁用（`E00C001`） | OpenHarmony 官方文档；定位是**调试通道** |
| **鸿蒙 PC**（x86_64） | box64 移植（HarmonyBox） | 同上 | 需 39-bit 地址空间 + LSE/ASIMDDP；HAP 自签名流程限制；**README 自陈 early stage** | 第三方，musl/glibc dynamic 各 14/14 通过；static 有已知问题 |
| **跨平台抽象** | DSH rung 链（数组候选 + 惰性探测 + `enforcement` 上报） | 逐平台写机制，不抽象 | — | 已实现；`darwin` / `win32` 各单候选 |
| **跨平台抽象** | Codex `SandboxType` 枚举 + 粘性选择 + feature 开关 | 逐平台写机制，枚举是选择结果 | — | 5 个变体；Windows 两个后端并存 |
| **跨平台抽象** | `birdcage`：`Sandbox` trait + `LinuxSandbox` / `MacSandbox` 类型别名 | 默认全拒 + 显式 exception | — | **0.8.1（2024-04-19）后停更约 2.5 年**；macOS 后端基于 `sandbox_init()` |
| **跨平台抽象** | `cross-sandbox`：`PlatformSandbox` trait + `Platform` 枚举 + 统一 `SandboxConfig` | 统一 ACL 规则模型 | — | **0.2.0（2026-06-17），下载 67 次**；README 未读到 |
| **跨平台抽象** | Microsoft MXC：统一 JSON schema + 9 个后端 | 取交集，缺口写在文档里 | 见上 | early preview，官方明说现在不能当安全边界 |

---

## ⑦ 边界：这份文件回答不了什么

按主题列，**每条都写明「为什么答不了」**。

**macOS**

- **SBPL 的规则求值顺序（deny/allow 冲突、last-match-wins、`require-not` 的语义）没有 Apple 一手规范。** `sandbox(7)` 与 `sandbox-exec(1)` 的 man page 都不讲规则代数；我能给的只有 Codex 的使用方式与代码里那句 `// Allegedly, ...`。标「一手未验证」。
- **`sandbox-exec` / `sandbox_init` 的废弃声明来自两个第三方 man page 镜像**（`keith.github.io/xcode-man-pages`、`manpagez.com`），**不是 `apple.com` 域**。我**没有**找到一份 Apple 官方的独立公告页。另外 man page 只说「建议改用 App Sandbox」，**没有移除时间表**。
- **App Sandbox / Endpoint Security / 其它 entitlement 对「未签名自用 CLI」是否可用**，我没有读 Apple 一手文档，**没有结论**。
- **没有实测**：本机不是 macOS，**一条 Seatbelt 命令都没有跑过**。

**Windows**

- **受限令牌那两条细节我没有读一手实现**：`WRITE_RESTRICTED` 与 `DISABLE_MAX_PRIVILEGE` 的逐字定义（在 `CreateRestrictedToken` 的 `Flags` 参数页里），以及 Low integrity（`icacls` / mandatory label）的官方机制页，我**都没有读**；这一节里关于它们的说法来自 [Restricted Tokens 概念页](https://learn.microsoft.com/en-us/windows/win32/secauthz/restricted-tokens)与 DSH 的笔记。
- **MXC 的 OS-version policy 页我没有读**（`docs/process-container/os-version-support.md`）—— 25H2 上哪些策略项能强制、哪些掉到 T3，我是**转引 DSH 笔记**。
- **Codex 的 Windows 后端自述缺失**：`codex-rs/windows-sandbox-rs/README.md` 在 `67727e7` 上是 404；我只读到 `Cargo.toml` 的三个二进制名与 `src/lib.rs` 的开头。
- **`codex-rs/windows-sandbox-rs/src/` 的文件清单没拿到**（GitHub API 被限流，`curl` 也因为管道提前关闭而失败）。
- **Claude Code 与 Devin 不给 Windows 支持的理由**，官方文档里确实没有；**「因为 Windows 机制不够」是读者推断**，我在 3.4 已明确标出。
- **没有实测**：本机不是 Windows，**一条 Windows 沙箱命令都没跑过**。

**鸿蒙**

- **HarmonyOS NEXT 的内核形态，一手材料只有 OSDI'24 那篇论文 + 华为官网的 EAL6+ 新闻稿。** 华为官方白皮书全文**没有**「微内核」「Linux」「鸿蒙内核」「namespace」这些词（我逐词检索过 82 页文本）。所以「HM 是微内核」这句我是靠**论文**立的，不是靠产品文档。
- **「bwrap / Landlock 在 HM 上不存在」是推论，不是论文原话。** 论文没有说「不支持 namespace」；我的依据是它对架构的描述（core kernel 只留三件事）+ 全文不把 namespace/LSM/seccomp 列为用户态可用机制。
- **HarmonyOS PC 版用的内核是不是同一个 HM，我没有一手证据。** 4.1 的材料讲的是 HM 在「smart routers / smart vehicles / smartphones」上的量产；**PC 版我一无所获**。我搜到的相关页面（华为开发者论坛的「纯血鸿蒙 PC？？Linux!!」等）都是**JS 渲染的社区帖**，`web_fetch` 拿不到正文。
- **HiShell 没有华为官方一手文档落入我手。** `developer.huawei.com` 的文档页是 JS 渲染的，抓到的只有「文档中心」；华为开发者论坛的帖子是社区内容，我不用。
- **「能不能跑 aarch64 Linux 原生二进制」的官方口径缺失。** 我拿到的是社区 tap 与 HarmonyBox 的**第三方**证据（它们证明「路被走通了」）；**华为官方对「第三方 Linux 二进制在鸿蒙上的支持程度」我没有找到任何一句**。
- **OpenHarmony 的 SELinux 我没有取证。** 官方白皮书讲的是 SE Harmony；OpenHarmony 侧据说有 SELinux 支持，我只在搜索结果里见到二手博客，**没有读一手文档**，所以对照表里那一格标了「本文件未取证」。
- **应用沙箱的「文件路径挂载隔离」具体是什么机制**（mount namespace？还是别的实现）我**没有读到**——官方文档只说「经过权限隔离与文件路径挂载隔离，形成了独立的路径视图」。所以「它等于 mount namespace」这种话我**没有说**。
- **`hdc shell` 的设备端权限模型**（普通开发者能看多少、写多少）我只读到「等效于系统进程视角」和错误码 `E00C001`，**没有完整材料**。
- **没有实测**：本机不是鸿蒙，**设备上一行命令都没跑过**。

**「留出位置」与跨平台抽象**

- **没有找到专门反对「过早抽象多平台沙箱」的公开文章。** 我找到的是通用软件工程材料与本文件 5.8 列的五类一手自陈材料，**性质不同**，我不把它们当「业界反对意见」。
- **`birdcage` 的停更原因没有一手说明。** README 无 archive 声明；GitHub API 当时被限流，**仓库是否 archived 我没有核到**。
- **`cross-sandbox` 的自述没读到**（`main` / `master` 的 README 都 404）。我只报了 crates.io 的版本/下载量与 docs.rs 的类型形状，**「它做得怎么样、限在哪里」答不了**。
- **`gaol` / `minijail` 等其它候选没有穷举**：`02` §⑦ 已查过一批（`landlock`、`seccompiler`、`libseccomp`、`nix`、`caps`、`syscallz`、`process_control`、`gaol`），本文件只补了 `birdcage` / `extrasafe` / `cross-sandbox` 三个，**没有穷举 crates.io**。
- **`landstrip`**（DSH 笔记里提到的自研候选）我**没有读它的仓库**，只转引了 DSH 对它的评价。

**通用**

- **本文件没有做任何设计建议，也没有替本仓库拍决定。** 「留位置留成什么形状」不是这份文件回答的问题。
- **版本会漂。** HarmonyOS NEXT、MXC、Claude Code 文档、Codex 都在快速迭代；本文件钉的是各节开头写的那些版本/commit/日期（观察时点 **2026-10-01**）。引用时请连同版本一起引用。
- **DSH 的部分不在本文件。** 任何「DSH 怎么做的」问题去看 [`01`](01-dsh-workspace-permissions-and-shell.md)；Linux 机制看 [`02`](02-linux-sandbox-primitives-and-tools.md)；产品先例看 [`03`](03-agent-sandbox-precedents.md)；本机实测看 [`04`](04-local-probe-bwrap-and-landlock.md)。
