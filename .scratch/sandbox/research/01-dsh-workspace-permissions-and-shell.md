# research：DSH 的「工作区内允许 / 区外要审批」与 shell 的处理

## 这份文件是什么

我们正在设计 `workspace` 模式（会话 cwd 内一律允许、cwd 之外需审批，对所有角色生效），难点是 `bash`：它无法静态判定会不会写到工作区外（本仓库 `src/tools/bash.rs:75-79` 的 `effect()` 恒为 `Exclusive`，注释写着「shell 能写任何东西」）。这份文件回答一个问题：**DSH（DeepSeek 自用 agent CLI，`@deepseek-ai/dsh`）是怎么处理「工作区内允许 / 区外要审批」的，尤其 shell 那一条。**

读者是接下来要拍 `workspace` 模式设计的人（先读结论①，再按需读③④）。**这不是设计建议**：① 到 ⑥ 是事实清点，⑦ 只写「代码里能读到的事实能支撑的推论」，⑧ 写清楚哪些问题这些产物回答不了。

## 来源、版本与复现

DSH 曾装在这台机器上，但已不在 PATH 里、`/usr/lib/node_modules` 下也没有；下面全部结论来自 **npm 上的公开发布物**，不是运行时观测。取包命令（**注意必须钉版本**：这些包的 `latest` dist-tag 大多停在 `0.0.1-rc.1`，比 `0.1.5-rc.3` 旧很多；不写版本号的 `npm pack @deepseek-ai/dsh-sandbox` 会拿到旧产物）：

```sh
mkdir -p /tmp/dsh-research && cd /tmp/dsh-research
V=0.1.5-rc.3
npm pack \
  @deepseek-ai/dsh@$V @deepseek-ai/dsh-base@$V @deepseek-ai/dsh-session@$V @deepseek-ai/dsh-agent@$V \
  @deepseek-ai/dsh-sandbox@$V @deepseek-ai/dsh-sandbox-local@$V @deepseek-ai/dsh-sandbox-policy@$V \
  @deepseek-ai/dsh-bash-sandbox@$V @deepseek-ai/dsh-pwsh-sandbox@$V @deepseek-ai/dsh-fs-sandbox@$V \
  @deepseek-ai/dsh-fs@$V @deepseek-ai/dsh-fs-local@$V @deepseek-ai/dsh-tool-fs@$V \
  @deepseek-ai/dsh-user-approval@$V @deepseek-ai/dsh-permission-presets@$V @deepseek-ai/dsh-tools@$V \
  @deepseek-ai/dsh-shell@$V @deepseek-ai/dsh-shell-env@$V @deepseek-ai/dsh-bash-local@$V \
  @deepseek-ai/dsh-tool-bash@$V @deepseek-ai/dsh-tool-bash-persistent@$V @deepseek-ai/dsh-terminal@$V \
  @deepseek-ai/dsh-terminal-bash@$V @deepseek-ai/dsh-subprocess@$V @deepseek-ai/dsh-subprocess-local@$V \
  @deepseek-ai/dsh-subagent@$V @deepseek-ai/dsh-tool-subagent@$V @deepseek-ai/dsh-tool-subagent-control@$V \
  @deepseek-ai/dsh-agent-presets@$V @deepseek-ai/dsh-workspace@$V @deepseek-ai/dsh-util-workspace-path@$V \
  @deepseek-ai/dsh-authorization@$V @deepseek-ai/dsh-cmdline@$V @deepseek-ai/dsh-headless@$V \
  @deepseek-ai/dsh-web-app@$V @deepseek-ai/dsh-tool-workflow@$V @deepseek-ai/dsh-workflow@$V \
  @deepseek-ai/dsh-fs-observation-policy@$V
npm pack @deepseek-ai/node-addon-system@0.1.2        # 沙箱 Linux rung 的原生 launcher（含 C 源码）
for f in *.tgz; do d="${f%.tgz}"; mkdir -p "$d" && tar -xzf "$f" -C "$d"; done
```

`@deepseek-ai/dsh-permissions` / `@deepseek-ai/dsh-approval` 在 npm 上不存在（权限相关的包名是 `dsh-user-approval`、`dsh-permission-presets`、`dsh-sandbox-policy`）。`@deepseek-ai/dsh-authorization` 存在但**与权限无关**（它是「让人类帮忙拿凭据」的流程注册表，`dsh-authorization@0.1.5-rc.3/README.md:1-8`），本 note 不用它。

**引用格式**：`包名@版本:包内相对路径:行号`，例如 `dsh-sandbox@0.1.5-rc.3:lib/index.js:186`。tarball 解出来是 `<pkg>-<ver>/package/...`，包内路径即 `package/` 之后的部分。代码是已编译的发布 JS/`.d.ts`；README 是包自带的正式文档（`kind: package-reference`），两者都当一手来源，凡是从编译产物推导、文档没明写的，正文里写明依据。

---

## ① 结论先行

1. **DSH 没有「按路径前缀判权限」的机制。** 它的边界是一个**进程级文件沙箱**，词表里只有三种 file effect：`read-only` / `workspace-write` / `danger-full-access`（`dsh-sandbox@0.1.5-rc.3:lib/types/index.d.ts:19`）。「工作区」= 会话创建时写死的 `SessionHeader.cwd`（不可变），`workspace-write` 允许写它 + 平台临时目录；**判据在内核里（bwrap 挂载 / Landlock allow-list / Seatbelt SBPL），不在 agent 的代码里**（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:22-76`）。
2. **shell 不解析 argv、没有命令黑名单、没有启发式。** `bash` 工具把 `['bash', '-c', command]` 整条 argv 交给 `ctx.shell`，`dsh-bash-sandbox` 再把这同一条 argv 交给 `ctx.sandbox.confine()` 包一层平台 runner，然后**直接把 runner 的 argv spawn 出去**（`dsh-bash-sandbox@0.1.5-rc.3:lib/index.js:229-235`）。在取到的 41 个包里 grep 不到任何命令 allowlist/denylist/模式匹配。
3. **审批不是常规路径，而是「越界升级」的一次性手势。** 正常调用根本不问人；是**沙箱先拒绝**（内核 EROFS/EACCES/`operation not permitted`），模型在同一个 turn 里带着 `sandbox_permissions`（严格更宽的档）+ 一句 `justification` 重试**同一条命令**，这一次才经 `ctx.approval` 弹一次审批（`dsh-sandbox@0.1.5-rc.3:lib/index.js:93-112`）。没有 allow-always、没有「记住我的选择」的授权库。
4. **沙箱不可用 = 失败关闭，不会降级成「问」。** 没有可用的 runner 时抛 `SANDBOX_UNAVAILABLE` 并**拒绝运行**（原文见 §⑤），错误文本自己给出两条出路：装 bubblewrap，或者换 `danger-full-access`。唯一的「兜底」是**部署时**改挂不沙箱的 `dsh-bash-local`。
5. **子 agent 不能比父更宽——靠「子没有审批通道」实现。** 委派时只把父**显式**的 sandbox 覆盖钉进子 session，并把 approval 策略**无条件钉成 `never`**（`dsh-subagent@0.1.5-rc.3:lib/types/child-agent.js:184-189`）；子 agent 的 prompts 里明说「权限范围在启动时定死，需要审批的操作会被自动拒绝」。

---

## ② 权限 / 审批模型

### 两个正交旋钮 + 一层打包

- **sandbox mode**（`SandboxMode`）：`read-only` / `workspace-write` / `danger-full-access`（`dsh-sandbox@0.1.5-rc.3:lib/types/index.d.ts:19`；`ConfinedSandboxMode = Exclude<SandboxMode, 'danger-full-access'>`，见 `:21` —— `danger-full-access` 永远不会到达 sandbox provider）。
- **approval policy**（`ApprovalPolicy`）：只有两档，`'ask'`（默认，交给 answerer；没人答就 fail closed）与 `'never'`（**不做任何交互**，每次询问确定性判定为 `'rejected'`）（`dsh-user-approval@0.1.5-rc.3:lib/types/index.d.ts:38-46`；实现 `dsh-user-approval@0.1.5-rc.3:lib/index.js:178`）。
- **preset**：把上面两个旋钮打成一个包给用户一个选择器（`dsh-permission-presets@0.1.5-rc.3:README.md:12`）。出厂表：`workspace-write`(sandbox workspace-write + approval ask)、`danger-full-access`(danger-full-access + never)、另有部署可加 `read-only`+`ask`（`dsh-permission-presets@0.1.5-rc.3:lib/index.js:80-96`；base bundle 的表在 `dsh-base@0.1.5-rc.3:cordis.patch.yml:232-241`）。

### 审批的档与词表

- 询问的**结果**是闭合四值：`'allowed-once' | 'rejected' | 'cancelled' | 'unavailable'`（`dsh-user-approval@0.1.5-rc.3:lib/types/types.d.ts:26`）。**只有 `allowed-once`**——文档明写「`allowed-once` but no `allow-always`, remembered rule, revocation, or grant store；session policy is only `ask` / `never`」（`dsh-user-approval@0.1.5-rc.3:README.md:155`）。
- 谁可以拒绝：`'never'` 在服务内部、waterfall 派发之前就短路掉，所以即使后来有 listener `prepend` 也绕不过（`dsh-user-approval@0.1.5-rc.3:README.md:78`）。
- 询问只在**一个打开的 turn 内**合法：`request()` 会在 turn 外抛出（`dsh-user-approval@0.1.5-rc.3:lib/index.js:133`）——理由写在 README：turn 是持久日志的 commit/replay 边界。
- 落盘：`approval/asked` + `approval/decided` 成对写进**请求方 session 的日志**（log-only，不进模型上下文）（`dsh-user-approval@0.1.5-rc.3:lib/index.js:135,142`；`lib/types/types.d.ts:33-51`）。

### 谁能改、改完存哪儿

| 谁 | 手段 | 作用域 | 出处 |
| --- | --- | --- | --- |
| 部署 | profile 的 `cordis.patch.yml` / 环境变量 `DSH_PERMISSION_MODE` | 进程（含新会话默认） | `dsh-base@0.1.5-rc.3:cordis.patch.yml:211,227` |
| 用户（会话内） | `/permission [preset]` 命令 | 当前会话 | `dsh-permission-presets@0.1.5-rc.3:lib/index.js:157-190` |
| 用户（设置） | `permission.defaultPreset`（只影响**未来**的会话，已存在的会话不动） | 用户级设置 | `dsh-permission-presets@0.1.5-rc.3:README.md:64` |
| 模型 | **只能**申请一次越界重试（`sandbox_permissions` + `justification`，经用户审批） | 单次调用 | `dsh-sandbox@0.1.5-rc.3:lib/index.js:93-112` |
| 子 agent | **不能**（approval 钉死 `never`） | 子会话 | `dsh-subagent@0.1.5-rc.3:lib/types/child-agent.js:184-189` |

存储方式统一是**会话事件流**，没有外部配置文件当真相源：`sandbox/mode`（最后一个事件即该会话的覆盖）、`approval/policy`、`permission/preset` 都是 log-only 事件，靠 replay 跨重启存活，两个会话永不互相污染（`dsh-sandbox-policy@0.1.5-rc.3:lib/types/session-mode.d.ts:1-25`、`dsh-user-approval@0.1.5-rc.3:lib/types/index.d.ts:14-28`）。模型**不当场**知道自己被切了档，但每次请求前会拿到一份运行时上下文快照 `sandbox:policy` + `approval:policy`，即「当前档位是什么、意味着什么」（`dsh-sandbox-policy@0.1.5-rc.3:lib/index.js:72-76,121-130`）；`setPolicy()` 还会额外插一条 user message 宣布变化（`dsh-user-approval@0.1.5-rc.3:README.md:48,111`）。

### 另有一条与沙箱平行的通用闸门（本 install 里没人用）

工具管线本身有一个 `tools/pre-execute` waterfall，可以返回 `allow` / `deny`（带 reason）/ `ask`（带 reason），其中 `ask` **只有**在 approval 服务返回 `allowed-once` 时才执行，否则变成拒绝；缺 approval 支持时 `ask` 直接降级为拒绝（`dsh-tools@0.1.5-rc.3:lib/types/index.d.ts:22-38,413-419`、`dsh-tools@0.1.5-rc.3:lib/index.js:3304-3330`）。**但 shipped 的 bash/fs 工具并不走它**：`dsh-tool-bash` 里留着一句 `TODO(permissions): deployment policy belongs in 'tools/pre-execute' and sandboxing executors`（`dsh-tool-bash@0.1.5-rc.3:lib/index.js:106-107`），`dsh-bash-local` 的模块文档写着「Execution policy belongs in `tools/pre-execute` or a sandboxing executor」（`dsh-bash-local@0.1.5-rc.3:lib/index.js:11-12`）。也就是说：**DSH 把 shell 的权限交给了沙箱，而不是闸门。**

---

## ③ cwd 与「工作区」的判定

### 「工作区」是哪两个东西

- **不是 `dsh-workspace`**。`@deepseek-ai/dsh-workspace` 是 UI 的项目列表注册表（侧栏分组、重命名、隐藏会话），它的 `ctx.workspaceRegistry.create(dir, title)` 只管展示，文档明写「it is invisible to models and adds no prompt or request-context cost」（`dsh-workspace@0.1.5-rc.3:README.md:12,32`）。它用的唯一路径规范 `realpathNormalize`（`fs.realpath`，symlink/`..`/尾斜杠全解析，字符串相等即同一项目）与本 note 的权限边界无关（`dsh-workspace@0.1.5-rc.3:lib/types/paths.d.ts:24-33`）。
- **是 `SessionHeader.cwd`**：`readonly cwd?: string`，「Absolute working directory the session was created in (if any)」，属于「Immutable validated storage metadata, kept outside the conversation event log」（`dsh-session@0.1.5-rc.3:lib/types/types.d.ts:56-69`）。它在会话创建时由宿主从 `meta.cwd` 传入：headless 应用就是 `meta: { cwd: process.cwd() }`（`dsh-headless@0.1.5-rc.3:lib/index.js:136`）；Web 端会话由客户端请求创建，**本 install 里读不出它怎么选 cwd**（见 §⑧）。
- 兜底根：`sandbox-policy` 的配置 `workspaceRoot`（默认 `process.cwd()`），用于「没有 cwd 的会话」和 agentless 调用（`dsh-sandbox-policy@0.1.5-rc.3:README.md:45-48`）。

### 解析与规范化

每调用一次 resolve，做成一个完整的 per-call policy：

```
mode:        approved explicit grant ?? session 最后的 sandbox/mode 事件 ?? 部署默认
workspaceRoot: resolve(canonicalPath(session.header.cwd ?? 配置的 workspaceRoot))
```

（`dsh-sandbox-policy@0.1.5-rc.3:lib/index.js:141-148`，其中 `resolveWorkspaceRoot = resolve(canonicalPath(path))` 在 `:68-70`；`canonicalPath` 就是 `realpathSync.native`，**解析失败时原样返回**，理由是「一个不存在的根不该被凭空发明出来」（`dsh-sandbox@0.1.5-rc.3:lib/index.js:139-145`）。）

### 四个执行面各自的判据

| 面 | 机制 | 区外写的表现 |
| --- | --- | --- |
| Linux bwrap | `--ro-bind / /`（整个宿主只读）+ `workspace-write` 时 `--tmpfs /tmp` 与 `--bind <root> <root>`，另加 `--dev /dev --unshare-pid --proc /proc --die-with-parent` | 内核对只读 bind 报 `read-only file system`（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:22-38,206`） |
| Linux Landlock | 原生 launcher `landlock-run --ro / --rw /tmp --rw <root> -- <argv>`，**allow-list**，其余全拒 | `permission denied`（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:45-51,207`） |
| macOS Seatbelt | SBPL：`(allow default)(deny file-write*)` + `(allow file-write* (literal "/dev/null"))` + 对每个 canonical 可写根 `(subpath "...")` | `operation not permitted`（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:65-76,208`） |
| Windows ACL | 受限令牌 runner，per-workspace 写 SID（常驻 ACE）+ 每 session 随机私有临时目录 SID | `access is denied` / `access to the path` / `permission denied`（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:209-214`） |
| fs 工具（进程内 fence） | 每次 mutation 前**当场重新 canonicalize** 目标 + 逐级祖先的包含判定（lexical 快路径，拼写不同则退到文件系统 identity 比较，覆盖 Windows 8.3/大小写） | 结构化 `FS_SANDBOX_DENIED`（`dsh-fs-sandbox@0.1.5-rc.3:lib/index.js:156-164`、`lib/types/containment.d.ts:1-6`） |

`workspace-write` 的可写根是**一个函数算出来的**：`writableRoots(policy) = canonical({workspaceRoot, "/tmp", os.tmpdir()})`（去重），`read-only` 返回空（`dsh-sandbox@0.1.5-rc.3:lib/index.js:155-162`）。Seatbelt profile 与 fs fence 共用它，文档给的动机是「『写工具不能写 /tmp 但 bash 能』这种不对称不能出现」（`dsh-sandbox@0.1.5-rc.3:lib/types/roots.d.ts:1-20`）。bwrap/Landlock 有各自的拼法（bwrap 是把 `/tmp` 换成临时 tmpfs），差异「由测试钉住」。

**`..`、symlink、挂载点**：全部在「canonicalize 之后再比」这一条上得到处理——三个内核面各自按 resolved path 匹配（Seatbelt 匹配的是 resolved path，所以 `/tmp` 就是 `/private/tmp`，这也是为什么 root 必须先 canonical 化）。fs fence 承认有一个「resolve 到 syscall 之间的 TOCTOU」残余，靠「写前立刻重新 canonicalize」收窄，**不做内核级紧边界**（文档明确说 `openat2` 那类原语的移植成本不值，见 `dsh-fs-sandbox@0.1.5-rc.3:README.md:79,123`）。

**读呢？** 全部放行。`read-only` 只禁写；bwrap 是「整机只读挂进来」，Landlock 是 `--ro /`，Seatbelt 是 `(allow default)` 只 deny `file-write*`；fs fence 只覆写 `writeText`/`editText`。文档反复写「confines model file writes and edits … while preserving the local filesystem's read behavior」「Reads, listings, and metadata work exactly as with `fs-local`」（`dsh-fs-sandbox@0.1.5-rc.3:README.md:12,50`）。**「工作区」在 DSH 里是一个「写边界」，不是「可见性边界」。**

---

## ④ shell 与沙箱（核心）

### 选的是哪条路

对问题里的 (a)–(e)：**不是 (a) 一律问，不是 (b) 一律放行，不是 (c) 解析 argv，也不是单纯的 (d)——而是 (d)+(e) 的一种特定组合：内核沙箱 + 「被拒绝之后的一次性升级审批」。** 注意这条组合与「沙箱内自动允许、越界就弹审批」**不等价**：越界不会当场弹审批，而是命令先**失败**，由模型读 deny marker 后主动发起一次升级请求。

### 调用链（一次 `bash` 工具调用）

1. 工具把 `['bash','-c',command]` 交给 `ctx.shell`，并把 resolve 出来的 policy 一起带上；`workdir` 先按会话 cwd 解析，**sandbox policy 的 canonical workspace root 优先**（`dsh-tool-bash@0.1.5-rc.3:lib/index.js:389-393`、`dsh-tool-bash@0.1.5-rc.3:README.md:96`）。
2. `dsh-bash-sandbox` 继承 `dsh-bash-local` 的进程机制，把**整条 argv** 交给 `ctx.sandbox.confine(['bash','-c',command], policy)`，spawn 返回的 argv（`dsh-bash-sandbox@0.1.5-rc.3:lib/index.js:229-235`）。
3. provider 按平台链选 runner，拼出 runner 的 argv：`[...runnerArgv, "--", ...callerArgv]`（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:298-315`）。
4. 结算时分类：**runner failure 优先于 denial**（命令根本没跑），然后是「stderr 命中本 backend 的拒绝方言」→ `denied: true`，最后给结果打上 `mode` / `enforcement`（`dsh-bash-sandbox@0.1.5-rc.3:README.md:91`）。

### runner 选择与平台依赖

- 平台链：`linux: ["bwrap","landlock"]`、`darwin: ["seatbelt"]`、`win32: ["windows-acl"]`；**只有多候选时才做功能探测**，单候选直接选（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:163-176,482-499`）。探测结果缓存在 provider 生命周期内（改换 runner 要重载插件）。
- **Linux 跑什么**：优先 `bwrap`（探测是 `spawnSync("bwrap", [...profile])` 的退出码，`:99-113`）；不可用则退回**自家原生 launcher** `landlock-run`（依赖 `@deepseek-ai/node-addon-system` 的 `landlock-run` 入口，`:4-6`）。该 launcher 的 C 源码随包发布，可读：它是「自己给自己装 Landlock ruleset 再 `execve`」的独立可执行文件，**先 `prctl(PR_SET_NO_NEW_PRIVS, 1, ...)`**，把 ABI 从 `MAX_ABI 5` 向下协商（`--ro` 给读+执行位，`--rw` 给该 ABI 能授的全部文件访问，其余全拒），任何内部失败**在 exec 之前**以退出码 **125** 加一行 stderr `landlock-run: <message>` 收场，ABI 较老时打印 `landlock-run: partial enforcement (older Landlock ABI)` 但仍然执行（`node-addon-system@0.1.2:src/main.c:1-50,184-190,230-261,264-292`；常量 `LAUNCHER_BIN = "landlock-run"`、`LAUNCHER_FAILURE_EXIT = 125` 见 `node-addon-system@0.1.2:lib/index.d.ts:1-14`）。
- **macOS**：`sandbox-exec -p <SBPL>`（系统自带，文档自陈依赖已废弃的私有策略引擎，Apple 移除就没了）（`dsh-sandbox-local@0.1.5-rc.3:README.md:130`）。
- **Windows**：ACL 受限令牌 runner（`lib/runner.js` 或 tsx 跑源码，`:524-536`），**自报 `enforcement: 'partial'`**，因为 WRITE_RESTRICTED 必须保留 Everyone、且 NTFS 硬链接让一个对象有两个路径（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:187-190`、`:85-91`）。
- 每个 runner 自带两类方言，随 wrap 一起交给消费者：**拒绝方言** `denialSignatures`（bwrap `"read-only file system"`、Landlock `"permission denied"`、Seatbelt `"operation not permitted"`、Windows 三条）与**runner 自身致命诊断** `runnerFailureRules`（bwrap `"bwrap: "`；Landlock 退出码 125 + `"landlock-run: "` + 一行视为无害的 `"landlock-run: partial enforcement (older Landlock ABI)"`；Seatbelt `"sandbox-exec: "`；Windows 退出码 127 + `"windows-acl-run: "`）（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:200-242`）。之所以要把两者分开：**「命令没跑」与「沙箱拦住了命令」必须可区分**（`dsh-sandbox@0.1.5-rc.3:lib/types/index.d.ts:84-92`）。
- `enforcement` 是**上报的事实**不是承诺：`full` / `partial`（`dsh-sandbox@0.1.5-rc.3:lib/types/index.d.ts:41-46`）。

### 允许/拒绝的判据（逐条）

- `read-only`：任何写都拒；`/dev` 里只有 `/dev/null` 可写，好让 `>/dev/null` 仍然能用（`dsh-bash-sandbox@0.1.5-rc.3:README.md:38`）。
- `workspace-write`：可写 = policy 的 workspace root + 平台临时区（bwrap 下 `/tmp` 是临时 tmpfs；Landlock 下是宿主 `/tmp`；Seatbelt 下 `/private/tmp` 与 per-user temp）（`dsh-bash-sandbox@0.1.5-rc.3:README.md:39`、`dsh-sandbox@0.1.5-rc.3:lib/index.js:155-162`）。
- `danger-full-access`：不沙箱，**provider 根本不被咨询**，结果固定带 `sandbox: { mode, denied: false }`（`dsh-bash-sandbox@0.1.5-rc.3:README.md:40`、`lib/index.js:150-158`）。
- **读**：`workspace-write` 下读整机（bwrap 只读 bind 整个 `/`）。
- **网络**：**不在词表里**。`SandboxMode` 只管文件效果，「network and process visibility are outside this vocabulary」，sandbox seam 明确「expresses no network, process, syscall, device, or credential restrictions」（`dsh-sandbox@0.1.5-rc.3:lib/types/index.d.ts:13-19`、`dsh-sandbox@0.1.5-rc.3:README.md:167`）。launcher 的 `handled_access_fs` 也只管理文件访问（`node-addon-system@0.1.2:src/main.c:240`）。**shell 里的 curl 不受任何约束。**（唯一沾边的是 web fetch 工具自己的「只允许公网目的地」策略，`dsh-base@0.1.5-rc.3:README.md:50`。）

### 拒绝之后模型看到什么（逐字）

- 拒绝标记（两个族共用一句，bash 的 subject 是 `command`，fs 是 `operation`）：`[sandbox: file access denied under <mode> mode]`（`dsh-sandbox@0.1.5-rc.3:lib/index.js:64-66`）。
- 升级提示：`[sandbox: escalation available — retry this exact command once with sandbox_permissions (the narrowest wider mode that suffices) + justification; the approval prompt asks the user]`（`dsh-sandbox@0.1.5-rc.3:lib/index.js:76-78`）。
- 后台进程的 runner 失败：`[sandbox: the sandbox runner itself failed under <mode> mode — the command did not run; this is a sandbox problem, not a command failure]`（`dsh-bash-sandbox@0.1.5-rc.3:README.md:141`）。
- 工具描述里还给模型下了纪律：不要为了审批先绕道聊天、不要 speculative 升级、被拒即该命令的最终结论（`dsh-tool-bash@0.1.5-rc.3:lib/index.js:129`）。

---

## ⑤ 沙箱与审批的关系、兜底

**沙箱是唯一的墙；审批是墙上的一个受控门，而不是墙坏了以后的替代品。**

- 严格更宽才能申请：`read-only → {workspace-write, danger-full-access}`，`workspace-write → {danger-full-access}`，在**执行时**检查（不是 schema 里的 enum；schema 只暴露闭合目标集 `["workspace-write","danger-full-access"]`，因为 schema 是注册表全局的、而有效档位是每次调用的事实）（`dsh-sandbox@0.1.5-rc.3:lib/index.js:30-33,42,95`）。
- 不是更宽 → **直接抛错，不会问人**（`:95`）。缺 approval 服务、调用无 agent、被拒、取消、无人应答各有自己的错误文本（`:96-109`）。`sandbox_permissions` 与 `justification` 必须成对出现，且 justification 非空（`:51-55`）。
- 沙箱不可用 → **失败关闭**。原文（`dsh-sandbox@0.1.5-rc.3:lib/index.js:185`，code `SANDBOX_UNAVAILABLE`）：

  > `sandbox mode "<mode>" is requested but no sandbox backend is usable on this host; refusing to run the command unconfined. Install bubblewrap or run a Landlock-enforcing kernel (Linux), ensure sandbox-exec is usable (macOS), or ensure the ACL restricted-token runner can start (Windows) — otherwise switch the consumer to danger-full-access.`

  runner 在执行期自己崩了，则在后面追加 ` Runner failure: <detail>`（同处；分类见 `dsh-bash-sandbox@0.1.5-rc.3:lib/index.js:159-167`）。
- **「沙箱不可用时的兜底」只有部署层**：换挂 `dsh-bash-local`（不沙箱）或把档位调成 `danger-full-access`——两者都是**显式配置**，没有自动降级（`dsh-bash-sandbox@0.1.5-rc.3:README.md:32`）。
- **审批不可用时的兜底是拒绝**，方向相反：没有 answerer 时 approval 解析为 `'unavailable'` → 升级失败 → 命令不跑（`dsh-user-approval@0.1.5-rc.3:README.md:12,155,157`）。headless 组合没有内建 answerer，所以它的沙箱越界是**死路**；`'never'` 档更是把「升级」这条路整体关掉，并明确告诉模型别再设 `sandbox_permissions`（`dsh-user-approval@0.1.5-rc.3:lib/index.js:39`：`"Approval prompts are disabled in this session: actions that require approval are rejected automatically — do not request sandbox escalation (do not set 'sandbox_permissions')."`）。
- **网络出口策略**：不存在（见 ④）。
- 一个附带的防降级闸门：**持久终端**（`dsh-terminal-bash`）在 owner 还有打开的 session 或正在 spawn 时**拒绝改档**，理由是「不能有一个用更宽档打开的终端在降级之后活下来」（`dsh-terminal-bash@0.1.5-rc.3:README.md:68,107`）。有意思的一条：它说明 DSH 认为「档位切换」与「长命进程」的竞争关系需要一个显式 fence。

---

## ⑥ 多角色 / 子 agent

- 委派时钉策略的代码只有一处：`captureDelegatedPolicyOverrides(parent)` 返回 `{ sandboxMode: parent 会话的**显式** sandbox 覆盖（没有就是 undefined）, approvalPolicy: 有 approval 服务时 'never'，否则 undefined }`，随后 `appendDelegatedPolicyOverrides` 把它们以 `source: 'delegation'` 写进**子会话自己的日志**（在未发布的创建窗口内），于是「子的有效策略只从它自己的日志就能重建」（`dsh-subagent@0.1.5-rc.3:lib/types/child-agent.js:184-189,199-206`；类型与理由见 `lib/types/child-agent.d.ts:107-128`）。
- 注释明确：「只捕获父会话的**显式**覆盖——绝不捕获部署默认，也绝不捕获一次性 grant」（同上）。因此父通过 escalation 拿到的那一次 `danger-full-access` **不会**传给子，而父用 `/permission` 切出来的档位会传下去。
- 子 agent 的 prompt 里有一段固定声明（`dsh-subagent@0.1.5-rc.3:README.md:152`）：

  > `You are a delegated subagent: your permission scope was fixed when you were started and cannot be widened from inside this session — operations that require approval are rejected automatically. When the job needs access beyond that scope, do not retry the denied operation; state the limitation in your reply so the delegating agent can handle it.`

- **不变量的实现方式值得注意**：DSH 没有写一个「子不能比父宽」的检查器；它把子的审批通道**关掉**（`'never'`），于是「更宽」在机制上不可达——子只能在继承来的沙箱档位内工作，越界就是硬失败。
- 其它「角色」：DSH 没有与我们的**讨论者**对应的角色（`dsh-persona` 是人设，`dsh-agent-presets` 是会话的工具/prompt 组合，都不改权限）。`dsh-tool-workflow` 的 fan-out 就是 subagent（同一套钉法），它的 worker 跑在 worker thread 里，README 只把「未来可以有进程/沙箱引擎替换 worker-thread 引擎」列为 deferred，**没有任何独立的权限策略**（`dsh-tool-workflow@0.1.5-rc.3:README.md:2-12`、`dsh-workflow@0.1.5-rc.3:README.md:141`）。「沙箱与审批栈」属于 HOST 组合，agent preset 只是消费它（`dsh-agent-presets@0.1.5-rc.3:presets/cordis/agent.cordis.yml:26`、`dsh-base@0.1.5-rc.3:cordis.patch.yml:205-241,479-480`）——即**策略是 host 级的、对所有会话同源**。

---

## ⑦ 看得到但别急着照抄：这对「没有进程级沙箱」的我们意味着什么

> 这一节只写能从上面的事实 + 本仓库现有代码直接读出的推论。本仓库的事实：`bash` 的 `effect()` 恒为 `Exclusive`（`src/tools/bash.rs:75-79`）；`ask` 档下 `(Ask, 非 ReadOnly) → Decision::Ask`（`src/permissions.rs:118-127`），即**「bash 一律问」已经就是 `ask` 档的现状**；模型给的文件路径被限制在会话 cwd 子树内、且是**规则无法放宽的 deny floor**（`src/tools/paths.rs:4-17,113-124`；`src/permissions.rs:441-446`）；README 明写「这不是沙箱，v1 不做进程级隔离，升级路径是 Linux 的 bubblewrap，且不预做抽象」。

**能借鉴的**

1. **边界的定义方式**：DSH 的 `workspace-write` 是一个**写边界**——读随便，写限根。这正好对应「shell 的问题只在它会写」。如果 `workspace` 模式的目标是「区外要审批」，那它天然只需要管文件效果；现有的 cwd 路径限制（读写都限）比 DSH 更严，两者关系需要我们自己拍：DSH 没有提供「读也要审批」的先例。
2. **升级手势的形状**（不依赖内核、可以整条照搬）：拒绝 → 同一个 turn 内**原样重试**同一条命令 → 严格更宽的档位 + 一句 justification → 一次审批 → `allowed-once`；被拒即终局；不允许 speculative；更宽的档位数用一个**执行时**检查的闭合表来判，而不是把档位枚举塞进 schema。这套东西的价值在于它把「一次例外」和「一直放开」分开了，而且**不需要**沙箱就能成立——它唯一需要的前提是「有一个能和调用绑定的拒绝事件」。
3. **落盘与观感**：把档位、审批对（asked/decided）都写成事件流里的 log-only 事件，靠 replay 恢复；切档时额外给模型一条可见通告；模型平时只知道「当前档位是什么、意味着什么」（而不是把整个能力清单摊给它）。这与本仓库「事件流是唯一真相源」的取向一致。
4. **子 agent 的收口方式**：与其写一个「子不能比父宽」的检查器，不如把子的某个能力**关掉**（DSH 关的是审批通道）。本仓库已有类似取向的更强版本：「沿委派链向下传播 `Deny`/`Ask`，`Allow` 不传播」（README 安全模型）。
5. **失败态的语气**：沙箱不可用时 DSH 是 fail closed + 给出可操作的两条出路，而不是静默放行。任何「工作区模式」的兜底判断都该用同一种语气写清楚。

**不能借鉴的（也是最重要的）**

6. **DSH 之所以能在「区外」自动把控制权交回给人，是因为内核先给了一个可靠的拒绝信号。** `workspace-write` 的越界不是被「预判」出来的，是被 `EROFS`/`EACCES`/`EPERM` 打回来的；模型因此知道「这条命令确实越界了」，用户看到的也是「一条已经失败的命令 + 一次重试请求」。**没有进程级沙箱的仓库拿不到这个信号**：我们无法在 `bash` 调用前知道它会不会写到区外，也无法在它写完之后可靠地知道它写了哪里。
7. **因此对问题里的两种候选，可达范围是**：
   - **「bash 一律问」**：在本仓库里**已经实现**——那就是 `ask` 档（`src/permissions.rs:118-127`）。把它再包成 `workspace` 模式，只是换了个名字；它不会带来任何「区外」语义，也不会比 `ask` 更精确。它的代价是明确的：每条命令都打断人（DSH 的取向相反——它教模型「先跑、读标记，别预设会被拒」，只在真的被拒时才升一次级，`dsh-tool-bash@0.1.5-rc.3:lib/index.js:129`）。
   - **「启发式看 argv」**：能从 DSH 借鉴的部分到此为止——DSH 的 41 个包里**没有任何**命令 allowlist / denylist / 危险模式匹配（§① 第 2 条），也没有任何「解析 shell 字符串」的代码；argv 全程是 `['bash','-c',command]` 一条不可分解的元素。这不是因为它没想到，而是因为「越界」的判据在它那里由内核给。启发式只能产出**预判**，而预判既不是拒绝也不是允许：模型可以用变量展开、`eval`、解释器、子进程、写脚本再执行等方式让静态形状与真实行为脱钩；而一旦预判错了方向，要么把大量无害命令变成打扰，要么给出一个假的「已检查」观感。
   - 结论性的一句：**「shell 在工作区外要审批」在没有进程级隔离的仓库里，能落地的只有「一律问」（= 现状 `ask`）或「一律放行」两端**；中间那条「区内自动、区外问」依赖一个我们目前没有的判定者。
8. **DSH 的沙箱模型因此不能替我们回答的那个问题**是：把 shell 纳入 `workspace` 模式的**可执行判据**是什么。DSH 的答案是「内核 + 拒绝后升级」，我们 README 已经写明 v1 不走这条路（「不预做抽象，升级路径是 Linux 的 bubblewrap」）。所以在此之前，「区外」由谁判定（模型自报？按 argv 粗判？承认做不到而把 shell 排除在 `workspace` 语义之外？）仍是本仓库自己要拍的决定，research 不替它答。

---

## ⑧ 边界：这些产物回答不了什么

- **Web 端会话的 cwd 怎么来**：base bundle 注释说「Web creates sessions on client request」（`dsh-base@0.1.5-rc.3:cordis.patch.yml:470-471`），但创建时 `meta.cwd` 具体由谁给、能不能中途改，本 install 里读不出（`dsh-web-app` 的 README/`lib/index.js` 里 grep 不到 `cwd`；相关的 session controller 包不在取到的集合里）。能确定的只有：header 里的 `cwd` 是 immutable（`dsh-session@0.1.5-rc.3:lib/types/types.d.ts:68-69`），以及 headless 用 `process.cwd()`。
- **`dsh-sandbox-windows-acl` 包未取**：Windows rung 的细节（随机私有 temp 目录、`--write-sid`/`--temp-write-sid`、ACE 生命周期）只有 `dsh-sandbox-local` 的 README/代码转述（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:85-91,333-425`），没有一手实现可读。
- **bwrap 探测只到「退出码是否为 0」**：哪些失败（未安装 / unprivileged userns 被禁 / LSM 拒绝 mount）在代码里不可区分（`dsh-sandbox-local@0.1.5-rc.3:lib/index.js:99-113`）；文档说 Landlock 是「bwrap 不可用时的替代」（`node-addon-system@0.1.2:src/main.c:4-9`）。
- **`landlock-run` 的发行形态未验证**：源码（`src/main.c`）与入口（`lib/index.d.ts`）可读，但实际二进制在 per-platform 可选包 `@deepseek-ai/node-addon-system-linux-{x64,arm64}` 里，本 note 没有取它；静态/动态链接与各发行版可跑性读不出。
- **hooks 是否也走权限**：`@deepseek-ai/dsh-hooks-claude-code` / `dsh-hooks-codex` 未取（`dsh-user-approval` 的文档提到「e.g. a hook's permission-decision reason」，`dsh-user-approval@0.1.5-rc.3:lib/types/types.d.ts:30-35`），所以「Claude Code 风格的 hook 能不能收紧/放宽本仓的闸门」在这些产物里没有直接答案。
- **没有穷举**：npm scope 下还有约 40 个 `@deepseek-ai/dsh-*` 包未取（UI、session 存储、telemetry、ACP/SDK 等）。「没有 argv 启发式」这句话的强度是「在取到的 41 个包里 grep 不到」，不是「DSH 一定没有」。
- **版本陷阱（给复现者）**：不带版本号 `npm pack @deepseek-ai/dsh-sandbox` 会拿到 `latest` tag = **0.0.1-rc.1**，与 `dsh@0.1.5-rc.3` 的依赖图不是同一代产物；本 note 全部按 `0.1.5-rc.3`（原生包按 `node-addon-system@0.1.2`）钉死。
