# ticket 03 prototype —— 折叠提示行 / 详情覆盖层 / 工具行的形态

**这是 throwaway 探针，不是产品代码。** 它回答一个问题：**这些折叠提示行和详情覆盖层应该长什么样。**

## 怎么跑

```sh
cd /home/forty/code/fortystory/fs-agent
CARGO_TARGET_DIR=/tmp/tui-ux-probe-target \
  cargo run --offline --manifest-path .scratch/tui-ux/prototype/Cargo.toml -- \
  .scratch/tui-ux/prototype
```

全部快照覆盖式写回本目录（`variant-*.txt`）。探针自带 `[workspace]` 自成一个 workspace root，仓库根 `Cargo.toml` / `src/` / `tests/` 一行未动；target 指到 `/tmp`。

## 一次真渲染，不是手绘

每张 `variant-*.txt` 的上半部分是 ratatui `TestBackend` 渲染出的**真实 buffer**（`Terminal::new(TestBackend::new(w,h))` + `terminal.draw(...)`，无 pty、无 raw mode）。下半部分的 `styles` 是必须补的一节：**文本 dump 只有字形，没有颜色**，所以每个角色的 `fg` 与 `UNDERLINED` / `BOLD` 逐行列在后面——配色决策看那一节，不要看字形。

## 变体清单（每个变体解决哪个问题）

### 可点击提示（同一个折叠契约，四种可发现性做法）

| 文件 | 做什么 |
| --- | --- |
| `variant-click-none.txt` | **对照**：可点但**没有任何视觉提示**（只看颜色/加粗） |
| `variant-click-underline.txt` | 可点行整体加 `UNDERLINED` |
| `variant-click-marker.txt` | 可点行行首加 `▸ ` |
| `variant-click-suffix.txt` | 可点行行尾加 ` [详情]` |

### 思考行

| 文件 | 做什么 |
| --- | --- |
| `variant-click-underline.txt` | 基准：`[kimi] 正在思考` / `[kimi] 思考完成`（无字形） |
| `variant-thinking-glyph.txt` | 加状态字形：`… 正在思考` / `✓ 思考完成` |

### 详情覆盖层

| 文件 | 做什么 |
| --- | --- |
| `variant-detail-rule.txt` | 分节用 `── 思考 ──` 横线；宽 72 |
| `variant-detail-colon.txt` | 分节用 `思考：` 标签；宽 72 |
| `variant-detail-wide.txt` | 同 rule，但宽 90（占更多横向空间） |
| `variant-detail-80x24.txt` | 覆盖层在最窄的「有右栏」尺寸下的样子 |

### 尺寸覆盖（基准形态：underline + rule + 无字形）

| 文件 | 尺寸 |
| --- | --- |
| `variant-size-40x10.txt` | 最小合法尺寸：转录只剩 1 行，尾部是失败行 |
| `variant-size-80x24.txt` | 右栏刚出现 |
| `variant-size-120x10.txt` | 宽而矮 |
| `variant-size-120x24.txt` | 常规 |

## 哪些是实测的，哪些是近似

| 项 | 状态 |
| --- | --- |
| 屏幕字形与边框位置 | **实测**：整屏由 ratatui 渲染进 buffer 后 dump |
| 颜色 / `UNDERLINED` / `BOLD` | **实测**（样式在渲染时施加），但在 `styles` 一节里**以文字列出**，因为 dump 只有字形 |
| 折叠提示行 / 工具行 / 覆盖层的**形态** | 本票要定的东西 |
| 四分区**几何** | **近似**：header 1–2 行、中段吃剩余、底部 4 行、右栏 `clamp(26%w,25,31)`。精确几何归 `prototype：删 airy 后的几何、降级阶梯与 Mark 间距` |
| 中文字符列宽 | **近似**：只按 East-Asian Wide/Fullwidth 判 2 列；真实实现用 `unicode-width` |
| 转录折行 | **近似**：按字符折（与既有 `wrap_line` 同口径） |

## 探测里编码的契约（来自票 02 的 Answer，不是本票要重开的）

- 思考提示**一个思考段一行**：`正在思考` 起，正文到达即原地定格 `思考完成`。
- `xxx` = `speaker_label`；工具行 `[kimi] 调用 bash command=…`；失败只在**行尾**加 `失败`。
- 折叠态里 post-hook 反馈仍显示；输出 / 错误 / 无结果进详情。
- 思考提示 `DarkGray`、工具失败行 `Red`、调用行 `BOLD`。
- 只有**完成态**可点；流式期的「正在思考」不可点。

## 选定（2026-09-23，第一轮评审）

| 项 | 选定 | 被否决的备选（留在 `variant-*`） |
| --- | --- | --- |
| 可点击提示 | **行首 `▸ ` 标记** | 整行下划线 / ` [详情]` 后缀 / 无提示 |
| 详情分节 | **`── 思考 ──` 横线** | `思考：` 标签 |
| 覆盖层宽度 | **90 列** | 72 列 |
| 思考字形 | **`…` / `✓`** | 纯文字 |

选定组合重渲染为 `chosen-*.txt`：

- `chosen-40x10.txt` / `chosen-80x24.txt` / `chosen-120x10.txt` / `chosen-120x24.txt` —— 折叠提示行（覆盖层关闭），尺寸矩阵
- `chosen-detail-120x24.txt` / `chosen-detail-80x24.txt` —— 同一组合 + 详情覆盖层打开

结果写入票 03 的 `## Answer` 与 `.scratch/tui-ux/map.md` 的 `Decisions so far`。

---

# ticket 05 几何探针 —— 删 airy 前 / 后

同一 crate 的第二个 bin（`src/bin/geometry.rs`），产物在 **`geometry/`** 子目录（避免与票 03 的 `chosen-*.txt` 撞名）：

```sh
cd /home/forty/code/fortystory/fs-agent
CARGO_TARGET_DIR=/tmp/tui-ux-probe-target \
  cargo run --offline --manifest-path .scratch/tui-ux/prototype/Cargo.toml --bin geometry -- \
  .scratch/tui-ux/prototype/geometry
```

- `geometry/geometry-table.md` —— **旧（当前 `plan()`，含 Mark header）→ 新（删 airy）** 的逐尺寸几何表 + 草稿涨高 + 阈值。
- `geometry/geometry-*.txt` —— 新布局的真渲染快照（`40×10` / `40×12` / `60×24` / `80×16` / `80×24` / `120×24` / `120×24-draft10` / 两个「太小」边界）。

它同样只做真渲染；四分区几何在这里是**实测**（不再是票 03 里的近似），因为这张票问的就是几何本身。文档漂移（spec §2 的几何表早于 Mark header）标在 `geometry-table.md` 顶部。
