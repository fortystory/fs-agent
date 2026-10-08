# 01 — 问卷立着时也能打开详情覆盖层

Type: implement
Status: done
Blocked by: —

> 来源：[`../spec.md`](../spec.md)。维护者 2026-10-09 的要求：回答问题时看不到轨迹里的详情、
> 也看不到文件里的文件。

## 目标

- `click_at` 里 `questioning` 那一串门（`files_click` / `changes_click` / `follow_link` /
  轨迹详情的 `open_detail`）全部去掉：落在底部问卷块之外的点击照旧分派。
- **键盘跟着当前那一层**：详情覆盖层与文件查看器立着时归浮层，`Esc` / `Ctrl-D` 关掉之后
  **原样还给问卷**（作答、焦点、翻页位置都留着）。这两支本来就排在问卷那一支前面，所以键盘
  那一侧一个字不用改 —— 要改的是指针那一侧与渲染那一侧。
- `layout::Regions` 多一个 `overlay_floor`（平时 = 屏幕底边，问卷立着时 = 问卷块的顶边），
  `overlay_area` 在自己那段高度里居中。**平时那块形状一个字不变**（上下各留两行转录）。
- 中间那三类确认（粘贴 / 清草稿 / 目标停下）仍占着整个指针 —— 那一条不动。

## 具体行为

- 覆盖层开着时，底部问卷区落在**框外** —— 点它会关掉覆盖层（既有语义），然后那一下落在问卷
  自己的区域上仍归问卷。
- 放不下正文时不开：`overlay_area` 回答 `None`，`draw_detail` 把详情收掉，键盘仍在问卷上。
- `draw_frame` 里 `draw_detail` 的注释与前提要改：它**可以**与一个问题同时立着。

## 测试

- `tests/render_layout.rs`：轨迹页开详情 + 题面仍可见；详情占着键盘（空格 / 回车都进不去）
  + `Esc` 关掉后键盘回到问卷并提交得到答案；文件页点一行开内容弹窗 + 题面仍可见；纯几何那
  一条（`overlay_floor` 与居中）。
- `cargo test` 全绿（改动前第 2 条会失败：`[x] 1. 甲` 早早就出现了）。

## 手工清单

[`docs/tui-manual-checklist.md`](../../docs/tui-manual-checklist.md) ㊳。