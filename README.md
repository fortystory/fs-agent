# 衡（heng）

```text
                 héng
                  衡
                           一套自用的 coding agent harness
                           forked synthesis · 分叉合成 · two forks, one stem
```

**衡（heng）**：一套自用的 coding agent harness（Rust，从零实现）。名字取「**衡量**」——合成器**不是裁判**，它只把两方判断放到秤上，画出共识 / 分歧 / 未决的选项空间，决定权在你。**分叉合成（Forked Synthesis）**是讨论协议那一步的机制名：两个异构讨论者分叉作答，合成器把分叉收束——前者是机制，后者是产出；它与「明确不做」一节里的 `fork/rewind` 手势无关。

自用 coding agent harness（Rust，从零实现）。核心是**一条只追加的事件流**加**每个 agent 自己的窗口**：事件流是会话的唯一真相源，每个讨论者 / 执行者这次调用要重放的 `messages` 都是从事件流加投影规则**重算**出来的纯函数产物——没有隐藏状态，历史永远可复盘。

> 术语以 [`CONTEXT.md`](CONTEXT.md) 为准：叙述、文档与 UI 用中文（讨论者 / 执行者 / 事件流 / 投影 / 会话 / 轮次 / 回合 / 技能），代码标识符用其中的英文名（`Debater` / `Executor` / `EventLog` / `project()`）。

## 它解决什么

- **单一模型的判断没有对照。** 两个**异构**讨论者（KIMI + DeepSeek）各自作答，只在结论真的冲突时开一轮定向第二轮，最后合成器画出「共识 / 分歧（含各自成立的前提）/ 未决」的选项空间——决定权在你，不在一个自动收敛的投票器。
- **探索和动手是两种活。** 执行者是讨论者用内建 `task` 工具派出的子 agent，带自己的 `parent_id` 与轮数预算（默认 25）；它的事件落在**同一条流**里，但过程默认不进任何讨论者的窗口，结论经派发者自己的发言进入讨论。
- **上下文预算是硬的。** 窗口（按每个 agent 各自的模型算）、轮数、会话累计 token、费用是四个不同的量，各有各的语义，撞顶时**降级收尾**而不是随机截断。
- **计划是模型的工具，不是一档权限。** 模型用内建 `todo` 工具维护自己的待办列表（一次提交整份，列表就在那次调用的参数里），左栏的 `todo` 标签把它显示出来；「能不能写」则由**你**那一档决定（`Shift+Tab` 循环 `readonly` / `ask` / `auto`）。
- **权限、秘密、可撤销。** 三个内置模式、断路器短路拒绝、cwd 路径限制、`.env` 家族默认拒、密钥在**入流前**打码、会话目录 `0700`、root 拒绝启动；每次 `edit_file` 都能 `/undo` 原样退回，且不碰你的 git。
- **要能复盘。** `sessions show / replay / stats` 只从会话自己的事件流回答「这一轮为什么停」「谁在哪一轮改了哪个文件」「这次编辑走了降级匹配吗」。

**状态**：v1 的 **34 张**实现票全部 `done`，此后每个 feature 也各自落了地，逐行的票数与完成度见 [`.scratch/README.md`](.scratch/README.md)，其中几条只剩**真机走查**（`ready-for-walkthrough`，清单在 [`docs/tui-manual-checklist.md`](docs/tui-manual-checklist.md)）。规模：`src/` **48,517** 行、`tests/` **46,844** 行（`wc -l`）、**1,366** 条测试（`cargo test` 的 passed 合计）—— 复核就跑 `wc -l` 与 `cargo test`。

## 快速开始

### 前提

- Linux x86_64（v1 只在这上面验证）
- **一个可用的 `bubblewrap`**：`bash` 与动态工具的沙箱靠它，没有它这两个工具会拒绝运行（[`docs/sandbox.md`](docs/sandbox.md)）
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

配置在 `~/.config/heng/config.toml`（认 `XDG_CONFIG_HOME`）。优先级是 **`config.toml` > 已导出环境变量 > 内置默认**；**项目里的 `.env` 永远不会被加载**，所以 clone 下来的仓库改不了你的行为。

内置三个 provider profile（Kimi 的两套系统 key 互不通用，所以是两个）：

| profile | `base_url` | key 环境变量 |
| --- | --- | --- |
| `kimi` | `https://api.moonshot.cn/v1` | `MOONSHOT_API_KEY` |
| `kimi-code` | `https://api.kimi.com/coding/v1` | `KIMI_API_KEY`（或 `KIMI_CODE_API_KEY`） |
| `deepseek` | `https://api.deepseek.com` | `DEEPSEEK_API_KEY` |

内置模型 id：`kimi-k3`、`k3`、`k3-256k`、`kimi-for-coding`、`kimi-for-coding-highspeed`、`deepseek-v4-pro`、`deepseek-flash`。默认 `kimi-k3`（也可用 `HENG_MODEL` 覆盖）。**未登记的 model id 在启动时报错，不静默降级。**

一份够用的配置：

```toml
# ~/.config/heng/config.toml
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

[ui]
number_style = "cn"           # "cn" 万/亿（默认）或 "si" k/M/G；小于 10000 仍是千分位
file_viewer = "builtin"       # "builtin" 内置只读预览（默认）或 "nvim" 在浮层里嵌一个 nvim
file_viewer_width = 135       # 浮层宽度上限（列）；只对 nvim 那一档有效，下界 20

[routing]                     # 弱模型分流只有两个落点；讨论者绝不被路由
# synthesizer_model = "deepseek-flash"
# executor_model = "deepseek-flash"

[discussion]                  # 讨论者「池子」：`/discuss` 从里面抽两个
debaters = ["kimi-k3", "deepseek-v4-pro"]     # 简写：名字就是模型 id
# 也可以给人物起名 + 写「灵魂」（性格 / 立场，整场讨论都照它来）：
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

文件页点开一个文件时，浮层里默认是**内置的只读预览**（渲染器读盘、高亮、带行号，瞬时、不起进程）。`[ui] file_viewer = "nvim"` 换成**一屏真的 nvim**：读你自己的 `~/.config/nvim`（`XDG_CONFIG_HOME` / `NVIM_APPNAME` 照常生效），只读（`-M -R`，状态行亮 `[RO]`）、不折行，键盘与鼠标都归它 —— `Ctrl-C` 或点浮层外面退出，`:q` 也行；宽度上限 `file_viewer_width`（缺省 135），起不来就回退内置预览。它不进事件流、不进模型上下文，也不过沙箱。

右侧统计里的数字默认写中文制式（`123.5万`），小于 `10000` 的仍写千分位（`9,999`）；`[ui] number_style = "si"` 换成 `k` / `M` / `G`。这一档只影响显示，`--plain` 的诊断行与 `sessions stats` 的输出不跟着换。

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
heng                        # 交互会话：终端上用 TUI，管道里用 plain 转录
heng --plain                # 强制 plain 转录
heng --tui                  # 强制 TUI（与 --plain 互斥）
heng --continue             # 接着跑本工作区最新的会话（会话 id 不变，前缀缓存继续命中）
heng -c 20261001T155845Z-7a69cbff   # 按 id 续指定的一场（不带 id 就是最新；先在本桶找、再全 store；它在别的工作区时会切到那个目录）
heng --session 20261001T155845Z-7a69cbff   # 同一个意思的显式拼写
heng --model deepseek-v4-pro
heng --config /path/to/config.toml  # 换一份配置文件
heng --cwd /path/to/repo
heng discuss "把权限模型换成 X，风险在哪？"   # 两个异构讨论者 + 合成器
heng discuss --plain "…" 2>/dev/null          # 只要合成产物（讨论过程走 stderr）
heng --help
```

会话里：`/undo` 回滚上一次编辑、`/discuss [--debaters A,B] [问题]` 就在**这个会话里**起一场多角色讨论（讨论者用本会话的上下文各自作答，事件写进同一条流；`--debaters` 指定池子里的哪两位，不写就随机抽两个；不带问题就用最后一个问题）、`/<技能名> [任务]` 直接运行一个技能（包括标了 `disable-model-invocation: true` 的；不带任务就按技能正文立刻开工）、`/quit` 退出。

TUI 里输入 `/` 会弹出补全窗口（命令 + 技能，跟随光标、按已输入的字符过滤，`Tab` 只补全、回车补全并提交），**Esc** 取消正在跑的回合（问卷立着时除外 —— 那里的 `Esc` 是「退出这次询问」，取消归 `Ctrl-C`）、**Shift+Tab** 在 `readonly` / `ask` / `workspace` / `auto` 四档权限模式之间循环（当前档位就在状态行上）。写类工具要不要问、`readonly` 档下能不能写、区外的写要不要停下来问一次，全由这一档决定；`--mode` 旗标与 `[permissions] mode` 是它的两个入口。

### TUI 长什么样

备用屏幕（alt screen）全屏，**一条左栏 + 一条主列**（[ADR 0002](docs/adr/0002-fullscreen-alt-screen-tui.md)）—— 没有外框，两栏之间只剩一条竖虚线：

```text
                                        ┆对话┆轨迹┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄
                                        ┆┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄
                 héng                   ┆
                  衡                    ┆                                                                         [用户]
                                        ┆                                                         看看这个仓库现在什么样
                                        ┆
                                        ┆[kimi]
┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┆工作区是干净的。
调用量┆文件┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┆
┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┆[kimi]
上下文 ▓░░░░░░░░░    1.2万 / 20万（6%） ┆我看了 git status：没有未提交的改动，也没有未跟踪的文件。
token  ▓▓░░░░░░░░    1.6万 / 10万       ┆
回合                                1   ┆
输入                            1.2万   ┆
输出                            3,345   ┆
缓存                    9,000 / 3,345   ┆
                                        ┆
                                        ┆模型 kimi-k3 ┆ 询问 ┆ 上下文 6% ┆ 🌑 就绪
                                        ┆┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄
                                        ┆❱
                                        ┆
                                        ┆
                                        ┆┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄
enter 发送 · ctrl-j 换行 · esc 取消 · shift+tab 模式 · PgUp/PgDn 滚动 · ctrl-o 左栏 · ctrl-c/ctrl-d 退出
```

**左栏**的宽度档、身份、六个读数与页签逐项在 [`docs/render.md`](docs/render.md) 的「外壳」一节，去留见 [`CONTEXT.md`](CONTEXT.md) 的「左栏」。

**主列**自上而下是：转录（右缘恒留两列 —— 滚动条与**回合条**）→ 状态行（四段 `模型 X ┆ 询问 ┆ 上下文 n% ┆ 🌑 就绪`；按宽度**先丢模型、再丢模式，最后剩上下文与状态词**，**这一行永远在**）→ 输入区（**最少三行**，草稿在第 4 行才继续把它撑高、10 行封顶；提示符 `❱ ` 会变色）→ 提示行。**回合条**一格一个回合（讨论里一格一个轮次）：最新的贴底、视口那格 `┃`、其余 `┊`，溢出的一端 `⋮`；点一格跳回那一轮**你自己敲的那句**。**cwd 与时钟不再显示**（它们随旧顶栏一起退场）。

降级阶梯：左栏收窄 → 隐藏 →（左栏内部按高度丢内容）→ 最小 40×10，再小只剩 `终端太小：至少 40×10`。块之间只在**换发言者**时空一行。120×24 下转录 **15 行**（主列页签条、两条分隔线、状态行与提示行合起来 6 行，另 3 行归输入区）；40×10 只够一边：转录 1 行、输入区 3 行 —— **转录的底线比输入区的最小高度优先**。**状态行紧贴转录最后一行**，中间那条线不画了。

转录里**中间过程是折起来的**：思考只留一行 `[kimi] ▸ ✓ 思考完成`，工具调用只留一行 `[kimi] ▸ 调用 bash 查看 git status`——描述从参数推出（`查询`/`查看`/`修改`/`运行` + 第一个路径或子命令），命令全文与输出都不铺在屏幕上。**点这两行的 `▸`** 打开详情覆盖层（**居中于屏幕**、虚线边框四角为空；发言者色落在**标题行**）：思考全文、工具参数、输出全文分节显示，可用 `PgUp`/`PgDn` 或滚轮翻，`Esc` 或点框外关掉；转录停在原处不动。

说话人名字按角色着色（讨论者 1 浅青、讨论者 2 浅品红、执行者 浅黄、用户 浅绿、系统 灰），正文保留原来的语义色。鼠标还能**点击作答**：权限 / 粘贴 / 清草稿三种覆盖层的候选键，以及 `ask_user_question` 问卷的选项行（点一下只**切换选中**、不翻页；翻页与提交走页脚）。键盘上退出是**双击**：空闲时 `Ctrl-C` 与 `Ctrl-D` 完全对等 —— 第一下只在提示行举手（`再按一次 ctrl-c/ctrl-d 退出`），半秒内第二下才真的走；忙时第一下 `Ctrl-C` 只取消当前回合（提示行说明），第二下退出（以 130 收尾，终端照常交还），忙时 `Ctrl-D` 一直忽略。退出（含忙碌那条）后 shell 里会多一行 `heng: 会话 <id>；接着跑：heng -c <id>`（stderr，与 `discuss` 收尾那行同一个前缀与生成器）—— 那行直接粘回终端就能续上这一场。启动横幅不打它。

### 子命令

| 命令 | 作用 |
| --- | --- |
| `heng discuss "问题" [--plain\|--tui] [--config PATH] [--cwd PATH] [--debaters A,B]` | 起一次多角色讨论：从 `[discussion] debaters` 池子里抽两个讨论者（`--debaters` 指定），各自独立作答（不同厂商最好；同厂商或同模型也允许），只在结论冲突时开一轮定向第二轮，最后由合成器画出共识 / 分歧 / 未决。讨论落在真会话里，`sessions show` 可复盘 |
| `heng probe [--model ID]...` | 对每个已配置 key 的模型发两次真实请求，打印归一化用量，用来看前缀缓存是否命中 |
| `heng prune [--keep N] [--cwd PATH] [--dry-run]` | 手动删除本工作区的会话目录，保留最新 N 个（默认 1）。除此之外没有任何东西会删你的会话 |
| `heng sessions ls [--all] [--limit N]` | 列出本工作区（`--all` 为全部桶）的会话 |
| `heng sessions show <id> [--round N] [--speaker X] [--kind K] [--tool T] [--only-error] [--files]` | 按轮次分组的转录，工具调用与结果归成一处；`--files` 是「谁在哪一轮改了这个文件」的工作区对象视图 |
| `heng sessions replay <id> --speaker X [--round N] [--model ID]` | 只从事件流**重算**某一次调用实际发给 provider 的内容——调投影 bug 的唯一手段 |
| `heng sessions stats <id> [--model ID]` | 固定指标集：token、费用、单侧缺席率、编辑匹配梯降级分布等 |

`--json` 都可用；**stdout 只放结果，诊断走 stderr**，所以能直接接进管道。这些命令是给人用的，不是给 agent 的新工具。

会话落盘在 `~/.local/share/heng/sessions/<cwd-slug>/<session-id>/`（认 `XDG_DATA_HOME`），一个会话就是一个可搬运的目录：JSONL 事件流 + `outputs/`（工具输出落盘）。目录 `0700`、文件 `0600`。

## 安全模型

四个内置模式：

| 模式 | 判据 |
| --- | --- |
| `readonly` | 非 `ReadOnly` 调用一律 `Deny`；`bash` 也拒（shell 里能写文件） |
| `ask` | 写类 `Ask`、只读 `Allow`——交互式的默认档 |
| `workspace` | 工作区内一律放行、**区外写问一次**；`bash` 靠沙箱与升级手势（见下） |
| `auto` | 默认 `Allow`，但**不等于跳过权限**：断路器、规则、hook 照常生效 |

模式是**会话的一档值**、不进事件流：三个入口是 `config.toml` 的 `[permissions] mode`（默认 `ask`）、`--mode readonly|ask|workspace|auto` 覆盖它、以及会话里 `Shift+Tab` 循环四档（`--continue` 回到配置里的那一档，审计看 `PermissionDecided.reason`）。切档**不注入任何东西**，所以模型不会事先知道档位变了，它是第一次被拒（理由里写着档位）才知道的——这是为了不掉前缀缓存换来的代价（[ADR 0003](docs/adr/0003-plan-leaves-the-permission-modes.md)）。**计划**不是第五档：它是模型自己的 `todo` 工具。完整决策地图见 [`docs/permissions.md`](docs/permissions.md)，第四档的决定与理由见 [ADR 0007](docs/adr/0007-workspace-permission-mode.md)。

四条对每个模式都成立：① 断路器短路在规则之前，任何模式都翻不动；② hook **只能收紧**，永远不能放松一个 `Deny`；③ 沿委派链向下传播的是 `Deny` / `Ask`，`Allow` 不传播（所以「派个子 agent 去写」绕不过你的拒绝）；④ 无交互渲染器时 `Ask → Deny`（脚本不会挂住等你）。

另外：模型给的路径被限制在会话 cwd 及其子树——**区外读**默认仍是拒绝，唯一出口是 `[permissions] outside_read = "deny" | "ask" | "allow"`（缺省 `"deny"`，四档都认它），**区外写**只有 `workspace` 档那个出口；`.env` 家族默认拒绝（`*.example` / `*.sample` / `*.template` 除外）；密钥**在入流前**按值打码（流水线是 `打码 → 截断 → 落盘`，于是**流上的文本 == 模型看到的文本**，而工具执行仍拿真值；`outputs/*.txt` 打码，`outputs/*.before` 不打码——它是 `/undo` 的字节级还原源）；以 root 启动直接拒绝。

**`bash` 有沙箱了，但它只管文件、不管网络。** 完整边界（整机只读挂载、可写根、被遮成空的目录、bubblewrap 用不了时拒绝跑 shell、内核拒了之后的升级手势、以及「网络不在这层的词表里」这句）见 [`docs/sandbox.md`](docs/sandbox.md)、[`docs/permissions.md`](docs/permissions.md)、[`docs/credentials.md`](docs/credentials.md) 与 [ADR 0006](docs/adr/0006-sandbox-by-bubblewrap.md)。

## 架构

```
hook.pre → 权限门 → [询问] → dispatch → hook.post → 追加事件
```

这条流水线的**全貌**（进程启动与组装 → 主循环与三条子命令 → 一次 turn → 委派与嵌套 → 收尾与两条退出路径）画在 [`docs/lifecycle.md`](docs/lifecycle.md)：五张 mermaid 图 + 逐节点的 `文件:行号` 证据表。

- **只追加的事件流**：信封是 `{ seq, at, speaker_id, payload }`，`seq` 就是 JSONL 行号、是唯一身份。重新生成 / 撤销 / compaction 一律追加一条 `HistorySuperseded`，历史一条不改。增量文本**不进流**，它走传输层旁路直达渲染。
- **投影是纯函数**：`project(log, speaker, caps) → messages`，住在 provider 适配器侧、按模型能力表分叉。裁剪（`trim`）是投影**之后**的另一个纯函数，只读、日志一条不删。
- **`Session` 是唯一持有可变状态的值**（事件流句柄 + 名册 + 预算 + 策略 + read set）。执行者是带 `parent_id` 的嵌套 `Session`，事件追加到父流。**权限档位**就是策略里的一个值：三个入口（`[permissions] mode` / `--mode` / `Shift+Tab`）改的都是它，它**不进事件流**，所以 `--continue` 从配置那一档重新开始，审计看 `PermissionDecided.reason`。
- **策略是纯函数，控制流在循环里**：hook 输出的是**约束**、权限门输出的是**裁决**，两者在 `Allow < Ask < Deny` 上取上确界——类型里根本没有「放松权限」这个变体。
- **16 个顶层边界，只向下依赖**：`events` · `config` · `provider` · `tools` · `web` · `mcp` · `permissions` · `questions` · `hooks` · `context` · `agent` · `discussion` · `goals` · `session` · `render` · `cli`（`questions` 是票 32 加的第三类发起者那两条端口，`goals` 是目标循环那一轮加的，`web` 是出网那一层，`mcp` 是 MCP 那一轮加的；与 `src/lib.rs` 的清单一致）。`events` 零内部依赖；`discussion` 不碰 provider；`tools → web` 与 `tools → mcp` 都是单向 —— 那两层自己发 HTTP / 起子进程，不碰会话的 provider。
- **三个前端**（headless / plain / TUI）共用一条广播通道与一个转录层，启动时选定且互斥；headless 的 stdout **只有最终产物**。TUI 是一条左栏 + 一条主列（[ADR 0002](docs/adr/0002-fullscreen-alt-screen-tui.md)）、**没有外框**：≥ 120 列时左栏 40 列画标记，80–119 列退成文字身份，再窄整栏隐藏（见上面的「TUI 长什么样」）。

默认数值：单 agent 100 回合、执行者 25 回合（这两个用 `[turn]` 调）、同批执行者并发 5、单个工具结果 25k 估算 token、`repo_map` 1k（上限 4k）、`bash` 120s（上限 600s）。

## 文档

| 去哪看 | 是什么 |
| --- | --- |
| [`CONTEXT.md`](CONTEXT.md) | 正式词汇表：**领域词汇**（事件流、投影、待办列表……）加末尾一节**流程词汇**（feature 目录 / spec / 票 / 决策图 / 分诊标签……），并写明哪两类词不收（通用编程概念、skills 工具名）（名字那两条：**衡（heng）**与**分叉合成（Forked Synthesis）**）。写文档、写代码、写票之前先看它 |
| [`.scratch/fs-agent-v1/spec.md`](.scratch/fs-agent-v1/spec.md) | v1 spec：问题陈述、用户故事、20 节实现决定、测试决定、明确的「明确不做」 |
| [`docs/`](docs/) | 逐面说明：[`bash`](docs/bash.md) · [`credentials`](docs/credentials.md) · [`custom-tools`](docs/custom-tools.md) · [`discussion`](docs/discussion.md) · [`executor`](docs/executor.md) · [`goals`](docs/goals.md) · [`grep`](docs/grep.md) · [`observability`](docs/observability.md) · [`permissions`](docs/permissions.md) · [`render`](docs/render.md) · [`repo-map`](docs/repo-map.md) · [`sandbox`](docs/sandbox.md) · [`skills`](docs/skills.md) · [`highlight`](docs/highlight.md) · [`tui-manual-checklist`](docs/tui-manual-checklist.md) · [`web`](docs/web.md) · [`mcp`](docs/mcp.md) |
| [`docs/lifecycle.md`](docs/lifecycle.md) | **运行时生命周期**：从敲下命令到进程退出的五张图（一张鸟瞰 + 四张分层详图）与逐节点的 `文件:行号` 证据表，把上面各份逐面说明接起来；护栏是 [`scripts/lifecycle-check.py`](scripts/lifecycle-check.py)（守指称完整性，见它的用法） |
| [`docs/adr/`](docs/adr/) | 不可逆的决定：[中文 UI 与冻结的模型文本](docs/adr/0001-chinese-ui-frozen-model-text.md) · [全屏备用屏幕（alt screen）TUI](docs/adr/0002-fullscreen-alt-screen-tui.md)（含标记与其代价）· [「计划」从权限模式里搬出来](docs/adr/0003-plan-leaves-the-permission-modes.md)（模式三档 + 模型的 `todo` 工具；后来由 ADR 0007 加了第四档 `workspace`）· [散文用中文，标识符与「进 `messages` / 进流」的文本留英文](docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md)（语言的线，加 `check-language.py` 的护栏；那张「英文只留三类」的清单已被 ADR 0005 取代） · [模型可见与进流的文本也走中文](docs/adr/0005-model-visible-text-in-chinese.md)（语言按「是不是标识符」分，推翻 ADR 0001 的那一半） · [让 shell 的写边界由内核担保：bubblewrap 沙箱](docs/adr/0006-sandbox-by-bubblewrap.md)（默认开 + fail closed；网络不在这一层） · [第四档权限模式 `workspace`](docs/adr/0007-workspace-permission-mode.md)（区外要问；被内核拒之后的一条升级通道） · [Markdown 的解析交给 `pulldown-cmark`](docs/adr/0008-markdown-parsing-by-pulldown-cmark.md)（渲染仍是我们自己的；`to_lines` 因此开始收宽度） · [目标是一份文件，进度与额度都从会话流派生](docs/adr/0009-goals-are-files-and-progress-is-derived.md)（目标不进会话状态；`/loop`、翻页与跨会话预算都建在这条上） · [问卷的键位按区域分派，单选与多选共用一个答案形状](docs/adr/0010-questionnaire-keys-dispatch-by-zone.md)（`Zone` 替换布尔；`selected` 与 `custom` 并存，推翻 §7 那条单选覆盖的约定） · [文档里的流程图用 mermaid](docs/adr/0011-diagrams-in-mermaid.md)（流程图用受约束的 mermaid 方言，图配证据表 + `scripts/lifecycle-check.py` 对账；已有五处 ASCII 图一个字不改） · [输入框里的记号是不可分割的一块](docs/adr/0012-input-tokens-are-atomic.md)（`@路径` 与 `/命令` 整块删、整块移） · [文件页那一档可以换成一块外来屏幕（内嵌 nvim）](docs/adr/0013-nvim-file-viewer-is-an-alien-screen.md)（`[ui] file_viewer = "nvim"`；三处例外见该 ADR） · [名字从 `fs-agent` 改成衡（`heng`）](docs/adr/0014-renamed-to-heng.md)（汉字是正身、`heng` 是拼音；历史不追改，`Forked Synthesis` 退作机制名） |
| [`docs/research/`](docs/research/) | 一手调研的**原始笔记**（`coding-agent-features.md` 是横向对比，`notes/` 下五份是上游正文，合计约 796KB）：材料，不是结论 —— 结论已折进 `.scratch/` 的 spec 与 `docs/` 的逐面文档 |
| [`.scratch/README.md`](.scratch/README.md) | **feature 索引**：一行一个 feature —— 是 spec 还是决策地图、一句话、票数与完成度 |
| [`AGENTS.md`](AGENTS.md) | agent 在本仓库工作时的约定（文档往哪写、语言怎么选、提交怎么写）；它的四个细目（issue tracker / triage labels / domain docs / commits）在 [`docs/agents/`](docs/agents/) |

**约定**（新文档照这个走，别猜；这条线由 [ADR 0004](docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md) 立、[ADR 0005](docs/adr/0005-model-visible-text-in-chinese.md) 改写）：

- **散文一律中文**：代码注释、`docs/` 下的逐面设计文档与 `docs/agents/`、`docs/adr/` 下的 ADR（含它的标题与小标题）、`AGENTS.md` 的正文（它的五个小标题是技能工具链的锚点、留英文）、`.scratch/` 下的 spec / map / 票、测试的断言消息、以及只在启动时打印给人的错误文本（`ConfigError`、harness 的 `Error`、provider 的告警）。
- **[ADR 0005](docs/adr/0005-model-visible-text-in-chinese.md) 起，模型可见与进流的散文也在内**：工具声明与描述、工具结果与错误、`AgentError.message`、`SessionError.detail`、`PermissionDecided.reason`、以及**模型自己产出的思考**（它随 `MessageCompleted.reasoning` 进流、也进详情弹窗那节 `── 思考 ──`；四段身份各自拼上 `src/agent.rs` 的 `agent::THINKING_IN_CHINESE`）。
- **英文只留给不是散文的东西**（[ADR 0005](docs/adr/0005-model-visible-text-in-chinese.md) 画的新线：按**词性**分，不按「谁读它」分）：① **标识符**（类型、函数、字段、文件名、CLI 旗标、事件 schema 的名字）；② **schema 值与协议标记**（`Ask` / `Allow` / `Deny`、`cwd` / `token` / `assistant`、`tool_call_id`、`CONCLUSION:`）；③ **路径、命令原文与代码片段**；④ `docs/research/` 的一手引文。
- **术语写「中文名（English）」**：中文是叙述里的正式用词，英文只夹注一次，供人对到 API 上（行内视口（inline viewport）、备用屏幕（alt screen）、回滚缓冲（scrollback）、panic 钩子（panic hook））。`cwd` / `token` / `assistant` 这类字段名与 schema 值不夹注、保持英文（CONTEXT.md 里「token 不给中文名」同一条）。
- **`docs/research/` 的原始笔记一个字不改**：那是上游文档的引文，它存在的意义是可核对。
- 为什么模型那一侧原来冻在英文、2026-09-30 又翻开，连同那两笔代价，都在 [ADR 0001](docs/adr/0001-chinese-ui-frozen-model-text.md) 里。
- 三条推论：**新增文档跟邻居走**；**中文文档里保留标识符英文**（写 `Session`、`project()`、`[permissions] mode`）；**这条线可以检查** —— `python3 scripts/check-language.py`（四条：① 模型可见 / 进流那一侧的两条棘轮 —— 中文串只许上升、英文散文串只许下降；② `docs/` 的逐面文档、`docs/adr/*.md`、`docs/agents/*.md` 与 `AGENTS.md` 的中文占比下限（清单在脚本的 `DOCS_MIN_RATIO`）；③ ADR 的标题与小标题是中文；④ `src/` 与 `tests/` 注释中文行的棘轮）。

## 开发

```sh
cargo test                              # 全量测试（条数见上面的「状态」）
cargo clippy --all-targets
python3 -m unittest                     # Python 护栏脚本的测试（从仓库根跑，发现 scripts/tests/）
python3 scripts/check-language.py       # 散文中文、英文只留给标识符的护栏（ADR 0004 / 0005）
python3 scripts/check-doc-size.py       # 40 份活文档的密度（单元 ≤500 / 入口三份的预算 / 占比余量）
python3 scripts/lifecycle-check.py      # docs/lifecycle.md 的图与证据表对账（ADR 0011）
python3 scripts/tui-startup-check.py    # TUI 启动冒烟（需要真终端）
```

测试只测**外部行为**：断言的对象是**事件流**（JSONL 里的 `seq` + payload）与**工作区副作用**（磁盘上的文件、临时 git 仓库），不是内部结构；不断言 `at` 时间戳。唯一的 e2e 接缝是库的组装入口 `assemble` / `assemble_discussion`——provider、路径锁、渲染 sink、配置全部从参数注入，并且**库不读环境**。测试里所有 provider 都是假 provider（两家都没有 `seed`，真调用不可复现），所以测试没有网络依赖。

最重要的一条不变量：用 `sessions replay` 重算的投影**必须等于**当时实际发给 provider 的 `messages`。这是「事件流是唯一真相源」的验收，也是投影 bug 的唯一探测器。

## 这一版不做

AST / tree-sitter 编辑、unified diff 编辑格式、原生多 provider 协议、向量检索 / RAG、IDE 与 IM 集成、两进程渲染、syntect 的 C 路径、内置编辑器、交互式 transcript 浏览器、裁判 / 仲裁者、N > 2 的讨论者、fork / rewind 手势、shadow git、SQLite、全局会话索引、自动清理、每次编辑自动 git commit、把工具打包进 skill、网络隔离。理由逐条写在 [v1 spec 的 `明确不做`](.scratch/fs-agent-v1/spec.md) 与各 feature 自己的 spec 里。

**`MCP client` 也已经从这里拿出去**：2026-10-03 起它另起了一个 effort（同日落地，见 [`docs/mcp.md`](docs/mcp.md)） —— [`mcp-support`](.scratch/mcp-support/map.md)（wayfinder 决策图，七张决策票），范围是 MCP **现行规范的全集**（tool / resource / prompt / elicitation + MRTR；已 deprecated 的 sampling / roots / logging 不在内）。v1 spec 的三处加了带日期的补记（两处在 `明确不做`、一处在 §14 自定义工具那节），原文不改写 —— 那是当初排除它的理由。

**`compaction` 已经从这里拿出去**：压缩在 [`goal-loop`](.scratch/goal-loop/spec.md) 里做完了——过八成就把历史折成一段摘要、随翻页注入新会话（[`docs/goals.md`](docs/goals.md) 的「过半提醒与翻页」）。仍然不做的是压缩之外的历史管理（fork / rewind、历史编辑）。

**这是一版的范围边界，不是永久判决。** 要动其中一条，先改 spec（或者像 [`sandbox`](.scratch/sandbox/spec.md) 那样另起一个 effort），而不是在实现里悄悄加一条路径——那份 sandbox spec 做的正是这件事：把「进程级沙箱」从这份清单里拿出去了一半（文件做到了，网络与其它平台仍不做）。

## License

MIT，见 [`LICENSE`](LICENSE)。
