# fs-agent

自用 coding agent CLI（Rust，从零实现）。核心是**一条只追加的事件流**加**每个 agent 自己的窗口**：事件流是会话的唯一真相源，每个讨论者 / 执行者这次调用要重放的 `messages` 都是从事件流加投影规则**重算**出来的纯函数产物——没有隐藏状态，历史永远可复盘。

> 术语以 [`CONTEXT.md`](CONTEXT.md) 为准：叙述、文档与 UI 用中文（讨论者 / 执行者 / 事件流 / 投影 / 会话 / 轮次 / 回合 / 技能），代码标识符用其中的英文名（`Debater` / `Executor` / `EventLog` / `project()`）。

## 它解决什么

- **单一模型的判断没有对照。** 两个**异构**讨论者（KIMI + DeepSeek）各自作答，只在结论真的冲突时开一轮定向第二轮，最后合成器画出「共识 / 分歧（含各自成立的前提）/ 未决」的选项空间——决定权在你，不在一个自动收敛的投票器。
- **探索和动手是两种活。** 执行者是讨论者用内建 `task` 工具派出的子 agent，带自己的 `parent_id` 与轮数预算（默认 25）；它的事件落在**同一条流**里，但过程默认不进任何讨论者的窗口，结论经派发者自己的发言进入讨论。
- **上下文预算是硬的。** 窗口（按每个 agent 各自的模型算）、轮数、会话累计 token、费用是四个不同的量，各有各的语义，撞顶时**降级收尾**而不是随机截断。
- **权限、秘密、可撤销。** 四个内置模式、断路器短路拒绝、cwd 路径限制、`.env` 家族默认拒、密钥在**入流前**打码、会话目录 `0700`、root 拒绝启动；每次 `edit_file` 都能 `/undo` 原样退回，且不碰你的 git。
- **要能复盘。** `sessions show / replay / stats` 只从会话自己的事件流回答「这一轮为什么停」「谁在哪一轮改了哪个文件」「这次编辑走了降级匹配吗」。

**状态**：v1 的 23 张实现票全部 `done`（约 24k 行 `src/`、21k 行 `tests/`、550+ 测试）。一处诚实的缺口：**讨论协议（讨论者 + 合成器）已在库层实现并有集成测试，但 CLI 入口还没接上**——从命令行启动的目前只有单 agent 会话，讨论经库 API `assemble_discussion` 驱动。

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

[budget]
session_tokens = 2000000      # 会话累计 token 上限（讨论者 / 执行者 / 合成器共用）
estimate_margin = 1.5         # 发出去之前的估算宽容倍数

[routing]                     # 弱模型分流只有两个落点；讨论者绝不被路由
# synthesizer_model = "deepseek-flash"
# executor_model = "deepseek-flash"

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
fs-agent --help
```

会话里：`/undo` 回滚上一次编辑、`/plan` 与 `/endplan` 进出硬计划模式、`/<技能名> [任务]` 直接加载一个技能（包括标了 `disable-model-invocation: true` 的）、`/quit` 退出；TUI 里 **Esc** 取消正在跑的回合、**Shift+Tab** 切计划模式。

### 子命令

| 命令 | 作用 |
| --- | --- |
| `fs-agent probe [--model ID]...` | 对每个已配置 key 的模型发两次真实请求，打印归一化用量，用来看前缀缓存是否命中 |
| `fs-agent prune [--keep N] [--cwd PATH] [--dry-run]` | 手动删除本工作区的会话目录，保留最新 N 个（默认 1）。除此之外没有任何东西会删你的会话 |
| `fs-agent sessions ls [--all] [--limit N]` | 列出本工作区（`--all` 为全部桶）的会话 |
| `fs-agent sessions show <id> [--round N] [--speaker X] [--kind K] [--tool T] [--only-error] [--files]` | 按轮次分组的转录，工具调用与结果归成一处；`--files` 是「谁在哪一轮改了这个文件」的工作区对象视图 |
| `fs-agent sessions replay <id> --speaker X [--round N] [--model ID]` | 只从事件流**重算**某一次调用实际发给 provider 的内容——调投影 bug 的唯一手段 |
| `fs-agent sessions stats <id> [--model ID]` | 固定指标集：token、费用、单侧缺席率、编辑匹配梯降级分布等 |

`--json` 都可用；**stdout 只放结果，诊断走 stderr**，所以能直接接进管道。这些命令是给人用的，不是给 agent 的新工具。

会话落盘在 `~/.local/share/fs-agent/sessions/<cwd-slug>/<session-id>/`（认 `XDG_DATA_HOME`），一个会话就是一个可搬运的目录：JSONL 事件流 + `outputs/`（工具输出落盘）。目录 `0700`、文件 `0600`。

## 安全模型

四个内置模式：

| 模式 | 判据 |
| --- | --- |
| `readonly` | 非 `ReadOnly` 调用一律 `Deny`；`bash` 也拒（shell 里能写文件） |
| `ask` | 写类 `Ask`、只读 `Allow`——交互式的默认档 |
| `auto` | 默认 `Allow`，但**不等于跳过权限**：断路器、规则、hook 照常生效 |
| `plan` | 同 `readonly`，唯一豁免是「写入集的全部路径 = 项目根 `PLAN.md`」 |

四条对每个模式都成立：① 断路器短路在规则之前，任何模式都翻不动；② hook **只能收紧**，永远不能放松一个 `Deny`；③ 沿委派链向下传播的是 `Deny` / `Ask`，`Allow` 不传播（所以「派个子 agent 去写」绕不过你的拒绝）；④ 无交互渲染器时 `Ask → Deny`（脚本不会挂住等你）。

另外：模型给的路径被限制在会话 cwd 及其子树；`.env` 家族默认拒绝（`*.example` / `*.sample` / `*.template` 除外）；密钥**在入流前**按值打码（流水线是 `打码 → 截断 → 落盘`，于是**流上的文本 == 模型看到的文本**，而工具执行仍拿真值；`outputs/*.txt` 打码，`outputs/*.before` 不打码——它是 `/undo` 的字节级还原源）；以 root 启动直接拒绝。

**这不是沙箱。** v1 不做进程级隔离，对「模型把密钥发到网上」基本无能为力；升级路径写明是「只做 Linux 的 bubblewrap」，且**不预做抽象**。真正的边界是别把赔不起的 key 交给它。完整边界见 [`docs/credentials.md`](docs/credentials.md)。

## 架构

```
hook.pre → 权限门 → [询问] → dispatch → hook.post → 追加事件
```

- **只追加的事件流**：信封是 `{ seq, at, speaker_id, payload }`，`seq` 就是 JSONL 行号、是唯一身份。重新生成 / 撤销 / compaction 一律追加一条 `HistorySuperseded`，历史一条不改。增量文本**不进流**，它走传输层旁路直达渲染。
- **投影是纯函数**：`project(log, speaker, caps) → messages`，住在 provider 适配器侧、按模型能力表分叉。裁剪（`trim`）是投影**之后**的另一个纯函数，只读、日志一条不删。
- **`Session` 是唯一持有可变状态的值**（事件流句柄 + 名册 + 预算 + 策略 + read set）。执行者是带 `parent_id` 的嵌套 `Session`，事件追加到父流。
- **策略是纯函数，控制流在循环里**：hook 输出的是**约束**、权限门输出的是**裁决**，两者在 `Allow < Ask < Deny` 上取上确界——类型里根本没有「放松权限」这个变体。
- **12 个顶层边界，只向下依赖**：`events` · `config` · `provider` · `tools` · `permissions` · `hooks` · `context` · `agent` · `discussion` · `session` · `render` · `cli`。`events` 零内部依赖；`discussion` 不碰 provider。
- **三个前端**（headless / plain / TUI）共用一条广播通道与一个转录层，启动时选定且互斥；headless 的 stdout **只有最终产物**。TUI 是全屏四分区（[ADR 0002](docs/adr/0002-fullscreen-alt-screen-tui.md)）。

默认数值：单 agent 100 回合、执行者 25 回合、同批执行者并发 5、单个工具结果 25k 估算 token、`repo_map` 1k（上限 4k）、`bash` 120s（上限 600s）。

## 文档

| 去哪看 | 是什么 |
| --- | --- |
| [`CONTEXT.md`](CONTEXT.md) | 正式词汇表。写文档、写代码、写票之前先看它 |
| [`.scratch/fs-agent-v1/spec.md`](.scratch/fs-agent-v1/spec.md) | v1 spec：问题陈述、用户故事、20 节实现决定、测试决定、明确的 Out of Scope |
| [`docs/`](docs/) | 逐面说明：[`bash`](docs/bash.md) · [`credentials`](docs/credentials.md) · [`custom-tools`](docs/custom-tools.md) · [`discussion`](docs/discussion.md) · [`executor`](docs/executor.md) · [`observability`](docs/observability.md) · [`plan-mode`](docs/plan-mode.md) · [`render`](docs/render.md) · [`repo-map`](docs/repo-map.md) · [`skills`](docs/skills.md) · [`tui-manual-checklist`](docs/tui-manual-checklist.md) |
| [`docs/adr/`](docs/adr/) | 不可逆的决定：中文 UI 与冻结的模型文本、全屏 alt screen TUI |
| [`.scratch/`](.scratch/) | `multi-agent-architecture/` 是决策地图，`fs-agent-v1/issues/` 是一张票一个文件的实现票（含 `Status:` 行） |
| [`AGENTS.md`](AGENTS.md) | agent 在本仓库工作时的约定（issue tracker、triage labels、domain docs） |

## 开发

```sh
cargo test                              # 550+ 测试
cargo clippy --all-targets
python3 scripts/tui-startup-check.py    # TUI 启动冒烟（需要真终端）
```

测试只测**外部行为**：断言的对象是**事件流**（JSONL 里的 `seq` + payload）与**工作区副作用**（磁盘上的文件、临时 git 仓库），不是内部结构；不断言 `at` 时间戳。唯一的 e2e 接缝是库的组装入口 `assemble` / `assemble_discussion`——provider、路径锁、渲染 sink、配置全部从参数注入，并且**库不读环境**。测试里所有 provider 都是假 provider（两家都没有 `seed`，真调用不可复现），所以测试没有网络依赖。

最重要的一条不变量：用 `sessions replay` 重算的投影**必须等于**当时实际发给 provider 的 `messages`。这是「事件流是唯一真相源」的验收，也是投影 bug 的唯一探测器。

## 明确不做

AST / tree-sitter 编辑、unified diff 编辑格式、原生多 provider 协议、MCP client、向量检索 / RAG、IDE 与 IM 集成、两进程渲染、syntect 的 C 路径、内置编辑器、交互式 transcript 浏览器、进程级沙箱、裁判 / 仲裁者、N > 2 的讨论者、fork / rewind 手势、shadow git、SQLite、全局会话索引、自动清理、每次编辑自动 git commit、compaction 的实现、把工具打包进 skill。理由逐条写在 [spec 的 `Out of Scope`](.scratch/fs-agent-v1/spec.md)——要动它们，先改 spec，而不是在实现里悄悄加一条路径。

## License

MIT，见 [`LICENSE`](LICENSE)。
