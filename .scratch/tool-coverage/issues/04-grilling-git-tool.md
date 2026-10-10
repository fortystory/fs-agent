# git 工具：一条入口与 op 分档

Type: grilling
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

冻结项 9 定了大方向：**一条 `git` 工具**，`effect()` 按 op 分档（读 op `ReadOnly`、
写 op `WritePaths` / `Exclusive`），写 op 同票定形状、实现可后置。理由是"两条工具"会割裂：
模型不会为"这条 git 该走哪条路"多花一次判断，它会一律走 bash（`grep` 劝阻八天无效、
`workdir` 0.01% 采用率是同一族证据）。

实测（bash 段里 `git` 共 1799 段）：读侧 `diff` 436、`status` 395、`log` 285、`show` 78、
`rev-parse` 12、`branch` 11、`ls-files` 9 ⇒ 约 1406（78%）；写侧 `add` 156、`commit` 152、
`merge` / `rebase` / `checkout` 更少 ⇒ 约 474（26%，与读侧有重叠，`stash` 106 两类都算）。

### 要拍的问题

1. **op 集合**：读侧哪些必进（`status` / `diff` / `log` / `show` 是四大件）；写侧
   `add` / `commit` 进不进这一轮（`commit` 152 次不算小）。`stash`、`branch`、`rev-parse`
   这类要不要覆盖，还是留给 bash。
2. **写 op 的 `effect()`**：`git add` 的路径能从参数解析出来（→ `WritePaths`）；
   `git commit` 写的是 `.git/`，枚举不出工作区路径，只能退到 `Exclusive`（与 bash 同档）。
   两种档位混在一条工具里是否可接受？还是整个工具取最严的那一档？
3. **输出形状**：原样回 stdout（省事、信息全）还是整形（省 token，但 `git diff` 的整形
   会丢上下文）？要不要复用 `context::truncate_result` 那条落盘 + 指针流水线。
4. **可选参数**：`status` / `diff` / `log` 各有自己的开关（`--short`、`--stat`、`-n`、
   路径过滤）。是做成 `op` + `args: string[]`（透传，模型自己写开关），还是把常用的几个
   提成字段（每个字段都是缓存前缀的永久成本）。
5. **在哪跑**：`git` 要在仓库里跑。固定会话 cwd（git 自己向上找仓库根）还是给 `workdir`
   参数？与[workdir 必传](03-grilling-bash-workdir-required.md)那条票的耦合要说清
   （若 `bash` 的 workdir 已成必传，git 工具要不要照同一套语义）。
6. **与 `diff-page` 的边界**：[`../diff-page/spec.md`](../diff-page/spec.md) 是**宿主侧**渲染取数
   （左栏改动页），带着"模型可见文本一个字节不动"这条不变量；本票是**模型侧**工具。
   两者会不会重复实现同一段 git 调用逻辑，要不要抽共用层。
7. **描述与命名**：`op` 列表会写进工具描述，直接决定前缀成本；工具叫 `git` 还是别的名字。
8. **`git-worktree` 的关系**：[`../git-worktree/seed.md`](../git-worktree/seed.md) 是会话级
   worktree 隔离（三个读法），与这条工具无关 —— 本票明确不碰它。

### 输入

- [`src/tools/tool.rs`](../../../src/tools/tool.rs) 的 `Effect` 三档；
  [`src/tools/mcp_call.rs:61`](../../../src/tools/mcp_call.rs#L61) 是按参数分档的现成先例；
  [`src/tools/file.rs:287`](../../../src/tools/file.rs#L287) / [`:367`](../../../src/tools/file.rs#L367)
  是按参数解析写目标。
- 仓库现状：`src/` 里 `git` 只作为命令名出现在权限策略与文案里，**没有任何地方跑过 git 子进程**
  （[`../diff-page/spec.md`](../diff-page/spec.md) 的 map 记的）。
- 实测分布：读 1406 / 写 474；`git diff` 436、`status` 395、`log` 285、`add` 156、`commit` 152。

### 约束

- 票内不得重开冻结项 9 的"一条工具 + op 分档"。
- 答案要自足；写 op 若判"后置"，要写清它挂在什么条件下再开。

## 作答

五问全部按推荐拍板。**一条落地约束在写的时候浮出来**（第 6 节），它需要一个确认。

### 1. 形状：一条工具，`op` 枚举 + `args` 透传

- 工具名 **`git`**（与 `grep` / `bash` 同级的短名）。
- schema：`{ op: 枚举（必填）, args?: string[] }`。
- `op` 取值：
  - **读**：`status` / `diff` / `log` / `show`
  - **写**：`add` / `commit` / `stash`
- `args` **原样透传**（位置参数与开关都进这里），工具不解释它们 —— `-C <path>`、`--stat`、
  `-1`、`--short` 全走这条路，所以不必为它们开字段。
- **不进 op 的**：`rev-parse`（12）/ `branch`（11）/ `ls-files`（9），以及
  `config`（66）/ `init`（30）/ `user`（7）/ `safe`（7）/ `core`（5）/ `color`（4）—— 后者是
  **在 `/tmp` 里搭测试仓库**的用法（441 段命令含 `/tmp`），不是开发工作流。全部留给 bash。

### 2. `effect()` 按 op 分档

- 读 op → **`Effect::ReadOnly`**（四档全放行、不取工作区锁）。
- 写 op → **`Effect::Exclusive`**（`add` / `commit` / `stash` 写 `.git/index`、`.git/objects`
  与工作区文件，**枚举不出 `WritePaths`**；与 `bash` 同档）。冻结项 9 的"写侧拿不到
  `WritePaths` 那一档"就是这么来的。
- 先例：[`mcp_call.rs:61`](../../../src/tools/mcp_call.rs#L61) 是按参数返回不同 `Effect` 的现成写法。
- 实现要求：`op` 必须在 `effect()` 之前就能解析，**参数错误在这里就报**（不能等到 `call()`）。
- 安全边界一个字不动：`.git/config` 与 `.git/hooks` 仍被沙箱压回只读
  （[`sandbox.rs:357-370`](../../../src/tools/sandbox.rs#L357-L370)）—— `git add` / `commit`
  要能写 `.git/index.lock`，所以**不能**把整个 `.git` 压只读。

### 3. 参数面：只有两个字段

- `op`（枚举，必填）+ `args`（字符串数组，可选）。**不设** `limit` / `paths` / `stat` 等字段
  （每个字段都是缓存前缀的永久成本，而模型已经会 git 自己的语法）。
- 输出：**原样**回 stdout / stderr 与退出码。**不加 `cwd:` 首行**（会话 cwd 固定，没有站位歧义；
  `cwd:` 是 [bash 的 workdir](03-grilling-bash-workdir-required.md) 那件事的配套）。
- 溢出仍走 `context::truncate_result` 那条流水线（落盘 + 头尾预览 + 指针），**不新增第二套截断**。
- 验收要覆盖的高频形状（实测）：`status`（默认 / `--short` 248 / `--porcelain` 145）、
  `diff`（`--stat` 125 / 路径限定）、`log`（`--oneline` 247 / `-1` 108 / `-3` 68）、
  `show`（`--stat` 39）、`add`（`-A` 104 / 路径）、`commit`（`-q -F - <<EOF` 43 / `-m`）、
  `stash`（`pop` 为主）。

### 4. 在哪跑：固定会话 cwd

- **不给 `workdir` 字段**：`git -C` 在 7703 次 bash 调用里出现 **0 次**，且 git 自己会向上找
  仓库根。需要换目录时 `args: ["-C", "sub/dir"]` 就是现成的出口 —— 这也正是"不给字段"的底气。
- 与 [03 的 workdir 必传](03-grilling-bash-workdir-required.md)无关：那是 bash 的参数，
  这条工具不吃它。

### 5. 与渲染层那份实现的边界

[diff-page](../diff-page/spec.md) §3 已经定了"渲染器自己在宿主侧跑 git、不进事件流、不过权限门
与沙箱"，实现在 [`changes.rs`](../../../src/render/changes.rs)（`const GIT`），通用件是
[`hostproc.rs`](../../../src/render/hostproc.rs)。

- **抽一层共用的**：只到"起一个只读子进程、拿 stdout"这一层（先看 `hostproc.rs` 能不能直接用）。
- **不共用**：解析与整形 —— 渲染层要 `@@` 行与 `diff --git` 的解析、要 ANSI 上色；模型侧要原样文本。
- **分界线写进 spec**：模型工具**必须过权限门与沙箱**；渲染层那条明确"不过权限门"。
  两条路各走各的门，共用件只负责起进程。

### 6. 一条落地约束（需要确认）：声明与实现必须同批

冻结项 9 写了"写 op 同票定形状、实现可后置"。真排落地次序时发现它和已定的"少作废一次前缀缓存"
（[02](02-grilling-grep-context.md) + [03](03-grilling-bash-workdir-required.md) 合并发布）
打架：

- **声明了不实现** → 模型会调用 `op: "commit"` 然后拿到失败，比没有它更坏；
- **分两批发布** → 第一次声明读 op、第二次再声明写 op = **多废一次前缀缓存**。

所以本票的结论是 **读四件 + 写三件同批声明、同批实现**（冻结项 9 的"实现可后置"在这一点上
按已定原则收窄）。若维护者要分批，代价就是多一次前缀缓存作废，那也应该显式记在 spec 的交付说明里。

### 7. 要同步的文档

- 新增 **`docs/git.md`**（与 [`docs/grep.md`](../../../docs/grep.md) 同规格：为什么不是一条 shell 命令、
  参数面、输出与上限、与渲染层那份实现的分界、与 `git-worktree` seed 无关）。改完过
  `check-doc-size.py` 与 `check-language.py`。
- `README.md` 的工具清单同步（若列了内建工具表）。
- **不立 `CONTEXT.md` 词条**：`git` 是工具名不是领域词；"宿主侧 / 模型侧"这条区分住 `docs/git.md`。

### 8. 明确不碰

- [`../git-worktree/seed.md`](../git-worktree/seed.md)（会话级 worktree 隔离）—— 另一条 effort。
- 渲染层 `changes.rs` 的行为与不变量（"模型可见文本一个字节不动"）—— 只约定共用子进程件。
