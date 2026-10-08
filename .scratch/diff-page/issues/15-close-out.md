# 15 — 收口：真机清单与验收

Type: implement
Status: done
Part of: ../map.md
Blocked by: 12, 13, 14

> 规格：[`../spec.md`](../spec.md) 的实现决定 §9 的后半与测试决定的最后一条。前置是
> [12 — 那份 diff 上色](12-diff-coloring.md)、[13 — 刷新与手动重取](13-refresh-and-manual-reread.md)、
> [14 — 交给外部工具那一档](14-external-diff-viewer.md)。

**What to build:** 这一 feature 收口 —— 真机要看的那些项进手工清单，三套文档护栏与三件套全绿。
其余文档（`CONTEXT.md` 的两处、`docs/render.md` 的各节）分摊在各自那张票里，这里只做清单与验收。

## 验收

- [x] `docs/tui-manual-checklist.md` 加一节：两档（40 / 28）下这一页的密度与路径截断、diff 两色
      在真配色下够不够分、拖选复制出来的东西、装了真外部工具之后的观感
- [x] `CONTEXT.md` 两处都在：**左栏**词条写上第四页、**改动页**新词条立起来（照**文件页**那条的先例）
- [x] `docs/render.md` 三处到位：左栏一节、详情覆盖层一节、外部工具那一档
- [x] `cargo test` 全绿、`cargo clippy` 干净、`cargo fmt --check` 只留既有漂移
- [x] 三套文档护栏（`check-language.py` / `check-doc-size.py` / `lifecycle-check.py`）与启动检查照旧通过
- [x] 不变量逐条核对：**事件 schema 一个字节没动、模型可见文本一个字节没动、`FileIndex` 的遍历
      规则一个字没改、左栏宽度两档与 `Ctrl-O` 意愿没动**
- [x] `.scratch/diff-page/map.md` 的「任务清单」在这一票之后全部勾上，`wayfinder-check.py` PASS

## 评论

收口：`CONTEXT.md` 三处（左栏写上第四页、`改动页` 新词条、`焦点行` 覆盖两页；外加**语义色板**
那条「一处例外」）、`docs/render.md` 的「改动页」一节与高亮 / 外壳 / 文件页 / 键盘四处跟上、手工清单 ㊲、
`scripts/check-doc-size.py` 收 `docs/adr/0018` 并把 `README.md` 的棘轮按实测上调。三套护栏与
`wayfinder-check.py` PASS；真机项在 pty 里做了初步走查（第四签、切页列清单、`Enter` 开弹窗），
两档密度与真配色下的观感仍留在清单里等人走查。
