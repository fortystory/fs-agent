# 02 — 答案只有一个形状：`selected` 与 `custom` 并存

Type: implement
Status: done
Blocked by: 01

> 来源：[`../spec.md`](../spec.md) §3、§4。它推翻 [`fs-agent-v1` §7](../../fs-agent-v1/spec.md)
> 那条**模型侧可观察**的编码约定，理由在 [ADR 0010](../../../docs/adr/0010-questionnaire-keys-dispatch-by-zone.md)。

## 目标

- **删掉三处互斥**：`type_custom` 里的单选 `selected.clear()`
  （[:903-905](../../../src/render/tui.rs)）、`confirm_highlight` 里的 `custom.clear()`
  （[:874-875](../../../src/render/tui.rs)）、`answers()` 里的单选清零
  （[:979-983](../../../src/render/tui.rs)）。三处一删，并存就是结构性的。
- **单选语义成 toggle，但集合至多一个**：高亮项未选中时确认 = **替换**现有选择；
  已选中时确认 = **取消它**。多选维持今天的 toggle。
- **确认不再自动前进**：删掉单选的 `advance`（[:808-810](../../../src/render/tui.rs)、
  [:820-822](../../../src/render/tui.rs)）。翻页留给 `→` 与页脚按钮。
- **`Enter` 按区域拆**：选项区里与空格**完全一致**（toggle），只有 `all_handled()` 时仍是**提交**
  （保留 [:801-803](../../../src/render/tui.rs)）；输入区里 = 当前题已成立时**前进**
  （今天 [:804-805](../../../src/render/tui.rs) 的语义整个移到这里）。删掉 [:804-805](../../../src/render/tui.rs)
  在选项区的适用。
- **回改契约**：[`fs-agent-v1/spec.md` §7](../../fs-agent-v1/spec.md) 那句「**单选**下 `custom`
  **覆盖**已选（`selected: []`）、**多选**下**补充**」改成「两者都**并存**交回」，
  `ask_user_question` 的**工具描述**同改。

## 现状（2026-10-02 核实，改前先复核）

- 互斥的三处如上；`answers()` 里 979-983 那条 `if !multi_select && custom.is_some() { Vec::new() }`
  是单选覆盖的最后一站。
- 「跳过」的编码是 `selected: []` 且无 `custom`（[:970-975](../../../src/render/tui.rs)）；
  这两条约定**不动**。
- `Enter` 今天的三个角色：提交（`all_handled()`）、前进（`handled()`）、确认
  （有选项时 `confirm_highlight`）——见 [:800-812](../../../src/render/tui.rs)。
- 钉住旧语义的测试：`tests/ask_user_question_tui.rs:248` 一带（自定义覆盖单选 / 补充多选）。

## 测试

- 改写 `tests/ask_user_question_tui.rs:248` 那条为**并存**：选一项再打字，提交后
  `selected` 与 `custom` **同时**在。
- 新增：单选 toggle 两条（已选再确认 = 取消、集合变空；确认另一项 = 替换）+ 「确认不前进」一条。
- 新增：选项区 `Enter` 与空格等价、`all_handled()` 时 `Enter` 提交、输入区里已成立则前进。
- 新增：`custom` 删空之后是 `None`，而 `selected` 不受影响；`selected` 空 + 无 `custom` 仍是跳过。
- `cargo test` 全绿、`cargo clippy --all-targets` 干净。

## 不做什么

- 不改「跳过」与「`answers` 里没有该 `id`」两条既有约定。
- 不动 plain 路径（它一次一行，表达不了并存，spec §10）。
- 不动 `Esc`（票 03）、页脚（票 04）。

## Comments

- **落地（2026-10-02）**：删掉三处互斥（`type_custom` 的单选清空、`confirm_highlight` 的
  `custom.clear()`、`answers()` 的单选清零），`confirm_highlight` 变成**切换**（单选集合仍至多一个，
  确认另一项是替换）；`press` 里单选的两次 `advance` 删掉（确认不翻页），`Enter` 按区域拆
  （选项区与空格一致、输入区前进、`all_handled()` 仍是提交）；鼠标 `select_option` 跟着不再翻页。
- **契约回改**：`fs-agent-v1` §7 那条「单选覆盖 / 多选补充」改成「两者可以同时出现」，
  `ask_user_question` 的工具描述与模块注释同改，`tests/ask_user_question.rs` 里钉住旧文案的那条
  断言跟着改。
- **先红后绿**：新增四条（并存、已选再确认=取消、确认另一项=替换、`Enter` 按区域），
  改写两条（单选确认前进 → 翻页归 `→`；`typing_overrides...` → 并存），另有三条既有测试因为
  「确认不前进」跟着改写。
- **验收**：`cargo test` 全绿（问卷 TUI 那个文件 24 条）、`cargo clippy --all-targets` 干净。
- **review 补记（2026-10-02）**：spec §2 的键位表原来把「全部处理完时提交」也写在**空格**那一
  行，与 §4「键盘上唯一的提交路径」和实现对不上 —— 表格已改，提交只归 `Enter`。
