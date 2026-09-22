# grilling：测试与验证迁移

Type: grilling
Status: open
Blocked by: 01, 03, 04
Part of: ../map.md

## Question

把本图所有决定的**可验证性**定下来：哪些进 `cargo test`、哪些进 pty、哪些只能进真终端手工清单；以及要为「重开一个已存在的会话」造什么 fixture。沿用 `.scratch/tui-ux/issues/08-grilling-test-and-verification-migration.md` 的分层先例。

## 需要定

1. **fixture 会话目录**：怎么造一个可复现的会话（`log.jsonl` + `outputs/<tool_call_id>.txt`）——手工写 JSONL？用现有 `Harness` 跑一个 mock provider 落盘？还是把 `tests/support/` 里的既有 fixture 扩展？给出**一种**做法与它落哪。
2. **重播的自动断言**（`TestBackend`）：重播后的 pane 内容与顺序（历史在前、live 在后）；分帧中途的画面（**部分历史 + 进度行**）；完成后进度行清除、状态行恢复；面板的 token 累计 / 回合数；header 的模式（含「进过计划模式又出来」的历史）；空 / 单条 / 512 / 513 条的边界。
3. **顺序与输入**：重播期间按 `Enter` 不提交（草稿仍在、没有 user 消息入流）；重播期间到达的 live 事件在完成后**按 `seq` 追加**（用一个可控的 fixture 事件注入来断言顺序）。
4. **历史详情**：历史 `▸` 行可点、点开显示全文（`outputs/` 在时）；`outputs/` 不在时显示 `全文不可用`；`reasoning` 重建的「思考完成」行可点。
5. **pty**：`scripts/tui-startup-check.py` 要不要加一条 `--continue` 路径（先造一个会话目录再启动、等首帧/重播收敛、退出交还终端）？它的 docstring 分界是「只有 pty 看得见的东西」；重播的**正确性**归 `cargo test`，pty 只保「重开路径不崩、首帧正确、退出干净」。给出加或不加与理由。
6. **手工清单**：`docs/tui-manual-checklist.md` 新增哪些项（真会话开一次看进度观感、重开后的历史详情点击、分帧中途上滚、超大会话的启动手感）。
7. **既有测试与纪律**：本图会不会打破既有断言（提示行 / 几何 / 面板 / 模式）；开工前核实 `cargo test --all-targets` 的准确通过数（charting 时为 **633 passed**）；`cargo clippy --all-targets` 干净、`cargo fmt --check` 只留两处既有漂移。
8. **spec 回改**：本图要回改哪些既有 spec（`.scratch/fs-agent-v1/spec.md` §19/§7？`docs/render.md`？），列成清单交给 `/to-spec`。

## 先读

- `tests/render_layout.rs` / `tests/render_tui.rs`（`TestBackend` 与合成事件的既有口径）
- `tests/support/`（现有 fixture 形态）
- `scripts/tui-startup-check.py`（docstring 与 `GESTURES`）
- `docs/tui-manual-checklist.md`（既有 ①–⑪ 的编号与口径）
- `.scratch/tui-ux/issues/08-grilling-test-and-verification-migration.md` 的 Answer（分层先例）
- 票 01 / 03 / 04 的答案

## 答案落点

可执行的清单级：fixture 做法、新增 / 改写的用例名与断言对象、pty 增删、手工项编号与步骤、spec 回改清单。不要写实现代码。
