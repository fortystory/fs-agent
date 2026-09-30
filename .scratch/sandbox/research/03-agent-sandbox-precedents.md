# research：同类 coding agent / harness 产品实际怎么做进程级沙箱

## 这份文件是什么

`fs-agent` 的立场是 v1 不做进程级沙箱，升级路径写的是「只做 Linux 的 bubblewrap」。这份文件回答一个先例问题：**别人实际怎么做的**——机制选择、工程形状、踩过的坑。**先例是事实，不是建议**：本文件不替本仓库拍决定，也不写设计建议。

姊妹篇 [`01-dsh-workspace-permissions-and-shell.md`](01-dsh-workspace-permissions-and-shell.md) 已经查清 DSH（`@deepseek-ai/dsh`）的做法，本文件**不重复调研 DSH**；只在需要对照时引用它已经建立的事实。

读者是接下来要拍「要不要做进程级沙箱、做哪条路」的人。**① 结论先行**是事实清点，各产品分节是细节，**横向对比表**用来并排看，**⑦ 共同踩过的坑**是复用价值最高的一节，**⑧** 写清楚这份文件回答不了什么。

## 来源、版本与观察时点

**观察时点：2026-09-30（UTC 11:30–12:40）**。下面每节各自钉住版本；凡是版本相关的都写在节内。

| 对象 | 版本 / 时点 | 取证方式 |
| --- | --- | --- |
| `openai/codex` | commit `a5cce8895a1400f94eb0a71771275284027fff17`（2026-09-30 11:05:59 +0000）；npm `@openai/codex` `latest = 0.159.2`（registry 时间 2026-09-30T00:03:49Z），`alpha = 0.161.0-alpha.3` | `git clone --depth 1 --filter=blob:none --sparse`（只取 `codex-rs/`、`docs/`）；源码行号即该 commit 的行号 |
| Claude Code 文档 | 抓取于 2026-09-30；文档内提到 `v2.1.212`、`v2.1.219`、`v2.1.260`、`v2.1.271` 等版本点 | `code.claude.com/docs/en/*.md`（Mintlify 的 Markdown 版本） |
| `anthropic-experimental/sandbox-runtime` | commit `6f0ce155ccb136bda33a8a72201fe7f54fe47d9b`（2026-09-29 18:08 -0700）；`package.json` `version 0.0.78`；npm `latest = 0.0.78`（2026-09-30T01:18:54Z） | `git clone --depth 1`；README 与 `src/` 行号即该 commit |
| Gemini CLI | `google-gemini/gemini-cli` 的 `main` 分支，2026-09-30 抓取（未钉 commit） | raw 文件与官方站点 |
| OpenHands | `docs.openhands.dev`，2026-09-30 抓取 | 文档 Markdown 版本 |
| Goose | `aaif-goose/goose` `main`（抓取日），官方 blog `v1.25.0`（2026-02-23） | 官方文档与 blog |
| Pi | `earendil-works/pi`：官网与包页抓取于 **2026-10-01**（未钉 commit）；第三方包只读 npm / GitHub 页面，**未读源码** | `pi.dev` 官方站与 `Oxel40/pi-os-guard`、`navikt/cplt` 的仓库页 |
| Cursor | `cursor.com/docs`，2026-09-30 抓取 | 文档 Markdown 版本 |
| Devin | `docs.devin.ai`，2026-09-30 抓取 | 文档 Markdown 版本 |
| E2B / Modal / Daytona / Vercel / Cloudflare | 各家官方站，2026-09-30 抓取 | 文档 Markdown 版本与 `llms.txt` |

**引用格式**：源码写 `owner/repo@commit:路径:行号`；官方文档给 `[标题](URL)` 并指出小节名。凡是我在文档里没有实际读到的行号，一律不给；凡是找不到一手说法的，写「**一手未验证**」，不拿二手内容顶替。

---

## ① 结论先行

1. **「工作区内自动、区外要审批」这条需求，在同类产品里没有一家是纯靠「猜 argv」做的。** 有四家把它拆成两个机制：**内核/内核级沙箱给硬边界**（bubblewrap / Landlock / Seatbelt / seccomp / microVM），**越界后由模型带着一个「更宽的档位申请 + 一句理由」重试，触发一次人工审批**。Codex 的形状是 `sandbox_permissions: "require_escalated"` + `justification`（`openai/codex@a5cce889:codex-rs/core/src/tools/handlers/shell_spec.rs:232-266`），Claude Code 是 `dangerouslyDisableSandbox` 重试（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “The unsandboxed retry escape hatch”），Devin 是 `sandbox.excluded` 的 `allow`/`ask`/`deny` 三条 `Exec(...)` 规则（[Sandbox](https://docs.devin.ai/cli/sandbox)，小节 “Excluded commands”），Cursor 是「进不了沙箱的命令由 Auto-review classifier 或你来批」（[Run Modes](https://cursor.com/docs/agent/security/run-modes)，小节 “Sandboxing”）。
2. **Linux 侧的主流不是「自己写 Landlock helper」，而是 bubblewrap。** Codex 是 bubblewrap + 进程内 seccomp（`codex-rs/linux-sandbox/README.md:24-49`），Claude Code / srt 是 bubblewrap + 代理（[How sandboxing works](https://code.claude.com/docs/en/sandboxing)，小节 “OS-level enforcement”），Devin CLI 要 `bwrap` + `socat`（[Sandbox](https://docs.devin.ai/cli/sandbox)，小节 “How the sandbox works”）。**Landlock 有两家在用**：Cursor（Landlock + seccomp，要求 kernel 6.2+）与 Codex 的 *legacy* 路径（默认关闭）。**把沙箱做成一个独立 helper 可执行文件、靠 argv 传 profile 再 exec 的，我读到的是 Codex 一家**（`codex-linux-sandbox`，问题里的假设成立，但细节与假设不同——见 ②）；srt 则自己生成 seccomp BPF 过滤器（随包发布预生成产物），但不是一个独立 helper binary。
3. **macOS 侧几乎一致：`sandbox-exec` + Seatbelt SBPL，生成 profile 字符串。** Codex、Claude Code/srt、Cursor、Gemini 都是这一条；四家都没提替代方案。**Windows 是唯一真正分化的一侧**：Claude Code 直接不支持 native Windows（只 WSL2），Devin CLI 在 Windows 上 hard-fail，Codex 走受限令牌 / Microsoft MXC，Gemini 用 `icacls` 打 Low integrity 标记。
4. **「沙箱不可用」的处理分两派，且都写得很显式**：Codex **内建兜底**——优先 PATH 上的 `bwrap`，缺失就用随包发布的 bundled bwrap，bwrap 建不了 user namespace 就在启动时警告（`codex-rs/linux-sandbox/README.md:10-39`）；Devin 与 Claude Code 是 **fail closed**——Devin「refuse to start rather than running unsandboxed」，Claude Code 有 `failIfUnavailable`（[Sandbox](https://docs.devin.ai/cli/sandbox)，小节 “How the sandbox works”；[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Enforce sandboxing with managed settings”）。这与 DSH 的 `SANDBOX_UNAVAILABLE` 是同一取向。
5. **沙箱即服务（E2B / Modal / Vercel / Daytona / Cloudflare）走的是另一条路：边界在**创建时**静态定死，运行中不存在「越界找谁审批」这个动作。** 它们是 microVM 或内核隔离容器，整台沙箱就是 agent 的地盘（[E2B Security](https://e2b.dev/security)，原文「Every sandbox runs in its own Firecracker microVM」；[Vercel Sandbox](https://vercel.com/docs/sandbox)，原文「isolated Linux microVMs」）。**没有一家在这类产品的官方文档里写「agent 越界时向人申请一次例外」**（见 ⑤，这是我读到的范围的结论，不是「一定没有」）。

---

## ② OpenAI Codex CLI（`openai/codex`，Rust）

版本：commit `a5cce889`（2026-09-30）/ npm `0.159.2`。以下行号都属该 commit。

### 词表：三档 sandbox + 四档 approval

- `SandboxMode` 是闭合三值：`read-only`（默认）、`workspace-write`、`danger-full-access`（`codex-rs/protocol/src/config_types.rs:104-114`）。
- 运行时策略是 `SandboxPolicy`：`danger-full-access`、`read-only { network_access }`、`external-sandbox { network_access: NetworkAccess }`、`workspace-write { writable_roots, network_access, exclude_tmpdir_env_var, exclude_slash_tmp }`（`codex-rs/protocol/src/protocol.rs:1072-1120`）。注意 `external-sandbox` 这一档是「进程已经在一个外部沙箱里，给全盘访问但按 `network_access` 走」。
- 审批策略 `AskForApproval`：`untrusted`（内部 `UnlessTrusted`，官方文档说它**已 retired**，保留会让客户端起不来）、`on-request`（默认）、`granular(GranularApprovalConfig)`、`never`（`codex-rs/protocol/src/protocol.rs:986-1009`；官方文档 [Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security)，小节 “Migrate from the retired `untrusted` approval policy”）。`GranularApprovalConfig` 把审批分成五类：`sandbox_approval` / `rules` / `skill_approval` / `request_permissions` / `mcp_elicitations`（`codex-rs/protocol/src/protocol.rs:1011-1026`）——即「哪类打断允许弹给人，其余自动拒绝」。
- 平台实现的选择是一个枚举：`SandboxType::{None, MacosSeatbelt, LinuxSeccomp, WindowsRestrictedToken, WindowsMxc}`（`codex-rs/protocol/src/sandbox.rs:8-16`）。**Linux 那一档的枚举名仍叫 `LinuxSeccomp`，但它今天的文件系统隔离者是 bubblewrap**（下节）。

**推论**：`sandbox_mode` 与 `ApprovalPolicy` 这两个旋钮与 DSH 的 `SandboxMode` + `ApprovalPolicy` 是同构的；差异在 Codex 的 approval 多了一个 `granular` 维度，以及多了一个给模型用的 `request_permissions` 工具。

### Linux：helper 可执行文件 `codex-linux-sandbox`，但文件系统隔离交给 bubblewrap

问题里的假设——「是一个独立 binary，靠 argv 传 profile，施加后 `execvp` 目标命令」——**成立**，但施加的机制不是「Landlock + seccomp 自己写的」，而是「bubblewrap + 进程内 seccomp」：

- crate 自述它产出「一个随 Node.js 版 Codex CLI 发布的独立 Linux 可执行文件 `codex-linux-sandbox`」，同时把逻辑以 `run_main()` 暴露成 lib，好让 `codex-exec` 检查自己的 arg0 是不是 `codex-linux-sandbox`，是就按 helper 跑（`codex-rs/linux-sandbox/README.md:3-8`；`codex-rs/linux-sandbox/src/lib.rs:36-38`；`codex-rs/linux-sandbox/src/main.rs:4-6`，注释说 cwd/env/argv 会原样保留到最终的 `execv`）。arg0 分派的实现见 `codex-rs/exec/src/main.rs:5-6` 与 `codex-rs/arg0/src/lib.rs:207-214`。
- **helper 的 argv 形状**（这就是「profile 怎么传」的答案）：`--sandbox-policy-cwd <路径> --command-cwd <路径> --permission-profile <JSON> [--use-legacy-landlock] [--managed-network <JSON>] -- <命令...>`（`codex-rs/sandboxing/src/landlock.rs:26-67`）。profile 是 **`PermissionProfile` 的 JSON 序列化**（同上，`:34-35`），`--` 之后是原命令（`:64-65`）。
- **helper 内部的三步**（`codex-rs/linux-sandbox/src/lib.rs:1-5` 与 `src/linux_run_main.rs:160-167`）：① 需要时用 bubblewrap 构造文件系统视图；② 在进程内施加 `no_new_privs` + seccomp；③ `execvp` 进最终命令。
- **Landlock 是 legacy、默认关闭**：`--use-legacy-landlock` 是 opt-in（`src/linux_run_main.rs:114-118`）；README 明写「Filesystem-restricted execution requires bubblewrap. The legacy Landlock option is rejected for these policies because it cannot isolate app-server Unix sockets.」（`codex-rs/linux-sandbox/README.md:40-42`）。代码里的 Landlock 挂载函数带注释「currently unused because filesystem sandboxing is performed via bubblewrap. It is kept for reference and potential fallback use.」（`codex-rs/linux-sandbox/src/landlock.rs:138-170`，其中 `:145-146` 是这句）。
- **bubblewrap 之外的进程内层**：`apply_permission_profile_to_current_thread()`（`codex-rs/linux-sandbox/src/landlock.rs:35-97`）负责 `PR_SET_NO_NEW_PRIVS` 与 seccomp；网络 seccomp 有三档 `Restricted` / `ProxyRouted` / `VmSocketRestricted`（`:99-127`），其中 `VmSocketRestricted` 的注释解释了为什么要挡 VM socket：在 WSL2 里它能通过 interop socket 的别名启动 Windows 进程（`:58-64`）。`no_new_privs` 只在需要 seccomp 或显式用 legacy Landlock 时才开，因为很多 bwrap 部署依赖 setuid（`:66-74`）。
- **bwrap 的 argv 形状**：`--new-session --die-with-parent --dev /dev --unshare-user --unshare-ipc [--unshare-pid] [--unshare-net] [--proc /proc]`（`codex-rs/linux-sandbox/src/bwrap.rs:283-307`）；文件系统挂载顺序（`--ro-bind / /` 或 `--tmpfs /` 起手，`--bind` 重开可写根，`--ro-bind` 复位保护子路径）写在 `:408-424` 的文档注释里。读的默认是整机只读、写的默认只有策略里列出的根（README `:46-58`）。
- 值得单列一条工程细节：**writable root 下面的 `.git`、解析出来的 `gitdir:`、`.codex` 会被 `--ro-bind` 重新压成只读**（README `:50-52`）；不存在的保护路径靠「在符号链接或第一个缺失组件上挂 `/dev/null`」挡住（README `:78-80`）。
- 官方还给了一个**本地试跑**入口：`codex sandbox macos|linux|windows [--permissions-profile <name>] [COMMAND]...`（macOS 另支持 `--log-denials`），同一命令也有别名叫 `codex debug`，平台 helper 另有别名（如 `codex sandbox seatbelt`、`codex sandbox landlock`）（[Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security)，小节 “Test the sandbox locally”）。

### 「bwrap 不总是可用」：Codex 的答案是随包带一份

- 优先用 PATH 上、且**不在当前工作目录里**的第一个 `bwrap`；`bwrap` 在但太老（不支持 `--argv0`）时走 no-`--argv0` 兼容路径；`bwrap` 缺失时回退到随 Codex 发布的 `codex-resources/bwrap`（`codex-rs/linux-sandbox/README.md:10-16`、`:24-34`）。
- 这两种降级都在**启动时**给用户一条警告（bwrap 缺失、或 bwrap 无法创建 user namespace），而不是等运行时第一条命令炸掉（README `:17-19`、`:33-36`）。**WSL1 被直接拒绝**：它建不了所需的 user namespace，Codex 在调 bwrap 之前就拒掉需要 bwrap 路径的沙箱命令（README `:20-22`、`:37-39`）。WSL2 走正常 Linux 路径。
- bundled bwrap 不是另写的：`codex-rs/bwrap/src/main.rs:1-28` 用 `#![cfg(bwrap_available)]` 调进 vendored 的 bubblewrap C 源码（`bwrap_main`），其 panic 文本写着「bubblewrap sources expected at codex-rs/vendor/bubblewrap」（同文件 `:31-40`）；bundled bwrap 摘要校验失败的退出码是 8（`codex-rs/linux-sandbox/src/lib.rs:31-33`）。
- 官方文档把版本点也钉死了：**WSL1 支持到 Codex `0.114`；从 `0.115` 起 Linux 沙箱迁到 `bwrap`，WSL1 不再支持**（[Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security)，小节 “OS-level sandbox”）。

### macOS：Seatbelt，`sandbox-exec` 只信 `/usr/bin` 下的那一个

- 常量 `MACOS_PATH_TO_SEATBELT_EXECUTABLE = "/usr/bin/sandbox-exec"`，注释解释：只考虑 `/usr/bin` 下的那个，是为了防有人在 PATH 上塞一个恶意版本；如果连 `/usr/bin` 的那个都被改了，攻击者本来就已经有 root（`codex-rs/sandboxing/src/seatbelt.rs:58-62`）。
- profile 是**拼出来的**：基础策略 `seatbelt_base_policy.sbpl` 以 `(deny default)` 开头，然后逐条 allow（`process-exec` / `process-fork` / `signal same-sandbox` / 一长串 sysctl / IOKit / `pseudo-tty` / `/dev/ptmx` 等）（`codex-rs/sandboxing/src/seatbelt_base_policy.sbpl:1-116`）。网络策略与「受限只读的平台默认」各自是单独的文件，用 `include_str!` 编译进来（`codex-rs/sandboxing/src/seatbelt.rs:21-28`）。
- 生成命令参数的入口是 `create_seatbelt_command_args()`（`codex-rs/sandboxing/src/seatbelt.rs:871-880`），核心在 `create_seatbelt_command_args_with_profile()`（`:882-`）：它从文件系统策略里取不可读根、可写根，把可写根的只读子路径的**祖先**也保护起来（理由是「否则把一个可写目录改名就能把它的子孙搬出策略」，`:919-940`）。官方文档确认调用形状是 `sandbox-exec` 加 `-p` profile（[Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security)，小节 “OS-level sandbox”）。

### Windows：受限令牌，以及新的 MXC 后端

- `SandboxType` 里 Windows 有两个：`WindowsRestrictedToken` 与 `WindowsMxc`，选择逻辑里 `WindowsMxc` 是粘性的（被显式选中就不会被旧的 level 改回去），`Disabled` 会归零成 `None`（`codex-rs/protocol/src/sandbox.rs:30-41`）。
- MXC 后端自述：它把命令通过当前 Codex 可执行文件直接送进 Microsoft 的 MXC `BaseContainerRunner`，需要 Windows 的 PSEC；**从不**调用 MXC 的 AppContainer dispatcher、不改宿主 ACL、不创建 sandbox 用户、不跑 setup、不请求提权；`windows.sandbox = "mxc"` 是 strict，且「命令失败永不触发后端回退」（`codex-rs/mxc-sandbox/README.md:1-20`、`:22-30`）。

### 越界之后：`require_escalated` + `justification`

- 工具 schema 暴露三个值：`use_default` / `with_additional_permissions`（配 `additional_permissions`）/ `require_escalated`（=未沙箱执行）；`justification` 的说明原文是「User-facing approval question for `require_escalated`; omit otherwise.」，另外还有一个可复用的 `prefix_rule`（`codex-rs/core/src/tools/handlers/shell_spec.rs:232-266`）。
- 编排器的模块注释把顺序写死了：**approval → select sandbox → attempt → 被拒后用一个升级过的沙箱策略重试（因为审批被缓存，不再次打扰用户）**（`codex-rs/core/src/tools/orchestrator.rs:1-8`）。
- 拒绝路径是显式的：拿不到「不沙箱」的许可就**不重试**，把拒绝原样返回（`orchestrator.rs:426-435`、`:461-470`）；重试理由由 `build_denial_reason_from_output` 从输出里构造（`:478`）。
- 有一条不变量值得一提：**如果策略里有「拒绝读」的路径，`require_escalated` 不能被解释成「去掉沙箱」**——因为拒绝读只存在于沙箱内，去掉沙箱等于悄悄把它们放行；这种情况下 `SandboxPermissions::RequireEscalated` 会被降回 `UseDefault`（`codex-rs/core/src/tools/sandboxing.rs:287-313`），且 `SandboxOverride::EscalatedSandboxWithRestrictions` 在没有文件系统沙箱时会直接报错（`:240-250`）。
- 审批本身也可以不给人：`approvals_reviewer = "auto_review"` 会把够格的审批请求先送给一个 reviewer agent，失败**fail closed**、超时也不执行（[Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security)，小节 “Automatic approval reviews”；默认策略在仓库里的 `codex-rs/core/src/guardian/policy.md`，同节引用）。

### 网络

- `workspace-write` 默认**关网**，打开是 `[sandbox_workspace_write] network_access = true`（官方文档同页，小节 “Network access”）。
- 想按域名限制，要开 `network_proxy`：`[features.network_proxy] enabled = true` + `domains = { "api.openai.com" = "allow", "example.com" = "deny" }`；文档明确「加域名规则本身不会打开代理」（同节，`:116-126`）。
- 在 bwrap 侧，网络受限且不走代理时用 `--unshare-net`；走托管代理时用 `--unshare-net` + 一个内部的 TCP→UDS→TCP 路由桥，桥生效后 seccomp 再挡掉新的 AF_UNIX/socketpair（`codex-rs/linux-sandbox/README.md:83-89`）。

---

## ③ Anthropic Claude Code 与 `sandbox-runtime`

版本：文档抓于 2026-09-30；`@anthropic-ai/sandbox-runtime` `0.0.78`（commit `6f0ce155`，2026-09-29）。

### Claude Code 内建的 Bash 沙箱

- 定位：**「Bash 沙箱让 Claude 不用每条命令都问」**——定义好能碰哪些文件与域名，由操作系统对每条 Bash / PowerShell / Monitor 命令及其子进程执行这个边界（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，开篇原文）。
- **两种 sandbox mode**，区别只在「沙箱内的命令是否自动放行」：`auto-allow`（能沙箱化的自动跑，不需要你的许可）与 `regular permissions`（都走常规权限流）（同页，小节 “Sandbox modes”）。`/sandbox` **不是** permission mode：permission mode 决定「这次调用跑不跑、先不问」，沙箱决定「跑起来之后能碰到什么」（同页，小节 “Permission modes”，`:606-616`）。
- 默认写：cwd 及其子目录 + `--add-dir` / `permissions.additionalDirectories` 加的目录 + `$TMPDIR` 指向的 per-user 临时目录；默认读：**整台机器**，除非被 deny，文档自己点出这仍然允许读 `~/.aws/credentials` 与 `~/.ssh/`（同页，小节 “Filesystem isolation”，`:495-497`）。`permissions.blockReadsOutsideWorkingDirectories` 可以把家目录的读也切掉（`:497`）。
- **protected paths**：即便在一个可写目录里，沙箱仍然拒绝写 Claude Code 自己加载配置/代码的路径（`.claude` 下的设置、skills/agents/commands/hooks 目录、`.mcp.json`、`.bashrc`/`.zshrc`、`.gitconfig`、`.vscode`/`.idea`、`.git/hooks`、`.git/config`、`~/.claude` 的大部分与 `.credentials.json` 等）；`allowWrite` 或 `Edit` 允许规则**都不能**豁免它们，唯一的关闭方式是关掉整个文件系统层（同页，小节 “Protected paths”，`:504-515`）。
- OS 级实现：macOS 用 Seatbelt，Linux 用 `bubblewrap`，WSL2 同 Linux；**WSL1 不支持**，因为 bubblewrap 需要只有 WSL2 才有的内核特性（同页，小节 “OS-level enforcement”，`:563-571`）。Linux 还需要 `socat`（代理桥）与可选的 seccomp 过滤器（`npm install -g @anthropic-ai/sandbox-runtime` 提供它，用于挡 Unix domain socket）（同页，小节 “Set up Linux and WSL2”，`:69-92`）。

### 越界之后：未沙箱重试的 escape hatch

这是 Claude Code 对「越界要审批」的答案，形状与 Codex 几乎一样：

- 沙箱把违规**报在失败命令的结果里**，点名被拒的路径或 host，让 Claude 看见；然后 Claude 可以带 `dangerouslyDisableSandbox` 参数**重试**这条命令。重试的命令在沙箱外跑，因此走常规权限流：Manual 模式下会给你一个确认提示（同页，小节 “The unsandboxed retry escape hatch”，`:151-157`）。
- 想关掉这条门：`"allowUnsandboxedCommands": false`（面板里叫 **Strict sandbox mode**），此时 `dangerouslyDisableSandbox` 被忽略，除非命令在 `excludedCommands` 里（同页 `:157`）。
- 网络侧的「一次例外」是另一种形状：**默认不预允许任何域名**，第一次需要新域名时提示你；选 Yes = 本 session 内该 host 一直允许，选 “Yes, and don't ask again” = 存一条 `WebFetch(domain:...)` allow 规则，跨 session 生效（同页，小节 “Network isolation”，`:522-527`）。管理员可以用 `strictAllowlist` / `allowManagedDomainsOnly` 把它锁成纯白名单（`:526-527`）。

### 内建沙箱自己声明的局限（原文要点）

- 网络过滤默认**不终止、不检查 TLS**；文档直接点名可以用 domain fronting 绕过域名白名单，要更强保证得自己上会解 TLS 的自定义代理（同页，小节 “Security limitations”，`:710-714`）。
- `allowUnixSockets` 可能把 `/var/run/docker.sock` 这类东西放进来，「实际上等于把宿主交给它」（`:716`）。
- 过宽的可写路径（`$PATH` 里的目录、系统配置、shell 启动文件）会导致提权（`:717`）。
- Linux 侧有一个 `enableWeakerNestedSandbox`，为的是在无特权命名空间的 Docker 里或 unprivileged user namespace 被 sysctl 关掉的宿主上还能跑；文档说它「considerably weakens security」，只在外面另有隔离时用（`:718`）。
- 平台与工具兼容：**不支持 native Windows 与 WSL1**（`:723`）；`docker` 命令不兼容、要进 `excludedCommands`（`:689`）；Go 写的 CLI（`gh`/`gcloud`/`terraform`）在 macOS 上会 TLS 校验失败（`:687`）；`open`/`osascript` 默认被 Apple Events 限制挡住（`:688`）；`pbcopy`/`xclip`/`wl-copy` 拿不到系统剪贴板（`:690-694`）。
- 范围：沙箱只管 **Bash 子进程**；内建的 Read/Edit/Write 工具走权限系统而不是沙箱；子 agent 与父同进程、用同一份沙箱配置（同页，小节 “Scope”，`:729-734`）。

### `sandbox-runtime`（`srt`）：把整个进程包起来

- 定位：`@anthropic-ai/sandbox-runtime` 用**和内建 Bash 沙箱同一套 Seatbelt / bubblewrap 隔离**去包住整个 Claude Code 进程，于是每个工具、hook、MCP server 都在边界里，而不只是 shell 命令；官方把它标成 **beta research preview**、配置格式可能变（[Sandbox environments](https://code.claude.com/docs/en/sandbox-environments)，小节 “Sandbox runtime”，`:82-86`）。
- 机制自述（README）：macOS 用 `sandbox-exec` + 动态生成的 Seatbelt profile；Linux 用 bubblewrap 做容器化并做网络命名空间隔离；Windows（alpha）用专用的 `srt-sandbox` 本地账户 + 按该账户 SID 生效的 Windows Filtering Platform 出网围栏 + 工作树上的 per-session 显式 ACE（`sandbox-runtime@6f0ce155:README.md:103-109`、`:539-543`）。
- **双隔离模型**是它的核心论证：文件与网络必须同时隔离，否则「有文件隔离但没网络隔离」可以外泄 SSH key、「有网络隔离但没文件隔离」可以给系统资源开后门（README `:113-116`）。读写语义是**不对称的**：读是 deny-then-allow（默认全允许，可以 deny 一大片再 allow 回来，`allowRead` 压过 `denyRead`），写是 allow-only（默认全拒，必须显式列，**`denyWrite` 压过 `allowWrite`**）（README `:117-122`）。
- 网络：默认全拒，流量必须走宿主上的代理——Linux 把进程的网络命名空间整个拿走，代理监听 Unix domain socket 再 bind-mount 进沙箱；macOS 的 profile 只允许连一个特定 localhost 端口；Windows 用 WFP 只放行 loopback 到代理端口段；HTTP/HTTPS 与其它 TCP 分别由 HTTP 代理与 SOCKS5 代理管，它们执行域名 allow/deny（README `:122-130`）。
- **Ubuntu 24.04 的 unprivileged user namespace 限制**，README 有专门一节：「Ubuntu 24.04+ note: These releases enable `kernel.apparmor_restrict_unprivileged_userns` by default, which allows `unshare(CLONE_NEWUSER)` but **strips capabilities from the resulting namespace**. Both bubblewrap and the seccomp isolation layer need capability-bearing user namespaces.」给出的解法是 `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`，或者加一个给相关二进制授予 `userns` 的 AppArmor profile（README `:562-568`）。同一节还写了 root 调用者的额外要求：需要 capability bounding set 里有 `CAP_SETFCAP`，否则每条命令都会在写 uid map 时 `Operation not permitted`（README `:570-572`）。
- 官方文档给的做法是**加 AppArmor profile**（不是改 sysctl）：在 Ubuntu 24.04 及以后，默认 AppArmor 策略阻止 bubblewrap 创建它需要的 user namespace；先 `sysctl kernel.apparmor_restrict_unprivileged_userns` 看是不是 `1`，是的话给 `bwrap` 加一段授权（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Ubuntu 24.04 and later: allow bubblewrap to create user namespaces”，`:95-122`）。
- `srt` 自己声明的局限（README 的 “Security Limitations” 与 “Known Limitations and Future Work” 两节，`:881-901`）：
  - 网络过滤只看域名、不看流量内容，允许 `github.com` 就等于允许往任意 repo push；仍警告 domain fronting（`:879`、`:886`）；
  - **Linux 的代理靠环境变量 `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`**，不遵守这些变量的程序会直接连不上网；README 把 proxychains + `LD_PRELOAD` 列为未来改进（`:897-901`）；
  - unix socket 与文件权限两类的提权风险（`:889-890`）；
  - `enableWeakerNestedSandbox`（`:891`）、macOS 的 `enableWeakerNetworkIsolation`（`:892`）与 `allowAppleEvents`（`:893`）各自削弱什么，逐条写明；其中 `allowAppleEvents` 的措辞是**「removes code-execution isolation, not just weakens it」**。
- 内建沙箱与 srt 的差异（官方口径）：Linux/WSL2 上 srt 的写授权**只对已存在的路径生效**；Linux 的强制 deny 列表**在启动时构建一次**，对会话期间新建的嵌套仓库（`git init`、`git clone`、脚手架）**不覆盖**（[Sandbox environments](https://code.claude.com/docs/en/sandbox-environments)，小节 “What the runtime blocks on its own”，`:119-128`）；settings 文件存在但为空/不可读/非法时**拒绝启动**（`:128`）。

---

## ④ 其它 agent / harness 产品

### Gemini CLI（`google-gemini/gemini-cli`）

来源：`raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/cli/sandbox.md`（main 分支，2026-09-30 抓取；Gemini CLI 官方 README 指向它的渲染版 <https://www.geminicli.com/docs/cli/sandbox>）。下面的行号是 raw 版的行号。

- 沙箱是一个**要显式打开的开关**（文档没有「默认已开」的说法）：用 `-s`/`--sandbox`、`GEMINI_SANDBOX=true|docker|podman|sandbox-exec|runsc|lxc`、或 `settings.json` 的 `tools.sandbox`（`docs/cli/sandbox.md:34-79`）。
- 五条机制（同文件）：
  1. **macOS Seatbelt**（`sandbox-exec`），六个内建 profile 由 `SEATBELT_PROFILE` 选；默认 `permissive-open` = 「denies operations by default; confines writes to the project directory while allowing broad file reads and network access」，其余五档在「网络允许/走代理」×「写限/严格」之间组合（`:86-101`）；
  2. **Docker/Podman 容器**，默认镜像 `ghcr.io/google/gemini-cli:latest`，把宿主 cwd **按同一绝对路径**挂进容器（`:103-119`）；
  3. **Windows native sandbox**：用 `icacls` 给要写的文件/目录打 **Low Mandatory Level**，文档明确警告这个完整性级别的改动**在文件系统上是持久的**，会话结束后仍然保留，需要手动 `icacls ... /setintegritylevel Medium` 复位（`:181-197`）；
  4. **gVisor / runsc**（Linux）：`docker run --runtime=runsc ...`，不自带探测、必须显式指定（`:199-221`）；
  5. **LXC/LXD**（Linux，实验性）：容器要先由用户建好，Gemini 不会自动创建（`:223-257`）。
- 另一条与沙箱正交的机制是 **Trusted Folders**（`security.folderTrust`，**默认关闭**）：信任决定是否加载项目级配置；不可信目录会进「safe mode」——不读项目 `settings.json`、不读 `.env`、不能装卸扩展、**任何工具都必须先问**（即使全局开了自动接受）、不自动加载记忆、不连 MCP、不加载自定义命令；CI 里没有交互界面时会直接抛 `FatalUntrustedWorkspaceError` 退出，除非用 `--skip-trust` 或 `GEMINI_CLI_TRUST_WORKSPACE=true`（`docs/cli/trusted-folders.md:8-23`、`:72-118`）。
- **一手未验证**：Gemini CLI 的 approval mode 词表（`default`/`auto_edit`/`yolo`）与沙箱开关在默认配置里的确切默认值，我没有读到官方一手页面（抓 `docs/cli/configuration.md` 得到 404），因此不写具体值。

### OpenHands（`All-Hands-AI/OpenHands`）

来源：`docs.openhands.dev`（2026-09-30 抓取）。

- V1 的术语是 **sandbox**（不是 runtime），有三种 provider：**Docker sandbox（推荐）**——agent server 跑在 Docker 容器里，「good isolation from your host machine」；**Process sandbox（unsafe, but fast）**——就是个普通进程，「No container isolation」；**Remote sandbox**——跑在远端（[Overview](https://docs.openhands.dev/openhands/usage/sandboxes/overview)，`:14-28`）。选择器是遗留的 `RUNTIME` 环境变量，`RUNTIME=docker` 是默认（同页 `:30-37`）。
- SDK 层的形状是「容器即工作区」：`DockerWorkspace` 用 context manager 管容器的拉镜像/启动/就绪/清理，`DockerDevWorkspace` 用于现场构建镜像；容器内是完整的 agent server，不是「只沙箱化 shell」（[Docker Sandbox](https://docs.openhands.dev/sdk/guides/agent-server/docker-sandbox)，`:15-19`、`:29-51`、`:101-118`）。
- 「越界审批」在 OpenHands 里是**动作确认**，不是路径边界：`confirmation policy` 有 `AlwaysConfirm` / `NeverConfirm` / `ConfirmRisky`（后者需要 security analyzer），被拒时可以给 agent 一段反馈让它换做法（[Security & Action Confirmation](https://docs.openhands.dev/sdk/guides/security)，`:18-65`）。另有一个 `security analyzer`（LLM 判定 / 自定义 / defense-in-depth 组合）给动作打风险级，推荐的确认基线是 HIGH（同页 `:248` 起、“Defense-in-Depth Security Analyzer” 一节 `:515-589`）。
- 官方文档里 Docker provider 的定位是「整台容器」（complete isolation from the host），没有「工作区外只读、越界问一次」这种按路径的语义。

### Aider（`Aider-AI/aider`）

- **一手取证结果：找不到沙箱机制。** 官方 README（`raw.githubusercontent.com/Aider-AI/aider/main/README.md`，12406 字节，2026-09-30 抓取）里 grep 不到 `sandbox` / `docker` / `container` / `isolat` 任何一个词；CLI 参数定义 `aider/args.py`（30298 字节）里 grep 不到 `sandbox`，唯一命中 `--no-` 的是 `--no-verify`（git pre-commit hooks）。
- 官方另有 Docker 文档页面（`aider.chat/docs/install/docker.html`，抓取 200），但那讲的是**在容器里运行 aider 这个程序**，不是 aider 自己给命令加边界。**表述限定**：这是「README 与 args.py 里没有」，不是「一定没有」。

### Pi（`earendil-works/pi`）

- **核心不带沙箱，而且「不做」是它写出来的卖点。** 主页的「What we didn't build」清单里有一条：`No permission popups` —— `Run in a container, or build your own confirmation flow with extensions`；`sandboxing` 则出现在「Primitives, not features」那一串里，与 sub-agents、plan mode、path protection、SSH execution 并列，指向仓库内的扩展示例 `examples/extensions/sandbox/`（[pi.dev](https://pi.dev/)，2026-10-01 抓取）。它的自我定位是 `Pi is a minimal agent harness.`，扩展是 TypeScript 模块、可打包成 npm 包（同页）。
- **沙箱在它的生态里全部是第三方的。** `pi-os-guard` 自述 `OS-sandboxed read-only / write-restricted execution for pi (bubblewrap on Linux, sandbox-exec on macOS)`（[Oxel40/pi-os-guard](https://github.com/Oxel40/pi-os-guard)，2026-10-01 检索）；npm 上另有 `@jasonish/pi-sandbox`、`pi-better-sandbox`、`@erichll/pi-sandbox` 等同类包；外部还有容器式方案 [navikt/cplt](https://github.com/navikt/cplt)，自述把 `Copilot CLI, Claude Code, OpenCode, Gemini CLI, Antigravity, Pi, goose or a plain shell` 装进 `a kernel-level sandbox`。
- **与 Aider 同侧，但理由不同**：Aider 是「没有这个机制」，Pi 是「**故意不内置**，交给你选的外部环境或扩展」。两者都不提供越界审批 —— 这反过来说明「沙箱 + 越界审批」不是 harness 的必需品，而是一个产品取向。

### Goose（`aaif-goose/goose`，现属 AAIF）

- 这条是一个**踩坑型先例**：v1.25.0（2026-02-23）加了 macOS seatbelt 沙箱，opt-in 环境变量 `GOOSE_SANDBOX=true`，能力包括文件系统限制、网络可见性、零开销、对任意 MCP 工具生效（[goose v1.25.0: Sandboxed, Streamlined, and More Secure](https://goose-docs.ai/blog/2026/02/23/goose-v1-25-0)，小节 “🔒 macOS Sandboxing”）。
- 同一篇 blog 顶部现在挂着 **Outdated** 声明（原文）：「The macOS seatbelt sandbox described in this section was experimental and has been removed. The `goose` server process (which executes tools) runs with the same permissions as your user account and is **not** sandboxed at the OS level. For security controls, see `GOOSE_MODE` (`approve`, `smart_approve`)...」（同页）。
- 也就是说：**Goose 做过 OS 级沙箱又撤掉了**，今天的官方口径是「进程无 OS 级沙箱，靠 `GOOSE_MODE` 的工具确认」。工具权限的现行文档在 `documentation/docs/guides/managing-tools/tool-permissions.md`（`aaif-goose/goose` main，5931 字节，2026-09-30 抓取）。

### Cursor

来源：[Run Modes](https://cursor.com/docs/agent/security/run-modes)、[Agent Security](https://cursor.com/docs/agent/security)、[Terminal](https://cursor.com/docs/agent/tools/terminal)（2026-09-30 抓取）。

- 三层叠加：**Run Modes**（Auto-review / Allowlist / …，决定哪些调用不问就跑）+ **沙箱**（对 shell 命令，决定它在哪跑）+ **classifier**（Auto-review 模式的审查者）。Cursor 自己写明「Sandboxing is a layer on top of Run Modes for shell commands. It controls where a supported terminal command runs, not whether the mode uses the Auto-review classifier.」（Run Modes，`:25`）以及 **「Auto-review is not a security boundary」**（`:29`）。
- **默认终端命令需要审批**；文件读取与代码搜索不需要；**工作区文件可以在不审批的情况下被改**（配置文件例外），改动立即落盘（Agent Security，`:11-17`）。默认设置下 agent 不能发任意网络请求，工具只允许 GitHub / 直接链接抓取 / web search provider（`:23-31`）。
- 沙箱配置是 `sandbox.json`：`~/.cursor/sandbox.json`（整机）与 `<project>/.cursor/sandbox.json`（单项目，可提交）；控制网络策略、额外可读/可写路径、临时目录写、共享构建缓存（Run Modes，`:83-107`）。默认：**网络先全部挡住**，再由 network mode 与 `sandbox.json` 打开；`/tmp` 与平台临时目录默认可写（`:87-92`）。
- 平台实现：**macOS 用 Seatbelt（`sandbox-exec`），profile 限制整棵子进程树的文件/网络/进程行为**；**Linux 用 Landlock + seccomp**（Landlock 管文件、seccomp 挡不安全 syscall），要求 **kernel 6.2+ 且 Landlock v3 支持（`CONFIG_SECURITY_LANDLOCK=y`）+ 打开 unprivileged user namespaces**；不满足要求时「Cursor falls back to **asking for approval** before running commands」（Run Modes，`:109-129`）。环境变量 `CURSOR_SANDBOX_LANDLOCK_STATUS` 会报告生效后端是 `fully_enforced`（Landlock）还是 `bubblewrap`（fallback）（`:159-162`）。
- 两个细节值得抄进坑列表：**Linux 沙箱把人 remap 成 namespace 内的 UID 0**，于是 `id -u` / `$UID` 不再是你，Cursor 让脚本改用 `CURSOR_ORIG_UID` / `CURSOR_ORIG_GID`（`:164-183`）；**AppArmor**：桌面版自带 profile，远端环境与独立 CLI 不带，所以官方单独发 `cursor-sandbox-apparmor_0.6.0` 的 deb/rpm（`:131-151`）。
- 网络三档：`sandbox.json Only`（只用你的白名单）/ `sandbox.json + Defaults`（加 Cursor 的内建默认，**这是默认档**）/ `Allow All`（`:185-195`）。
- 越界形状：**进不了沙箱的命令会在沙箱外跑，Cursor 会指示这一点并要你审批**（`:94`）。Cloud Agents 不使用 Run Modes（`:321`）。

### Devin（Cognition）

来源：[Devin environment setup](https://docs.devin.ai/onboard-devin/environment) 与 [Sandbox](https://docs.devin.ai/cli/sandbox)（2026-09-30 抓取）。

- 产品形态：Devin 的环境是**一台 Linux 虚机**，预装 repo/工具/依赖/凭据；配置冻结成 **snapshot**，每个 session 从 snapshot 启动，**session 的改动不会写回 snapshot**（environment setup，`:9-31`）。这属于「整台机器给你」的边界，与「工作区可写、区外只读」不是一回事。
- CLI 有一个显式的 `--sandbox`：写路径由**授予的 `Write(...)` 权限范围 + workspace 目录**推出，其余全部只读；可读路径是除 `Read(...)` deny 规则覆盖之外的一切，而被 `Read(...)` deny 的路径**整个藏在沙箱外**；会话中途新授予的 `Write(...)` 会**动态扩大**后续命令的沙箱（Sandbox，`:9-17`）。
- **fail closed**：沙箱解析失败（例如平台没有相关工具）时 **CLI 拒绝启动**，而不是无沙箱地跑；无论 `--sandbox` 是团队强制还是用户自带都如此（Sandbox，`:19-20`）。已知失败原因：Windows **根本不支持** OS 级沙箱（传 `--sandbox` 或者在 IDE 里被强制时会 hard-fail）；Linux 需要 `bubblewrap`（`bwrap`）与 `socat`，缺了会带着安装说明 hard-fail（`:22-27`）。
- 网络过滤**目前不稳定**（原文「Sandbox network filtering is currently unstable」）；配置在 `sandbox` 段（仅用户级配置），有 `allowed_domains` / `denied_domains` / `network_mode`（`"full"` 允许所有 HTTP 方法，`"limited"` 只允许 GET/HEAD/OPTIONS），deny 优先于 allow（Sandbox，`:29-49`）。
- **越界审批的形状是规则表**：`sandbox.excluded` 有 `allow`（匹配的命令自动在沙箱外跑）/ `ask`（匹配的命令要你批一次才能出去）/ `deny`（永不出去），规则语法是 `Exec(...)`；解析规则是「同一来源里更具体的匹配胜出，跨来源时更严格的裁决胜出（`deny` > `ask` > `allow`）」；**没匹配到规则的命令一律留在沙箱里**，解析不了也是 fail closed（Sandbox，`:72-102`）。走的持久 PTY shell 的命令**永远留在沙箱内**（`:101`）。
- 企业侧有 `Optional`（默认）/ `Required` 两档强制，`Required` 会替所有用户打开 `--sandbox`；文档警告 Windows 用户会在 `Required` 下**完全用不了 CLI**（Sandbox，`:104-119`）。

### 没能确认的

- **Cline / Sourcegraph Amp / Codebuff**：没查（时间与上下文预算），不在本文件里下任何断言。
- **Gemini CLI 的 approval mode 细节**（见上）。
- **OpenHands 的 `sysbox` 与 docker-in-sandbox**：文档索引里有这两页（`enterprise/k8s-install/sysbox.md`、`enterprise/docker-in-sandbox.md`），我没有读，不写结论。

---

## ⑤ 沙箱即服务（microVM / VM 路线）

这一节回答的是「这些产品的边界画在哪里、由谁判、什么时候问人」。

### E2B

- 「Every sandbox runs in its own Firecracker microVM, so code from one customer cannot read or reach another's.」（[Security and compliance](https://e2b.dev/security)，`:11`）
- 「Every E2B sandbox runs in its own Firecracker microVM with its own kernel. Isolation is at the hypervisor boundary, not the container or process boundary... Sandboxes never share a kernel, a filesystem, or memory with another customer's sandboxes.」（同页，`:40`）
- 文档索引里的定位是「isolated machines」：agent 需要「an isolated Linux computer to execute generated code, run shell commands, work with files, install packages, access the internet」（[E2B llms.txt](https://e2b.dev/llms.txt)，开篇）。

### Modal

- `modal.Sandbox` 有两种 runtime：**gVisor**（Google 的容器 runtime，「provides strong isolation and is suitable for most workloads」）与 **VM**（「run the Sandbox in a virtual machine with its own Linux kernel」，用于需要完整 Linux 环境、在沙箱里跑 Docker、挂 FUSE、嵌套 cgroup 的场景；嵌套虚拟化要 Team/Enterprise）——由 `runtime` 参数选，不设就由 Modal 挑（[Sandboxes](https://modal.com/docs/guide/sandboxes)，小节 “Runtimes”，`:683-699`）。
- 网络是**创建时静态策略**：`block_network=True` 丢掉所有出站；`outbound_cidr_allowlist` 只放行列出的 CIDR；`outbound_domain_allowlist` 按域名；两个 allowlist 可以叠加；`block_network` 打开时不能再配 allowlist（[Sandbox networking](https://modal.com/docs/guide/sandbox-networking)，`:13-17`、`:30-39`、`:68-69`）。

### Daytona

- 官方文档描述可选 **nested virtualization / KVM sandbox**：「Daytona provides nested virtualization for Linux VM sandboxes. Nested virtualization exposes KVM (`/dev/kvm`) inside the guest, so the sandbox can run its own virtual machines with hardware acceleration.」（[Sandboxes](https://www.daytona.io/docs/en/sandboxes)，`:1217`）；KVM sandbox 的 fork 会继承 `kvm` 设置，且需要组织开通，否则返回 `403`（同页 `:2913`）。

### Vercel Sandbox

- 「Run untrusted or agent-generated code in isolated Linux microVMs.」（[Sandbox](https://vercel.com/docs/sandbox)，`:24`）
- 「Each sandbox runs in a secure Firecracker microVM with its own filesystem and network.」（同页，`:108`）

### Cloudflare Sandbox SDK

- 「Isolated execution on Cloudflare using Containers and Dynamic Workers」；环境二选一：Linux VM（Containers）或 Dynamic Workers；网络与凭据由 Worker 侧决定——「Add credentials in your Worker so the sandbox never holds them, and decide which services code in a sandbox can reach.」（[Sandboxes](https://developers.cloudflare.com/sandbox/llms.txt)，索引与 “Credentials and network access” 条目）。

### 这一节的事实性小结

五家的官方文档都把隔离描述成**创建时给定的一台机器/一个 VM**（有自己的内核或自己的容器边界），而不是「运行中对某个越界动作做一次审批」。**我没有在任何一家的官方文档里读到「agent 越界时向人申请一次例外」这种交互**；与之最接近的是「创建时把域名/网络策略定死」（Modal 的 allowlist、Cloudflare 的 Worker 侧凭据与可达服务）。**这是「我读到的范围」的结论，不是「这些产品一定没有」**。

---

## ⑥ 横向对比表

| 产品 | 平台 | 机制 | 无特权要求 | 越界后的审批形状 | 已知限制（有来源的） |
| --- | --- | --- | --- | --- | --- |
| **Codex CLI** `0.159.2` / `a5cce889` | macOS / Linux / Windows | macOS `sandbox-exec`+SBPL；Linux **bubblewrap（系统 bwrap，缺失则随包 bundled）+ 进程内 seccomp + no_new_privs**；Windows 受限令牌或 MXC | Linux 优先无特权 bwrap；WSL1 被拒 | 工具参数 `sandbox_permissions: require_escalated` + `justification`（`with_additional_permissions` 是中间档），编排器「被拒后用升级策略重试一次」；审批可改送 `auto_review` | 三档 sandbox（`read-only` 默认 / `workspace-write` / `danger-full-access`）；`workspace-write` 默认**关网**；Landlock 只剩 legacy；Windows MXC 要 PSEC |
| **Claude Code**（内建 Bash 沙箱） | macOS / Linux / WSL2（**不支持 native Windows / WSL1**） | macOS Seatbelt；Linux `bubblewrap`（+`socat`、可选 seccomp 过滤 Unix socket） | 需要能建 user namespace（Ubuntu 24.04 要 AppArmor profile 或 sysctl） | 沙箱把违规报回结果里，Claude 可带 `dangerouslyDisableSandbox` 重试 → 走常规权限流并提示你；`allowUnsandboxedCommands:false` 关掉这条门 | 默认**读整机**（自己承认仍能读 `~/.ssh`、`~/.aws/credentials`）；网络只看域名、不解 TLS（domain fronting）；`allowUnixSockets` 可导致逃逸；容器里要 `enableWeakerNestedSandbox` |
| **sandbox-runtime（`srt`）** `0.0.78` | macOS / Linux / WSL2 / Windows（alpha） | macOS `sandbox-exec`+SBPL；Linux bwrap + 网络命名空间 + 宿主 UDS 代理；Windows `srt-sandbox` 账户 + WFP + ACE | 同上；root 调用者还需 `CAP_SETFCAP` | 没有内建审批：它是「包住整个进程」的库/CLI，审批由外层（Claude Code）做 | 读默认全允许（deny-then-allow）、写默认全拒；Linux 写授权只对已存在路径、deny 列表启动时构建一次；Linux 代理靠环境变量、不遵守的程序直接断网；`allowAppleEvents` 会移除代码执行隔离 |
| **Gemini CLI** | macOS / Linux / Windows | 五选一：Seatbelt（6 个 profile）/ Docker·Podman 容器 / Windows `icacls` Low integrity / gVisor·runsc / LXC·LXD | 容器方案要 Docker/Podman；LXC 要预先建容器 | 没有「路径越界审批」；有 Trusted Folders（不可信目录 → 任何工具都要问）与工具确认 | 沙箱是显式打开的开关（文档未写默认已开）；Windows 的 Low integrity **持久留在文件系统上**；LXC 是 experimental |
| **OpenHands** | 宿主平台 + Docker | **容器**（Docker sandbox 推荐 / Process 无隔离 / Remote） | 要 Docker（Process 方案不需要，但没有隔离） | `confirmation policy`：`AlwaysConfirm` / `NeverConfirm` / `ConfirmRisky`（+ security analyzer 打风险级） | Process sandbox 官方自陈「unsafe」；确认是**动作级**而非路径级 |
| **Aider** | — | **无沙箱**（README 与 `args.py` 里没有） | — | 无（没有内建审批模型） | 官方有「在容器里跑 aider」的文档，但那是用户自备的边界 |
| **Pi** | — | **无内置沙箱**（官方把 sandboxing 列为「自己用扩展建」） | — | 无；官方口径是「跑在容器里，或自己搭确认流程」 | 生态里全是第三方：[`pi-os-guard`](https://github.com/Oxel40/pi-os-guard)（bwrap / `sandbox-exec`）、`@jasonish/pi-sandbox`、容器式的 [`navikt/cplt`](https://github.com/navikt/cplt) |
| **Goose** | 曾 macOS | **已移除**的实验性 Seatbelt 沙箱（曾 `GOOSE_SANDBOX=true`） | — | 现行是 `GOOSE_MODE`（`approve`/`smart_approve`）的工具确认 | 官方声明：server 进程与用户同权限、**不在 OS 级沙箱里** |
| **Cursor** | macOS / Linux（桌面；CLI/远端另说） | macOS Seatbelt（`sandbox-exec`）；Linux **Landlock + seccomp**（`CURSOR_SANDBOX_LANDLOCK_STATUS` 报告 `fully_enforced` 或 `bubblewrap` 回退） | 要 **kernel 6.2+ / Landlock v3** 与 unprivileged userns；桌面自带 AppArmor profile，CLI/远端要装 `cursor-sandbox-apparmor_0.6.0` | 进不了沙箱的命令在沙箱外跑并**要你审批**；Auto-review 模式下先给 classifier | 官方声明「Auto-review is not a security boundary」；工作区文件改**不需要审批**；Linux 沙箱内 UID 被 remap 成 0（要用 `CURSOR_ORIG_UID`） |
| **Devin** | Linux VM / CLI | 环境是**一台 Linux 虚机 + snapshot**；CLI `--sandbox` 用 OS 级隔离（Linux 要 `bwrap`+`socat`） | 需要 bwrap/socat；**Windows 完全不支持** | `sandbox.excluded` 的 `Exec(...)` 规则表：`allow`（自动出沙箱）/`ask`（问一次）/`deny`（永不）；未匹配一律留沙箱 | fail closed（解析失败拒绝启动）；网络过滤**目前不稳定**；PTY shell 里的命令永远留沙箱内 |
| **E2B / Modal / Daytona / Vercel / Cloudflare** | 云端 | Firecracker microVM（E2B、Vercel）、gVisor 或 VM（Modal）、KVM 嵌套虚拟化可选（Daytona）、Containers/Dynamic Workers（Cloudflare） | 无（云端服务） | **没有**「运行中越界审批」；边界在创建时定（网络策略、可挂载的文件系统） | 各家的网络/文件系统策略都是创建参数；沙箱即代码执行环境，不是「工作区外的审批门」 |

---

## ⑦ 共同踩过的坑

每条都附来源；「推论」另行标注。

1. **Ubuntu 24.04+ 的 AppArmor 默认策略会掐掉 unprivileged user namespace 的能力。** Claude Code 文档：「On Ubuntu 24.04 and later, the default AppArmor policy prevents bubblewrap from creating the user namespaces it needs for isolation.」，解法是给 `bwrap` 加 profile（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Ubuntu 24.04 and later…”）。srt README 说得更狠：那个 sysctl「allows `unshare(CLONE_NEWUSER)` but **strips capabilities** from the resulting namespace」，而且**不只是 bubblewrap**——「Both bubblewrap **and the seccomp isolation layer** need capability-bearing user namespaces」（`sandbox-runtime@6f0ce155:README.md:562-568`）。Cursor 干脆发了一个 AppArmor 包来对付同一件事（[Run Modes](https://cursor.com/docs/agent/security/run-modes)，`:131-151`）。Codex 的应对是**启动时警告**「bubblewrap cannot create user namespaces」（`codex-rs/linux-sandbox/README.md:35-36`）。
2. **在容器里跑容器内的沙箱**：Claude Code 的 `enableWeakerNestedSandbox`，理由是「in an unprivileged container, bubblewrap can't mount a fresh `/proc` filesystem」，报错形如 `Can't mount proc on /newroot/proc: Operation not permitted`；官方强调这个开关**只在外面已经有一层隔离时**才能用，因为它把宿主进程信息暴露给沙箱内（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Troubleshooting”）。srt README 同一条并写明「considerably weakens security」（`README.md:891`）。Codex 用另一个办法绕：`--proc /proc` 被拒时保留继承来的 `/proc` 但仍建 PID namespace，代价是沙箱内的 PID 可能与 `/proc` 暴露的不一致（`codex-rs/linux-sandbox/README.md:90-95`）。
3. **Landlock 有内核门槛，且要求编译期开启。** 内核官方文档：「Landlock was first introduced in Linux 5.13 but it must be configured at build」（[Landlock](https://docs.kernel.org/userspace-api/landlock.html)）。Cursor 把它写成硬要求：**kernel 6.2 或更高、Landlock v3、`CONFIG_SECURITY_LANDLOCK=y`**，不满足就退回「命令前问审批」（[Run Modes](https://cursor.com/docs/agent/security/run-modes)，`:124-129`）。Codex 的 landlock 代码声明的是 `ABI::V5`（`codex-rs/linux-sandbox/src/landlock.rs:150`），且它已经不用了（`:145-146`）。
4. **Landlock 的能力不够时会成为「假边界」。** Codex README 的原话：「Filesystem-restricted execution requires bubblewrap. The legacy Landlock option is rejected for these policies **because it cannot isolate app-server Unix sockets**.」（`codex-rs/linux-sandbox/README.md:40-42`）——即「把文件系统挡住」不等于「把逃逸通道挡住」。
5. **`sandbox-exec` 是 macOS 的私有/不再演进的东西。** 这条我**一手未验证**：Apple 官方的 deprecation 声明我没有拿到（`apple-oss-distributions/sandbox` 的路径猜测 404）；能拿到的线索是社区 issue「sandbox-exec was deprecated on MacOS few years ago」（[openai/codex#215](https://github.com/openai/codex/issues/215)）与 man page 镜像（[FreeBSD man server 的 macOS 10.13.6 man page](https://man.freebsd.org/cgi/man.cgi?query=sandbox-exec&sektion=1&manpath=macOS+10.13.6)）。**事实层面确定的是**：Codex、Claude Code/srt、Cursor、Gemini 四家的 macOS 路径**全部**依赖 `/usr/bin/sandbox-exec`（前文各有出处），DSH 的文档也自称依赖「已废弃的私有策略引擎」（转引自 [`01`](01-dsh-workspace-permissions-and-shell.md) §④）。
6. **文件隔离与网络隔离必须同时存在，缺一个就漏。** srt README：「Both filesystem and network isolation are required for effective sandboxing. Without file isolation, a compromised process could exfiltrate SSH keys... Without network isolation, a process could escape the sandbox and gain unrestricted network access.」（`README.md:113-116`）Claude Code 文档用一整段 Warning 说同一件事（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Scope”）。
7. **代理式网络隔离有两个已知弱点**：默认**不检查 TLS**，允许宽域名就能被 domain fronting 绕过（Claude Code 文档 “Security limitations”；srt README `:879`、`:886`）；Linux 上靠 `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` 环境变量转发，**不遵守这些变量的程序会直接连不上网**，README 把 proxychains + `LD_PRELOAD` 列为未来的加固方向（srt README `:897-901`）。
8. **放行一个 unix socket 可能等于放行整个宿主。** srt README 点名 `/var/run/docker.sock`（`README.md:889`）；Claude Code 文档同一条并补一句「allowing access to `/var/run/docker.sock` effectively grants access to the host system through the Docker socket」（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Security limitations”）。Codex 为此专门写了一个 seccomp 模式挡 VM socket，理由是 WSL2 里它能通过 interop socket 启动 Windows 进程（`codex-rs/linux-sandbox/src/landlock.rs:58-64`）。
9. **macOS 的兼容坑是长尾，且每个「兼容开关」都在削弱隔离。** Claude Code 列了：Go 写的 CLI（`gh`/`gcloud`/`terraform`）TLS 校验失败 → `enableWeakerNetworkIsolation`（打开等于开一条经 trustd 的外泄通道）；`open`/`osascript` 失败 → `allowAppleEvents`（打开等于**移除代码执行隔离**）；`watchman` 与沙箱不兼容 → `jest --no-watchman`（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，小节 “Troubleshooting” 与 “Security limitations”；srt README `:892-893`）。
10. **「允许一个域名」不等于「限制在这个域的某个能力」。** srt README 原话：允许 `github.com` 就等于允许进程 push 到任意 repository（`README.md:879`）。
11. **有些工具就是进不了沙箱，所有人都得留豁免通道。** Claude Code 的 `excludedCommands`（`docker *`、Go CLI）；Devin 的 `sandbox.excluded` `allow`/`ask`/`deny`；Codex 的 `require_escalated`。**共同点**：豁免不是「默认全开」，而是**显式列出 + 越界时问一次**；哪家的默认都是 fail closed。
12. **沙箱不可用时的取向是「宁可拒绝」而不是「静默降级」。** Devin：「the CLI will **refuse to start** rather than running unsandboxed」（[Sandbox](https://docs.devin.ai/cli/sandbox)，`:19-20`）；Claude Code 有 `failIfUnavailable`（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，`:640`）；Codex 的三级兜底也都有显式提示（`codex-rs/linux-sandbox/README.md:10-39`）。**这与 DSH 的 `SANDBOX_UNAVAILABLE` 同一取向**（转引自 [`01`](01-dsh-workspace-permissions-and-shell.md) §⑤）。
13. **沙箱做出来了也可以撤掉。** Goose 的 seatbelt 沙箱从 v1.25.0 的实验特性到被移除，官方现在的口径是「进程与用户同权限、不在 OS 级沙箱里」（[goose v1.25.0 blog](https://goose-docs.ai/blog/2026/02/23/goose-v1-25-0)，顶部 Outdated 声明）。**推论**：OS 级沙箱的维护成本（兼容矩阵 + 每个新工具的破例）是它容易被砍掉的原因之一，但 blog 没有给撤销理由，这一点属于我的推断。
14. **Windows 至今是「每家都不一样」的一侧**：Claude Code 不支持 native Windows（只 WSL2，[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，`:723`）；Devin 在 Windows 上 hard-fail 且在 `Required` 策略下用户**完全用不了 CLI**（[Sandbox](https://docs.devin.ai/cli/sandbox)，`:24`、`:117-119`）；Gemini 用 `icacls` Low integrity 且**持久残留**（[Sandboxing in Gemini CLI](https://www.geminicli.com/docs/cli/sandbox)，`:181-197`）；Codex 有受限令牌与 MXC 两条后端（`codex-rs/mxc-sandbox/README.md`）。
15. **以 root + 无审批运行的组合会被显式拒绝。** Claude Code：`--dangerously-skip-permissions` 在 root 或 sudo 下被拦，理由是「root access combined with no permission prompts can modify any file or service」，在识别出的沙箱里会自动跳过这个检查（[Configure the sandboxed Bash tool](https://code.claude.com/docs/en/sandboxing)，`:702`）。

---

## ⑧ 边界：这份文件回答不了什么

- **不是穷举。** Codex 我只读了 `codex-rs/linux-sandbox`、`codex-rs/sandboxing`、`codex-rs/protocol` 的关键文件与 `core/src/tools` 的沙箱/审批路径；`execpolicy`、`guardian/policy.md` 的判定规则、`windows-sandbox-rs` 的实现细节都没读。Claude Code 我只读了 `sandboxing` 与 `sandbox-environments` 两个文档页（另抓了 `permission-modes`、`permissions` 两页但没读）与 srt 的 README + 文件清单，`src/sandbox/linux-sandbox-utils.ts` 的逐行实现没读。
- **没有实测。** 本文件全部来自源码与官方文档阅读；没有在本机跑过任何一条沙箱命令，也没验证过任何「bwrap 在这台机器上能不能起来」。
- **「越界审批」的形状我只追到了 schema/文档层**：Codex 的 `require_escalated` 我没追到 TUI 里那次批准的最终落盘格式；Claude Code 的 `dangerouslyDisableSandbox` 我没读实现源码。
- **`sandbox-exec` 的官方废弃声明缺失**（见 ⑦ 第 5 条），只能给社区线索，标注为一手未验证。
- **Cline / Amp / Codebuff / 其它 harness 未覆盖**；Gemini 的 approval mode 词表、OpenHands 的 sysbox/docker-in-sandbox 两页也没读。
- **版本会漂。** Codex 处在快速迭代（同一天有 `0.159.2` 与 `0.161.0-alpha.3`），本文件钉的是 `a5cce889`；Claude Code 文档自己提到多个 `v2.1.x` 版本点，行为在这些版本之间变过（例如 plan mode 下 bare `Bash` ask 规则是否被跳过，见 “Sandbox modes” 最后一条）。**引用本文件时请连同版本一起引用。**
- **DSH 的部分不在本文件。** 任何「DSH 怎么做的」问题去看 [`01`](01-dsh-workspace-permissions-and-shell.md)。
