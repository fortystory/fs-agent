# grilling：历史详情覆盖层的复用与降级

Type: grilling
Status: open
Blocked by: 02
Part of: ../map.md

## Question

定下「重开后的历史行怎么支撑详情弹窗」。范围冻结为：**与 live 用同一套覆盖层**（tui-ux 票 03 的形态），历史里的 `▸` 行同样可点。

## 需要定

1. **可点范围**：历史里哪些行带 `▸` 并可点——`✓ 思考完成` 行与工具行？点击命中与 live 用同一张命中表机制（tui-ux 票 04：绘制时当帧记录）？重播期间（历史还没铺完）能不能点？重播完成后呢？
2. **工具全文的读取**：`outputs/<tool_call_id>.txt` 在 `--continue` 后仍在（会话目录未删）——确认读取路径与 live 完全相同；`prune` / 手工删掉 `outputs/` 之后，详情显示事件里的 head/tail 预览 + `全文不可用`（tui-ux 契约的降级）。给一张「文件在 / 不在 / 空」的表。
3. **思考全文的读取**：从历史 `MessageCompleted.reasoning` 重建的「思考完成」行，点开后显示的全文与 live 一致；`reasoning: None` 但有思考增量的历史（合成器路径）在**重播时无法重建**（增量不在日志里）——明确此时历史里**没有**该提示行，还是要显示别的东西。
4. **覆盖层的开关与滚动**：与 live 完全一致（`Esc` / 再点关闭、`↑/↓`、`PgUp/PgDn`、滚轮、打开时转录冻结）；打开时新到的 live 事件如何表现（冻结是否也挡住吸底）。
5. **与 tui-ux 契约的对齐**：一句话确认本票**不新增**形态；若发现 tui-ux 的契约在历史场景下不成立（例如历史行的命中矩形在分帧重播中会漂移），明确指出并给出历史专用的例外。
6. **边界**：历史里同一次工具调用出现多次（同一 `tool_call_id` 不会有）；历史里 `tool_call` 没有结果（悬空，票 02 会查明其日志形状）时点击显示什么。

## 先读

- `map.md` 的 Notes（冻结项与与 tui-ux 的接口）
- `.scratch/tui-ux/issues/02-grilling-collapse-and-detail-contract.md` 的 Answer（详情覆盖层契约）
- `.scratch/tui-ux/issues/04-grilling-mouse-click-answer-hit-contract.md` 的 Answer（命中表机制）
- `.scratch/tui-ux/issues/03-prototype-collapse-hint-and-detail-overlay.md` 的 Answer（选定形态）
- `.scratch/tui-ux/research/01-collapse-detail-data-sources.md` 事实 24-34（`outputs/` 与指针）
- `research：分帧重播的接缝事实与成本实测`（票 02）的答案

## 答案落点

契约级：可点范围 + 读取路径与降级表 + 与 tui-ux 契约的一致性声明（或例外）。不要写实现代码。
