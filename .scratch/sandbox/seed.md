# 种子材料：进程级沙箱（让「工作区内自动 / 区外要审批」对 shell 也成立）

> **这不是 spec，也不是票。** 它是 `/grill-with-docs` 之前的路由判断与待谈分叉，
> 外加这次调研拿到的一手结论。规格以将来那轮 grilling 的产物为准。
>
> **写就于 2026-09-26。** 下文凡说「**现状**」，指的都是**那一天的代码**（时间词在这里会烂，所以定死一次）。

## 原始请求与它的转向

用户先要的是**一个权限模式**：`workspace`——项目目录（会话 cwd）内一律允许、目录之外需要审批，对所有角色（主会话 / 讨论者 / 执行者）生效。

问到 `bash` 这一格时（它是唯一无法静态判定会不会写到工作区外的工具：`src/tools/bash.rs:75-79` 的 `effect()` 恒为 `Exclusive`，注释写着「shell 能写任何东西」），用户要求**先查 DSH 怎么做的**。调研结论（见 [`research/01-dsh-workspace-permissions-and-shell.md`](research/01-dsh-workspace-permissions-and-shell.md)，85 处一手引用）：

- DSH **没有**「路径前缀权限」这回事。它的墙是**内核**：三种 file effect `read-only` / `workspace-write` / `danger-full-access`，「工作区」= 会话创建时写死的 `SessionHeader.cwd`，`workspace-write` = **整机只读 + 工作区根可写**（只限写、不限读）。
- **shell 完全不解析命令**：整条 `['bash','-c',command]` 交给 `ctx.sandbox.confine()` 包一层 runner 直接 spawn；41 个包里没有任何 allowlist / denylist / 命令启发式。
- 越界**不是事先问出来的**：内核先拒绝，模型同一条命令原样重试 + 一句 `justification`，才触发一次审批（`allowed-once`，**没有** allow-always）；沙箱不可用则 **fail closed**（`SANDBOX_UNAVAILABLE`，不降级成问）；派子 agent 时把审批通道钉成 `never`，所以「子比父宽」在机制上不可达。

**转向**：用户据此决定 —— **`workspace` 模式先不做，先记下「做进程级沙箱」的意向**；具体怎么做（机制、平台、读写的边界、审批形状）后续单独商量。本文件就是那份意向。

**依赖关系**：`workspace` 模式的 shell 那一半**阻塞在沙箱上** —— 没有内核给的拒绝信号，「区内自动、区外问」在 shell 上没有可执行的判据（只有「一律问」或「一律放行」两端，见调研文件 §⑦）。

## 已拍但暂缓：`workspace` 模式那一版的决议

这些是用户已经拍过的，**不要丢**；将来做 `workspace` 模式时直接拿来用（要么作为本 effort 的后续票，要么单独开一个 `.scratch/workspace-mode/`）：

| 议题 | 决议 |
| --- | --- |
| 模式名 | `workspace`，中文标签「工作区」（状态行 `模式 工作区`） |
| 怎么选到它 | `config.toml` 的 `[permissions] mode = "workspace"` 作默认 + `--mode workspace` 旗标（现状：模式是硬编码 `Ask`，三处组装点 `src/cli.rs:369/663/1557`，没有 `--mode`）；运行期不加切换手势（`Shift+Tab` 仍归 plan） |
| 工作区内的操作 | 一律允许（现状：项目**内**的 `.env` 家族 / `.git` / `.ssh` / shell rc 文件 / 断路器短路**照旧硬拒**，不放宽） |
| 工作区外的操作 | **读与写都要问**（现状：两者都被硬拒 —— `SessionPaths` 在解析期拒掉，且那是规则无法下压的 deny floor，`tests/permission_gate.rs::the_path_limit_is_a_deny_floor`）。注意这与 DSH 不同：那边读是随便的，我们更严 |
| 工作区外的 shell | **未定**，见下 |
| 对所有角色生效 | 结构上已经成立：执行者 `Policy::for_mode(parent_policy.mode())`（`src/agent/executor.rs:101`），讨论者与主会话共用同一份 policy |

**shell 那一格的四个候选**（当时问过、用户选了「先做沙箱」，其余三个留在下面，将来若沙箱不成再回来挑）：

1. **一律问** —— 与 `ask` 档**等同**（`src/permissions.rs:118-127`：`(Ask, 非 ReadOnly) → Ask`），包成新模式只是改名，不带来任何「区外」语义；
2. **一律放行** —— 区内手感等同 `auto`，代价是「区外要审批」对 shell 不成立（`rm -rf ~/x` 不问）；
3. **argv 启发式** —— DSH 里没有任何先例，且 `cd ..; rm -rf x`、`$(cat /etc/passwd)`、写脚本再执行都能绕过；它产出的是「预判」，既不是拒绝也不是允许，还会给人假的「已检查」观感；
4. **先做进程级沙箱**（用户选的那条）—— 见下。

## 意向：做一个进程级沙箱

**要它干什么**：让「工作区内自动、区外要审批」在 **shell 上也成立** —— 越界由内核拦住（而不是我们预判），人只在真要越界时被打断一次。

**已经可以照搬的形状**（这些不依赖内核，调研文件 §⑦ 逐条给了引用）：

- **升级手势**：拒绝 → 同一条命令原样重试 + 一句理由 → 一次审批 → `allowed-once`；被拒即终局；不允许 speculative 放行。
- **失败态的语气**：沙箱不可用就 **fail closed** 并给出可操作出路，不静默放行、不降级成一个看起来在保护的「问」。
- **落盘与观感**：档位与每次审批（asked / decided）都进事件流（只读的 log 事件），靠 replay 恢复；平时只告诉模型「当前档位与它的含义」，不摊能力清单。
- **子 agent 收口**：不是写「子不能比父宽」的检查器，而是把子的某个能力**关掉**（本仓库已有的更强版本：`Allow` 不沿委派链传播）。

**待商量的分叉**（下次 grilling 的起点）：

1. **机制与平台矩阵**：Linux 先做哪条（bubblewrap？Landlock 原生 launcher？）；macOS 要不要（`sandbox-exec`）；Windows 是否明确不做（DSH 那边也只自报 `partial`）。我们 README 现在的立场是「升级路径只做 Linux 的 bubblewrap，且不预做抽象」——这条要不要改，是本 effort 的第一刀。
2. **边界：只限写还是读写都限**：DSH 是整机只读 + 工作区可写。我们的 cwd 限制比它严（读写都限）。沙箱做出来后，「读」要不要一起放开？这直接决定上面那条「读也问」的决议还算不算数。
3. **与现有权限门的关系**：沙箱是**又一层**（断路器 / `.env` 家族 / `.git` / `.ssh` 地板照旧），还是取代其中某几条？「区内自动」是否要求沙箱**先于**权限门生效？
4. **审批那条路**：我们已有 `Asker` / 权限模态这条接缝，DSH 的 upgrade 语义要不要照搬（尤其是「先失败、再升级」对我们现有「未出结果的 tool_call 绝不调 provider」这条不变量的影响）。
5. **不做沙箱时的替代**：如果某平台没有可用机制，是 fail closed（拒绝跑 shell）还是明确告知用户「这一档在这里退化成一律问」。
6. **怎么测**：沙箱的可测部分（参数生成 / 拒绝方言 / fail-closed）与只能在真机验的部分（真内核行为）怎么分层——本仓库的先例是 `TestBackend` + pty 脚本 + 手工清单三层。
7. **范围与流程**：这是一个独立 effort（可能又是一次 wayfinder），还是先 `grill-with-docs` 一个 session 能装下的切片（例如「只做 Linux、只限写、只包 shell」）。

## 一手材料

- [`research/01-dsh-workspace-permissions-and-shell.md`](research/01-dsh-workspace-permissions-and-shell.md) —— DSH 的权限/沙箱/shell 处理，221 行、85 处引用，含**可复制的取包命令**（必须钉 `0.1.5-rc.3`：这些包的 `latest` dist-tag 停在更旧的 `0.0.1-rc.1`）。tarball 与解出的树在 `/tmp/dsh-research`。
- 该文件 §⑦ 是「对我们（无沙箱仓库）意味着什么」，§⑧ 是**读不出来的部分**（例如 Web 端会话的 cwd 从哪来）。

## 与现有文档的关系

- README《安全模型》现在写着「**这不是沙箱**。v1 不做进程级隔离……升级路径写明是『只做 Linux 的 bubblewrap』，且**不预做抽象**」——本 seed **不推翻**它，只是把意向记下来；真要动它，按仓库规矩需要一条 ADR（它会改写一条已经写进文档的边界）。
- `docs/credentials.md` 记着 cwd 限制「是 deny floor、任何规则都松不开」，并说明「安全的放宽需要一种现在还不存在的规则形状」——沙箱正是那条路的一种答案（判据在内核，不在规则代数里）。
