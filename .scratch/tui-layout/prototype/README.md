# ticket 02 prototype —— 四分区几何与降级阶梯（修订 2 = 选定组合）

**这是 throwaway 探针，不是产品代码。**

用户看过第一版（1 列暗色竖线 + 无边框 + 紧凑留白）后选了**另一套**，本目录的
`chosen-*.txt` 就是按选定组合重渲染的；第一版的 `screens-*.txt` / `variant-*.txt`
留在原处作**被否决的记录**（见文末）。

## 选定组合

| 项 | 选定 |
| --- | --- |
| Q1 中左/中右分割比例 | 右栏 `clamp(⌊26% × w⌋, 25, 31)`（外框列数，含边框） |
| Q2 分隔样式 | **四个区域各自带 1 行 `Block` 边框**；header 的**底边框就是** header 下那条横线（不重复画） |
| Q3 留白 | **airy**：header 下 1 行空行 + 输入区上 1 行空行 |
| Q4 header 字段分隔符 | ` · `（只在 1 行 header 下可见） |
| Q5 多行输入续行缩进 | 2 格（`> ` 只在第一行） |

中左/中右相邻，两个 Block 的边框**共用接缝那 1 列**（不画两条），接缝两端用
`┬` / `┴` 补成 T 形，整块读起来是一个被竖线分开的大框。

## 怎么跑

```sh
cd /home/forty/code/fortystory/fs-agent
CARGO_TARGET_DIR=/tmp/tui-layout-probe-target \
  cargo run --offline --manifest-path .scratch/tui-layout/prototype/Cargo.toml -- \
  .scratch/tui-layout/prototype
```

产物覆盖式写回本目录：`chosen-*.txt`、`geometry-table.md`、`SNAPSHOTS.md`。
（第一版的 `screens-*` / `variant-*` 不再由本源码生成，也不会被覆盖。）

## `TestBackend` 可用吗？—— **可用**（票 01 交来的未知项）

- `ratatui 0.30.2/src/lib.rs:505` 无条件 `pub use ratatui_core::backend::{Backend, ClearType, TestBackend, WindowSize};`
  —— **没有 `#[cfg(feature = ...)]` 门**，默认特性下就在。
- `ratatui-core 0.1.2/src/backend.rs:112-113`：`mod test; pub use self::test::TestBackend;`，同样无特性门。
- `TestBackend::new(width, height)` 建固定尺寸 buffer；`impl Display` 逐 cell 打 symbol 并
  **自动跳过被宽字符占掉的续列**，所以 CJK 对齐可以直接看。
- 本 crate 用 `Terminal::new(TestBackend::new(w, h))` + `terminal.draw(...)` **真的渲染跑通了**，
  不进 raw mode、不需要 pty、可重复。
- 嵌套 crate 没有触发 workspace 问题：自带 `[workspace]` 自成一个 workspace root，
  仓库根 `Cargo.toml` / `src/` / `tests/` 一行未动；`CARGO_TARGET_DIR` 指到 `/tmp`，仓库里没有 target。

## 哪些数字是实测的，哪些是近似的

| 类别 | 状态 |
| --- | --- |
| 每一张 `chosen-*.txt` 的像素 | **实测**。整屏由 ratatui 渲染进 buffer 后 dump；边框落在哪一列、`┬`/`┴` 接缝、宽字符占几列、模型名在哪截断，都是 ratatui 的真实输出。 |
| `geometry-table.md` 的行列数 | **实测**（由同一套 `plan()` 直接打印）。 |
| 中文文案 | **抄自 `src/render/wording.rs`**（`mod w` 里每条注明来源行号）；`PANEL_*` 是票 05 §6 的提议标签。`TOO_SMALL` 是票 02 新提议的字符串。 |
| 假数据（模型名 / token 数 / cwd / 时钟） | 手填，但格式按票 05 §2 的口径（千分位半角逗号、`已用 / 上限`）。 |
| 字符列宽 `char_w()` | **近似**：只按 East-Asian Wide/Fullwidth 区间判 2 列。它只用于「widget 跑之前」的预折行与放不下判断；屏幕上看到的对齐仍由 ratatui 决定。真实实现对宽度必须用 `unicode-width`（`src/render/tui.rs` 的 `cell_width` 口径）。 |
| 转录折行 | **近似**：按字符折，不按词边界（与 fs-agent 既有 `wrap_take` 同款口径），所以截图里会出现 `fs-age`/`nt` 的断行——这是探针的折行器，不是布局结论。 |
| 输入区/中段/右栏的高度联动 | **实测**：`plan()` 的搜索顺序（先保 header 第二行，再保 airy；右栏独立按宽度与中段行数门控）直接决定表格与截图。 |

## 探针里编码的规则（供 ticket Answer 引用）

- 四个区域都带 1 行边框 → **固定 chrome = 7 行**（header 边框 2 + 中段边框 2 + 底部边框 2 + 提示行 1）。
- 右栏外框 `clamp(⌊26%·w⌋, 25, 31)`，内容宽 = 外框 − 2；**% 需要内容宽 ≥ 29 → w ≥ 120**。
- 右栏出现：`w ≥ 80` 且**中段内容行 ≥ 4**；否则整栏隐藏。
- `plan()` 的候选顺序：`(header 2 行, airy)` → `(header 2 行, 无 airy)` → `(header 1 行, airy)` → `(header 1 行, 无 airy)`；
  取第一个放得下的。**airy 是第一个被丢的**，header 压缩最后（且它压缩时右栏必然已经不在）。
- 输入区行数 = `clamp(草稿折行数, 1, min(10, h − chrome − header − 2·airy − 1))`。
- 提示行是底部 Block 的**最后一行内部内容**；`ctrl-c 退出` 恒在（算法为它预留位置）。
- 提示集**不含 `shift+enter 换行`**（票 04：不开键盘增强协议，Shift+Enter 等同提交）。

## 被否决的旧基线（不要据此实现）

`screens-*.txt` 与 `variant-*.txt` 是第一版探针的产物：中左/中右之间 1 列暗色竖线、
其余区域无边框、紧凑留白。**其中提示行含已废弃的 `shift+enter 换行`** —— 票 04 已否决，
本源码已删除该提示。当前基线只看 `chosen-*.txt`。

## 已知的探针简化（不要当成产品结论）

- 转录按「整行 bottom-anchor」画，没有实现滚动状态机（票 03 的活）。
- 没有光标定位（ASCII dump 里看不到光标；票 04 的活）。
- 忙碌词（`工作中` / `就绪`）没有画进任何一张图 —— 落位归票 06。
- 没有权限询问态的输入区形态。
- 主题与配色未定，边框/标签在截图里都按 dim 画，只是示意。
