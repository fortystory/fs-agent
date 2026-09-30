# 第四档 `workspace` 与区外读的旋钮

Type: implement
Status: done
Blocked by: 01

> 规格：`.scratch/workspace-mode/spec.md` §1（档位与入口）、§2（区外读的旋钮）、§3（区外写事前问）、§6（没有沙箱就没有这一档）、§8（文档）。
> 依赖票 01：`workspace` 档下 shell 的行为建立在升级手势上——没有它，这一档的 shell 每次越界都只能失败。

## 目标

让「区内自动、区外要问」成为一个真的档位：`Mode` 加第四档 `Workspace`，`SessionPaths` 的路径上限在**写**这一侧从拒绝地板变成 `Ask`，区外**读**给一个缺省仍拒的配置旋钮，而这一档在没有沙箱的地方**不存在**。

## 落点

`src/permissions.rs`（`Mode::Workspace`、`stance`、`outside_read`、`Call` 的方向）、`src/config.rs`（`[permissions] outside_read`）、`src/render/wording.rs`（「工作区」标签）、`src/lib.rs`（组装期拒绝）、`docs/permissions.md`（新增）、`README.md`、`docs/credentials.md`、`docs/bash.md`、`CONTEXT.md`、`.scratch/README.md`、`tests/` 下的权限与配置测试、`docs/tui-manual-checklist.md`（真机那一节）。

## 具体行为

1. **`Mode::Workspace`**：`as_str()` 是 `"workspace"`，`parse()` 接上它（`[permissions] mode` 与 `--mode` 因此自动可用）；中文名**「工作区」**（状态行照旧显示档位）。
2. **`Mode::next()`**：`readonly → ask → workspace → auto → readonly`。顺序是**严格度**（对同一次区内写：拒 / 问 / 允许 / 允许），所以 `Shift+Tab` 仍然是「按一次松一档」。
3. **`stance()` 对 `workspace` 档**：`ReadOnly` → `Allow`；`WritePaths` → 全部落在 `cwd` 之内时 `Allow`，否则 `Ask`；`Exclusive` → `Allow`（判不出，判据在沙箱那一侧，由票 01 的升级通道接住）。
4. **`[permissions] outside_read = "deny" | "ask" | "allow"`**（缺省 `"deny"`）：解析成既有的 `Decision`，存在 `Policy` 上，**全局**生效（四个档位都认它）。它管的是**读**目标落在 `cwd` 之外的情形。缺省 `deny` 不动摇：这是 key 所在的那条路，写下来才算放弃。
5. **`Call` 的 `path_error` 带方向**：现在是 `Option<&str>`，分不清读还是写。改成 `Option<PathError>`（`direction: Read | Write` + message），`decide()` 的 ④ 段据此分支——读的越界按 `outside_read` 给裁决，写的越界按档位给（`workspace` → `Ask`，其余 → `Deny`）。
6. **只有 `workspace` 档放松写那一侧**：`ask` 档的区外写照旧是地板 `Deny`，也不加对称旋钮。钉住这条地板的测试（`tests/permission_gate.rs` 的 `the_path_limit_is_a_deny_floor`）改成「在**另外三档**下仍是地板」。
7. **没有沙箱就没有这一档**：组装期（`lib.rs`）检查——沙箱 `mode == "off"`，或者探测结果 `Unavailable`，而权限模式是 `workspace` → 启动错误；文案给两条出路（换回 `[sandbox] mode = "bwrap"`，或换一档）。`cli` 的三个组装点照旧把错误打到 stderr。
8. **文档（§8）**：新增 `docs/permissions.md`（决策地图：四档、`outside_read`、升级手势、写死的边界、代码住哪；形状照 `docs/sandbox.md` 那份邻居，表头齐全）；README 安全模型改写区外读那一段、「这一版不做」里拿掉「越界之后的一次性审批」；`docs/credentials.md` 的 (a) 条写明这个旋钮会改变 key 的暴露面；`docs/bash.md` 记升级手势与 `escalation`；`CONTEXT.md` 加词条「升级（Escalation）」并给「权限模式」补第四档；`.scratch/README.md` 的 `workspace-mode` 行从「种子」改成 spec + 票数与状态。
9. **手工清单**：`docs/tui-manual-checklist.md` 加一节，覆盖 `Shift+Tab` 到「工作区」、区内写一路放行、区外写弹一次问、`outside_read` 三个值的手感、以及无沙箱平台启动被拒的文案。

## 测试

- 纯函数：`decide()` 在 `workspace` 档下的四条（区内写 `Allow`、区外写 `Ask`、只读读 `Allow`、shell `Allow`）；`readonly` 的地板仍在；`ask` 档的区外写仍是 `Deny`。
- `outside_read` 三值 × 四个档位 × 读/写两个方向的裁决矩阵（`deny` 时不许出现任何 `Allow`）。
- 配置解析：`outside_read` 缺省 `deny`、三个合法值、非法值报错；`mode = "workspace"` 能解析；`--mode workspace` 能解析。
- `Mode::next()` 的四步循环。
- 集成：`workspace` 档下文件工具写区外弹一次问、批准后真的写进去；写区内不问；无沙箱（`mode = "off"` 与探测不可用两条路）组装 `workspace` 档是启动错误、文案两条出路。

## 验收

- [ ] `cargo test`、`cargo clippy --all-targets`、`python3 scripts/check-language.py`、`python3 scripts/tui-startup-check.py` 全过。
- [ ] 真机：`--mode workspace` 下 `touch` 工作区里的文件不问；`touch ~/x` 弹一次问、批准后成功、拒绝后失败。
- [ ] `outside_read` 配成 `ask` 时读区外文件弹一次问；配成 `allow` 时不问；不配时是拒绝。
- [ ] 六处文档都改到，且 `docs/permissions.md` 进了 `scripts/check-language.py` 的 `DOCS_MIN_RATIO`（按实测值减 2 收紧）。


## Comments

- 2026-10-01 落地（依赖票 01，同日完成）。`Mode::Workspace`（`as_str` = `"workspace"`，
  标签「工作区」，`next()` 按严格度：`readonly → ask → workspace → auto`）；
  `[permissions] outside_read = "deny" | "ask" | "allow"`（缺省 `deny`）解析进
  `Policy::outside_read`，四档都认它；`Call::path_error` 改成带方向的 `PathError`，门的 ④ 段
  按读/写分支（读看旋钮、写看档位）。门放行一个越界目标之后，循环把那一次调用的
  `SessionPaths` 按方向放宽、重新解析一次事实 —— 门看到的是越界，工具拿到的是真正的目标。
  组装期（`lib.rs`）在 `[sandbox] mode = "off"` 或探测不可用时拒绝 `workspace` 档，文案给
  两条出路；**运行期的 `Shift+Tab` 是这一档的第二个入口**，所以没有可用沙箱时模式循环也
  **跳过**它（`ask` 直接走到 `auto`）—— 两处共用同一个判据（`sandbox_unavailable_reason`）。
  `outside_read` 是策略级的旋钮，执行者（`agent/executor.rs`）与讨论者一样沿用派发者的值，
  否则同一场会话里讨论者读得到区外、执行者读不到。钉住旧地板的测试改成 `the_path_limit_is_a_deny_floor_in_every_mode_but_workspace`。
  文档：新增 `docs/permissions.md`（并进 `check-language.py` 的 `DOCS_MIN_RATIO`，实测 35.3%
  收到 33），README 安全模型与「这一版不做」、`docs/credentials.md`、`docs/bash.md`、
  `docs/sandbox.md`、`CONTEXT.md`（新词条「升级（Escalation）」与「区外读」、四档）、
  `.scratch/README.md`、`docs/tui-manual-checklist.md` ⑲。测试：纯函数矩阵、配置解析、
  四步循环、`tests/workspace_mode.rs` 的端到端（区外写弹一次问并真的写进去、区内写不问、
  区外读三个值、无沙箱组装被拒）。
- 双轴 code review（Standards + Spec，fixed point `ab9b543`）之后的收口在 `df00dd4`：执行者
  沿用派发者的 `outside_read`、模式循环在没有可用沙箱时跳过 `workspace` 档、`docs/sandbox.md`
  里那条过时的「下一个 effort」改掉、几处重复收拢、弹窗行数与 `.env` 判据的措辞对齐。
- **未跑的那一条如实记在这里**：验收里的**真机**那一格没有勾 —— 这台机器上没有 provider key，
  所以 `docs/tui-manual-checklist.md` ⑲ 的九个步骤只写好了、没有逐条手工跑过。自动化那一侧
  （`cargo test` 821 passed、`clippy`、`check-language`、`tui-startup-check`）全绿，假 `bwrap`
  的端到端链路与纯函数矩阵都在测试里。

