# 03 — 文档与索引

Type: implement
Status: done
Blocked by: 01

> 规格：[`../spec.md`](../spec.md) §4、§6.4。代码落点与形状由 [票 01](01-time-server-bin.md) 定
> 死，本票只把它们写给人看。

## 目标

一个人读完 [`docs/mcp.md`](../../../docs/mcp.md) 就知道：仓库自带一台时间 server、它怎么挂上、
为什么建议开那两个信任位、以及它不做什么。

## 现状（2026-10-06 核实，改前先复核）

- **`docs/mcp.md` 的节次**：三层 / 开关与配置 / 四个元工具 / 信任三个位 / server 进程 /
  结果进上下文 / 错误码 / 两条边界 / 提示词模板 / 没做的 / 代码落点。新一节插在**「提示词模板」
  之后、「没做的」之前**：它是**一台具体的 server**，排在协议那几节之后才顺。
- **`docs/mcp.md` 受 [`scripts/check-doc-size.py`](../../../scripts/check-doc-size.py) 的
  「单元 ≤500 字符」约束**，且该文件有自己的违规计数基线：**新增的超长单元立刻报红**。所以这一
  节要拆成小段。
- **`.scratch/README.md` 的索引表**：一行一个 feature，形态列写 `spec`，票列写 `N/N done` 这种
  形状（`grep -n 'time-mcp\|mcp-support' .scratch/README.md` 看邻居怎么写）。

## 落点

`docs/mcp.md`、`.scratch/README.md`、`scripts/check-doc-size.py`（入口三份的预算表）。

## 具体行为

1. **`docs/mcp.md` 新一节「自带的 time server」**，四小段（每段各自是 ≤500 字符的单元）：
   - 它是什么：`fs-agent-mcp-time`，不写盘、不出网、不读配置，只答三条方法；
   - 怎么挂上：项目级 `.mcp.json` 的样例（与 config.toml 的样例，含 `trust_effects` +
     `read_only_tools`，并说明为什么建议开这两个位而 `trust_results` 保持缺省）；
   - 返回什么：那一行时间（本地时区 + 偏移 + 时区名 + 星期几）与身份里那句指引的呼应；
   - 它不做什么：时区参数、时间戳换算、`resources` / `prompts`（指向 spec §7）。
2. **`docs/mcp.md` 的「代码落点」**补 `src/bin/mcp_time.rs` 与 `tests/mcp_time_server.rs`。
3. **`README.md` 不动** —— 原计划在「构建」补一句「装上的三个 bin 里有 `fs-agent-mcp-time`」，
   落地时否掉：入口三份的体量预算**只许降**（见下面那条评论），而那句话在 `docs/mcp.md` 的新一节
   里已经有了，README 的文档索引也指得到它。
4. **`.scratch/README.md`** 的索引表加一行 `time-mcp`：一句话说清它是什么 + 票数；**表格行以
   文件真实状态为准**（票都 done 之后写 `3/3 done`）。
5. **台词一致**：server 名 `time`、工具名 `get_current_time` 在文档里与票 01、票 02 的代码逐字
   一致。

## 验证

1. `python3 scripts/check-doc-size.py` 与 `python3 scripts/check-language.py` 都绿（后者要求
   `docs/**` 的中文占比达标、`.scratch/` 的 tracker 标题是中文）。
2. `python3 -m unittest` 绿（护栏脚本自己的测试）。
3. 人读一遍 `docs/mcp.md` 新一节：按样例配置**能照做**（一条命令装 bin、一段 JSON 挂上、一次
   `mcp_list` 验到）。

## 评论

- **2026-10-06 落地**：`docs/mcp.md` 的新一节在「提示词模板」之后、「没做的」之前；「没做的」多了
  一条（`get_current_time` 不吃参数）；「代码落点」多了这个二进制与它的测试。
- **`.scratch/README.md` 的索引行**按仓库惯例跟着**显式上调**入口预算表的实测值：9,201 / 56 →
  9,420 / 57（`scripts/check-doc-size.py` 的 `ENTRY_BUDGET`，同日那几行注记的同一类动作）。
  `README.md` 与 `AGENTS.md` 两份仍只许降，所以「给 README 补一句」那条被否掉（见 §具体行为 3）。
