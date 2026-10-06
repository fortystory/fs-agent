# 13 — 转录取色、换发言者空行、严重度收敛

Type: implement
Status: done
Part of: ../map.md
Blocked by: 11

> 规格：[`../spec.md`](../spec.md) 实现决定 §4、§6、§23。

## 目标

转录里**每一条语义各归其位**：过程行退后、用户与助手的话保持全亮度、要你注意的才着色；**换人时空一行**，于是减色之后「谁在说」仍然一眼看得出；严重度的两套映射收成一套。

## 现状（改前先复核）

- 转录的语义各自为政：工具失败 `Red`（不带粗）、诊断与 hook 反馈 `Yellow`、上下文注入 `LightBlue`、分歧行 `Magenta`+`BOLD`、轮次开始 `Cyan`+`BOLD`、`Notice` 走 `narration` 的 `DarkGray`、`Severity::Good` 是 `Green`。
- **严重度有两套映射各写各的**：TUI 一份、plain 的 `Severity::ansi` 一份，彼此**没有任何直接绑定**（只共用「哪个收尾原因属于哪一档」那个判据），而且已经漂移了一处 —— TUI 的 `Bad` 多一个 `BOLD`。
- 块与块之间**不空行**，靠「名字独占一行 + 正文顶格」分段。

## 落点

- `src/render/tui.rs`：块绘制的那条主分支、`narration`、严重度三处（`severity_style` / `severity_line` / `severity_speaker_line`）。
- `src/render/palette.rs`：取色。
- `src/render/severity.rs`：语义判据（**只读**，见「不做什么」）。

## 具体行为

1. **prefactor：严重度收敛到一个语义来源**。TUI 从色板拿 `style(severity)`；**plain 的 `Severity::ansi` 输出一个字节不动**（它是 plain 可见输出的一部分）。收敛到「共用语义表」为止。
2. **按判据取色**：
   - 过程 → 静音：工具摘要、思考完成、迭代号、权限询问与裁决、hook 事件、执行者派生、用量摘要、**轮次开始**（+`BOLD`；它从 `Cyan` 降下来）、`Severity::Good` 与 `Note`（它们从绿 / 青降下来 —— **正常完成不再抢注意力**）。
   - 内容 → 正文：`Notice` 那一类、**分歧行**（`Magenta` 退场，`!! ` 前缀 + `BOLD` 已经标记了它）。
   - 信号 → 警告 / 错误：诊断、hook 反馈、`Warn`、工具失败、`Bad`、会话错误。
   - **例外**：上下文注入行保留它的专色（既有 spec 明写「注入行与别的叙述行同灰，于是注入 / 用户 / 助手在轨迹页上分不开」）。
3. **换发言者空一行**，同一人连发的块不拆散。**实现位置是块序列生成期**（与轨迹页隔行底色同一个位置的做法），不是绘制期 —— 于是两个视图各自决定，滚动时不重排。

## 验证

1. 逐格缓冲快照：逐语义的样式（工具摘要 / 失败 / 诊断 / 注入 / 分歧 / 轮次开始 / 完成与通知各一条）；空行出现在**换发言者**处、不出现在同一人连发的工具行之间。
2. `tests/render_layout.rs` 里转录块的行号会整体平移（空行导致）—— 按新事实改，**不要**为了少改断言而放弃空行。
3. `tests/render_tui.rs` 的严重度三色断言按新语义改写。
4. `render_block` / `render_block_uncoloured` 那条路径同样要生效（否则测试与真屏不一致）。

## 不做什么

- 不动 `Severity::of` 的**判据**，也不动 plain 的输出。
- 不做 `▸` 的判据修正（归[字形符号表](17-glyph-table-and-spacing.md)）—— 本票只取色与分段。
- 不动转录的**分工**（对话视图收什么、轨迹视图收什么）—— 那是既有 spec 的契约。

## 实现记录（2026-10-06）

**已落地**（自动化部分全绿：`cargo test --all-targets` 1216 passed / 0 failed、`cargo clippy --all-targets` 与 `cargo fmt --check` 干净、`scripts/check-doc-size.py` 与 `check-language.py` 绿、`scripts/tui-startup-check.py` 15/15 绿）。

转录取色按判据收敛、`Severity::Good`/`Note` 退成静音、换发言者空一行（块序列生成期、两个视图各记各的）。**真终端**：手工清单 ⑮。
