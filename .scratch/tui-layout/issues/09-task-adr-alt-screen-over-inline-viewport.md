# task：ADR —— 以全屏 alt screen 取代 inline viewport

Type: task
Status: resolved
Blocked by: 02, 03, 04, 05, 06, 07, 08

## Question

写一条新 ADR（编号照 `docs/adr/` 现状顺延），把本 effort 对 spec §19 的推翻正式记录在案。**必须在其余票都定了之后再写** —— 因为 ADR 的「代价」一节要引用它们的结论。

## 必须记录的内容

1. **决策**：TUI 渲染器改用 **alt screen 全屏四分区布局**；删除 inline viewport 路径（`Terminal::insert_before`、`paint_scrollback`、视口保留、`MoveTo(0, rows-1)` 底部锚定）。
2. **被推翻的是什么**：spec §19 的 TUI 栈那一行（`.scratch/fs-agent-v1/spec.md:526`）与用户故事 129 的「**否决 alt screen** —— 转录要能滚动 / 复制」。
3. **为什么**：inline viewport 下「光标不跟随 `>`、`>` 在后续回合消失」的 bug 在 `e25097e` 之后**仍然存在**，无法稳定修复；改成"对话面板里根本没有光标"绕开它。**这是主要动机，必须如实写出来**，不能事后包装成纯粹的美观改进。
4. **代价**（要具体，引用票的结论）：
   - 终端原生选择**只覆盖可见区**（用户已接受）
   - **滚轮不再滚转录**（鼠标捕获关闭，只留 PgUp/PgDn）
   - 转录历史**只在进程内存**里（上限 20 000 行，超出丢最旧）
   - 退出 alt screen 后转录**不留在 scrollback**（票 06 若决定退出时 dump，记下这个补偿）
5. **被否决的替代方案与否决理由**：
   - 继续修 inline viewport 光标（为什么放弃：修了几轮没修好，见 `e25097e`）
   - 两进程拆分（TUI / agent server）—— spec §19 已否决，理由（IPC 会逼"单一写入者 + `seq` 唯一身份"重做）**仍然成立**，本 ADR 重申
   - 多行输入延后（用户选择现在就做，代价是重做刚完成的单行编辑器）
6. **与 ADR 0001 的关系**：本 ADR **不动模型可见文本**，只动呈现 —— 冻结清单（debater/synthesizer system prompt、投影 `[轮 N · 名字]` 前缀、`AgentError.message`、fs-agent 工具结果）逐条都不受影响。

## 产出

- **`docs/adr/0002-fullscreen-alt-screen-tui.md`**（`docs/adr/` 现在只有 `0001-chinese-ui-frozen-model-text.md`，所以新号是 **0002**）。格式随 `0001`：标题、状态、背景、决策、后果。
- **没有 ADR 索引文件**（已核实：`docs/adr/` 下只有那一个 ADR，`CONTEXT.md` 与 `docs/agents/domain.md` 都没有清单）——所以**不要**新建索引，也不要往 `CONTEXT.md` 里塞一条。照 `docs/agents/domain.md` 的约定，ADR 就是独立文件，用的时候被读。
- 按 `docs/agents/domain.md` 的「Flag ADR conflicts」惯例，ADR 正文里**显式写明它推翻了 `spec §19` 的哪一条**（spec 不是 ADR，但冲突要露面，不能悄悄覆盖）。
- 在 `map.md` 的 `## Decisions so far` 追加一行指针。

**本票只写文档，不改代码。** 答案必须自足（`/implement` 在 `/clear` 后读它）。

## Answer

**已完成（2026-09-21）。ADR 写在 `docs/adr/0002-fullscreen-alt-screen-tui.md`**，格式照 `0001-chinese-ui-frozen-model-text.md`（标题 + 决策与理由正文 + `## Consequences`，无状态行、无索引文件）。

正文覆盖本票要求的六项：

1. **决策** —— alt screen 全屏四分区，转录自理滚动缓冲（20 000 源行上限、吸底不抢）；删除 inline 路径。
2. **被推翻的** —— spec §19 的 TUI 栈（`.scratch/fs-agent-v1/spec.md:526`）与用户故事 129 的「否决 alt screen」，在正文里逐字点名。
3. **为什么** —— inline 视口下光标不跟随 `>`、`>` 消失，`e25097e` 之后仍在；根因是 `Frame::area().y` 是视口位置、被 `insert_before` 往下推。**如实写成主要动机，没有包装成审美改进。**
4. **代价** —— 原生选择只覆盖可见区、**复制要按住 Shift 拖拽**、转录只在内存、**退出不 dump**；另加两条实现后果：`EnableMouseCapture` / `EnableBracketedPaste` 必须在三条退出路径上成对撤销；保真度唯一变更（非 assistant 消息不再截断）。
5. **被否决的替代方案** —— 继续修 inline 光标、两进程拆分（重申 spec §19 的理由）、多行输入延后。
6. **与 ADR 0001 的关系** —— 不动模型可见文本；冻结清单逐条不受影响；新增字符串全在措辞层。

**注意一处与 charting 冻结项的出入**：`map.md` 的 Notes 原本写「不开鼠标捕获」，票 03 grilling 时用户改判为**开**，所以 ADR 的代价写的是「复制要按住 Shift 拖拽」，**不是**「滚轮不能滚转录」。map 的 Notes 已同步更正。
