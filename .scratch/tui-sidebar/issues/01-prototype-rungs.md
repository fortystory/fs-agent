# 原型：外壳改版的四个有争议的档

Type: prototype
Status: resolved
Blocked by:

> 设计票（prototype）。产出是**帧**，不是产品代码。票面规格见 `.scratch/tui-sidebar/spec.md`。

## Question

spec §1–§2 的几何规则能不能画出可读的界面？其中四条是「不看图定不下来」的：

1. **左栏宽档 40 还是 42** —— 40 的内宽正好是 mark 的 38（无左右留白）；42 是 mark + 左右各 1 格 air，代价是转录少 2 列。
2. **中档 34 这个数字** —— 它只是「面板需要 28 + 一点余量」，不是推出来的。34 与「直接收到 28」哪张读起来更好？
3. **焦点格的颜色** —— 黄（现有「可以动手」语义）还是亮品红（mark 同族）。
4. **40×10 地板尺寸** —— 外框 + 1 列回合条 + 1 列滚动条 + 状态行 + 提示行之后，转录还剩几列、还读不读得懂。

## What to build

`.scratch/tui-sidebar/prototype/` 下一个 **throwaway** 探针 crate（自带 `[workspace]`，只有 `ratatui = "0.30"`，`publish = false`，`CARGO_TARGET_DIR` 指到 `/tmp`）。**不要动仓库的 `src/`、`tests/`、根 `Cargo.toml`。** 照抄 `.scratch/tui-layout/prototype/` 的做法：`ratatui::Terminal::new(TestBackend::new(w, h))` + `terminal.draw(...)`，把整屏 dump 成 txt。那里的 README 说明了 `TestBackend` 在默认特性下可用、以及「哪些是实测、哪些是近似」。

按 **spec §1–§2** 实现新几何（不是旧的四块边框），画出：外框、分隔线（`┬ ┴ ├ ┤ │` 字形表）、左栏（mark / 文字身份 / tab 条 / 面板字段）、主列（转录样本 + 滚动条列 + 回合条、状态行、输入区、提示行）。

文案一律抄 `src/render/wording.rs`（含新的 `调用量` / `轨迹` / `文件` / 占位符 / `上下文 <n>%` 短形式）。假数据（模型名、token、回合数）手填，但要能看出位数与对齐。

**尺寸矩阵**：`40×10`、`60×24`、`80×14`、`80×24`、`100×24`、`120×24`、`174×50`；外加 `120×24` 草稿 3 行 / 12 行各一张。

**变体**（每张注明是哪个变体）：左栏 40 vs 42（120×24）、中档 34 vs 28（100×24）、焦点格黄 vs 亮品红（120×24）。

**回合条**至少画两个状态：① 3 个单位（焦点在最新）；② 30 个单位（溢出、最上一格 `⋮`、焦点在第 12 个）。

## Deliverable

- `prototype/frames/*.txt`（真实渲染色，一张一档）
- `prototype/geometry-table.md`（每档的行列数与左栏/状态行/转录各占多少，实测）
- `prototype/README.md`（怎么跑；哪些数字实测、哪些近似；探针里编码了哪些 spec 规则；已知简化）
- 回话里报告：四条争议档各自的图看起来如何、以及**实现 spec 时发现哪里含糊或做不到**。

## Answer

探针在 `.scratch/tui-sidebar/prototype/`，19 张帧 + 几何表全部由 ratatui `TestBackend` 实测
（重跑命令见该目录 `README.md`，已验证可复现）。**四条争议档的结果**（前三条是看图决定、留给用户；
40×10 已实测）：

1. **左栏 40 vs 42** —— 三张帧：`frames/120x24.txt`（40、mark 居中、左右各 1 格 air）、
   `-variant-sidebar-40-flush.txt`（40、mark 贴左边框）、`-variant-sidebar-42.txt`（左右各 2 格 air）。
   40 居中那版留白够用；flush 那版明显挤；42 多花 2 列只买到第 2 格 air。
2. **中档 34 vs 28** —— `frames/100x24.txt` 与 `-variant-sidebar-mid-28.txt`：差 6 列文本宽；
   28 那档上下文那一行会丢 `（6%）`（最宽的值连标签要 29 列，不是 spec 原先写的 28）。
3. **焦点格颜色** —— `frames/120x24.txt`（亮品红）与 `-variant-focus-yellow.txt`（黄）。
4. **40×10** —— `frames/40x10.txt`：左栏隐藏、主列 38 列、**转录 2 行 × 36 文本列**、
   状态行退到「模式 + 上下文」、提示行 1 条 + 退出 + 就绪。**认得出来，能用。**

**发现的两个 spec 洞（已折回 spec）**：

- **状态行「整行消失」这一档到不了**：要主列内容宽 < 11，即 `w < 13`，早于 40×10 地板就被
  `too_small` 拦下（`geometry-table.md` §F）。spec §2 的状态行阶梯由四档改**三档**，
  「整行消失、2 行还给转录」删除。
- **回合条溢出时会出现「整条没有亮格」**：原写法「只保留最近 N 格」在视口停在第 12/30 个单位时，
  焦点那一格根本不在条上（`frames/120x24-rail-30-units-focus-12-gap.txt`），用户故事 12 失效。
  spec §4 改为**窗口跟着焦点走**（两端各自按需画 `⋮`）。

**另外三处与 spec 写法的偏差（已改 spec）**：tab 条实占 **3 行**（上下横线 + 标签行），不是 2；
左栏内部**高度确实会触发降级**（`h = 10` 时身份行与「缓存」已让位），不是「永远触发不到」；
120 列空闲态提示行是 **4 条 + 退出（放不下 `就绪`）**，忙碌态因退出提示短 7 列反而放得下。

日志：`.scratch/tui-sidebar/prototype/geometry-table.md`（A–H 八张实测表）、`SNAPSHOTS.md`（哪张帧回答什么）、
`README.md`（实测 vs 近似、探针里编码的规则、已知简化）。
