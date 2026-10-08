# 13 — 收口：回改旧 spec、渲染文档与手工清单

Type: implement
Status: done
Part of: ../map.md
Blocked by: 11, 12

> 规格：[`../spec.md`](../spec.md) 的实现决定 §9 与「补记」；决策图的
> [prototype 票](../issues/04-prototype-page-and-detail.md) C 组列的真机观感项。

**What to build:** 行为齐了之后把文档收到位。这一页整节改了 [`todo-and-modes/spec.md`](../todo-and-modes/spec.md)
§4（页签是闩、计数行留最后一行、列表逐行截断——那三条今天就错了），所以那一节要重写并在旧处留一行
指向本目录；渲染那篇要写上这一页现在是什么；真机观感项要进手工清单。

## 验收

- [ ] **回改** `todo-and-modes/spec.md` §4 整节，并在旧处留一行指向本目录
- [ ] `docs/render.md` 的左栏一节写上这一页现在是什么：页顶一条进度行 + 全部未完成项（正在做置顶）
      + 长文本折行 + 已完成项折掉 + 整页可点开整份列表（快照）
- [ ] `docs/tui-manual-checklist.md` 加一节真机观感项，五条：
      两档下读不读得下去（窄档内容 23 列，一条 25 字就折 3 行）、`●` 在真字体下是一列还是两列、
      完成项的降暗退不退得后（三枚字形在 28 列里够不够分）、进度行右端那四列的按钮好不好找、
      折行把路径断成两片刺不刺眼
- [ ] `.scratch/README.md` 的 `todo-page` 行更新（map + spec + 票数与完成度）
- [ ] `CONTEXT.md` 的「待办列表」与「待办工具」两条**核一遍**：预期是不改（列表仍是调用参数、仍是
      模型自己的、左侧栏仍是非执行者那一份）；若发现要改，先写清理由
- [ ] 两条取舍写进文档：折行照旧硬切（英文与路径会被从中间断开）、看全的入口是鼠标按钮且不给键位
      （对照九家，Claude Code 与 Gemini 都是 `Ctrl+T`，见
      [research 票](../issues/03-research-other-agents-plan-ui.md)）
- [ ] 那一处**对既有纪律的有意例外**要留痕：折行件的文档写着「续行从第零列起 —— 缩进会主张一种
      折行后的文字并没有的结构」，这一页选了缩进，理由是「这片属于哪一项」是真结构。写在折行件
      自己的文档注释里，不只写在 spec 里
- [ ] `scripts/wayfinder-check.py` 对本目录仍 PASS；`spec.md` 抬头的 `Status:` 改成实际完成度
- [ ] 验证按仓库门槛跑过并记进提交信息（照 `docs/agents/commits.md`）：`cargo test`、
      `cargo clippy --all-targets`、`cargo fmt --check` 与两道文档护栏

## 评论