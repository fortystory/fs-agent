# 改名：`fs-agent` → 衡（`heng`）

`fs-agent` 这个名字改过两次口。`fs` 最初是 **fortystory** 的缩写（这个仓库所在的那层目录），后来被重新解释成 **Forked Synthesis**（分叉合成）。为了让后一个解释站得住，[`README.md:11`](../../README.md) 写了一句「`fs` 只是叙述里的框架名，**不是**命令行或路径的一部分」，[`CONTEXT.md`](../../CONTEXT.md) 的**名字**词条又挂了一条 `_Avoid_` 拦住 `filesystem` 的读法。一处缩写要两句自证，这是名字在收利息。

2026-10-06 定下新名：**衡**，命令与包名写 `heng`。理由两条：

1. **`fs` 的第一联想是 filesystem。** `fsck`、`node:fs`、`/proc/fs` 都在那一侧；「分叉合成」要读到 README 第 11 行才成立。
2. **产品的定位是 harness，不是一个 agent。** 它编排两个讨论者、若干执行者与一次合成器调用，自己提供事件流、投影、权限、沙箱与 TUI；[`CONTEXT.md`](../../CONTEXT.md) 甚至规定 `agent` 只作泛称、不作类型名。名字里带 `-agent` 把整套装置说成了其中一个角色。

**衡**取「衡量」：合成器**不是裁判**（合成器词条把 judge / 裁判列进 `_Avoid_`，[`docs/discussion.md`](../../docs/discussion.md) 记着这是被证据否掉的方案），它只把两方判断放到秤上——共识 / 分歧 / 未决就是秤上的读数。它同时是一个名词（衡器），正好接住「这是一套装置」；拼音与项目的中文优先同调（[ADR 0004](../../docs/adr/0004-prose-in-chinese-identifiers-and-model-text-in-english.md)、[ADR 0005](../../docs/adr/0005-model-visible-text-in-chinese.md)）。

## 问题陈述

1. **`fs` 有歧义。** 命令行世界里 `fs` 默认指 filesystem；这个词条已经要靠 `_Avoid_` 防误读。
2. **`-agent` 把产品说小了。** 它是一套 harness，agent 是里面被装配出来的发言方（两个讨论者、执行者），甚至被明确规定不作类型名。
3. **解释成本落在文档里。** README 第 11 行整句、`CONTEXT.md` 的名字词条都在为 `fs` 辩护；`Cargo.toml` 的 description、`--help`、`--version` 都没提过 Forked Synthesis，读者第一眼拿不到那个解释。
4. **标记画的是旧名字。** [`src/render/wording.rs:1664`](../../src/render/wording.rs) 的 `logo_lines()` 五行块字拼出 `fs-agent`，README 的 banner 同一图案。

## 方案

- **名字与边界**（§1）：真名「衡」，标识符一律 `heng`；Forked Synthesis 退作机制名；历史文件不追改。
- **标识符与路径**（§2）：Cargo 的包 / lib / bin 名、配置与数据路径、技能根、环境变量、对外标识。
- **文档与 ADR**（§3）：README、`CONTEXT.md`、`docs/**`，新增 ADR 0014。
- **标记**（§4）：`logo_lines()` 改成「汉字**衡**在上、拼音 `héng` 标注在上方」的五行网格；README banner 同步。
- **迁移与远端**（§5）：本机旧目录一条 `mv`；GitHub 仓库改名与本地目录改名放最后。

### §1 名字与边界

- 真名 **衡**；命令、包名、lib 名、落盘路径段一律 `heng`。
- **Forked Synthesis / 分叉合成保留**，但只当**讨论协议**这个机制的框架名，不再是产品名。README 第 11 行那段自证删掉，`CONTEXT.md` 的名字词条重写成「衡（heng）」。
- **不追改**：`.scratch/` 下的历史票（spec、票、评论）与 `docs/research/` 下的一手引文。前者是历史事实，后者按 ADR 0004 属"引文一字不改"。新 spec 在此声明，免得以后有人以为是漏改。
- **旧 ADR 正文不改**：ADR 是决策的历史记录。受影响的 ADR（如 0009 写着 `~/.local/share/fs-agent/goals/`）加一行补注指向 ADR 0014。
- **不做双名兼容**：没有 `fs-agent` 别名，不读旧路径，不认 `FS_AGENT_*`。本机旧状态一次性搬走（§5）。

### §2 标识符与配置路径

| 旧 | 新 | 备注 |
| --- | --- | --- |
| `fs-agent` | `heng` | 包名、bin、命令、`fs-agent: ` 前缀、路径段 |
| `fs_agent` | `heng` | `[lib] name`、测试里的 `use fs_agent::…` |
| `FS_AGENT_*` | `HENG_*` | 只有 `FS_AGENT_MODEL` 一个 |
| `.fs-agent` | `.heng` | 技能发现根（`src/context/skills.rs`） |
| `fs-agent-mcp-time` | `heng-mcp-time` | 第二个 bin |
| `fake-mcp-server` | 不动 | 测试辅助，与名字无关 |

落点（人读得到的那些）：

- `Cargo.toml`：`name`、`default-run`、`[lib] name`、两个 `[[bin]] name`；`description` 从"append-only event stream plus per-agent windows"改成写明 harness 定位。
- `src/config.rs`：配置目录 `~/.config/heng`、会话根与会话桶、goals 根、沙箱掩码里那条路径、`HENG_MODEL`。
- `src/context/skills.rs`：项目级技能根 `.heng`、用户级 `~/.config/heng`。
- `src/render/wording.rs`：`fs-agent: ` 前缀（一处收口）、usage / help 全文、`identity()`（它读 `CARGO_PKG_NAME`，跟着包名自动变）。
- `src/agent.rs`：模型可见的身份提示词——改成「你是衡（heng），一套自用的 coding agent harness…」。它是**模型可见文本**，改它会让新会话的前缀缓存换一次；老会话 `--continue` 重放出来的仍是旧文本（事件流回放本就该如此）。
- `src/mcp/rmcp_client.rs`、`src/web/fetch_http.rs`：MCP client 名与 HTTP user-agent。
- `scripts/tui-startup-check.py`：它拿 `--version` 的输出当锚点。

### §3 文档与 ADR

- `README.md`：标题、banner、命令与路径示例；第 11 行那段「`fs` 只是叙述里的框架名」整段删掉，换成一句「衡（heng）——一套自用 coding agent harness」。
- `CONTEXT.md`：**名字**词条重写为「衡（heng）」（汉字、拼音、与合成器"只称量不裁决"的关系）；**分叉合成（Forked Synthesis）**词条保留、改成纯机制名；其余引用逐处替换。
- `docs/*.md`：路径、命令、示例（`docs/research/` 除外）。
- 新增 **ADR 0014「名字从 fs-agent 改为衡（heng）」**：记两条理由、拼音方案、历史不追改的边界，以及"Forked Synthesis 降为机制名"。
- `.scratch/README.md`：feature 索引加一行（`rename-to-heng/`）。

### §4 标记

- `logo_lines()` 返回**五行**、每行显示宽度等于 `layout::LOGO_WIDTH`（38）——这是布局的契约（`mark_lines()` 有 `debug_assert`），不因为内容变短就改契约。
- 构图：**拼音 `héng` 在上一行、汉字「衡」在下一行**，两者中心对齐；其余行留白。颜色坡道不动（`MARK_BRIGHT` → `MARK_DIM`）。
- 宽档画标记、窄档画 `identity()` 文字的阶梯不动；[`CONTEXT.md`](../../CONTEXT.md) 里**下落短横已退出屏幕**（"别把它想回来"），本 effort 不碰它，但 `tests/render_layout.rs` 里那条按旧字形取"那一格"的断言要改成"整块标记在运行中不变"。
- README 的 banner 与左栏标记同源，同步重画。

### §5 迁移与远端

- 本机旧状态：`~/.config/fs-agent` → `~/.config/heng`；`~/.local/share/fs-agent` → `~/.local/share/heng`（3 个会话、2 个目标、一份配置）。同分区 `mv`，原子。
- GitHub：`gh repo rename heng`、更新 remote、改仓库描述。放**最后**（用户定），旧地址由 GitHub 重定向。
- 本地目录 `~/code/fortystory/fs-agent` → `~/code/fortystory/heng`：**也放最后**，因为它是当前会话的工作目录，改名会让本会话的 cwd 失效。

## 验收

1. `cargo test` 全绿（改名不引入任何行为变化）。
2. `cargo build` 后 `--version` 印 `heng {version}`，`--help` 里没有旧名。
3. 在 `Cargo.toml` / `src/` / `tests/` / `scripts/` / `README.md` / `CONTEXT.md` / `docs/`（不含 `research/` 与旧 ADR 正文）里搜不到 `fs-agent`、`fs_agent`、`FS_AGENT`、`.fs-agent` 的残留。
4. 标记五行、宽度 38，宽档左栏真机看一眼；窄档显示 `heng {version}`。
5. 搬完旧目录后，`heng -c` 能续上那 3 场旧会话，`/loop` 能看到那 2 个目标。

## 落地

2026-10-06 当天完成，四张票见 [`issues/`](issues/)：

- **标识符与配置路径**（票 01，`done`）：`fs-agent` → `heng`、`fs_agent` → `heng`、`FS_AGENT_MODEL` → `HENG_MODEL`、`.fs-agent` → `.heng`、`fs-agent-mcp-time` → `heng-mcp-time`，外加一个驼峰变体 `FsAgentHandler` → `HengHandler`。186 个文件、约 1 900 处。
- **文档与 ADR**（票 02，`done`）：README（标题 / banner / 定位句）、`CONTEXT.md` 的名字与分叉合成两条词条、`docs/**`，新增 [ADR 0014](../../docs/adr/0014-renamed-to-heng.md)，旧 ADR 0002 与 0009 各加一行补注。
- **标记**（票 03，`done`）：`logo_lines()` 换成五行网格里的「`héng` / 衡」，README 的 banner 与帧、`docs/render.md`、手动清单同步；布局契约（5 行 × 38 列）不变，相关断言改成按**显示宽度**与整块比较。
- **迁移与远端**（票 04，`ready-for-walkthrough`）：旧状态已搬（3 个会话桶、2 个目标，配置里的 `fs-agent-mcp-time` 也改成 `heng-mcp-time`），GitHub 已改名 `fortystory/heng`；**只剩本地目录改名**，留给维护者 —— 当前会话的 cwd 与文件沙箱的工作区都钉在旧路径上。
- 顺带修掉改名暴露的两条过时期望：`tests/wording.rs` 里两条按名字宽度算的标题测试（`fs-agent` 8 列 → `heng` 4 列，40 列标题能多放下一段路径与一个目标）。

验收：`cargo test` **1,366 passed / 0 failed**（cargo exit 0）；范围内（`Cargo.toml`、`src`、`tests`、`scripts`、`README.md`、`CONTEXT.md`、`docs/**` 除 `research/` 与旧 ADR 正文）搜不到旧名，只剩两处**有意保留**：`CONTEXT.md` 的 `_Avoid_` 点名旧名、README 指向历史 feature 目录 `.scratch/fs-agent-v1/`。
