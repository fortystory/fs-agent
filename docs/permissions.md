# 权限

每一次工具调用都过同一个纯函数 `permissions::decide`。它读一份策略（一档**权限模式**、一条**区外读**旋钮、一组规则）与一个 `Call`（工具名、已解析的路径、将要跑的 argv），回答那个封闭的三态：`allow` / `ask` / `deny`。

这份文件是给人看的决策地图：四档各是什么意思、区外读那一条怎么改、shell 越界之后那条升级手势是什么、哪些东西谁都不能放开、代码住在哪。决定与理由在 [ADR 0007](adr/0007-workspace-permission-mode.md)，完整规格在 [`.scratch/workspace-mode/spec.md`](../.scratch/workspace-mode/spec.md)。

权限门答的是「跑不跑、要不要问」；[沙箱](sandbox.md)答的是「跑起来能碰到什么」。两者之间那条缝由**升级手势**补上。

## 四档模式

`Shift+Tab` 按**严格度**循环它们：对同一次工作区内的写，`readonly` 拒、`ask` 问、`workspace` 允许、`auto` 允许。

| 档位 | 工作区内的写 | 工作区外的写 | 读 | shell |
| --- | --- | --- | --- | --- |
| `readonly` | 拒（地板） | 拒（地板） | 放行，区外按 `outside_read` | 拒——连跑都不让 |
| `ask` | 问 | 拒（地板） | 放行，区外按 `outside_read` | 问 |
| `workspace` | 放行 | **问一次**（这一次调用） | 放行，区外按 `outside_read` | 放行，越界靠升级手势 |
| `auto` | 放行 | 拒（地板） | 放行，区外按 `outside_read` | 放行，越界靠升级手势 |

两处值得说清：

- **`workspace` 只放松写的那一侧**，而且只放松到「问」。它仍然是另外三档的路径地板 —— 选这一档就是那个声明，不需要第二处配置（[ADR 0007](adr/0007-workspace-permission-mode.md)）。
- **shell 在 `workspace` 与 `auto` 下的裁决都是放行**：`bash` 的 `Effect` 是 `Exclusive`，没有路径集，门在命令跑起来之前判不出区内区外。真正的判据在内核（[沙箱](sandbox.md)），而出路是下面的升级手势。

`readonly` 的地板是**任何规则都降不下去**的；其余档位的「问」与「拒」同理，只有模式自己的缺省能被规则抬高或降低。这条代数是**一个格、一次合并**：`deny > ask > allow`，不看专指程度。

## 区外读：`[permissions] outside_read`

```toml
[permissions]
mode = "workspace"
outside_read = "deny"        # "deny"（缺省）| "ask" | "allow"
```

它管的是**读**目标落在会话 cwd 之外的情形，**全局**生效：四档都认它，因为它是一条策略级的地板、与档位正交。`ask` 档配上 `"allow"` 恰好就是 DSH 的「读全放、写要问」。

缺省永远是 `deny`，因为这条地板保住的正是 `~/.config/fs-agent/config.toml`——provider key 就在那儿，而打码只是**值级 best-effort**。这与 `[sandbox] mode = "off"` 是同一个立场：**显式写下来才算放弃**；改它会改变 key 的暴露面，见 [credentials.md](credentials.md)。

区外**写**没有对称的旋钮：它只有 `workspace` 档这一个出口。

## 升级手势

**只有 `bash` 有这条路。** 动态工具的 schema 与 argv 模板是使用者在 `config.toml` 里声明的，插不进新参数；它们的越界就是失败，要放宽就写 `[sandbox] writable_roots`。

```
bash(command: "echo x > ~/.npm/probe")
  → 内核拒（只读文件系统）——这是这条命令的结论
bash(command: "echo x > ~/.npm/probe",
     escalation: { justification: "构建产物要写到 ~/.npm 的缓存目录",
                   writable_paths: ["/home/ada/.npm"] })
  → 弹一次审批 → 批准后这一次调用多一条可写根
```

四条性质：

- **`justification` 与 `writable_paths` 都要非空、必须成对**；半截的写法是参数错误，不是静默忽略。
- **批准的就是声明的那个路径本身**——文件就绑文件、目录就绑目录，**不做父目录提升**。
  要放开的路径必须已经存在（`bwrap` 只能绑存在的源）；批准一条不存在的路径是一条工具错误，
  不是静默白批。要新建文件就声明它**存在的父目录**。
- **只这一次调用，而且一条命令被拒后只给一次重试**。不进规则、不写 `config.toml`、不进会话状态；同一条命令第二次被拒之后照常返回命令结果，运行时不做计数——话术在 `bash` 的工具描述里。
- **它在各档都有效，`readonly` 除外**：它请的是放宽沙箱，而沙箱不因权限模式而关，所以 `auto` 档下写 `~/.npm` 照样被拒、这条出口对它同样必需。

弹窗在原三行之上加三行——这是升级、理由、要放开的路径——而**命令行仍是最后一行**：它是授权时唯一必须看得见确切命令的地方。发起者不是主会话时（执行者 / 讨论者）第一行点名说话人。审批走既有的 `Asker` 通道，事件流里复用的是 `PermissionAsked` / `PermissionDecided`，`reason` 里写明这是一次升级与哪条路径。

## 写死的边界不接受升级

| 类别 | 清单 | 为什么 |
| --- | --- | --- |
| 遮罩目录 | `~/.config/fs-agent`、`~/.ssh` | provider key 与 ssh 私钥在那儿；挂载表里它们是空且只读的 tmpfs |
| 保护路径 | 工作区里的 `.git/config`、`.git/hooks`，以及 `.env` 一族的文件（`.example` / `.sample` / `.template` 除外） | 改 remote、换 hooks、把凭据写进工作区 |

它们在挂载表里排在可写根**之后**，所以就算批准把其中的路径加进可写根，那条遮罩或只读挂载也会盖掉它——**批准等于白批，而用户会以为自己批准了**。门里直接拒，是唯一诚实的做法。

升级那一侧的判据是 `tools/sandbox.rs` 的 `sealed(path, cwd, masks)`：遮罩目录来自 `SandboxSettings.masks`，`.env` 一族按**名字**判（与 `permissions::is_env_file` 同一份口径，也就在这一档下与权限门的地板一致）。挂载表另有一份**只含存在路径**的清单，那纯粹是 `bwrap` 的要求（不存在的挂载目标会让它报错）——两份互相漂离的「写死清单」是不能接受的，所以 `.env` 的名字判据只有一处。

## 没有沙箱就没有这一档

`workspace` 对 shell 的承诺完全建立在[沙箱](sandbox.md)上，所以它有两个入口、两处都要挡住：

- **组装期**：`[sandbox] mode = "off"`，或者探测说 `bwrap` 在这台机器上起不来，而权限模式是 `workspace` → 启动错误，文案给两条出路（换回 `"bwrap"`，或换一档）；
- **`Shift+Tab`**：没有可用沙箱时，模式循环**跳过**这一档（`ask` 直接走到 `auto`）。

理由是一样的：宁可让这一档不存在，也不给一个看起来在保护、实际有一个大洞的档位。

## 代码住在哪

| 部件 | 模块 |
| --- | --- |
| 权限门（纯函数）、四档、`outside_read`、升级裁决 | `permissions.rs` |
| 路径收容与按方向的放宽 | `tools/paths.rs` 的 `SessionPaths` |
| `sealed()`、遮罩与保护路径清单、`--bind` 的拼装 | `tools/sandbox.rs` |
| `escalation` 参数的形状与工具描述 | `tools/bash.rs` |
| 解析这次调用（写目标、读路径、argv、升级申请） | `tools/registry.rs` 的 `CallFacts` |
| 询问、批准、把裁决记进流 | `agent.rs` 的 `authorize` |
| 弹窗的措辞与那几行 | `render/wording.rs`、`render/tui.rs`、`render/input.rs` |
| `[permissions]` 的解析与默认值 | `config.rs` |
| 「没有沙箱就没有这一档」的组装期拒绝 | `lib.rs` |

## 这一版不做

- **`allow-always` / 授权库 / 跨会话记忆**：升级永远是 `allowed-once`。
- **动态工具的升级**：它们的 schema 由使用者声明，插不进参数。
- **`ask` 档的区外写**：不加对称旋钮；要区外写就问，换 `workspace` 档。
- **关闭子会话的审批通道**：我们选了「标明来源」，而不是 DSH 的「子 agent 一律 `never`」。
