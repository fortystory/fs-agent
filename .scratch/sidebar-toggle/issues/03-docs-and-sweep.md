# 文档收口：`CONTEXT.md` 词条回改与索引

Type: implement
Status: ready-for-walkthrough
Blocked by: 01, 02

> 规格：[`.scratch/sidebar-toggle/spec.md`](../spec.md)（`来源` 段的两条推翻、§4 的手工面）。
> 代码归 [票 01](01-toggle-state-and-geometry.md) 与 [票 02](02-hint-line-entry.md)；本票只动文档，
> **不改写任何原文** —— 被推翻的句子旁边加一条带日期的补记。

## 目标

让「左栏去留只由宽度决定」这句旧话在两处都读得出**已经被推翻**，让 `Ctrl-O` 在给人看的文档里
有名字，并把这轮的真实状态写进索引。

## 落点

`CONTEXT.md`、`.scratch/tui-sidebar/spec.md`、`docs/render.md`、`docs/tui-manual-checklist.md`、
`.scratch/README.md`、`README.md`（**实现期补的落点**：它的「状态」段写着测试条数与行数，本轮
改了代码它就会过时，照仓库规矩要跟着实现走 —— 「文档」那一节仍然不动）。

## 具体行为

1. **`CONTEXT.md` 的**左栏（Sidebar）**词条**（`:239-241` 那一处）：把「**去留只由宽度决定**」
   改成「**去留 = 用户意愿 × 宽度档**」并补一句 `Ctrl-O` 切换、意愿只活在进程内；`_Avoid_:` 一行不动。
   照 `CONTEXT.md` 既有的密度写，别把实现细节（字段名、函数名）搬进词条。
2. **`.scratch/tui-sidebar/spec.md` §2**：加一条**带日期的推翻补记**（照该文件已有的「实现期修正」
   与 `tui-chrome` 补记的写法），说明「去留只由宽度决定」已被本目录推翻、宽度那一层不变。
   **原文一字不改** —— 那是当时的理由。
3. **`docs/render.md`**：「外壳」一节写左栏的两种去留（宽度档、用户意愿）与主列宽度的算法；
   「键盘」一节把 `Ctrl-O` 加进键位表（若那张表按族分组，放「视图 / 显示」那一类）。
4. **`docs/tui-manual-checklist.md`**：新增一节（当前最大编号是 ㉕ 问卷，所以这一节是 **㉖**），
   覆盖：120 列以上按 `Ctrl-O` 左栏整列消失且转录变宽；再按回来页签与选中页还在；80–119 列的
   28 档收起 / 叫回；< 80 列按下去什么都不发生；跑一个回合时按下去也生效；详情覆盖层立着时
   没反应；提示行最末那条在真终端里读起来够不够清楚（你自己终端多少列、它出不出现）。
5. **`.scratch/README.md`**：把 `sidebar-toggle/` 那一行的状态改成**实际**读数（票全绿之后写
   `3/3 done`；没做完就如实写 `N done + M ready-for-agent`），并在那段历史记录里补一句本轮
   （一次 `/wayfinder` 会话的十问折叠、直接实现路线、不建图）。
6. **不做**：不改 `README.md` 的「文档」一节（`.scratch/` 从来不在那张表里；要改的是它的
   「状态」段，见「落点」）；不动 `.scratch/README.md` 的**数法**段与「需求池」段（本轮没有新增 seed）。

## 验证

```sh
grep -n "去留" CONTEXT.md .scratch/tui-sidebar/spec.md docs/render.md
grep -n "ctrl-o\|Ctrl-O" docs/render.md docs/tui-manual-checklist.md CONTEXT.md
ls .scratch/sidebar-toggle/issues/*.md | wc -l          # 必须等于 spec 里列的票数（3）
grep -h '^Status:' .scratch/sidebar-toggle/issues/*.md  # 收尾后应全是 done
python3 scripts/check-language.py                       # 文档护栏
```

手工清单那一节要在真终端里走过一遍才写 `done`；没走完就按仓库规矩写 `ready-for-walkthrough` 并
在票里说清剩下哪几条（见 `.scratch/README.md` 对这两个状态的区分）。

## Comments

- **落地（2026-10-02）**：文档全部落地。

- **`CONTEXT.md`** 的**左栏**词条：「去留只由宽度决定」→「**去留 = 用户意愿 × 宽度档**」，
  `_Avoid_:` 一行不动。
- **`.scratch/tui-sidebar/spec.md` §2**：加带日期的推翻补记（指向本目录），原文一字未改。
- **`docs/render.md`**：「外壳」一节的左栏 bullet 补意愿与几何；「页签条」bullet 点明
  `Ctrl-O` 是**整栏**的键、不是给页签的；「键盘」一节新增一段写它的守卫边界（忙闲都生效、
  不清举手；详情覆盖层与历史重放拦住它；`Ctrl-Z` 仍穿得过）。
- **`docs/tui-manual-checklist.md`**：新增 ㉖ 节（7 条走查项）。
- **`.scratch/README.md`**：本目录那一行与那段历史记录按实际写。
- **`README.md`**（「落点」里实现期补的那处）：「状态」段的测试条数与行数按实测更新
  （1,009 条测试 / 47 个 test binary / 0 failed；`src/` 36,712 行、`tests/` 35,705 行），
  并补一句本轮的落地。
- **`docs/render.md` 的 `Ctrl-Z` 那句是实现期补的**（不在票面「具体行为」3 里）：收尾审查指出
  「`Ctrl-O` 是唯一一个只关界面的键」过强（`PgUp`/`PgDn` 同类），所以改成「一个**纯视图手势**」
  并把 `PgUp`/`PgDn` 一并点出。
- **`CONTEXT.md` 是直接改写的**（不是「加补记」）：spec 的「来源」段原先把两处推翻写成同一种
  处置，但词汇表不是历史文档 —— 它该反映当前事实，所以词条照本票「具体行为」1 直接改；只有
  `.scratch/tui-sidebar/spec.md` 那种**记录当时理由**的文档才原文不改写、另加带日期的补记。
  spec 的「来源」段已按这条改正。
- **收尾审查（`/code-review` 双轴）之后还改了**：`.scratch/sidebar-toggle/spec.md` 的六个英文
  小标题改成中文（`问题陈述` / `方案` / `实现决定` / `测试决定` / `明确不做` / `补充说明`，照
  `suspend-gesture` 的写法 —— AGENTS.md 把票与 spec 的散文一并算中文）；三张票的落地记录从
  顶层 `## 实现完成` 挪到 `## Comments` 之下（`docs/agents/issue-tracker.md` 的约定）；
  `spec.md` 里一处「侧栏」改成「左栏」（CONTEXT.md 的 `_Avoid_`）。

**剩下的是人**（所以这票是 `ready-for-walkthrough`，不是 `done`）：拿 ㉖ 在真终端里走一遍 ——
其中三条只有人答得了：第 3 条（79 列以下按 `Ctrl-O` 什么都不该发生）、第 5 条（详情覆盖层
立着时没反应）、第 6 条（`ctrl-o 左栏` 在你自己的常用宽度下出不出现）。走完之后把这一票改成
`done`，并把 `.scratch/README.md` 那一行同步成 `3/3 done`。
