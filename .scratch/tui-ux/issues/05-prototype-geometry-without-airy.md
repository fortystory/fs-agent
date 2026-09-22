# prototype：删 airy 后的几何、降级阶梯与 Mark 间距

Type: prototype
Status: resolved
Blocked by: —
Part of: ../map.md

## Question

`AIRY_ROWS` 删掉后，四分区几何与降级阶梯要**重新实测**，并把 Mark header 的间距改成「logo 5 行 + 空 1 行 + 信息 1 行」。既有几何数字的唯一来源是 `.scratch/tui-layout/prototype/`（`TestBackend` 实测像素）；本票产出的新数字将成为 `/to-spec` 回改 tui-layout spec §2 的依据。

## 冻结的输入

- **删 airy**：header⇄中段、中段⇄底部各 1 行留白都不要，中部多出 2 行给转录。
- **Mark header 内部间距**：logo 5 行 + 空 1 行 + 信息（cwd / 模式 / 时钟）1 行，`LOGO_HEIGHT` 净高**仍 7**；`LOGO_MIN_WIDTH` / `LOGO_MIN_HEIGHT` 是否随之变化由实测决定。
- 只动 TUI；这一条**推翻 tui-layout spec §2** 已落地的数字（原型是唯一来源，先改原型再改 spec）。

## 要重算/实测

1. **固定 chrome**：`CHROME = 3×BORDER_ROWS + HINT_ROWS` 不含 airy，删掉 `AIRY_ROWS` 后 `plan()` 的高度分配、`fits_airy` / `max_input_rows` 的存废与替代判据。
2. **逐尺寸几何表**：至少 `40×10` / `40×12` / `60×24` / `80×16` / `80×24` / `120×24` / `174×50`，记录 header 内容行 / 中段内容行 / 转录外框与内容 / 右栏外框与内容 / 输入区行数。重点复核两条旧结论是否还成立：**`120×24` 草稿涨到 10 行时右栏消失**；**右栏能出现的最小尺寸 80×16**。
3. **降级顺序**：仍是「先隐藏右栏、再压 header」吗？删除 airy 后顺序或阈值是否会变；`LOGO_MIN_HEIGHT` 要重算（旧值由含 airy 的公式推导）。
4. **Mark 间距**：`draw_mark` 里 logo 与信息行的行距改法；`80×?` 与 `LOGO_MIN_HEIGHT` 边界上 logo 是否仍「画整个或完全不画」。
5. **快照**：删 airy + Mark 间距后的真渲染屏幕存成 `chosen-*.txt`。
6. **边界尺寸**：`39×24`（太小）、`40×9`（太小）、`40×10`（完整降级布局）在新几何下是否仍如 spec 所述。

## 方法

复用 `.scratch/tui-layout/prototype/` 的 `TestBackend` 实测方式（无 pty、无特性门）。产物落在 `.scratch/tui-ux/prototype/`，命名沿用 `geometry-table.md` / `chosen-*.txt` 同族。**把「旧值 → 新值」的对照写进 `geometry-table.md`**，方便 `/to-spec` 回改。

## 产出

`.scratch/tui-ux/prototype/` 下的几何表与快照；本票 `## Answer` 记选定数字与「哪些旧结论被推翻」。

## 先读

- `.scratch/tui-layout/spec.md` §2（要被推翻的几何与降级阶梯）
- `.scratch/tui-layout/prototype/geometry-table.md`、`README.md`、`chosen-*.txt`
- `src/render/layout.rs`（`AIRY_ROWS` / `CHROME` / `fits_airy` / `max_input_rows` / `header_content_rows` / `LOGO_*`）
- `src/render/tui.rs` 的 `draw_mark` / `draw_header`

## 进度

**100%** —— 完成。几何探针落在 `.scratch/tui-ux/prototype/src/bin/geometry.rs`（同一 throwaway crate 的第二个 bin），产物在 `.scratch/tui-ux/prototype/geometry/`：`geometry-table.md`（旧→新逐尺寸对照）+ 9 张真渲染快照。用户已确认采用公式值 `LOGO_MIN_HEIGHT = 18` 并接受其余派生数字。结论见 `## Answer`。

**下一步**：无（已 resolved）。`/to-spec` 按 §7 的回改清单改 tui-layout spec §2。

## Answer

**几何定稿（2026-09-23，`TestBackend` 实测）。** 探针 = `.scratch/tui-ux/prototype/src/bin/geometry.rs`；产物 = `.scratch/tui-ux/prototype/geometry/`（`geometry-table.md` + `geometry-*.txt`）。

### §1 删 airy 的算术后果

- `AIRY_ROWS`、`fits_airy`、`plan()` 里的 airy 分支**整体删除**；`CHROME` 不变（`3×BORDER_ROWS + HINT_ROWS = 7`）。
- `max_input_rows` 的 `room = h − (CHROME + header_rows + MIN_MIDDLE_ROWS)`（不再减 airy）。
- 中段内容行 = `h − CHROME − header_rows − input_rows`，所以**凡是旧 `airy=on` 的尺寸，中段一律 +2 行**；旧 `airy` 本来就 OFF 的尺寸（`40×10`、`120×10`）**不变**。

### §2 逐尺寸几何表（空白草稿）

完整表在 `geometry/geometry-table.md`；要点：

| 尺寸 | 旧 中段内容 | 新 中段内容 | 备注 |
| --- | --- | --- | --- |
| 40×10 | 1 | 1 | 不变（旧 airy 已 OFF） |
| 40×12 | 1 | 3 | 旧 airy=on |
| 60×24 | 7 | 9 | |
| 80×16 | 4 | 6 | 右栏仍在 |
| 80×24 | 7 | 9 | |
| 120×24 | 7 | 9 | |
| 174×50 | 33 | 35 | |
| 120×10 | 1 | 1 | 不变 |

右栏宽度不变（`clamp(⌊26%w⌋, 25, 31)`），转录与右栏共享接缝的列数不变。

### §3 被推翻的旧结论（两条）

1. **右栏能出现的最小尺寸：`80×16` → `80×14`。** 旧：`80×14` 时 `fits_airy` 为真、中段只剩 2 行 → 右栏隐藏；新：中段 4 行 → 右栏出现（`25×6 / 23×4`）。
2. **Mark header 的最小高度：`LOGO_MIN_HEIGHT` 20 → 18。** 旧值公式里含 airy 的 2 行；删掉后由同一梯子读出 18，所以 Mark 从 `h ≥ 18` 就出现（例：`42×18` 新画 Mark + 中段 3 行，旧是两行文本 header + 中段 7 行）。**用户已确认采用公式值 18。**

### §4 仍然成立的旧结论

- **`120×24` 草稿涨到 10 行时右栏消失**：成立（中段只剩 1 行）。但**输入上限从 7 升到 9**（`max_input_rows` 多了 airy 的 2 行），所以 9 行与 10 行草稿现在同档。
- **降级顺序「先隐藏右栏、再压 header」**：不变；§2 的栏内降级（内容宽 <29 丢 `（%）`、行数不足先丢缓存）不在本票范围，未改。
- **`40×10` 仍渲染完整降级布局**（1 行 header + 1 行中段 + 1 行输入 + 1 行提示）；`39×24` / `40×9` 仍是「终端太小」。

### §5 Mark 间距

- `draw_mark` 改成 **logo 5 行 + 空 1 行 + 信息 1 行**，`LOGO_HEIGHT` 净高仍 **7**，几何不变（快照 `geometry-120x24.txt`）。
- logo 仍「画整个或完全不画」：`header_content_rows` 的 Mark 分支不变，只是阈值降到 18。

### §6 一个既有的文档漂移（本票发现，不在本票修）

- tui-layout spec §2 的几何表写在 **Mark header 落地之前**，所以它写 `60×24` 的 header 内容行 = 2，而当前代码（「旧」列）已经是 **7**（Mark）。这不是删 airy 引入的；`/to-spec` 回改 §2 时要一并修正，已在 `geometry-table.md` 顶部标出。

### §7 给 `/to-spec` 的回改清单

- 删 `AIRY_ROWS` / `fits_airy`；`max_input_rows` 与 `plan()` 去掉 airy 分支。
- `LOGO_MIN_HEIGHT` 公式去掉 `AIRY_ROWS` 项，值 **20 → 18**。
- 几何表按 `geometry-table.md` 的「新」列重写（每个旧 `airy=on` 的尺寸中段 +2）。
- 降级阶梯里「右栏能出现的最小尺寸 80×16」→ **80×14**；「`h ≤ 11` 丢掉 airy」整条删除。
- `draw_mark` 的 Mark 间距改动写进 §2（几何不变）。
- 顺带修正 Mark header 引入后遗留的 header 内容行列。
