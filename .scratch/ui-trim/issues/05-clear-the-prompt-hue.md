# 05 — 随 `❱` 一起清掉色相/呼吸那套

Type: implement
Status: done
Blocked by: 04
Part of: ../spec.md

`palette::prompt_colour` 与四个 `PROMPT_*` 常量、`hsv_to_rgb`、画家那一处 `prompt_style`，
存在的唯一理由都是给 `❱` 上色。票 04 之后它们没有任何调用者。

规格见 [`spec.md`](../spec.md) 的问题陈述 5 与「定下来的四条」第三条。

## 要做什么

- 删掉 `palette::prompt_colour`、`PROMPT_HUE_PER_SECOND`、`PROMPT_SATURATION`、
  `PROMPT_SATURATION_BREATH`、`PROMPT_BREATH_PER_SECOND`、`PROMPT_VALUE`、`hsv_to_rgb` 与它那
  条色环单元断言（`hsv_to_rgb_matches_the_shortcut_table`）—— 一起删，不留「将来也许还要」的
  函数。
- 删掉 `tui.rs` 的 `prompt_colour(frame)` 与 `draw_input` 里给 lead span 上色那一段（连同它
  的注释）、以及那条「提示符色相与脚本同刻一致」的单元断言。
- `PULSE_FRAME` 与脉冲计数器留着：左栏扫光与光标闪烁仍挂在上面。
- `docs/render.md` 的「输入区」与「默认色板」两处、`CONTEXT.md` 的**提示符色相**词条与
  **脉冲**词条里那句「给提示符上色的色相」、`docs/tui-manual-checklist.md` ⑯ 一并改写 ——
  这几处今天都在描述一套不存在的颜色。

## 验收

- `grep -rn "prompt_colour\|PROMPT_HUE\|PROMPT_SATURATION\|PROMPT_BREATH\|PROMPT_VALUE\|hsv_to_rgb"
  src/ tests/` 为空（文档里的历史记录按仓库惯例不改）。
- `cargo clippy --all-targets` 不新增警告（死代码本来就是警告）。

## 评论

落地（2026-10-08）。`palette` 的提示符专色一节（四个 `PROMPT_*` 常量、`prompt_colour`、
`hsv_to_rgb` 与它的色环断言）与 `tui.rs` 的 `prompt_colour(frame)`、`draw_input` 里那段
上色、`the_prompt_colour_is_the_script_at_the_same_moment` 一并删掉，原位留一段注释说明它
为什么不在。`PULSE_FRAME` 与脉冲计数器留着（扫光与光标闪烁还挂在上面）。

`grep -rn "prompt_colour\|PROMPT_HUE\|PROMPT_SATURATION\|PROMPT_BREATH\|PROMPT_VALUE\|hsv_to_rgb" src/ tests/`
只剩历史注释里的那几处引用；`docs/render.md`、`CONTEXT.md`（删掉**提示符色相**词条、改
**脉冲**与**字形循环**）、`docs/tui-manual-checklist.md` ⑯ 一并改写。
