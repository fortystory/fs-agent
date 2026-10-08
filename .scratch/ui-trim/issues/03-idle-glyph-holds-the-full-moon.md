# 03 — 就绪时状态字形钉在满月

Type: implement
Status: done
Part of: ../spec.md

月相循环是「还在跑」的一条信息通道，而今天空闲时它也一格一格地走。就绪时它应当**不动**，
停在 `🌕`。

规格见 [`spec.md`](../spec.md) 的问题陈述 3 与「定下来的四条」第四条。

## 要做什么

- `wording::status_spinner(frame, busy)`：`busy` 为假时返回 `🌕`，不看 `frame`；为真时照旧按
  `PULSE_GLYPHS` 走（运行中每 2 帧一格）。
- `PULSE_GLYPHS` 那一集留着（运行中仍在用），只把「空闲也在动」这件事收掉。
- 就绪时脉冲仍照常推进：光标闪烁与左栏扫光都挂在同一个计数器上，这一条字形的静止不动它们。

## 验收

- 单元断言：`status_spinner(0, false) == status_spinner(7, false) == status_spinner(64, false)
  == "🌕"`；`status_spinner(0, true) == "🌘"` 且运行时仍逐格变。
- 布局断言：空闲的两帧之间，屏幕上一个格子都不变（状态字形不再是变化项）。

## 评论

落地（2026-10-08）。`status_spinner` 在 `busy` 为假时直接返回新的 `IDLE_GLYPH`（`🌕`），
不看帧；`IDLE_FRAMES_PER_GLYPH` 那半条推导退场，换成 `BUSY_FRAMES_PER_GLYPH = 2`。

断言：`tests/render_tui.rs` 的速率一条改成「运行中每 2 帧、就绪一个帧区间都不换」；
`render_layout.rs` 的 `the_status_glyph_moves_while_idle_too` 改名成
`the_status_glyph_holds_still_while_idle` 并改成「64 帧之后仍是 `🌕`」。`tick` 的注释与那条
`the_pulse_runs_in_idle_too...` 一起改写：空闲那一半仍在走的理由是光标闪烁与那几条回执的寿命。
