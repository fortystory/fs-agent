# 第四条权限模式 `workspace`：区外要问，以及被拒之后的一条升级通道

新增一档 `workspace`（标签「工作区」）：会话 cwd 之内一律放行、之外要问一次。它把三样以前互相纠缠的东西拆开——**权限模式**答「跑不跑、要不要问」，[沙箱](0006-sandbox-by-bubblewrap.md) 答「跑起来能碰到什么」，而两者之间的缝由一条**升级手势**补上：命令被内核拒了，模型带理由原样重试一次，人批一条路径，这一次调用就多一条可写根。

这条 ADR 同时放松了一条既有不变量：`SessionPaths` 的路径上限（任何规则都降不下去的拒绝地板）在 `workspace` 档下变成 `Ask`。完整规格在 [`.scratch/workspace-mode/spec.md`](../../.scratch/workspace-mode/spec.md)，一手材料是沙箱那轮的 [`.scratch/sandbox/research/`](../../.scratch/sandbox/research/)（DSH 的越界升级形状在 [01 §⑦](../../.scratch/sandbox/research/01-dsh-workspace-permissions-and-shell.md)）。

## 为什么是第四档，而不是把 `ask` 改聪明

`ask` 现在的意思是一句完整的话：**写要问、读放行**，而且它是缺省档。把它改成「区内不问、区外问」，会让 `--mode ask` 在不同场合变成两件事，也会让「缺省档」这个位置带上一条没人要求的路径语义。第四档的代价只是 `Shift+Tab` 的循环多一格——它按严格度插在 `ask` 之后（对同一次**区内**写：`readonly` 拒、`ask` 问、`workspace` 允许、`auto` 允许），顺序因此仍是「按一次松一档」。

## 为什么区外读仍默认拒绝，却给一个旋钮

`SessionPaths` 的 cwd 收容是这三条地板里最值钱的一条：它保住的正是 `~/.config/fs-agent/config.toml`（provider key 就在那儿），而打码只是**值级 best-effort**。把它降成「问」，等于把「读 key」变成一次点击。所以缺省不动。

> **2026-10-06 补注（正文不改，只记更名）**：程序已更名为**衡**（`heng`），本文里的 `~/.config/fs-agent` 读作 `~/.config/heng`（[ADR 0014](0014-renamed-to-heng.md)）。

要放开的人可以写下来：`[permissions] outside_read = "deny" | "ask" | "allow"`（缺省 `"deny"`），**全局**生效——这条地板是策略级的，与档位正交，`ask` 档配上 `"allow"` 恰好就是 DSH 的「读全放、写要问」。这与 `[sandbox] mode = "off"` 是同一个立场：**显式写下来才算放弃**。

区外**写**不给对称的旋钮：它只有 `workspace` 档这一个出口，因为这一档本身就是那个声明。于是 `SessionPaths` 的路径上限在这一档下从 `Deny` 变 `Ask`——**选这一档就是同意「区外要问」**，不需要第二处配置。

## 为什么 shell 的区外判据只能来自沙箱

`bash` 与动态工具的 `Effect` 是 `Exclusive`，没有路径集：它们的 argv 里藏着什么，权限门在命令跑起来之前看不出来。这一格当初正是整个 `workspace` 意向停下的地方。沙箱把它解开了：区外的写由内核以 `EROFS` 打回，而这条拒绝是**命令结果的一部分**——模型读得到。

于是 shell 走一条与文件工具**不同**的路：文件工具的写目标事前已知，直接问；shell 先跑，被拒之后由模型发起**升级**。两条路的落点一样（越界要人点一次头），只是判据来源不同。这也意味着 shell 在 `workspace` 档下的门裁决就是「放行」——与 `auto` 相同——真正的差别在文件工具与那条升级通道。

## 升级为什么是「模型声明 + 批准一次」

升级的形状照 [DSH 的越界升级](../../.scratch/sandbox/research/01-dsh-workspace-permissions-and-shell.md)：**模型**在同一个回合里带一份理由重试同一条命令，这一次才弹审批，结果是 `allowed-once`（没有 allow-always、没有授权库、范围是单次调用）。

三个决定值得记下来，因为它们都能反向选：

- **路径由模型声明，不从拒绝输出里猜。** 实测（bwrap 0.13.0，九类命令）表明拒绝消息虽然都带路径，但形态不齐：`cd` 之后的相对路径、`mkdir -p` 报的是中间目录、一条命令可能牵涉多条、退出码只反映最后一条。从文本反推「用户想写哪条」既会漏也会错——所以升级参数是 `escalation = { justification, writable_paths }`，**声明式**。
- **批准的就是声明的那个路径本身**，不做父目录提升：批准 `/home/ada/.npmrc` 只放开那个文件，而不是整个配置目录。
- **只这一次调用，而且一条命令被拒后只给一次重试。** 它在两个方向上同时是闸门：升级不会变成「刷问」，而模型也不会把它当常规路径用。DSH 那套话术（被拒即终局、不许先绕道去问、不许投机性升级）一并进 `bash` 的工具描述。

## 这几样东西不接受升级

遮罩目录（`~/.config/fs-agent`、`~/.ssh`）与保护路径（`.env` 家族、`.git/config`、`.git/hooks`）都是**写死的安全默认**：它们挂在可写根之后，就算批准把其中的路径加进可写根，那条遮罩或只读挂载也会盖掉它——批准等于白批，而用户会以为自己批准了。所以门里直接拒绝，理由写清「这一条没有任何通道放宽」。

**动态工具也不支持升级**：它们的 schema 与 argv 模板都是使用者在 `config.toml` 里声明的，插不进新参数。它们的越界就是失败；要放宽就写 `[sandbox] writable_roots`——那是使用者自己的声明，责任清楚。

## 没有沙箱的地方，这一档不存在

`workspace` 对 shell 的承诺完全建立在沙箱之上，所以**没有可用 `bwrap`，或者显式写了 `[sandbox] mode = "off"`，这一档就不可用**（组装期拒绝，出路是换回 `bwrap` 或换一档）。这与沙箱自己的 fail closed 是同一条立场：宁可让这一档不存在，也不给一个看起来在保护、实际有一个大洞的档位。

## 后果

- **`bash` 的 schema 多一个参数**（`escalation`），而工具声明是请求前缀的一部分：这条契约一次定死。`escalation` 在**各档都有效**（`readonly` 除外，那一档连跑都不让），因为它请的是放宽沙箱、不是放宽权限模式。
- **路径上限那条地板被改写了**：它仍然是另外三档的地板，但 `workspace` 档下是 `Ask`。钉住它的那条测试如今叫 `tests/permission_gate.rs` 的 `the_path_limit_is_a_deny_floor_in_every_mode_but_workspace`（「在另外三档下仍是地板」）。
- **`auto` 档也拿到了出口**：沙箱不因权限模式而关，所以 `auto` 档下写 `~/.npm` 照样被拒——升级手势对它同样有效，否则用户唯一出路是去改配置文件。
- **弹窗多两行**（理由与要放开的路径），并在发起者不是主会话时点名说话人（执行者 / 讨论者）。本仓库的 `Asker` 是会话级的、会被 fork 继承，所以子会话本来就能弹审批；这次选择**标明来源**而不是把通道关掉——执行者的 `brief` 是模型写的，人至少要知道这次问的是谁。
- **文档**：新增 [`docs/permissions.md`](../permissions.md)（决策地图）；README 的安全模型改写区外读那一段；[`docs/credentials.md`](../credentials.md) 的 (a) 条要写明这个旋钮会改变 key 的暴露面；[`docs/bash.md`](../bash.md) 记升级手势与 `escalation`；`CONTEXT.md` 加词条「升级」并给「权限模式」补第四档。
