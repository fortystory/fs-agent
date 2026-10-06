# 0013. 文件页那一档可以换成一块外来屏幕（内嵌 nvim）

- 状态：接受（2026-10-06）
- 背景：[`.scratch/nvim-file-viewer/spec.md`](../../.scratch/nvim-file-viewer/spec.md)；
  原型与它的实测在 `prototype/embed-nvim` 分支的 README 里。

> **2026-10-06 补注（正文不改，只记更名）**：程序已更名为**衡**（`heng`），本文里作为产品名的 `fs-agent` 读作「衡」（[ADR 0014](0014-renamed-to-heng.md)）。

## 决定

`[ui] file_viewer = "nvim"` 时，文件页点开一个文件不再走内置的只读预览（详情覆盖层 +
`DetailKind::File`），而是在浮层里嵌一屏**真的 nvim**：pty 里 spawn、`vt100` 解析成网格、
键与鼠标原样转发。缺省 `"builtin"` 一个字都不变。

## 为什么

内置预览是渲染器自己读盘排版出来的 —— 有高亮、有行号，但它只有**看**。真编辑器带着读的人
自己的配置（LazyVim、LSP、搜索、跳转），而不必为看一个文件离开 TUI。缺省不动：内置那一档
瞬时、不起进程、任何机器上都在。

## 代价：三处例外

1. **一块矩形不受视觉纪律约束。** `to_lines` 纯函数、语义色板、字形语法管的都是我们自己画
   的东西，而这一块是别人的屏幕：它自己铺满底色，浮层因此**去框留白**。
2. **键盘独占，且 `Esc` 关不掉它。** 详情覆盖层立着时 `Esc` / `Ctrl-D` 是关；这一档里
   `Esc` 与 `Ctrl-D` 必须进 nvim（退出插入模式、向下翻半屏），关闭改由 `Ctrl-C`、框外点击
   与 nvim 自己的 `:q` 三条。`Ctrl-Z` 照旧留给挂起。
3. **fs-agent 自己当终端。** pty 另一端没有终端，nvim 启动时问的 DSR / DA1 / DECRQM /
   Kitty 键盘协议 / DECRQSS / XTGETTCAP 要由我们答（`src/render/viewer.rs` 的 `respond_to`），
   不答它就停在 `E1568` 上不画。

## 没走的两条

- **`nvim --embed` + UI 协议**：没有 ANSI 解析层，但网格状态机、属性表、键记法、鼠标都得
  自己写 —— 那是写一个 nvim GUI 前端。
- **让位终端全屏跑 nvim**（复用挂起那条路交还终端）：技术上最省，但它离开了 TUI，也就没有
  「边看转录边看文件」这件事了。
