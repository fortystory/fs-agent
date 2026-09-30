# 第四档权限模式 `workspace`：区内自动、区外要问

新增一档权限模式 `workspace`（标签「工作区」）：会话 cwd 之内一律放行，之外的**写**要人点一次头；再给 shell 配一条**升级手势**——命令被内核拒了，模型带理由原样重试一次，人批一条路径，这一次调用就多一条可写根。

它建立在刚落地的 [bubblewrap 沙箱](../sandbox/spec.md) 上，因为 shell 的「区外」判据只能来自内核。决定与理由在 [ADR 0007](../../docs/adr/0007-workspace-permission-mode.md)；种子材料在 [`seed.md`](seed.md)（本文件把它折成构建计划，沙箱那轮寄存的《已拍但暂缓》决议也一并迁进来了）。

来源：2026-10-01 的一轮 grilling（Q1–Q19）。一手引用在 [`../sandbox/research/`](../sandbox/research/)（DSH 的越界升级形状在 [01 §⑦](../sandbox/research/01-dsh-workspace-permissions-and-shell.md)）。

## Problem Statement

**权限模式与沙箱回答的是两个问题，而它们之间没有桥。** 权限门答「跑不跑、要不要问」（[`Mode`](../../src/permissions.rs) 三档），沙箱答「跑起来能碰到什么」。于是：

- `auto` 档下一条写 `~/.npm` 的命令会被内核拒，而用户**没有出口**——唯一的办法是去改 `[sandbox] writable_roots`，然后重启会话。
- `ask` 档下写要问，但问的是「要不要跑这条命令」，不是「要不要放开这条路径」；用户面对一条 shell 命令，没法判断它会不会越界。
- `SessionPaths` 的 cwd 收容是**任何规则都降不下去**的拒绝地板，所以「区内自动、区外问」这一档在文件工具上也无从表达：区外不是「要问」，是「不可能」。

`workspace` 这一档要把这三件事同时解开：给档位一个第四档，给区外读一条显式的旋钮，给 shell 一条被拒之后能走的路。

## Solution

- **档位**：`Mode` 加第四档 `Workspace`，`Shift+Tab` 循环按严格度插在 `ask` 之后。
- **区外读**：默认仍硬拒；`[permissions] outside_read = "deny" | "ask" | "allow"`（缺省 `"deny"`）是唯一的出口，**全局**生效。
- **区外写**：只有 `workspace` 档能问；在这一档下，`SessionPaths` 的路径上限从 `Deny` 变 `Ask`——**选这一档就是那个声明**。
- **shell 与动态工具**：门判不出区内/区外（`Effect::Exclusive` 没有路径集），所以走**升级手势**：先跑，被内核拒，模型带 `escalation = { justification, writable_paths }` 原样重试，弹一次审批，批准后这次调用多一条可写根；只批这一次，一条命令只给一次重试。
- **写死的边界不接受升级**：遮罩目录（`~/.config/fs-agent`、`~/.ssh`）与保护路径（`.env` 家族、`.git/config`、`.git/hooks`）在门里直接拒。
- **没有沙箱就没有这一档**：无可用 `bwrap` 或 `[sandbox] mode = "off"` 时，`workspace` 档不可用（组装期拒绝）。

## User Stories

1. 作为用户，我想让 `workspace` 档下**工作区内的写入与命令一路放行**（包括 `cargo test`、`git commit`），不必为一次构建点十次头。
2. 作为用户，我想让**区外的写**停下来问我一次，并且问的时候我能看见「它要写哪条路径、为什么」，而不是只看见一条我看不懂的命令。
3. 作为用户，我想在**被内核拒绝之后**仍有一个出口（批准一次那条路径），而不是只能去改配置文件、重启会话。
4. 作为用户，我想让 `~/.config/fs-agent` 与 `~/.ssh` **不给任何通道**——包括这条升级通道。
5. 作为用户，我想让区外的**读**默认仍然拒绝（provider key 就在区外），但允许我在配置里显式放开（`deny` / `ask` / `allow` 三选一）。
6. 作为用户，我想让这一档在**没有沙箱的机器上不存在**，而不是给我一个看起来在保护、实际有一个大洞的档位。
7. 作为模型，我想在 `bash` 的描述里读到：被沙箱拒绝是这条命令的结论；要越界就带理由重试一次；不许先绕道去聊天、不许投机性升级。
8. 作为审计者，我想在事件流里看到**每一次升级**（谁问的、问了什么路径、批没批），以便回头核对某次放开是不是我点的。

## Implementation Decisions

### §1 档位与入口

- [`Mode`](../../src/permissions.rs) 加变体 `Workspace`，`as_str()` 是 `"workspace"`，`parse()` 接上它——两个入口（`[permissions] mode` 与 `--mode`）自动跟着走，因为「哪些词是模式」只有那一张表。
- `Mode::next()`：`readonly → ask → workspace → auto → readonly`。顺序是**严格度**，不是叙事顺序：对同一次区内写，`readonly` 拒、`ask` 问、`workspace` 允许、`auto` 允许。
- `Mode::stance()` 对 `workspace` 档：
  - `Effect::ReadOnly` → `Allow`（读这一侧由 §2 的旋钮管）；
  - `Effect::WritePaths` → 所有写目标都落在 `cwd` 之内时 `Allow`，否则 `Ask`；
  - `Effect::Exclusive` → `Allow`（判不出区内区外，判据在沙箱那一侧，见 §4）。
- 状态行与所有模式标签走既有的措辞层：`workspace` 的中文名是**「工作区」**。

### §2 区外读：一个全局旋钮

- `[permissions]` 加一个字段：

  ```toml
  [permissions]
  mode = "workspace"
  outside_read = "deny"        # "deny"（缺省）| "ask" | "allow"
  ```

- 解析成 [`Decision`](../../src/events.rs)（三值同形，`as_str()` 就是 `allow` / `ask` / `deny`），存在 [`Policy`](../../src/permissions.rs) 上；`allow` 与 `deny` 之间的**上确界**合并因此是既有代数的一部分，不新造语义。
- 它管的是「**读**目标落在 `cwd` 之外」的情形，**全局**生效（`readonly` / `ask` / `workspace` / `auto` 都认它）：这条地板是策略级的、与档位正交，`ask` 档配上 `"allow"` 恰好就是 DSH 的「读全放、写要问」。
- 为此 `Call` 的 `path_error`（现在是一个字符串）要带**方向**：`path_error: Option<PathError>`，`PathError { direction: Direction, message: String }`（`Direction = Read | Write`）。`decide()` 的 ④ 段据此分支：读的越界按 `outside_read` 给裁决，写的越界按档位给（§3）。
- 缺省永远是 `deny`：与 `[sandbox] mode = "off"` 同一个立场——**写下来才算放弃**。

### §3 区外写：文件工具事前问

- `workspace` 档下，写目标落在 `cwd` 之外 → `Ask`（一次批准，范围是**这一次调用**，批准后照常跑）。区内 → `Allow`。
- **只有这一档**如此。`ask` 档保持现状（区外写仍被地板拒绝），也不加对称旋钮：`ask` 的意思是「写要问」，不是「区外要问」。
- 因此 `SessionPaths` 的路径上限在 `workspace` 档下不再是地板——它仍然是**另外三档**的地板。钉住它的测试要改成这个说法（`tests/permission_gate.rs` 的 `the_path_limit_is_a_deny_floor`）。
- 区外写被批准时**不**碰沙箱：文件工具不经 shell，沙箱那一层与它无关。

### §4 升级手势：shell 的区外判据

- **只有 `bash` 有这条通道。** 动态工具的 schema 与 argv 模板都是使用者在 `config.toml` 里声明的，插不进新参数；它们的越界就是失败，要放宽就写 `[sandbox] writable_roots`。这不是疏忽，是 [ADR 0007](../../docs/adr/0007-workspace-permission-mode.md) 里的一个决定。
- `bash` 的参数加一个嵌套对象：

  ```json
  {
    "escalation": {
      "justification": "构建产物要写到 ~/.npm 的缓存目录",
      "writable_paths": ["/home/ada/.npm"]
    }
  }
  ```

  `justification` 非空、`writable_paths` 非空，两者**成对**（不给「有理由没路径」的半截状态）。
- 门的处理：看到 `escalation` 且档位不是 `readonly` 时——
  - 声明的路径落在**遮罩目录或保护路径**里 → `Deny`，理由写清「这一条是写死的安全默认，没有任何通道放宽」；
  - 否则 → `Ask`，**一次**。批准 = 这次调用多一条可写根（**声明的那个路径本身**，文件就是文件、目录就是目录，不做父目录提升）；拒绝 = 终局。
- `escalation` 在**各档都有效**（`readonly` 除外，那一档连跑都不让）：它请的是放宽沙箱，而沙箱不因权限模式而关——`auto` 档下写 `~/.npm` 照样被拒，这条出口对它同样必需。
- 每次调用的可写根：`AllowedCall` 加 `sandbox_grants: Vec<PathBuf>`，`Sandbox` 在构造时把这批路径追加进 `SandboxSpec.writable_roots`（它是每次调用现构造的，见 [`agent.rs`](../../src/agent.rs)）。
- **拒绝即终局，且只给一次重试**：同一条命令第二次被拒之后，模型必须换做法或报告受阻。DSH 那套话术进 `bash` 的工具描述：被沙箱拒绝是这条命令的结论；要越界就带理由原样重试一次；不许先用别的方式探路；**没被拒就不许投机性升级**。
- 遮罩与保护路径的判据由 [`tools/sandbox.rs`](../../src/tools/sandbox.rs) 提供一个纯函数（遮罩目录来自 `SandboxSettings.masks`，保护路径由 `cwd` 推出：`.git/config`、`.git/hooks`、存在的 `.env` 家族），门用它做上面第一条分支——两份互相漂离的「写死清单」是不能接受的。

### §5 写死的边界不接受升级

- 遮罩目录：`~/.config/fs-agent`（provider key 在那儿）、`~/.ssh`。
- 保护路径：工作区里的 `.git/config`、`.git/hooks`，以及**存在的** `.env` 家族文件（`.example` / `.sample` / `.template` 除外）。
- 它们在挂载表里排在可写根**之后**，所以就算批准把其中的路径加进可写根，那条 `--tmpfs` 或 `--ro-bind` 也会盖掉它——批准等于白批，而用户会以为自己批准了。门里直接拒，是唯一诚实的做法。

### §6 没有沙箱的地方，这一档不存在

- 判据：沙箱 `mode == "off"`，或者探测结果不可用（`SandboxAvailability::Unavailable`）。
- 时机：**组装期**（[`lib.rs`](../../src/lib.rs) 的组装路径），因为它是唯一同时看得到权限策略与沙箱状态的地方；`cli` 的三个组装点照旧把 `Error` 打到 stderr。
- 出路（文案里给两条）：把 `[sandbox] mode` 换回 `"bwrap"`，或者换一档（`ask` / `auto`）。
- 理由：`workspace` 对 shell 的承诺完全建立在沙箱上；让它「半残」地存在，等于给用户一个看起来在保护、实际有一个大洞的档位。

### §7 审批与可见性

- 升级走既有的权限询问通道（[`Asker`](../../src/permissions.rs)），弹窗在原三行（问题 / 描述 / 原文命令）之上加两行：

  ```
  权限询问：调用 bash（执行者 planner 发起）
    被沙箱拒绝，申请写工作区之外
    理由：构建产物要写到 ~/.npm 的缓存目录
    要放开的路径：/home/ada/.npm
    命令：bash（command=…）
  ```

  说话人**只在发起者不是主会话时**出现（执行者 / 讨论者）——本仓库的 `asker` 是会话级的、会被 fork 继承，这次选择**标明来源**而不是把子会话的通道关掉。命令行保持最后一行：它是授权时唯一必须看得见确切命令的地方。plain 前端同构（逐行）。
- 事件流：复用 `PermissionAsked` / `PermissionDecided`（`reason` 里写明是升级、哪条路径）。不新增事件类型——审计者要看的正是「谁问了、批没批」。
- 界面：状态行照旧显示档位（`工作区`）；沙箱状态已经有它自己的一行（log-only + 转录），这里不重复。

### §8 文档与决定记录

- [ADR 0007](../../docs/adr/0007-workspace-permission-mode.md)（已写）。
- 新增 **`docs/permissions.md`**：像 [`docs/sandbox.md`](../../docs/sandbox.md) 那样的决策地图——四档、`outside_read` 旋钮、升级手势、写死的边界、代码住哪。形状照邻居写（表头齐全、`##` 小节）。
- README 的安全模型：区外读「默认仍拒、可用 `[permissions] outside_read` 放开」；「这一版不做」里那条「越界之后的一次性审批」拿掉。
- [`docs/credentials.md`](../../docs/credentials.md)：(a) 条写明这个旋钮会改变 key 的暴露面（配成 `ask` / `allow` 之后，读 `~/.config/fs-agent` 不再是「不可能」）。
- [`docs/bash.md`](../../docs/bash.md)：升级手势、`escalation` 参数、「被拒即终局」那几句；「`bash` 不包含什么」里那条与沙箱有关的话要跟着改。
- [`CONTEXT.md`](../../CONTEXT.md)：加词条**「升级（Escalation）」**；给「权限模式」补第四档。
- `.scratch/README.md`：`workspace-mode` 那一行从「种子」改成 spec + 票数与状态。

## Testing Decisions

三层，与仓库先例一致：

1. **纯函数（主力）**——`decide()` 的新矩阵逐条断言：`workspace` 档下区内写 `Allow`、区外写 `Ask`、shell `Allow`；`readonly` 的地板仍在；`ask` 档的区外写仍是 `Deny`；`outside_read` 三值 × 四个档位；`escalation` 的裁决（`readonly` 拒、遮罩目录与保护路径拒、正常路径 `Ask`、空理由/空路径是参数错误）。这一层不需要进程、也不需要 bubblewrap。
2. **集成**——`bash` 的升级全链路：假 `bwrap` 拒 → 模型带 `escalation` 重试 → 弹一次问 → 批准后这次调用的 argv 里多一条 `--bind <声明的路径>`；拒绝即终局；同一条命令第二次被拒不再问；文件工具的区外写在 `workspace` 档下弹一次问、批准后跑；无沙箱平台组装 `workspace` 档是启动错误（文案两条出路）；`PermissionAsked` / `PermissionDecided` 里能看到这次升级。
3. **真机（手工清单 / pty）**——只有这一层能验「内核真的给了出口」：真 bwrap 下 `echo x > ~/.npm/...` 被拒 → 批准那条路径后同一条命令成功；遮罩目录（`~/.ssh/authorized_keys`）的升级被拒；`.env` 的升级被拒；`cargo test` 在区内照常。

## Out of Scope

- **网络隔离**：沙箱那一层就不做，这里也不碰。
- **macOS / Windows / 鸿蒙**：没有沙箱就没有这一档（§6）。
- **`allow-always` / 授权库 / 跨会话记忆**：升级永远是 `allowed-once`。
- **动态工具的升级**：它们的 schema 由使用者声明，插不进参数。
- **`ask` 档的区外写**：不加对称旋钮；要区外写就问，换 `workspace` 档。
- **关闭子会话的审批通道**：我们选了「标明来源」（§7），而不是 DSH 的「子 agent 一律 `never`」。
- **审批的持久化**：不写 `config.toml`、不进任何授权库；一次批准只活在这一次调用里。

## Further Notes

- **两张票，先升级后模式**：升级手势先落地时它在 `ask` / `auto` 档下就已经有用（沙箱不因权限模式而关），中间态自洽；而 `workspace` 档的 shell 行为依赖升级存在——所以票 02 被票 01 阻塞。
- **一处刻意的不对称**：文件工具的区外写是**事前问**（写目标已知），shell 是**事后升级**（判不出来）。两条路的落点一样，判据来源不同；不要为了「统一」把文件工具也改成事后。
- **shell 在 `workspace` 档下的门裁决就是「放行」**，与 `auto` 相同——真正的差别在文件工具与那条升级通道。这是 `Effect::Exclusive` 没有路径集这个事实的直接结果，不是疏漏。
- **DSH 的形状不能整套照搬**：它的越界判定、拒绝方言、每调用档位都建立在它自己的 runner 上；我们照搬的是**手势**（模型声明 + 一次批准）而不是它的机制。理由与实测在 ADR 0007 与 [`../sandbox/research/01`](../sandbox/research/01-dsh-workspace-permissions-and-shell.md)。
