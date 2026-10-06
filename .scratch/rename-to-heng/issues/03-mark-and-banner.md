# 03 — 标记：汉字「衡」与上方的拼音 `héng`

Status: done
Part of: [../spec.md](../spec.md) §4

**What to build:** `wording::logo_lines()` 从"拼出 fs-agent 的五行块字"改成"拼音 `héng` 在上、汉字「衡」在下、两者中心对齐"的五行网格；README 的 banner 同步。

## 验收

- [x] `logo_lines()` 仍是**五行**，每行显示宽度等于 `layout::LOGO_WIDTH`（38）——契约不变，`mark_lines()` 的 `debug_assert` 继续成立
- [x] 构图：第二行 `héng`、第三行 `衡`，中心对齐；其余行留白
- [x] 颜色坡道不动（`MARK_BRIGHT` → `MARK_DIM`），标记仍然静止（下落短横不复活）
- [x] 窄档 `identity()` 显示 `heng {version}`
- [x] `tests/wording.rs` 的五行同宽断言保留；`tests/render_layout.rs` 里按旧字形取"那一格"的断言改成"整块标记在运行中不变"
- [x] README 的 banner 与左栏标记同源重画
- [x] `cargo test` 全绿；真机看一眼宽档左栏
