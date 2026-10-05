# 04 — 文档与真机走查清单

Type: implement
Status: ready-for-walkthrough
Part of: ../spec.md
Blocked by: 02, 03

> 规格：[`../spec.md`](../spec.md) §7。

## 目标

仓库里关于「轨迹在左栏」的那几处文字全部改成新的归属，手工清单里补上这一轮真终端才能回答的问题
（79 列的可读性、时间戳列、主列页签的手感）。文档护栏与数字（`DOCS_MIN_RATIO` 一类）照旧通过。

## 现状（改前先复核）

- `CONTEXT.md`：**轨迹视图**（写着「左栏 `轨迹` 页」）、**页签条**（只讲左栏那条）、**对话视图**。
- `docs/render.md`：「外壳」那张图与那一节（`CHROME` 4、转录行算式）、「两个视图」一节（宽度
  38 / 26、`over_trace` 的轮毂分派、不降级那一条）、「键盘」一节（三键只作用对话视图）。
- `docs/tui-manual-checklist.md`：⑩（左栏轨迹页）与 ㉙（轨迹视图：全量块、滚轮与轮次分隔线）。
- `.scratch/README.md` 的索引行（`trace-tab` 那一行 + 本目录新起一行）；`README.md` 的「文档」一节
  若提到轨迹页的位置。
- `scripts/check-doc-size.py`（单元 ≤ 500 字符）与 `scripts/check-docs.py` 那套护栏。

## 落点

`CONTEXT.md`、`docs/render.md`、`docs/tui-manual-checklist.md`、`.scratch/README.md`、`README.md`、
`docs/agents/domain.md`（若词条引用需要跟着改）。

## 具体行为

1. `CONTEXT.md` 三条词条：轨迹视图 = 主列 `轨迹` 页；页签条分两条（左栏 / 主列）；对话视图那半句
   「缩在左栏里的过程行」的说法删掉。行内链接指回本 spec。
2. `docs/render.md`：外壳图重画（主列页签条那一行）、`CHROME` 与转录行算式改成长度 7、两个视图那
   一节写上宽度与「常驻」、滚轮与键盘的归属改写、新增「时间戳」一小节（来源 `Event.at`、本地
   时区、`STAMP_COLUMNS`、只画轨迹页、渲染层那两条在源头打时）。
3. `docs/tui-manual-checklist.md`：⑩ 改成主列页签的对表；㉙ 改成「轨迹视图（主列）」—— 要看的
   是 79 列下表格与代码块的可读性、时间戳列在真配色里分不分得开、切页保留滚动位置、`Ctrl-O` 收起
   左栏后轨迹页仍在。
4. `.scratch/README.md` 索引：`trace-tab` 那一行补一句「轨迹页已搬进主列（见 `trace-in-main/`）」；
   本目录新起一行，形如 `| [trace-in-main/](trace-in-main/spec.md) | spec | 轨迹视图搬进主列 + 时间戳
   （推翻 `trace-tab` 的「轨迹 = 左栏的一页」） | N/N done |`。
5. 跑 `python3 scripts/check-doc-size.py` 与既有的文档护栏脚本，违规清零。

## 验收

- 全仓 `轨迹` 的每一处描述都与新归属一致（`rg -n '左栏.{0,12}轨迹|轨迹.{0,12}左栏'` 只剩
  「原先 / 推翻」这类历史叙述）。
- 两套文档护栏全绿。
- 手工清单那一节留成「人要走查」的形态：本票写完是 `ready-for-walkthrough`，不假装自动化过了。
