# `bash` 的 `workdir`：在工作区内给一条命令换个站位

Status: 2/2 done（2026-10-09 经 `/grill-with-docs` 四轮 grilling 定下；拆出
[`issues/01`](issues/01-workdir-tracer-bullet.md) 与
[`issues/02`](issues/02-docs-and-glossary.md)，blocking edge 是 `01 → 02`；两张票同日落地）

模型今天要换个目录干活，只能把 `cd x && …` 写进**每一条**命令。这不是能力缺失，而是那条
前缀每条都要重写一遍，而 `bash -lc` 已经能在一行里做完它。`workdir` 把「站在哪」从命令文本
里拿出来交给参数，换来的是省掉重复、并且让这次调用站在哪能被单独看。

**范围被压到最小**：它只能在会话工作区**之内**换站位，于是沙箱的写边界、兄弟工具的路径
基准、权限门与断路器，一处都不用改。这不是省事，是判据——一旦允许站位跑到工作区之外，
四处以工作区为锚的不变式就得逐条重论证（那份论证本仓库今天没有）。

来源与一手材料：

- 四轮 grilling 的逐题答案在文末《决定速查》。需求本身是「哪些 seed 还没实现」时顺带发现的。
- 外部先例在
  [`research/01-cwd-parameter-precedent.md`](research/01-cwd-parameter-precedent.md)：Codex 的
  `workdir`、Gemini CLI 的 `dir_path`、opencode 的 `workdir` 都是「参数 + 每次新进程」；
  Anthropic 的 API 级 bash 规格与 goose 把 cwd 留给应用侧，Claude Code 则把 `cd` 记成会话状态。
  本仓库的进程**每次都是新的**，没有可沿用的状态，所以选参数这一侧。
- 逐面文档改在 [`docs/bash.md`](../../docs/bash.md)（§`workdir`）与
  [`docs/sandbox.md`](../../docs/sandbox.md)（边界锚点）。

## 1. 形状

| 部件 | 值 | 为什么 |
| --- | --- | --- |
| 参数 | `workdir?: string` | 唯一新增的东西；`required` 仍是 `["command"]` |
| 相对基准 | **会话工作区** | 它是模型给的路径本来就按之解析的那个基准 |
| 绝对路径 | 接受 | 模型从 `pwd` 或报错信息里抄到绝对路径很常见，拒掉只会让它反复改写 |
| 区外判定 | **按解析后的真实位置** | `src/..` 合法、`src/../../..` 越界；按字面判会让 `..` 变成一条绕过 |
| `""` | 参数错误 | 与 `timeout_ms` / `escalation` 半截写法同一种形状 |
| `"."` | 合法，等价不写 | 常见的合法写法 |
| 目录不存在 | 工具错误 | 不替它 `mkdir`——那是另一条命令的事，隐式写入不该藏在参数里 |
| `/tmp` | **不开特例**，算区外 | 它在沙箱里是每次调用重建的 tmpfs，站过去必不存在；且会破掉四处不变式 |

模型可见的描述是一句常量（工具声明是请求前缀的一部分，定了就不再改）：

> 可选，这条命令在工作区内的哪个目录跑；相对路径按工作区解析，目录必须已存在且落在工作区
> 内。用它代替在命令里写 cd；只影响这一条命令的进程，不改变其他工具解析相对路径的基准。

## 2. 四条工具错误，不弹审批

`workdir` **不构成写**（目录是既有的，站过去也不新增可写根），所以它不经过权限门发问；
它自己的错误一律是工具错误，形状照 `timeout_ms` 与 `escalation` 的既有一致：

1. 不是非空字符串（含 `""`）；
2. 解析后落在工作区之外；
3. 那个目录不存在；
4. 与 `escalation` 同现——一个说「站在哪」、一个说「额外能写哪」，混起来就等于让模型自选
   工作区，那要重做一遍边界论证。

## 3. 三条不变式

**边界不动。** 沙箱的可写根与 `.git/config`、`.git/hooks`、`.env` 那一族保护路径**仍绑会话
工作区**。于是 `tools/process.rs` 的 `run()` 与 `tools/sandbox.rs` 的 `wrap()` 从「一个 `cwd`
一参两用」**拆成边界与站位两个参数**：可写根与保护路径取边界，`current_dir` 取站位。

这一拆不是洁癖。`wrap()` 现在把同一个 `cwd` 同时喂给 `writable_roots(cwd)` 与
`protected_paths(cwd)`；若传同一个值，`workdir: "src"` 会让 `src/.git/config`（不存在）取代
工作区的 `.git/config`（存在）——工作区的 `.git/config` 于是**静默变可写**，而那正是地板要防的
「改 remote」。自定义动态工具**不加**这个参数：它在调用点把两个参数给同一个值，语义不变。

**兄弟工具的基准不动。** `read_file` / `write_file` / `edit_file` / `grep`、`@` 记号、左栏文件页与
改动页，全部仍按工作区解析。所以 `workdir: ".scratch/x"` 之后 `read_file("spec.md")` 读的是
工作区根那份。描述里那半句「不改变其他工具解析相对路径的基准」就是说这件事——不写，模型会
按 shell 的直觉以为基准跟着走了。

**门与断路器不动。** `Call.cwd` 仍是工作区：`rm` 断路器与路径 glob 按它折叠相对参数，而
`bash` 不声明写目标（`effect()` 恒 `Exclusive`）。在子目录里按工作区折叠**只会更保守**（更容易
拒、不会漏），所以这不是漏洞，是「不去改动一处本来就不需要改的东西」。

**事件流不另记。** `ToolCallStarted.args` 记的是模型发的原文，`workdir` 自然进流，回放能重算；
不新增 payload 类型。

## 4. 可观察性：不新画

`workdir` 一旦进 `args`，现成位置就自动多出四处：TUI 详情覆盖层的参数段（pretty JSON 全文）、
权限弹窗与 plain/headless 的参数摘要（`summarize_args`，上限 160 字符）、`sessions show` 的
头行（`args.to_string()` 全文）。TUI 的折叠行只读 `args["command"]`、描述上限 60 字符
（`DESCRIPTION_MAX_CHARS`），在那里拼路径要重新推导动词、还会让轨迹页变宽——**不画**。

## 5. 落点

| 位置 | 改什么 |
| --- | --- |
| `src/tools/bash.rs` | schema 的 `workdir` 属性与那句描述常量；解析函数（形状照 `requested_timeout_ms`）；`call()` 把站位交给 `run()` |
| `src/tools/tool.rs` | `ToolContext` 补一个**严格**解析入口：`read_paths` 可能是 `relaxed_read`（区外读放行时不收容），拿它解析 `workdir` 会让「窄」漏成「区外可跑」 |
| `src/tools/process.rs` | `run()` 多收一个站位；`current_dir` 用它 |
| `src/tools/sandbox.rs` | `wrap()` 拆边界与站位；可写根与保护路径取边界 |
| `src/tools/custom.rs` | 调用点给同一个值，语义不变 |
| `src/permissions.rs`、`src/agent.rs` | **不动**（见 §3 第三条） |

`workdir` 不进 argv（与 `escalation` 同理），也不进 `Tool::command()`：那是「将要执行什么」，
站位不是命令的一部分。

## 6. 验收

测试只测外部行为（照 README 的规矩：断言事件流与工作区副作用，不断言 `at`）：

1. 区内相对路径：`{"command":"pwd","workdir":"src"}` 的输出是工作区下的 `src`；
2. 区内绝对路径：同一件事用绝对路径写，输出相同；
3. 区外：`{"workdir":"../"}`、绝对路径指向工作区之外、以及 `{"workdir":"src/../../.."}` 三种
   都是**工具错误**（不是权限裁决）；
4. `""` 是参数错误；`"."` 等价不写；
5. 不存在的目录是工具错误，且**盘上没有多出任何目录**；
6. `workdir` 与 `escalation` 同现是参数错误；
7. 沙箱：`workdir: "src"` 时 `wrap()` 产出的 argv 里仍有工作区根的 `--bind` 与
   `.git/config` 的 `--ro-bind`，`current_dir` 却是 `src`；
8. 兄弟工具不受影响：`workdir` 生效的同一次会话里，`read_file` 仍按工作区解析相对路径；
9. 事件流里 `ToolCallStarted.args` 带着 `workdir` 原文。

不需要真机走查：参数落在 `args` 之后，plain 下 `bash（command=…，workdir=…）` 一眼可见，
而真终端里能做的事与 §4 那四处一致。

## 7. 明确不做

- **会话中途换工作区**（那是 `git-worktree` 那条 seed 的「工作区身份」分叉，一场会话一个工作区
  这条不变式今天不动）。
- **`/tmp` 特例**、**目录不存在就建**、**让 `workdir` 改沙箱边界**、**记 `cd` 成会话状态**、
  **给动态工具加这个参数**、**在转录里单独画**。
- **不动 `docs/permissions.md` 的区界定义**：它说的是「会话工作区」，那正是这份 spec 保持不变
  的东西。

## 决定速查

| 轮 | 问题 | 答案 |
| --- | --- | --- |
| 1 | 参数叫什么 | `workdir` |
| 1 | 值域 | 只能落在会话工作区之内 |
| 1 | 越界怎么办 | 工具错误，不弹审批 |
| 1 | 主要痛点 | 省掉每条命令重复写 `cd` 前缀 |
| 2 | `/tmp` 特例 | 不开，算区外（每次调用重建的 tmpfs + 会破不变式） |
| 2 | 区外判定 | 按解析后的真实位置 |
| 2 | 目录不存在 | 报错，不替它建 |
| 2 | 与 `escalation` 同现 | 参数错误 |
| 2 | 转录里显不显示 | 不画，进 `args` 后自然出现 |
| 3 | 绝对路径 | 给，只要解析后落在区内 |
| 3 | 描述措辞 | 定稿（§1 引的那句） |
| 3 | 沙箱形状 | 拆边界与站位两个参数 |
| 3 | 动态工具 | 不加 |
| 3 | 事件流与门 | 不另记事件，门不看 `workdir` |
| 4 | 词汇表 | 立一条「工作区」，并点明它不因 `workdir` 变 |
| 4 | ADR | 不单独写，决定落在 `docs/bash.md` |
| 4 | `""` 与 `"."` | `""` 参数错误，`"."` 允许 |
| 4 | feature 目录名 | `bash-workdir` |
| 4 | 下一步 | 先落文档与词汇，再折 spec 拆票 |
