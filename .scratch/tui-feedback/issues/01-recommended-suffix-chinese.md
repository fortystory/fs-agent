# 01 — 推荐标记中文化：约定改成 `(推荐)`，旧后缀仍认

Type: implement
Status: done
Part of: ../spec.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1。

## 目标

模型侧那条推荐约定从 `(Recommended)` 改成 `(推荐)`：工具描述让模型写中文后缀，判定同时认新旧
两种。用户看得见的是——屏幕上任何地方都不再出现英文后缀，旧会话里写着 `(Recommended)` 的 label
照旧被剥掉、照旧带徽标。

## 现状（改前先复核）

- `src/render/wording.rs:1024` 的 `RECOMMENDED_SUFFIX = "(Recommended)"` 是唯一判据；
  `recommended_label(label)`（同文件）按它剥后缀，返回 `(显示文本, 是否推荐)`；
  `recommended_badge()` 返回 `"（推荐）"`（全角括号，与后缀不是同一串）。
- `src/tools/ask_user.rs:135` 的工具描述里那句「让它的 `label` 以 `(Recommended)` 结尾」。
- 断言：`tests/wording.rs:1180` 起（徽标 + 新旧 label）、`tests/ask_user_question.rs:202`
  （描述里含 `(Recommended)`）、`tests/ask_user_question_tui.rs:558` 起（选项行里不出现后缀）。
- 界面侧的调用点只有两个：`src/render/input.rs:468`（plain）与 `src/render/tui.rs:3738`（问卷），
  两处都走 `wording::questionnaire_option`，所以后缀那一层改完不必动它们。

## 落点

`src/render/wording.rs`、`src/tools/ask_user.rs`、`tests/wording.rs`、
`tests/ask_user_question.rs`、`tests/ask_user_question_tui.rs`。

## 具体行为

1. `RECOMMENDED_SUFFIX` 改成 `"(推荐)"`；新增 `RECOMMENDED_SUFFIX_LEGACY = "(Recommended)"`，
   注释写明它只为读旧会话与旧 label 而留。
2. `recommended_label` 先试新后缀、再试旧后缀，两种都剥；仍然是「区分大小写、只在结尾」——
   `"推荐 (推荐) 吗"` 这种中间出现的不动。
3. 徽标 `recommended_badge()` 一个字不改（界面文案，与后缀两回事）。
4. 工具描述里那半句改写成 `(推荐)`，并说明答案里带的就是那条 label 原样（含标记）。

## 验收

- `tests/wording.rs`：`recommended_label("serde (推荐)")` → `("serde", true)`；
  `recommended_label("serde (Recommended)")` → `("serde", true)`（旧后缀兼容）；
  `recommended_label("推荐阅读")` → `("推荐阅读", false)`；中间出现的标记不误伤。
- `questionnaire_option` 对两种后缀都产出 `…（推荐）`，且结果里不含 `(Recommended)` 与 `(推荐)`
  之外的原文后缀。
- `tests/ask_user_question.rs` 的描述断言改成 `(推荐)`。
- `cargo test` 全绿。
