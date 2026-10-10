# 内置工具的按用量覆盖：哪些 bash 用法该收编成一等工具（wayfinder 决策图）

Label: `wayfinder:map`
Status: 7 resolved（决策图走完）+ 5 ready-for-agent（实现票）
Tracker: local markdown —— 见 [`docs/agents/issue-tracker.md`](../../docs/agents/issue-tracker.md)
Charting: **已完成**（2026-10-10，四轮 grilling 共十四问：产出形状 / 判据 / 范围 / 计数单位 /
收编语义 / 预算策略 / 落点 / grep 上下文 / workdir 必传 / git 边界，外加两处追问）。
本图只做**规划**，不产代码改动。

## 目的地

一份**可执行的 spec**（交给 `/to-tickets` 折成实现票）：按用量给内置工具扩编，逐条定下
**做 / 变形做 / 不做**，每条给出参数形状与优先级。

**范围**：`src/tools/**`（`grep` / `bash` / 新增工具）与它们进 `builtin()` 的注册、
`docs/grep.md`、`docs/bash.md`、`CONTEXT.md` 的相应词条，以及这次要**推翻的两处既有决定**的
改正记录（[`grep-tool/spec.md`](../grep-tool/spec.md) 的《明确不做》与
[`bash-workdir/spec.md`](../bash-workdir/spec.md) 的 `required` 明文）。

**判据 = bash 调用数与段数下降（主）、token 下降（辅）**，用同一套统计可复跑
（口径由[验收口径](issues/06-grilling-measurement.md)定）。

## 笔记

- **数据源与基线**（快照 2026-10-10，分母随会话增长略有出入）：heng 自己的会话日志
  `~/.local/share/heng/sessions/*/*/log.jsonl`，269 个会话；工具调用 11751 次，其中
  **bash 7203 次（61.3%）**、`edit_file` 1740、`read_file` 1442、`grep` 194；
  bash 展开成 27398 个命令段（平均 **3.8 段/调用**），其中**胶水段 42.2%**
  （`cd` 6379、`echo` 4271、`for`/`do`/`done` 各约 220）。
- **bash 的形状**：86.1% 的调用以 `cd` 开头（其中 `cd "$(pwd)"` 472 次是纯冗余）、
  80.4% 含 `&&`、67.7% 含管道、17.3% 带 heredoc、45.3% 以 `| head` 收尾。
- **`grep` 的缺口面**（bash 内 grep/rg 共 5495 次）：管道位置 **32.6%**、带上下文
  `-A`/`-B` **20.1%**（`-A` 903、`-B` 202、`-C` 1）、带 `-l/-c/-v/-o/-i` 8.7%、
  cwd 之外 0.9%；**严格可被现有内置 `grep` 替代的只有 24.2%**。
- **采纳率的两个先例**（说明"描述层劝阻"无效）：`grep` 工具 2026-10-02 上线后，bash 内 grep
  的密度八天不降（10-05 72% → 10-09 73%）；`bash` 的 `workdir` 2026-10-09 上线后，
  3175 次调用里只有 **1 次**带它（0.01%）。
- **权限打扰这条理由实测不成立**：12118 次裁决里 Allow 12104 / Deny 14，98% 跑在 `workspace`
  档。原始 [`grep-tool` spec](../grep-tool/spec.md) 的第一理由（默认 `ask` 档每次搜索打断人）
  在今天的使用里几乎不为真。
- **既有决定必读**：[`docs/grep.md`](../../docs/grep.md)（《为什么它不是一条 shell 命令》）、
  [`grep-tool/spec.md`](../grep-tool/spec.md)（含《明确不做》两条）、
  [`bash-workdir/spec.md`](../bash-workdir/spec.md)（`workdir` 的语义与四处不变式）、
  [`workspace-mode/spec.md`](../workspace-mode/spec.md)（升级手势）、
  [`fs-agent-v1/spec.md`](../fs-agent-v1/spec.md)（v1 工具集清单与
  `deny 不从上下文移除工具` 那条）。
- **缓存前缀纪律**：工具表与参数面是 provider 请求前缀的一部分，
  [ADR 0001](../../docs/adr/0001-chinese-ui-frozen-model-text.md) 与
  [ADR 0003](../../docs/adr/0003-plan-leaves-the-permission-modes.md) 钉着「组装期决定、
  中途不增删」。改一次声明＝所有存量会话的前缀缓存废一次；每多一个参数＝每次请求多带一点。
- **`effect()` 可以按调用参数分档**（重要的实现事实）：
  [`mcp_call.rs:61`](../../src/tools/mcp_call.rs#L61) 是现成先例，
  [`file.rs:287`](../../src/tools/file.rs#L287) 与 [`file.rs:367`](../../src/tools/file.rs#L367)
  也是按参数解析写目标。一条工具的读 op 可以是 `ReadOnly`、写 op 走 `WritePaths`/`Exclusive`。
- **一手调研的现成材料**：[`docs/research/notes/shell-vs-first-class-tools.md`](../../docs/research/notes/shell-vs-first-class-tools.md)
  （六家对照，含 Gemini 的「≤3 条命中自动补上下文」这一条唯一实现）。
- **要咨询的 skills**：`/research`（事实票）、`/grilling` + `/domain-modeling`（决策票；
  术语会进 `CONTEXT.md`）。
- **每张票的答案必须自足**：`/to-tickets` 会在别处的会话里读它，看不到本图与 charting 对话。
- **统计脚本的硬约束**：这个沙箱里跨 bash 调用的临时文件会被清（实测 `/tmp`、`/dev/shm`、
  仓库根部都发生过）。任何统计都要**能在一条命令内跑完**，或把产物写进
  `.scratch/tool-coverage/research/` 里再消费。
- **文档纪律**：改 `docs/**` 要过 [`scripts/check-doc-size.py`](../../scripts/check-doc-size.py)
  与 [`scripts/check-language.py`](../../scripts/check-language.py)。

### 冻结项（charting 的 grilling 定下，票里不得重开）

1. **destination 是一份 spec + 决策票**；本图不含实现落地，实现由 `/to-tickets` 在图外拆。
2. **判据 = bash 调用数与段数下降（主）、token 下降（辅）**，并要可复跑同一套统计。
3. **范围 = 整体重扫 bash 用法**，不只是四条提名（grep 上下文 / workdir 必传 /
   文件目录操作 / git 工具）。
4. **计数单位 = 命令形态聚类**（不是 argv0 段级，也不是意图类别）；
   排序量 = 出现次数 × 可省段数 ÷ 实现成本（成本分三档：改既有工具参数 / 开一条新工具 / 改架构）。
5. **「收编」= 收敛**：只收编「高频 + 高胶水 + 参数补不动」的形态；结构上不该收编的
   （管道读 stdin、上一条命令的产物、批量文件系统写）坦然留在 bash。目标是让一等路径存在，
   不是把 bash 清零。
6. **优先改参数**，新工具只开给既有工具结构上接不住的类别。
7. **`grep` 加 `after` / `before`（不加 `context`）**，并推翻
   [`grep-tool/spec.md`](../grep-tool/spec.md) 第 205 行那条《明确不做》。
8. **`bash` 的 `workdir` 改必传**（与 `command` 并列）、结果**回显 cwd**、识别 `cd` 冗余。
   *（2026-10-10 由 [workdir 必传与 cwd 反馈](issues/03-grilling-bash-workdir-required.md) 修正：
   charting 时那句「不允许 `.`」**撤回** —— 它把"工作区根"的唯一自然写法禁掉了；改判为
   **必填、且允许 `.` 表示根**。同票另取消 `workdir` 与 `escalation` 的既有互斥，否则必传会
   打死每一次升级调用。）*
9. **`git` 做一条工具**，`effect()` 按 op 分档（读 op `ReadOnly`、写 op
   `WritePaths` / `Exclusive`），写 op 同票定形状、实现可后置。
10. **工具名前缀统一 → 范围之外**（维护者明示「后续再做」）。

### Tracker 事实与降级（本图适用）

- map = `.scratch/tool-coverage/map.md`，child = `.scratch/tool-coverage/issues/NN-*.md`；
  阻塞 = 票面 `Blocked by: NN`；claim = `Status:` 改成 `claimed`；
  resolve = 票底写 `## 作答` + `Status: resolved` + 本文 `已定的决定` 追加一行。
- **没有 native sub-issue / 依赖边**，回退到正文约定：本文 `## 任务清单` 的条目数必须等于
  `issues/` 下的文件数，每张子票顶部写 `Part of: ../map.md`。
- 校验：`python3 scripts/wayfinder-check.py .scratch/tool-coverage/map.md`，宣布图走完前必须 PASS。
- 每个 session 至多 resolve 一张票（research 票除外）。用户可能并行跑 unblocked 的票。

## 任务清单

<!-- 条目数必须等于 issues/ 下的子票数（scripts/wayfinder-check.py 校验）。
     frontier 的权威查询仍是扫 issues/ 里 open + unblocked + unclaimed 的票。 -->

**事实票**（AFK，charting 当天派出 subagent）：

- [x] [bash 用法的形态底账：哪些形态值得收编](issues/01-research-bash-shape-inventory.md)

**决策票**（HITL，grilling；前三条是四条提名里已定方向的形状票）：

- [x] [grep 的上下文参数：`after` / `before` 的形状](issues/02-grilling-grep-context.md)
- [x] [bash 的 workdir 必传与 cwd 反馈](issues/03-grilling-bash-workdir-required.md)
- [x] [git 工具：一条入口与 op 分档](issues/04-grilling-git-tool.md)

**依赖事实票的两条**（01 的底账回来之后才拿得动）：

- [x] [收编的逐条判定：文件目录、命令结果、窗口读](issues/05-grilling-intake-verdicts.md)
- [x] [验收口径：怎么量调用数、段数与 token](issues/06-grilling-measurement.md)

**05 升级出来的**（判「做」的那一条）：

- [x] [文件枚举工具：只读的 `ls` / `find` 替代](issues/07-grilling-file-enumeration.md)

**实现票**（`/to-tickets` 从 [`spec.md`](spec.md) 拆出，2026-10-10；由 `/implement` 认领，
wayfinder 会话跳过）：

- [ ] [08 — `grep` 的上下文行](issues/08-grep-context-lines.md)
- [ ] [09 — `bash` 的站位必传与 `cwd:` 回显](issues/09-bash-workdir-required.md)
- [ ] [10 — 一条 `git` 工具](issues/10-git-tool.md)
- [ ] [11 — 文件枚举 `glob`](issues/11-glob-tool.md)
- [ ] [12 — 文档索引与验收收口](issues/12-docs-and-acceptance.md)

共 **12** 张票（1 research + 6 grilling + 5 implement）：七张决策票**全部 `resolved`**（图已走完），
目的地折成 [`spec.md`](spec.md)；五张实现票 `ready-for-agent`，**frontier 是 08 / 09 / 10 / 11**，
12 是它们全落地后的收口。**08 与 09 必须同批发布**（两次改工具声明只作废一次前缀缓存）。

## 已定的决定

<!-- 索引：每条一行，够判断相关性即可；细节住在票里。按名字引用，不写裸编号。 -->

- [bash 用法的形态底账：哪些形态值得收编](issues/01-research-bash-shape-inventory.md) — 274 个会话 /
  7703 次 bash 调用 / 35809 段（全切，含管道）/ 1590 个形态；两种切法都印了（不切管道 27283 段），
  票面的散数差异全部由此解释。**第一名是 `cd <WS>`（5673 段 / 5604 调用）**，其后 `head -<N>`
  3657 段（100% 在管道下游）、`echo <STR>` 3502 段、`sed -n <SCRIPT> <PATH>` 2329 段 / 1847 调用、
  `grep -n <PAT> <PATH>` 1386 段、带 `-A`/`-B`/`-C` 的 grep 1310 段（占 grep/rg 的 23.2%）。
  排序视图前 5：`cd <WS>` 10479、`head -<N>` 4123、`echo <STR>` 2427、`sed -n` 1581、`grep -n` 1283。
  **stdin 类合计 177 形态 / 8558 段**（冻结项 5 说的「留在 bash」）；「需要 PTY」全量只有 1 形态 / 5 段。
  两条硬边界：**`cd` 冗余率判不了**（日志只记 `SessionStarted.cwd`，没有每次调用的实际 cwd）——
  于是 [workdir 必传](issues/03-grilling-bash-workdir-required.md) 要拿「5673 段 `cd <WS>`」这个量级立论，
  不能说「86% 是冗余」；真 token 没算（只有字符近似，且结果长度是上界）。
  产物 [`research/01-shape-inventory.md`](research/01-shape-inventory.md) + 一行可复跑脚本
  `python3 .scratch/tool-coverage/research/01-shape-inventory.py --top 80`。
- [grep 的上下文参数：`after` / `before` 的形状](issues/02-grilling-grep-context.md) — 输出**照抄
  ripgrep**：命中 `path:line:text`、上下文 `path-line-text`（真实行号）、块间 `--`，相邻命中
  重叠的上下文合并成一块。只加两个可选非负整数 `after` / `before`，负数是参数错误，
  **不设上限也不夹取**。`MAX_MATCHES = 500` 的语义改成**「最多列出的行」= 命中行 + 上下文行合计**
  （常量不动、不加第二把刀），末尾那句改成数**未列出的命中行数**；`count: true` 与它们同给时
  **忽略**。不加 `context`（冻结项 7）、不加 `files_only` / `invert`（留 05）、不加 `path`
  （留 05，0.9% 不值得把 `ReadOnly` 拖进区内外之分）。描述从"劝阻"改成正向用法说明，
  留一句短的。要同步改 [`grep-tool/spec.md`](../grep-tool/spec.md) 第 205 行那条《明确不做》
  （改成已推翻 + 实测：`-A` 903 / `-B` 202 / `-C` 1、带上下文占 grep/rg 段 23.2%）与
  [`docs/grep.md`](../../docs/grep.md)（参数表、输出形状、上限语义、计数模式互斥）。
  不立 `CONTEXT.md` 词条。与前一条改动是否合并成一次发布（省一次前缀缓存作废），
  由 [workdir 必传](issues/03-grilling-bash-workdir-required.md) 一起拍。
- [bash 的 workdir 必传与 cwd 反馈](issues/03-grilling-bash-workdir-required.md) — `required` 改成
  `["command", "workdir"]`；**允许 `.` 表示工作区根**（charting 时"不允许 `.`"那条**撤回**：
  根需要一个自然写法，而空串 / 绝对路径 / 哨兵都更差；强制力来自字段必填，不是禁用根）。
  结果**每次首行**加 `cwd: <相对工作区路径，根写作 .>`（无条件 —— `cd "$(pwd)"` 517 段的痛点
  就是不确定自己站在哪）。`cd` 冗余**只改描述**，不加提示也不拒绝（含 `cd` 的命令占 85%，
  提示会变噪声；`cd` 在 `/tmp`、子 shell、`for` 里是合法用法）。**取消 `workdir` 与 `escalation`
  的既有互斥** —— 必传后两者永远同现，那条规则会打死每一次升级调用；原意图（防止借 workdir
  自选工作区）在必传 + 严格解析之后不再成立。`WORKDIR_NOTE` 重写成"必填 + 根写 `.`"。
  与 grep 的参数改动**合并成一次发布**（少作废一次前缀缓存），落地次序写进 spec 的交付说明、
  不加依赖边。要同步改 [`bash-workdir/spec.md`](../bash-workdir/spec.md) 第 30 行的 `required`
  明文与 §2 的互斥条。动机口径用「`cd <WS>` 5673 段 / 5604 调用」，不许写"其中 86% 是冗余"。
- [git 工具：一条入口与 op 分档](issues/04-grilling-git-tool.md) — 一条 `git` 工具，schema 只有
  `op`（枚举：读 `status`/`diff`/`log`/`show`、写 `add`/`commit`/`stash`）+ `args` 原样透传；
  `effect()` 按 op 分档：读 `ReadOnly`、写 `Exclusive`（`add`/`commit` 写 `.git/index`，
  **枚举不出 `WritePaths`**）。`rev-parse`/`branch`/`ls-files` 与那组搭测试仓库的
  `config`/`init`/`user`/`safe`/`core`（441 段命令含 `/tmp`）**不进 op**，留给 bash。
  输出原样 + 走既有截断流水线；**不加 `cwd:` 首行**（会话 cwd 固定）；**不给 `workdir`**
  （`git -C` 实测 0 次，要换目录就透传 `-C`）。与渲染层 [`changes.rs`](../../src/render/changes.rs)
  / [`hostproc.rs`](../../src/render/hostproc.rs) **只共用"起只读子进程拿 stdout"那一层**，
  不共用解析与上色；模型工具**必须过权限门与沙箱**，渲染层那条明确不过 —— 这条分界写进 spec。
  **一条落地约束**：写 op 与读 op **同批声明、同批实现**（声明了不实现会让模型拿到失败，
  分两批发布则多废一次前缀缓存），冻结项 9 的"实现可后置"按这条收窄。要新增
  `docs/git.md`、同步 README 的工具清单；不立 `CONTEXT.md` 词条。
- [收编的逐条判定：文件目录、命令结果、窗口读](issues/05-grilling-intake-verdicts.md) — 八类候选
  **做 1 不 7**：只有**文件枚举**（`ls` 728 段 + `find` 104）判做，升级成
  [文件枚举工具](issues/07-grilling-file-enumeration.md)；**窗口读**（2627 段）判不做工具、
  只改 `read_file` 描述 —— 01 给它的成本档 1 **不成立**（没有参数可加，`offset`/`limit` 早就有）；
  **命令输出过滤**（stdin 类 8558 段、占全部段 24%）判不做、坦然留 bash，但留一条**条件项**
  （累积采纳数据后重判泛化 `run(command, filter)`，进「尚未明确」）；**写文件与就地改写**
  （634 段）判不做（`edit_file`/`write_file` 已覆盖，而根因"模型为什么不用"读不到，不开猜测性工具）；
  `python3 << HEREDOC`（1135）、会话日志挖掘（52）、`curl`（67–104，已有 `web_fetch`）、
  PTY（5 段）一律不做；写侧 `mkdir`/`rm`/`mv`/`cp` 不做。「不做」清单连理由整段进 spec 的
  对应一节。
- [验收口径：怎么量调用数、段数与 token](issues/06-grilling-measurement.md) — 三个量都报，
  主判据三个：**每会话 bash 调用数中位数** + **段数/调用（不切管道切法）** + **命令与结果字符数
  中位数**（token 用字符近似，不碰 `UsageRecorded`）；bash 占比作辅。**基线钉成文件**
  [`research/06-baseline-2026-10-10.txt`](research/06-baseline-2026-10-10.txt)（274 会话 / 7703 调用 /
  35809 段），**存档后不再重跑**；改动后只看发布日（含）之后的会话、累计 **≥ 20 个含 bash 的会话**
  再判（样本本来就偏：173 个会话一条 bash 都没有，两天占 63%，所以用中位数）。冲突判据：
  token 升 **> 10%** 才要求解释，记为「待观察」、不阻塞。**已落地**：调研脚本加了
  `--since YYYY-MM-DD`（默认行为未变），验收是一条命令 ——
  `python3 .scratch/tool-coverage/research/01-shape-inventory.py --since <发布日> --top 120`。
- [文件枚举工具：只读的 `ls` / `find` 替代](issues/07-grilling-file-enumeration.md) — 形状由折 spec
  那一步定下并写进 [`spec.md`](spec.md) §4：**一个字段 `pattern`**（相对会话 cwd 的 glob、支持 `**`，
  不加 `path`）、名字 **`glob`**、**忽略规则与 `grep` 同一套**（遵守 `.gitignore`、跳过隐藏文件）、
  输出一行一条相对路径且**按路径排序**、上限 **`MAX_PATHS = 500`** + 可操作收尾、
  **`effect() = ReadOnly`**、不做 `tree` 层级、描述里写清与 `repo_map` / `grep` 的三方分工。
  三处按惯例推断（名字 / 忽略规则 / 排序口径）在 spec 的《补记》里点明，改起来只需动 §4。

## 尚未明确

<!-- 通往目的地、但还不够清晰到能成票的视野。解决一张票会把它前方的一片 fog 升级成新票。
     图已走完：下面第一条不是未决的 fog，而是**有触发条件的暂缓项**（见 spec 的《明确不做》）。 -->

- **泛化的 `run(command, filter)`**：`cargo test … | grep` 那类占管道 grep 的大头（stdin 类
  8558 段）。[收编判定](issues/05-grilling-intake-verdicts.md) 的裁定是"先不做"，
  **触发条件 = 累积一段时间的采纳数据之后重判** —— 那时才分得清"模型不用工具"与"根本没有工具"。
- **「输出整形」要不要成为一条通用约定**：`grep` 的截断收尾、`repo_map` 的「省掉了多少」已经形成
  惯例，新工具（文件枚举、`git`）要不要统一照抄。
- **`effect()` 按参数分档要不要写进规范**：现在有两处先例（`mcp_call`、将要有的 `git`），
  够不够格成为 `docs/` 里的一条约定。
- **新工具与既有工具的边界**：`repo_map`（有哪些符号）、`grep`（某个名字在哪）、文件枚举
  （有哪些文件）、`git`（仓库状态）各答哪一问；`CONTEXT.md` 是否要吃新词条。

## 范围之外

<!-- scope 边界，不是路线的一步：封闭的结论，永远不 graduate。 -->

- **工具名前缀统一**：维护者明示「后续再做」，是另一条 effort；它与本图的 destination 不同轴
  （命名规范 vs 按用量覆盖）。
- **实现落地**：本图只产 spec 与决定；构建切片由 `/to-tickets` 在图外拆、由 `/implement` 认领。
- **替换或删掉 `bash`**：冻结项 5 是收敛不是消灭；管道、命令组合、一次性脚本仍然只有它接得住。
- **给 `grep` 加 stdin / 管道能力**：那是让 `grep` 变成第二条 bash，`Effect::ReadOnly` 的前提
  （只读工作区）当场失效。命令输出的过滤另找归宿。
- **权限模式与沙箱的档位语义**：`workspace` / `ask` / `readonly` 的定义不在本图内；
  本图只在"新工具的 `effect()` 该是哪一档"上做决定。
- **`repo_map` 的改造**：它已有自己的 spec 与文档，本图不重开。
- **`git-worktree`（会话级 worktree 隔离）**：那是另一条 seed，与"给模型一条 git 工具"无关。
- **`diff-page`（左栏改动页的宿主侧取数）**：它是渲染层，不变量是"模型可见文本一个字节不动"；
  本图只处理模型侧工具，两者不共享实现。

## 进度

**charting 完成（2026-10-10）**：四轮 grilling 定下 destination、判据、范围、计数单位、
收编语义、预算策略与三个已定方向的形状；建出 6 张票。

**01 已走完（同日）**：[形态底账](issues/01-research-bash-shape-inventory.md) 的 subagent 当天派出、
当天收回；产物落 [`research/`](research/)（300 行报告 + 924 行可复跑脚本，一行命令只写 stdout、
不落中间文件）。它把票面散数与实测的差异解释清楚（全切/不切管道两种切法），并给出下游两条硬边界：
`cd` 冗余率判不了、真 token 没算。**05 / 06 随之解锁。**

**02 已走完（同日）**：[grep 的上下文参数](issues/02-grilling-grep-context.md) 一轮五问收口 ——
输出照抄 ripgrep、`after`/`before` 两个可选整数、500 改成"列出的行"（含上下文行）、
`count` 与它们互斥（忽略）、不加 `files_only`/`invert`/`path`、描述改成正向用法说明；
并钉下要同步改的两份文本（`grep-tool/spec.md:205` 的《明确不做》与 `docs/grep.md`）。

**03 已走完（同日）**：[workdir 必传与 cwd 反馈](issues/03-grilling-bash-workdir-required.md) 一轮五问收口，
并**修正了 charting 的一条冻结项**（冻结项 8 里"不允许 `.`"撤回 —— 根需要一个自然写法，
改判为必填 + 允许 `.`），另发现并取消了 `workdir`/`escalation` 的既有互斥（必传后两者永远同现）。

**04 已走完（同日）**：[git 工具与 op 分档](issues/04-grilling-git-tool.md) 一轮五问收口 ——
一条 `git` 工具、`op` 枚举 + `args` 透传、effect 按 op 分档（读 `ReadOnly` / 写 `Exclusive`）、
不加 `cwd:` 首行、不给 `workdir`、与渲染层只共用"起只读子进程"那一层。写票时浮出一条落地约束
（声明与实现必须同批，否则"声明了不能用"或"多废一次前缀缓存"二选一），已写进作答待确认。

**05 已走完（同日）**：[收编的逐条判定](issues/05-grilling-intake-verdicts.md) 逐条判了八类候选 ——
**做 1 不 7**：只有文件枚举升级成 [文件枚举工具](issues/07-grilling-file-enumeration.md)；
窗口读不是能力缺口（`read_file` 早有 `offset`/`limit`，只改描述）；命令输出过滤坦然留 bash，
但留一条条件项进「尚未明确」；写文件、`python3` heredoc、日志挖掘、`curl`、PTY 全部不做，
结论连理由进 spec 的「不做」一节。

**06 已走完（同日）**：[验收口径](issues/06-grilling-measurement.md) 一轮四问收口，并把口径
**当场落成可跑的东西** —— 调研脚本加了 `--since`（5 处改动，默认行为未变），基线存档
[`research/06-baseline-2026-10-10.txt`](research/06-baseline-2026-10-10.txt)（284 行），
验收从此是一条命令。

**07 已走完（同日）**：形状由折 spec 那一步定下（见上一条），写进 [`spec.md`](spec.md) §4。

**实现票已拆（同日）**：`/to-tickets` 从 spec 拆出五张 tracer bullet ——
[08 `grep` 的上下文行](issues/08-grep-context-lines.md)、
[09 `bash` 的站位必传](issues/09-bash-workdir-required.md)（**与 08 同批发布**）、
[10 一条 `git` 工具](issues/10-git-tool.md)、
[11 文件枚举 `glob`](issues/11-glob-tool.md)、
[12 文档索引与验收收口](issues/12-docs-and-acceptance.md)（被前四张 block）。
拆票时修正了 spec §3 的一处说法：与渲染层**代码不共用**，不需要 prefactor。

**图走完了（7/7，2026-10-10）**：目的地 = 一份可执行的 spec，已折好并打上 `ready-for-agent` ——
[`spec.md`](spec.md)：四项改动（`grep` 的上下文参数、`bash` 的 `workdir` 必传 + cwd 回显、
一条 `git`、一条文件枚举）+ 一条验收 + 八条《明确不做》。下一步 `/to-tickets` 拆实现票、
`/implement` 认领。唯一的**暂缓项**（泛化 `run(command, filter)`）留在《尚未明确》里，
它不阻塞 —— 触发条件是采纳数据。
