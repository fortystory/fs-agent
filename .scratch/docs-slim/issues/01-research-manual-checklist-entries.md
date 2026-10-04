# research：手工清单的条目现状

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

[`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md) 有 26 个条目（①–㉖）、668 行，其中 **548 行是散文**，是 scope 内最大的散文体量（13 段 ≥800）。冻结项 9 把它纳入「压表达」，但压表达的前提是**这些条目还有活读者** —— 一个已经被推翻的功能的走查步骤，压得再漂亮也是废纸。

要查清并写下来的，**逐条**（① 到 ㉖，一条不漏）：

1. **它验的功能今天还在不在**：条目描述的行为在 `src/` 里是否仍成立（例如「输入区三行」「左栏开关 `Ctrl-O`」「挂起 `Ctrl-Z`」）。被后来的 feature 推翻的要标出来 —— 已知的推翻链：`tui-sidebar` 的外壳被 `tui-chrome` 推翻、`tui-layout` 的几何被 `tui-sidebar` 推翻。
2. **它是否已被自动化覆盖**：[`scripts/tui-startup-check.py`](../../../scripts/tui-startup-check.py) 或 `tests/*.rs` 是否已经验了同一件事 —— 若是，手工项就是重复劳动。**注意**：写「光标、鼠标、resize 留手工」这条分界的是 `scripts/tui-startup-check.py` 的 docstring 与 [`docs/render.md`](../../../docs/render.md):295，要如实引用，不要把本该手工的项判成重复。
   （**2026-10-04 更正**：这一句原写「那个脚本的 docstring 与 `docs/skills.md` 都写明了」—— 实测 [`docs/skills.md`](../../../docs/skills.md) 通篇没有这句话，它讲的是「一条指令该放在 `AGENTS.md` / skill / hook 哪一边」。见本票 `## 作答`。）
3. **它引用的界面元素是否还存在**：条目里点名的提示行文案、键位、页签、以及**条目编号自身**（①–㉖ 被 `docs/render.md` 的 ⑭、`docs/goals.md` 的 ㉒ 引用过）是否还与代码一致。
4. **它有多长**：每条的散文行数 —— 后半段的条目（⑰–㉖）是否比前半段更臃肿（它们是逐次追加的）。

产物：一张 26 行的表（条目 / 功能还在否 / 已被自动化否 / 引用的元素还准否 / 散文行数），外加一句总结：这 548 行里有多少是「还有活读者、只是写得长」（压表达的对象），多少是「已经失效」（冻结项 4 的规则覆盖不到的那一类）。

**本票只查事实，不做「要不要删失效条目」的决定** —— 那个决定归 [票 06](06-grilling-checklist-and-lifecycle.md)。

## 作答

findings：[`.scratch/docs-slim/research/01-manual-checklist-entries.md`](../research/01-manual-checklist-entries.md)（26 行逐条表 + 推翻链 + 被引用编号清单）。
- **26 条全部还有活读者**（功能都在 `src/` 里）：**0 条整体失效**、**1 条全自动**（⑨ 的几何已被两条测试逐值钉住）、**22 条混合**、**3 条纯手工**（① 光标 / ③ 鼠标 / ⑱ 沙箱真机记录）。
- **没有一条是重复劳动**：`scripts/tui-startup-check.py` 的 docstring 明写「光标、鼠标与缩放仍留在手工清单里」；写这条分界的是它 + `docs/render.md`:295，**`docs/skills.md` 通篇没有这句话**（票面这处引用要改）。
- **4 处引用失效，全是旧外壳的数字**（`tui-chrome` 之前）：④.5「转录只剩 1 行」、⑨ 的「7 / 14 行」、⑯.3「40×10 输入区 2 行」、⑫.2「详情居中于主列 `min(主列宽−4,135)`」（实为屏幕居中，⑳.5 已改口）。今天的值是 10 / 10 / 17 / 3 行，公式是 `转录 = h − 4 − 输入行数`（`CHROME` 由 7 降到 4）。
- **压表达对象与「失效」是两拨**：548 行那笔账对应前一类（只是写得长）；上面 4 处是**信息本身错了**，冻结项 4「只删复述、不改信息量」管不到这一类。
- 行数口径：按 map 的口径（列表行并入相邻块）26 条共 **534** 行块内容（368 散文 + 164 列表 + 2 代码）；后半段确实被追加，但**不比前半段明显臃肿**（⑰–㉖ 是 20–46，①–⑬ 是 4–27，最长的是 ⑱ 的真机记录 46 行）。
- 票 06 若动条目编号：外部引用至少 5 处要同步（`docs/render.md` ⑭、`docs/goals.md` ㉒、`.scratch/sandbox/spec.md` ⑱ ×2、`.scratch/tui-chrome/spec.md` ⑳、`.scratch/sidebar-toggle/spec.md` ㉖）。
