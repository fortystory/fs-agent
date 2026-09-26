# fs-agent

```text
  ▄▀▀█ ▄▀▀█      ▄▀▀▄ ▄▀▀▀ ▄▀▀█ █  █ ▀█▀
  ▓▄▄  ▓         ▓▄▄▓ ▓ ▀▓ ▓▄▄  ▓▄ ▓  ▓   fs
  ▒     ▀▀▄ ▀▀▀▀ ▒  ▒ ▒  ▒ ▒    ▒ ▀▒  ▒   forked synthesis · 分叉合成
  ░    ░  ░      ░  ░ ░  ░ ░  ▄ ░  ░  ░   two forks, one stem
  ▀    ▀▀▀       ▀  ▀  ▀▀▀  ▀▀▀ ▀  ▀  ▀
```

**fs** = **Forked Synthesis**（分叉合成）：两个异构讨论者**分叉**作答，合成器把分叉**收束**成共识 / 分歧 / 未决——前者是机制，后者是产出。`fs-agent` 即「分叉合成的 agent」；`fs` 只是叙述里的框架名，**不是**命令行或路径的一部分（命令仍是 `fs-agent`，落盘仍是 `~/.config/fs-agent/`）。`Forked` 的「分叉」只指讨论协议这一步，与 Out of Scope 里的 `fork/rewind` 手势无关。

自用 coding agent CLI（Rust，从零实现）。核心是**一条只追加的事件流**加**每个 agent 自己的窗口**：事件流是会话的唯一真相源，每个讨论者 / 执行者这次调用要重放的 `messages` 都是从事件流加投影规则**重算**出来的纯函数产物——没有隐藏状态，历史永远可复盘。

> 术语以 [`CONTEXT.md`](CONTEXT.md) 为准：叙述、文档与 UI 用中文（讨论者 / 执行者 / 事件流 / 投影 / 会话 / 轮次 / 回合 / 技能），代码标识符用其中的英文名（`Debater` / `Executor` / `EventLog` / `project()`）。

## 它解决什么

- **单一模型的判断没有对照。** 两个**异构**讨论者（KIMI + DeepSeek）各自作答，只在结论真的冲突时开一轮定向第二轮，最后合成器画出「共识 / 分歧（含各自成立的前提）/ 未决」的选项空间——决定权在你，不在一个自动收敛的投票器。
- **探索和动手是两种活。** 执行者是讨论者用内建 `task` 工具派出的子 agent，带自己的 `parent_id` 与轮数预算（默认 25）；它的事件落在**同一条流**里，但过程默认不进任何讨论者的窗口，结论经派发者自己的发言进入讨论。
- **上下文预算是硬的。** 窗口（按每个 agent 各自的模型算）、轮数、会话累计 token、费用是四个不同的量，各有各的语义，撞顶时**降级收尾**而不是随机截断。
- **计划是模型的工具，不是一档权限。** 模型用内建 `todo` 工具维护自己的待办列表（一次提交整份，列表就在那次调用的参数里），左栏的 `todo` 标签把它显示出来；「能不能写」则由**你**那一档决定（`Shift+Tab` 循环 `readonly` / `ask` / `auto`）。
- **权限、秘密、可撤销。** 三个内置模式、断路器短路拒绝、cwd 路径限制、`.env` 家族默认拒、密钥在**入流前**打码、会话目录 `0700`、root 拒绝启动；每次 `edit_file` 都能 `/undo` 原样退回，且不碰你的 git。
- **要能复盘。** `sessions show / replay / stats` 只从会话自己的事件流回答「这一轮为什么停」「谁在哪一轮改了哪个文件」「这次编辑走了降级匹配吗」。

**状态**：v1 的 **32 张**实现票全部 `done`（含收尾审查补记的 30/31/32），`todo-and-modes` 的 **4 张**也已落地（模式回到三档、计划交给模型的 `todo` 工具，见 [ADR 0003](docs/adr/0003-plan-leaves-the-permission-modes.md)）；`src/` **31,404** 行、`tests/` **29,171** 行（`wc -l`）、**756** 条测试（`cargo test` 的 passed 合计）。讨论的 CLI 入口已经接上：`fs-agent discuss "问题"` 起一次多角色讨论（讨论者是配置里的「人物」池，一次讨论抽两个、3 或 5 次调用）；活会话里也能用 `/discuss` 就地讨论。库层的组装入口仍是 `assemble` / `assemble_discussion`。

## 快速开始

### 前提

- Linux x86_64（v1 只在这上面验证；**不做进程级沙箱**）
- **非 root**：以 root / sudo 启动一律拒绝，且没有 bypass flag
- Rust（edition 2021）
- 至少一个 provider 的 key

### 构建

```sh
cargo build --release
# 或者装到 PATH
cargo install --path .
```

### 配置

配置在 `~/.config/fs-agent/config.toml`（认 `XDG_CONFIG_HOME`）。优先级是 **`config.toml` > 已导出环境变量 > 内置默认**；**项目里的 `.env` 永远不会被加载**，所以 clone 下来的仓库改不了你的行为。

内置三个 provider profile（Kimi 的两套系统 key 互不通用，所以是两个）：

| profile | `base_url` | key 环境变量 |
| --- | --- | --- |
| `kimi` | `https://api.moonshot.cn/v1` | `MOONSHOT_API_KEY` |
| `kimi-code` | `https://api.kimi.com/coding/v1` | `KIMI_API_KEY`（或 `KIMI_CODE_API_KEY`） |
| `deepseek` | `https://api.deepseek.com` | `DEEPSEEK_API_KEY` |

内置模型 id：`kimi-k3`、`k3`、`k3-256k`、`kimi-for-coding`、`kimi-for-coding-highspeed`、`deepseek-v4-pro`、`deepseek-flash`。默认 `kimi-k3`（也可用 `FS_AGENT_MODEL` 覆盖）。**未登记的 model id 在启动时报错，不静默降级。**

一份够用的配置：

```toml
# ~/.config/fs-agent/config.toml
default_model = "kimi-k3"

# key 也可以只导出环境变量；写在 base_url 旁边即显式配对
[providers.kimi]
api_key = "sk-..."

[models.kimi-k3]
temperature = 0.6
# reasoning_effort = "high"   # 会话开始前定死，中途切档会废掉前缀缓存

[turn]                        # 回合上限：一个回合最多多少次 provider 调用（spec §3 / §16）
# max_iterations = 1000         # 一个回合的调用上限（默认 100），讨论者也走这条；
#                               # 长任务被默认值半路掐死就调它
# executor_max_iterations = 500 # 执行者自己的回合上限（默认 25）：独立计数，
#                               # 所以一个失控的执行者吃不掉会话的回合预算

[budget]
session_tokens = 2000000      # 会话累计 token 上限（讨论者 / 执行者 / 合成器共用）
estimate_margin = 1.5         # 发出去之前的估算宽容倍数

[routing]                     # 弱模型分流只有两个落点；讨论者绝不被路由
# synthesizer_model = "deepseek-flash"
# executor_model = "deepseek-flash"

[discussion]                  # 讨论者「池子」：`/discuss` 从里面抽两个
debaters = ["kimi-k3", "deepseek-v4-pro"]     # 简写：名字就是模型 id
# 也可以给人设起名 + 写「灵魂」（性格 / 立场，整场讨论都照它来）：
# [[discussion.debaters]]
# name = "张三"
# model = "deepseek-v4-pro"
# soul = "法外狂徒，思路不受限制：先质疑规则，再谈方案"
# [[discussion.debaters]]
# name = "李四"
# model = "deepseek-flash"
# soul = "守法好公民：先找依据，再评估风险"
# `soul` 只给它自己看（对方的窗口里没有），并在流上留一份，`sessions replay` 可重算
# 池子多于两个时，每次讨论随机抽两个；`--debaters 保守,激进` 指定抽哪两个。
# 不同厂商最好；同厂商、甚至同一个模型也能跑（多样性会弱，会有一行提示）。
# max_rounds = 2              # 独立首轮 + 至多一次定向第二轮（默认 2）

[pricing.deepseek-flash]      # USD / 百万 token；费用只作显示，闸门只数 token
miss_input = 0.28
cached_input = 0.028
output = 0.42
```

动态工具（不写 Rust 就能加一个工具，线级名是 `custom__<命名空间>__<工具>`，argv 模板**不经 shell**）：

```toml
[tools.git.status]
description = "Show the working tree status as porcelain."
command = ["git", "status", "--porcelain"]
parameters = { type = "object", properties = {} }
```

详见 [`docs/custom-tools.md`](docs/custom-tools.md)。

### 跑

```sh
fs-agent                        # 交互会话：终端上用 TUI，管道里用 plain 转录
fs-agent --plain                # 强制 plain
fs-agent --continue             # 继续本工作区最新的会话（会话 id 不变，前缀缓存继续命中）
fs-agent --model deepseek-v4-pro
fs-agent --cwd /path/to/repo
fs-agent discuss "把权限模型换成 X，风险在哪？"   # 两个异构讨论者 + 合成器
fs-agent discuss --plain "…" 2>/dev/null          # 只要合成产物（讨论过程走 stderr）
fs-agent --help
```

会话里：`/undo` 回滚上一次编辑、`/discuss [--debaters A,B] [问题]` 就在**这个会话里**起一场多角色讨论（讨论者用本会话的上下文各自作答，事件写进同一条流；`--debaters` 指定池子里的哪两位，不写就随机抽两个；不带问题就用最后一个问题）、`/<技能名> [任务]` 直接运行一个技能（包括标了 `disable-model-invocation: true` 的；不带任务就按技能正文立刻开工）、`/quit` 退出；TUI 里输入 `/` 会弹出补全窗口（命令 + 技能，跟随光标、按已输入的字符过滤，`Tab` 只补全、回车补全并提交），**Esc** 取消正在跑的回合、**Shift+Tab** 在 `readonly` / `ask` / `auto` 三档权限模式之间循环（当前档位就在状态行上）。写类工具要不要问、`readonly` 档下能不能写，全由这一档决定；`--mode` 旗标与 `[permissions] mode` 是它的两个入口。

### TUI 长什么样

alt screen 全屏，**一圈外框 + 一条全高左栏 + 一条主列**（[ADR 0002](docs/adr/0002-fullscreen-alt-screen-tui.md)）：

```text
┌────────────────────────────────────────┬─────────────────────────────────────────────────────────────────────────────┐
│ ▄▀▀█ ▄▀▀█      ▄▀▀▄ ▄▀▀▀ ▄▀▀█ █  █ ▀█▀ │fs-agent：会话 01J8ZQ4K7M · 模型 kimi-k3 · 模式 询问 · ~/code/fs-agent       │
│ ▓▄▄  ▓         ▓▄▄▓ ▓ ▀▓ ▓▄▄  ▓▄ ▓  ▓  │[用户] 看看这个仓库现在什么样                                                │
│ ▒     ▀▀▄ ▀▀▀▀ ▒  ▒ ▒  ▒ ▒    ▒ ▀▒  ▒  │[kimi] 回合开始（第 1 次迭代）                                               │
│ ░    ░  ░      ░  ░ ░  ░ ░  ▄ ░  ░  ░  │[kimi] ▸ ✓ 思考完成                                                          │
│ ▀    ▀▀▀       ▀  ▀  ▀▀▀  ▀▀▀ ▀  ▀  ▀  │[kimi] 工作区是干净的。                                                      │
├────────────────────────────────────────┤[kimi] ▸ 调用 bash 查看 git status                                           │
│调用量│todo│轨迹│文件───────────────────│[kimi] 用量 in=12345 out=3345 cached=9000 miss=3345                          │
├────────────────────────────────────────┤[kimi] 回合结束：完成                                                        │
│上下文            12,345 / 200,000（6%）│                                                                             │
│token                   15,690 / 100,000│                                                                             │
│回合                                   1│                                                                             │
│输入                              12,345│                                                                             │
│输出                               3,345│                                                                             │
│缓存                       9,000 / 3,345│                                                                            ┃│
│                                        ├─────────────────────────────────────────────────────────────────────────────┤
│                                        │ 模型 kimi-k3 │ 模式 询问 │ 上下文 6%                                        │
│                                        ├─────────────────────────────────────────────────────────────────────────────┤
│                                        │❱                                                                            │
│                                        │                                                                             │
│                                        │                                                                             │
│                                        ├─────────────────────────────────────────────────────────────────────────────┤
│                                        │enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 模式 · ctrl-c/ctrl-d 退出    │
└────────────────────────────────────────┴─────────────────────────────────────────────────────────────────────────────┘
```

**左栏（全高）**放身份与读数：宽档（≥ 120 列，40 列宽）画 fs 标记（5 行字符画，亮品红→品红渐变），窄档（80–119 列，28 列宽）退成一行 `fs-agent <版本>`，再窄就**整栏隐藏**、转录吃掉全部宽度。界面里**唯一在动的东西是输入区的提示符**：它是 `❱ `，**fs-agent 干活时**颜色一直在走 —— 色相每 3.3 秒绕一圈、饱和度同时以 2.1 秒的周期轻轻呼吸（24 位真彩，取值来自一条自用脚本）；**轮到你自己打字时它停住**，停在那个固定的静止色上，一眼就能分清「它在想」和「该我说了」。左栏的 mark **完全静止**（下落动画做过、看下来不好看，已关掉，代码留着）。标记下面是 tab 条（`调用量` / `轨迹` / `文件`，**用鼠标点**切换，后两页还没做、写一句占位），再下面是六个读数（上下文 / token / 回合 / 输入 / 输出 / 缓存）。**`todo` 是第四个标签，而且是有条件的一个**：会话里第一次出现非空待办列表时它插到 `调用量` 右边，此后**不再消失**（全做完、被清空都留着——标签在读者眼皮底下消失会把页面挪走）。那一页一行一项（`☐` 待办 / `▸` 进行中 / `✓` 已完成）+ 一行 `已完成 2/5`；不滚动，装不下的项用一行 `＋3 项` 交代。执行者调 `todo` 只在转录里留一行，**不上左栏**。**去留由宽度决定，内容由高度决定**：高度不够时先丢标记、再丢身份行、最后从尾部丢字段，上下文 / token / 回合这三行最后才走。

**主列**自上而下是：转录（右缘恒留两列 —— 滚动条与**回合条**）→ 状态行（`模型 … │ 模式 … │ 上下文 …%`，按宽度先丢模型、再丢模式，**这一行永远在**）→ 输入区（**最少三行**，草稿在第 4 行才继续把它撑高、10 行封顶；提示符 `❱ ` 会变色）→ 提示行。**回合条**一格一个回合（讨论会话一格一个轮次）：最新的一格贴底、视口所在的那一格是亮色 `┃`、其余是暗色 `┊`，溢出的一端画 `⋮`；窗口跟着焦点走，所以任何滚动位置上都有一格是亮的，点一格就跳回那一轮**你自己敲的那句**。**cwd 与时钟不再显示**（它们随旧顶栏一起退场）。

降级阶梯是：左栏收窄 → 左栏隐藏 →（左栏内部按高度丢内容）→ 最小 40×10，再小只剩一句 `终端太小：至少 40×10`。块与块之间**不留空行**，那几行留给了转录：120×24 下转录有 14 行（3 行归输入区，7 行归外框与三条分隔线、状态行、提示行）；40×10 那一档转录只剩最后 1 行，因为**转录的底线比输入区的最小高度优先**（输入区在那档只有 2 行）。

转录里**中间过程是折起来的**：思考只留一行 `[kimi] ▸ ✓ 思考完成`，工具调用只留一行 `[kimi] ▸ 调用 bash 查看 git status`——描述从参数推出（`查询`/`查看`/`修改`/`运行` + 第一个路径或子命令），命令全文与输出都不铺在屏幕上。**点这两行的 `▸`** 打开详情覆盖层（居中于主列，边框是被点那行说话人的颜色）：思考全文、工具参数、输出全文分节显示，可用 `PgUp`/`PgDn` 或滚轮翻，`Esc` 或点框外关掉；转录停在原处不动。

说话人名字按角色着色（讨论者 1 浅青、讨论者 2 浅品红、执行者 浅黄、用户 浅绿、系统 灰），正文保留原来的语义色。鼠标还能**点击作答**：权限 / 粘贴 / 清草稿三种覆盖层的候选键，以及 `ask_user_question` 问卷的每个选项行、翻页与提交。键盘上 `Ctrl-C` 忙时取消、闲时退出，**`Ctrl-D` 闲时弹退出确认**（忙时忽略）。

### 子命令

| 命令 | 作用 |
| --- | --- |
| `fs-agent discuss "问题" [--plain\|--tui] [--config PATH] [--cwd PATH] [--debaters A,B]` | 起一次多角色讨论：从 `[discussion] debaters` 池子里抽两个讨论者（`--debaters` 指定），各自独立作答（不同厂商最好；同厂商或同模型也允许），只在结论冲突时开一轮定向第二轮，最后由合成器画出共识 / 分歧 / 未决。讨论落在真会话里，`sessions show` 可复盘 |
| `fs-agent probe [--model ID]...` | 对每个已配置 key 的模型发两次真实请求，打印归一化用量，用来看前缀缓存是否命中 |
| `fs-agent prune [--keep N] [--cwd PATH] [--dry-run]` | 手动删除本工作区的会话目录，保留最新 N 个（默认 1）。除此之外没有任何东西会删你的会话 |
| `fs-agent sessions ls [--all] [--limit N]` | 列出本工作区（`--all` 为全部桶）的会话 |
| `fs-agent sessions show <id> [--round N] [--speaker X] [--kind K] [--tool T] [--only-error] [--files]` | 按轮次分组的转录，工具调用与结果归成一处；`--files` 是「谁在哪一轮改了这个文件」的工作区对象视图 |
| `fs-agent sessions replay <id> --speaker X [--round N] [--model ID]` | 只从事件流**重算**某一次调用实际发给 provider 的内容——调投影 bug 的唯一手段 |
| `fs-agent sessions stats <id> [--model ID]` | 固定指标集：token、费用、单侧缺席率、编辑匹配梯降级分布等 |

`--json` 都可用；**stdout 只放结果，诊断走 stderr**，所以能直接接进管道。这些命令是给人用的，不是给 agent 的新工具。

会话落盘在 `~/.local/share/fs-agent/sessions/<cwd-slug>/<session-id>/`（认 `XDG_DATA_HOME`），一个会话就是一个可搬运的目录：JSONL 事件流 + `outputs/`（工具输出落盘）。目录 `0700`、文件 `0600`。

## 安全模型

三个内置模式：

| 模式 | 判据 |
| --- | --- |
| `readonly` | 非 `ReadOnly` 调用一律 `Deny`；`bash` 也拒（shell 里能写文件） |
| `ask` | 写类 `Ask`、只读 `Allow`——交互式的默认档 |
| `auto` | 默认 `Allow`，但**不等于跳过权限**：断路器、规则、hook 照常生效 |

模式是**会话的一档值**、不进事件流：三个入口是 `config.toml` 的 `[permissions] mode`（默认 `ask`）、`--mode readonly|ask|auto` 覆盖它、以及会话里 `Shift+Tab` 循环三档（`--continue` 回到配置里的那一档，审计看 `PermissionDecided.reason`）。切档**不注入任何东西**，所以模型不会事先知道档位变了，它是第一次被拒（理由里写着档位）才知道的——这是为了不掉前缀缓存换来的代价（[ADR 0003](docs/adr/0003-plan-leaves-the-permission-modes.md)）。三档之外没有第四档：**计划**是模型自己的 `todo` 工具，不是一种权限。

四条对每个模式都成立：① 断路器短路在规则之前，任何模式都翻不动；② hook **只能收紧**，永远不能放松一个 `Deny`；③ 沿委派链向下传播的是 `Deny` / `Ask`，`Allow` 不传播（所以「派个子 agent 去写」绕不过你的拒绝）；④ 无交互渲染器时 `Ask → Deny`（脚本不会挂住等你）。

另外：模型给的路径被限制在会话 cwd 及其子树；`.env` 家族默认拒绝（`*.example` / `*.sample` / `*.template` 除外）；密钥**在入流前**按值打码（流水线是 `打码 → 截断 → 落盘`，于是**流上的文本 == 模型看到的文本**，而工具执行仍拿真值；`outputs/*.txt` 打码，`outputs/*.before` 不打码——它是 `/undo` 的字节级还原源）；以 root 启动直接拒绝。

**这不是沙箱。** v1 不做进程级隔离，对「模型把密钥发到网上」基本无能为力；升级路径写明是「只做 Linux 的 bubblewrap」，且**不预做抽象**。真正的边界是别把赔不起的 key 交给它。完整边界见 [`docs/credentials.md`](docs/credentials.md)。

> **沙箱：有一条明确的意向，尚未设计。** 「项目目录内自动、目录外要审批」这个想法在 shell 那一格卡住了 —— 没有进程级隔离就没有可靠的「越界」信号（候选方案与 DSH 的做法、一手引用见 [`.scratch/sandbox/seed.md`](.scratch/sandbox/seed.md)）。那一步做完之前不动权限模式。

## 架构

```
hook.pre → 权限门 → [询问] → dispatch → hook.post → 追加事件
```

- **只追加的事件流**：信封是 `{ seq, at, speaker_id, payload }`，`seq` 就是 JSONL 行号、是唯一身份。重新生成 / 撤销 / compaction 一律追加一条 `HistorySuperseded`，历史一条不改。增量文本**不进流**，它走传输层旁路直达渲染。
- **投影是纯函数**：`project(log, speaker, caps) → messages`，住在 provider 适配器侧、按模型能力表分叉。裁剪（`trim`）是投影**之后**的另一个纯函数，只读、日志一条不删。
- **`Session` 是唯一持有可变状态的值**（事件流句柄 + 名册 + 预算 + 策略 + read set）。执行者是带 `parent_id` 的嵌套 `Session`，事件追加到父流。**权限档位**就是策略里的一个值：三个入口（`[permissions] mode` / `--mode` / `Shift+Tab`）改的都是它，它**不进事件流**，所以 `--continue` 从配置那一档重新开始，审计看 `PermissionDecided.reason`。
- **策略是纯函数，控制流在循环里**：hook 输出的是**约束**、权限门输出的是**裁决**，两者在 `Allow < Ask < Deny` 上取上确界——类型里根本没有「放松权限」这个变体。
- **12 个顶层边界，只向下依赖**：`events` · `config` · `provider` · `tools` · `permissions` · `hooks` · `context` · `agent` · `discussion` · `session` · `render` · `cli`。`events` 零内部依赖；`discussion` 不碰 provider。
- **三个前端**（headless / plain / TUI）共用一条广播通道与一个转录层，启动时选定且互斥；headless 的 stdout **只有最终产物**。TUI 是外框 + 全高左栏 + 主列（[ADR 0002](docs/adr/0002-fullscreen-alt-screen-tui.md)）：≥ 120 列时左栏 40 列画标记，80–119 列退成文字身份，再窄整栏隐藏（见上面的「TUI 长什么样」）。

默认数值：单 agent 100 回合、执行者 25 回合（这两个用 `[turn]` 调）、同批执行者并发 5、单个工具结果 25k 估算 token、`repo_map` 1k（上限 4k）、`bash` 120s（上限 600s）。

## 文档

| 去哪看 | 是什么 |
| --- | --- |
| [`CONTEXT.md`](CONTEXT.md) | 正式词汇表（含名字：`fs` = Forked Synthesis / 分叉合成）。写文档、写代码、写票之前先看它 |
| [`.scratch/fs-agent-v1/spec.md`](.scratch/fs-agent-v1/spec.md) | v1 spec：问题陈述、用户故事、20 节实现决定、测试决定、明确的 Out of Scope |
| [`docs/`](docs/) | 逐面说明：[`bash`](docs/bash.md) · [`credentials`](docs/credentials.md) · [`custom-tools`](docs/custom-tools.md) · [`discussion`](docs/discussion.md) · [`executor`](docs/executor.md) · [`observability`](docs/observability.md) · [`render`](docs/render.md) · [`repo-map`](docs/repo-map.md) · [`skills`](docs/skills.md) · [`highlight`](docs/highlight.md) · [`tui-manual-checklist`](docs/tui-manual-checklist.md) |
| [`docs/adr/`](docs/adr/) | 不可逆的决定：中文 UI 与冻结的模型文本、全屏 alt screen TUI（含标记与其代价）、「计划」从权限模式里搬出来（模式三档 + 模型的 `todo` 工具） |
| [`docs/research/`](docs/research/) | 一手调研的**原始笔记**（`coding-agent-features.md` 是横向对比，`notes/` 下五份是上游正文，合计约 796KB）：材料，不是结论 —— 结论已折进 `.scratch/` 的 spec 与 `docs/` 的逐面文档 |
| [`.scratch/README.md`](.scratch/README.md) | **feature 索引**：一行一个 feature —— 是 spec 还是决策地图、一句话、票数与完成度 |
| [`AGENTS.md`](AGENTS.md) | agent 在本仓库工作时的约定（文档该往哪写、语言怎么选，也在这里指回本节）；细目在 [`docs/agents/`](docs/agents/)：[issue tracker](docs/agents/issue-tracker.md) · [triage labels](docs/agents/triage-labels.md) · [domain docs](docs/agents/domain.md) |

**约定**（新文档照这个走，别猜）：

- **写中文的**：`README.md`、`CONTEXT.md`、`.scratch/` 下的 spec / map / 票、`docs/adr/`、`docs/tui-manual-checklist.md` —— 面向**使用与流程**：怎么说、怎么验、为什么这么定。
- **写英文的**：`docs/` 下的逐面设计文档与 `docs/agents/` —— 面向**代码内部**：模块边界、不变量、代码在哪。代码标识符一律英文。
- **例外**：`docs/highlight.md` 是中文 —— 它回答的是「这个模块为什么留着、什么会让它回来」，读者是将来的接手人。
- 两条推论：**新增文档跟邻居走**；**跨语言引用保留标识符英文**（中文文档里也写 `Session`、`project()`）。

## 开发

```sh
cargo test                              # 全量测试（条数见上面的「状态」）
cargo clippy --all-targets
python3 scripts/tui-startup-check.py    # TUI 启动冒烟（需要真终端）
```

测试只测**外部行为**：断言的对象是**事件流**（JSONL 里的 `seq` + payload）与**工作区副作用**（磁盘上的文件、临时 git 仓库），不是内部结构；不断言 `at` 时间戳。唯一的 e2e 接缝是库的组装入口 `assemble` / `assemble_discussion`——provider、路径锁、渲染 sink、配置全部从参数注入，并且**库不读环境**。测试里所有 provider 都是假 provider（两家都没有 `seed`，真调用不可复现），所以测试没有网络依赖。

最重要的一条不变量：用 `sessions replay` 重算的投影**必须等于**当时实际发给 provider 的 `messages`。这是「事件流是唯一真相源」的验收，也是投影 bug 的唯一探测器。

## 明确不做

AST / tree-sitter 编辑、unified diff 编辑格式、原生多 provider 协议、MCP client、向量检索 / RAG、IDE 与 IM 集成、两进程渲染、syntect 的 C 路径、内置编辑器、交互式 transcript 浏览器、进程级沙箱、裁判 / 仲裁者、N > 2 的讨论者、fork / rewind 手势、shadow git、SQLite、全局会话索引、自动清理、每次编辑自动 git commit、compaction 的实现、把工具打包进 skill。理由逐条写在 [spec 的 `Out of Scope`](.scratch/fs-agent-v1/spec.md)——要动它们，先改 spec，而不是在实现里悄悄加一条路径。

## License

MIT，见 [`LICENSE`](LICENSE)。
