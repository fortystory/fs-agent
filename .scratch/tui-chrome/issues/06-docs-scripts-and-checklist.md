# 收口：脚本锚点、文档与真机清单

Type: implement
Status: done
Blocked by: 01, 02, 03, 04, 05

> 规格：`.scratch/tui-chrome/spec.md` §1–§5 与 `Testing Decisions`、`Further Notes`。
> 依赖票 01–05 全部落地：本票只收它们留下的账。

## 目标

把票 01–05 推翻的东西在仓库其余地方交代清楚：被推翻的旧 spec 段落留交棒痕迹、启动检查脚本的锚点跟着换、术语与文档索引更新、真机清单加一节。

## 落点

`scripts/tui-startup-check.py`；`docs/tui-manual-checklist.md`；`CONTEXT.md`；`README.md`（只在文档索引确实要动时）；`.scratch/README.md`；`.scratch/tui-sidebar/spec.md`（交棒补记）；`.scratch/tui-chrome/spec.md`（若有实现期修正）；`src/render/mod.rs` 与 `src/render/tui.rs` 的模块级 rustdoc（它们都写着「一圈全屏外框」）。

## 具体行为

1. **`scripts/tui-startup-check.py`**：`BORDER_H = "─"` / `BORDER_V = "│"` 那两条**数框线数量**的断言在本轮之后必然为 0（屏幕上再没有实线框）。改成数 `┄` / `┆`（阈值照实测重取，别拍脑袋），并把那段注释从「窗格的框还在」改成「外壳的分隔线与浮层的虚线边框还在」。`MARK_ROW` / `STATUS_ANCHOR` / `STATUS_TAIL` / 提示行 / `TEARDOWN` 那几处锚点不受影响，**不要**顺手改。
2. **`docs/tui-manual-checklist.md`** 加一节（编号顺延），覆盖：
   - 没有外框之后，左栏与主列在真终端里是否仍然分得开；
   - 三层虚线（`┄` / `┆` / 回合条的 `┊`）读起来是否分得清；
   - `CHROME_LINE` 的深度在**你自己的背景色**上是否合适（不合适就调那一个常量，把实测值记在这一节里）——这是本轮唯一一个「随终端配色而变」的取值；
   - 详情覆盖层看起来是不是屏幕正中，左栏在时尤其看一眼；
   - 问卷立着时在转录上滚一格、在问卷上滚一格，各滚到谁头上；
   - 40×10 那一档：转录 3 行、输入区 3 行是否真的够用。
3. **`CONTEXT.md`**：输入区那一条写着「它**没有自己的边框**——上下的分隔线由外壳画，界面里只有一圈外框」。**「只有一圈外框」这句要改**（外框已经不在）。同时把「分隔线」的字形（若是箭头字符）改成虚线，或在词条里点明它现在是虚线。若「回合条」词条里有关于焦点/普通格字形的句子，确认它仍然成立（回合条本轮不动）。
4. **`src/render/mod.rs` 与 `src/render/tui.rs` 的模块级 rustdoc**：都写着「一圈全屏外框」（`mod.rs` 第 13 行附近、`tui.rs` 第 5 行附近）。改成描述现在的外壳（无外框、竖分隔列 + 两条横线、虚线、详情覆盖层居屏幕）。
5. **`.scratch/tui-sidebar/spec.md`**：在 §1 与 §7 各补一条**带日期的交棒补记**（照该文件 §8 已有的「实现期修正」写法），说明外框与「待答问题吃掉一切指针」这两条已被 `tui-chrome` 推翻，并指向本目录。**不改写原文**——那些是当时的理由。
6. **`.scratch/README.md`**：加 `tui-chrome` 一行（形态 spec、一句话、票数与状态），并把它记进那次「由 `ls` / `grep '^Status:'` 核过」的说明里。
7. **`README.md` 的文档索引**：只有在新增了 `docs/**` 文件时才动。本票不新增文档文件，所以**预期不动**——核一遍再决定。
8. **`docs/agents/` 不涉及**：issue tracker 与 triage labels 的约定没有变。

## 测试

- `python3 scripts/check-language.py` 全过（它是文档语言的棘轮；`CONTEXT.md` 与 rustdoc 的中文改动若把某个比率推歪，按实测值收）。
- `python3 scripts/tui-startup-check.py` 在真终端下全过（它要 pty）。
- `python3 scripts/wayfinder-check.py .scratch/tui-chrome/map.md` **不适用**（本目录没有 `map.md`，不是 wayfinder effort）——不要为它造一张图。

## 验收

- [ ] `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` 与票 01–05 落地时一致。
- [ ] `python3 scripts/check-language.py` 全过。
- [ ] `python3 scripts/tui-startup-check.py` 全过。
- [ ] 真机清单那一节按上面的条目走一遍，把「`CHROME_LINE` 在维护者背景色上的实测」写下来。
- [ ] `.scratch/README.md` 的表格与「数法」那一段都跟得上实际。

## Comments

- 2026-10-01 落地。
- `scripts/tui-startup-check.py`：`BORDER_H` / `BORDER_V` 从 `─` / `│` 改成 `┄` / `┆`，阈值仍是「每个方向 ≥ 3 格」（降级护栏，不是几何断言）。改前实测 `0 horizontal / 2 vertical`（那 2 是状态行自己的 `│` 分隔符），改后 **12/12 GREEN**；脚本抬头与锚点注释里「一圈外框」的说法一并改掉。
- `docs/tui-manual-checklist.md` 加第 ⑳ 节（8 条），抬头那段的「一圈外框 + 一条全高左栏 + 一条主列」也跟着改了。
- `CONTEXT.md` 的**输入区**词条：拿掉「界面里只有一圈外框」，并把 40×10 那半句从「输入区只拿 2 行」改成「输入区 3 行、转录 3 行」。
- `src/render/mod.rs` 与 `src/render/tui.rs` 的模块 rustdoc 都写着「一圈全屏外框」，已改。
- `.scratch/tui-sidebar/spec.md` 的 §1 与 §7 各补一条带日期的**推翻**补记（指向本目录），原文一字不动 —— 那是当时的理由。
- `README.md` 的文档索引**没有动**：本轮不新增 `docs/**` 文件（核过一遍才决定）。
- 验收：`python3 scripts/check-language.py` OK；`cargo test --all` 全绿；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩既有的一处漂移。
