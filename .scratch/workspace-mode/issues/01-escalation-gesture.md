# 升级手势：被沙箱拒了之后的一次批准

Type: implement
Status: done
Blocked by: —

> 规格：`.scratch/workspace-mode/spec.md` §4（升级手势）、§5（写死的边界不接受升级）、§7（审批与可见性）。
> 这一票不依赖第四档：它请的是放宽**沙箱**，而沙箱不因权限模式而关——落地之后 `ask` 与 `auto` 档下就已经有用。

## 目标

给 `bash` 一条出口：命令被内核拒（`EROFS`）之后，模型能带一份理由与它要写的路径，原样重试一次；这一次弹一次审批，批准后**这次调用**多一条可写根。只批这一次，一条命令只给一次重试。

## 落点

`src/tools/bash.rs`（`escalation` 参数与工具描述）、`src/tools/tool.rs`（`Call` 那一侧的形状）、`src/tools/registry.rs`（`CallFacts` 解析、`AllowedCall.sandbox_grants`）、`src/permissions.rs`（`escalation` 的裁决与遮罩/保护路径的拒绝）、`src/tools/sandbox.rs`（写死边界的纯函数、`Sandbox` 接受 grants）、`src/agent.rs`（构造 `Sandbox` 时带上批准的那批路径）、`src/render/wording.rs`（弹窗两行）、`tests/` 下的工具与权限测试。

## 具体行为

1. **`bash` 的 schema 加一个嵌套对象参数** `escalation`：

   ```json
   {
     "escalation": {
       "justification": "构建产物要写到 ~/.npm 的缓存目录",
       "writable_paths": ["/home/ada/.npm"]
     }
   }
   ```

   `justification` 与 `writable_paths` 都非空、必须成对；给了半截（有理由没路径、或路径为空）是**参数错误**，不是静默忽略。参数是可选的整体：不给就没有升级这回事。

2. **工具描述加上 DSH 那套话术**（模型可见、进请求前缀、一次定死）：被沙箱拒绝是这条命令的**结论**；要越界就带理由与路径原样重试**一次**；不许先绕道去聊天里问；**没被拒就不许投机性升级**（预先声明更宽的档位）。
3. **门的裁决**（`permissions::decide`，仍是纯函数）：
   - 没有 `escalation` → 一切照旧；
   - 有 `escalation` 且档位是 `readonly` → `Deny`（那一档连跑都不让，谈不上放开沙箱）；
   - 有 `escalation` 且声明的路径落在**遮罩目录或保护路径**里 → `Deny`，理由写清「这一条是写死的安全默认，没有任何通道放宽」；
   - 其余 → `Ask`，**一次**。批准 = 这次调用多一条可写根；拒绝 = 终局。
4. **写死的边界判据只有一份**：`tools/sandbox.rs` 提供一个纯函数（形如 `sealed(path, cwd, masks) -> bool`），覆盖遮罩目录（`SandboxSettings.masks`）与保护路径（`cwd` 下的 `.git/config`、`.git/hooks`，以及 `.env` 一族 —— **按名字判**，与权限门的地板同一口径；`.example` / `.sample` / `.template` 除外）。权限门用它，**不要抄第二份清单**——保护路径那套口径在 `permissions::is_env_file` 与 `sandbox::protected_paths` 之间已经共享过一次。
5. **批准之后怎么跑**：`AllowedCall` 加 `sandbox_grants: Vec<PathBuf>`；派发时构造的 `Sandbox` 把这批路径追加进 `SandboxSpec.writable_roots`（`Sandbox` 是每次调用现构造的）。粒度就是**声明的那个路径本身**——文件就绑文件、目录就绑目录，**不做父目录提升**。
6. **只这一次、只重一次**：批准只对这一次调用生效，不进任何规则、不写配置文件、不进会话状态；同一条命令第二次被拒之后照常返回命令结果（模型自己负责换做法或报告受阻，运行时不做计数——话术在描述里）。
7. **弹窗**（走既有的 `Asker` 通道）：加上三行 —— 「被沙箱拒绝，申请写工作区之外」、理由、要放开的路径 —— 命令行仍是**最后一行**；发起者不是主会话时，第一行点名说话人（`执行者 <id>` / `讨论者 <id>`）。plain 前端同构。
8. **事件流**：复用 `PermissionAsked` / `PermissionDecided`，`reason` 里写明这是一次升级、以及要放开的路径。不新增事件类型。

## 测试

- 纯函数：`escalation` × 档位（`readonly` 拒、其余 `Ask`）× 路径类别（遮罩目录、`.git/config`、`.env`、普通路径）的裁决矩阵；半截参数是参数错误。
- `sealed()` 的纯函数断言：遮罩目录的前缀、保护路径的三个入口、`.env` 在列而 `.env.example` 不在、工作区外的普通路径不在列。
- 集成（假 `bwrap`）：被拒 → 模型带 `escalation` 重试 → 弹**一次**问 → 批准后这次调用的 argv 里多一条 `--bind <声明的路径>`；拒绝之后那条命令的结果是失败而不是工具错误；遮罩路径的升级**不问**、直接拒。
- 审计：`PermissionAsked` / `PermissionDecided` 里能看到这次升级与那条路径。
- 参数形状：`escalation` 不进 `bash` 的 `command()`（它不是 argv 的一部分），但**进**工具声明——所以请求前缀会变，这是接受的代价（规格 §4）。

## 验收

- [ ] `cargo test`、`cargo clippy --all-targets`、`python3 scripts/check-language.py` 全过。
- [ ] 真机（手工清单加一节）：真 `bwrap` 下 `echo x > ~/.npm/probe` 被拒 → 带 `escalation` 重试并批准（声明的路径必须已经存在）→ 同一条命令成功、宿主上真的出现那个文件。
- [ ] `~/.ssh/authorized_keys` 与工作区里 `.env` 的升级被拒（不问、批不了）。
- [ ] `bash` 的描述里那几句话在（逐字断言）。


## 评论

- 2026-10-01 落地。`bash` 的 `escalation`（`justification` + `writable_paths`，成对非空，
  半截是参数错误）→ `CallFacts::escalation`（路径已解析成绝对）→ 门里的裁决
  （`readonly` 天然拒；`sandbox::sealed` 覆盖遮罩与保护路径，命中即 `Deny`；其余 `Ask` 一次）
  → 批准后 `AllowedCall::sandbox_grants` 进这一次调用的 `Sandbox::with_grants`。
  写死的边界判据只有 `tools/sandbox.rs::sealed` 一份，`.env` 与 `permissions::is_env_file`
  共用口径。弹窗（TUI 的 `Modal.notes`、plain 的 `permission_prompt_with_context`）多三行，
  命令行仍在最后；发起者不是主会话时第一行点名说话人。事件流复用
  `PermissionAsked` / `PermissionDecided`，`reason` 里写明是升级与哪条路径。
  **一处实现上的补充**：批准一条**不存在**的路径会是一条工具错误（`bwrap` 只能绑存在的源），
  而不是静默跳过 —— 静默跳过等于用户批了一条什么都不发生的路径。见 `docs/permissions.md`。
  测试：`tests/permission_gate.rs` 的升级矩阵、`tests/sandbox.rs` 的 `sealed()` 纯函数与假
  `bwrap` 全链路（被拒 → 带 `escalation` 重试 → 弹一次问 → argv 多一条 `--bind`）、参数形状
  与工具描述的逐字断言。真机那一条记在 `docs/tui-manual-checklist.md` ⑲。
- 双轴 code review（Standards + Spec，fixed point `ab9b543`）之后的收口在 `df00dd4`：执行者
  沿用派发者的 `outside_read`、模式循环在没有可用沙箱时跳过 `workspace` 档、`docs/sandbox.md`
  里那条过时的「下一个 effort」改掉、几处重复收拢、弹窗行数与 `.env` 判据的措辞对齐。
- **未跑的那一条如实记在这里**：验收里的**真机**那一格没有勾 —— 这台机器上没有 provider key，
  所以 `docs/tui-manual-checklist.md` ⑲ 的九个步骤只写好了、没有逐条手工跑过。自动化那一侧
  （`cargo test` 821 passed、`clippy`、`check-language`、`tui-startup-check`）全绿，假 `bwrap`
  的端到端链路与纯函数矩阵都在测试里。

