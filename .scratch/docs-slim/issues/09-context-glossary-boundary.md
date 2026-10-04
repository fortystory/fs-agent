# 09 — `CONTEXT.md` 剥到「一句定义 + 指针」

Type: implement
Status: done
Part of: ../map.md
Blocked by: 08

> 规格：[`../spec.md`](../spec.md) §4。判据来自 [`CONTEXT.md` 的词条边界](07-grilling-context-boundary.md)
> 的 `## 作答`；逐词条的四类片段清点与「别处是否已有一份」的落点表在
> [`../research/02-context-implementation-details.md`](../research/02-context-implementation-details.md)。

## 目标

读术语表的人在一页里就能分清近义词：每个词条先给**一句「它是什么」**，细节想去再点 `docs/` 链接；表里
**不再出现任何 `§N` 与机制名**；那 6 条「别处没有一份」的实现细节在 `docs/` 落地。剥完这个文件的单元违规
从 **8 项降到 0**。

## 现状（2026-10-04 核实，改前先复核）

- **体量与欠账**：`CONTEXT.md` 348 行 / 44KB、15 节 **72 个词条**；纯实现细节 **246 处指称 + 193 条行为描述**，
  格式必需的英文槽位 **78 处**（约 1 : 3.2）；单元口径下 **8 项超长违规（全 scope 最多）**。
- **落点表**：`../research/02` §7.3 是按词条抽查过的 `docs/` 落点；§7.2 是 6 条 coverage `none` / partial 的证据。
- **14 个「会空掉」的词条**（`../research/02` §6 的 A 档）：渲染节占 8 个 —— 提示符色相 / 脉冲 / 下落短横 /
  状态行 / 输入区 / 选项区 / 焦点回合 / 回合条。
- **两条硬输入**：`PromptHue` / `FallingDash` / `StatusRow` 在 `src/` 查无此名（真实形态是
  `PROMPT_HUE_PER_SECOND` / `DASH_FALL` / `wording::status_row()`）；`.scratch/tui-input-pulse/spec.md`:6 证明
  「spec 主动往 `CONTEXT.md` 加词条」是**仍在生效的流程**（本票的 §6 规矩就是治它）。
- **增补 `docs/` 的占比额度很紧**：`docs/observability.md` 余量 2.53 点（纯英文额度约 163 字符）、
  `docs/render.md` 余量 1.87 点（约 543 字符）。**用中文叙述包住标识符**，别贴代码清单。

## 落点

`CONTEXT.md`、`docs/render.md`、`docs/observability.md`、`docs/discussion.md`。**不新增文件。**

## 具体行为

1. **目标形态**：`**中文名（English）**: <这个词是什么，一到两句>。细节见 <`docs/` 链接>`。
   删掉机制名、内部类型与事件变体、调用顺序、参数与阈值（246 处指称 + 193 条描述）。
2. **指针必须落到真实来源**：用 `../research/02` §7.3 的落点表；落不到的先补 `docs/`（见第 3 条）。
3. **6 条独有内容搬进 `docs/`**：问卷请求 / 问题选项（`none`）、作答草稿 / 问卷文案（partial）→
   `docs/render.md` 的问卷一节；价目表 → `docs/observability.md`；落点 → `docs/discussion.md`。
4. **9 处 `§N` 全部换掉**：`spec §5` / `§15`（讨论者）→ `docs/discussion.md`；6 处裸 `§N` 按落点表换；
   **`spec §7` 存疑 → 删**。换完 `grep -n '§' CONTEXT.md` **零命中**。
5. **14 个空壳词条各补一句纯领域定义**（它是什么、读者凭什么认出它），不许只剩标题 + 指针。
6. **硬约束句全留**（「不做 / 永远不 / 一律 / 绝不」），边界按**句子性质**分：留规范句、删描述句；
   一句两半都有的，留规范那半、删描述那半。
7. **第 10 行的规矩改一行**：英文槽位是代码里的标识符 / 类型名；**若这个概念只在 spec 与文档里立了名、
   代码里没有对应实体，槽位就写那个已被文档采用的写法**。四个造名槽位（`PromptHue` / `FallingDash` /
   `StatusRow` / `Pulse`）保留；它们的代码形态**不进正文**，随指针去 `docs/`。
8. **顺带清掉 `docs/render.md` 的两处**（本票已经在动它）：`docs/render.md` L279-288 那块复述
   （`../research/03` §3 判定「删，三处独有信息并回 README」）与它那个 ≥500 的清单项。
9. **拆掉剩下的超长单元**：剥离后仍 ≥500 的按顶层项 / 段落拆开。

## 验证

1. `grep -n '§' CONTEXT.md` 零命中；`grep -c '::' CONTEXT.md` 与改前对比大幅下降（机制名清掉）。
2. `python3 scripts/check-doc-size.py` 退出 0，且 `CONTEXT.md` 的违规数 **8 → 0**（把该文件的基线**显式收紧到 0**，
   在提交信息里写明理由）。
3. `python3 scripts/check-language.py` 全绿 —— 特别注意 `docs/observability.md` 与 `docs/render.md` 的占比余量。
4. 抽查三条落点：讨论者 → `docs/discussion.md`、权限三兄弟 → `docs/permissions.md`、渲染 → `docs/render.md`
   都能读到被剥掉的机制。
5. 14 个空壳词条各有一句定义（逐个点过去，不是抽样）。
6. `git status` 只看得到本票的四个落点。

## 不做什么

- 不动词条的中文名与格式（第 10 行只改英文槽位那半句）、不动 `_Avoid_` 行、不动第 14 行
  「`agent` 是泛称、类型名用 `Debater` / `Executor`」的规矩。
- 不新增文件；不动 `docs/research/` 与 `.scratch/*/spec.md`（冻结项 3）。
- 不在这张票里改入口三份、手工清单或 lifecycle（各有自己的票）。

## 评论

（2026-10-04，实现会话。落点只有 `CONTEXT.md`、`docs/render.md`、`docs/observability.md`、`docs/discussion.md`
四个文件；没建分支、没提交，交给 Lead 统一提交。）

### 实测数字（改前 = `git show HEAD:`，改后 = 工作区）

- **`CONTEXT.md` 非空白字符数：20,374 → 17,199**（总字符 21,876 → 18,228；行数 349 → 341）。
- **超长单元：8 → 0。** 改前 `scripts/check-doc-size.py` 打在 `CONTEXT.md` 的 8 项是
  L24 622 / L106 547 / L110 647 / L114 568 / L148 539 / L217 609 / L259 535 / L324 637；改后
  `--list` 里该文件最长单元 423，一个 >500 都没有。**没有改
  `scripts/check-doc-size.py` 的 `VIOLATION_BASELINE`**（它仍写着 `"CONTEXT.md": 8`）——它是
  「不许恶化」的棘轮，把该文件基线显式收紧到 0 是**收口票 12** 的统一动作。
- **`§` 命中：9 处出现 → 0**（`grep -n '§' CONTEXT.md` 零行）。9 处的去向：
  `spec §5` / `spec §15`（讨论者）→ `docs/discussion.md`；`spec §7`（问卷，存疑）→ **删**；
  6 处裸 `§N` 全在渲染节（举手 `questionnaire-keys/spec.md §5`、左栏 `sidebar-toggle/spec.md §2`、
  输入区 `tui-chrome/spec.md §1`、提示符色相 `tui-input-pulse/spec.md §2b`、脉冲的
  `exit-gesture/spec.md §6` 与 `questionnaire-keys/spec.md §5`）→ 全部换成 `docs/render.md`。
- **`grep -c '::'`：9 行 / 10 处出现 → 3 行 / 3 处出现。** 剩下的 3 处都是「问题选项」那一条的
  格式必需槽位与它 `_Avoid_` 里点的 `wording::Choice`（票面「不动中文名与格式」），机制名
  `discussion::pick_pair`、`Config::debaters_share_a_vendor`、`discussion::last_round`、
  `context::usable_input`、`render::wording`、`wording::DASH_FALL` 等已清出正文。
- **`python3 scripts/check-doc-size.py`：退出 0**，`--list | grep CONTEXT.md` 无 >500 项。
- **`python3 scripts/check-language.py`：全绿**（模型可见 / 进流两条棘轮未回退、docs 与 ADR 是中文散文、
  注释中文行数未回退）。增补后三份余量：`docs/render.md` 44.35% / 下限 42（余量 2.35）、
  `docs/observability.md` 45.15% / 43（2.15）、`docs/discussion.md` 45.34% / 42（3.34）——都在票面
  说的紧张额度内，增补用中文叙述包住标识符。
- **`git status --short`**：本票的四个落点是 `CONTEXT.md`、`docs/render.md`、`docs/observability.md`、
  `docs/discussion.md`；其余条目是别的票正在并行改的
  （`README.md` / `AGENTS.md` / `.scratch/README.md` / `docs/lifecycle.md` / `docs/tui-manual-checklist.md`
  / 票 08 / 票 10 / `scripts/check-doc-size.py` 那两个新文件），不是本票动的。

### 6 条「别处没有一份」的搬运（不新增文件，中文叙述包住标识符）

- **问卷请求**、**问题选项**（coverage `none`）与**作答草稿**、**问卷文案**（partial）→
  `docs/render.md` 的「问卷：区域与键位」一节，在「答案只有一个形状」之后加了四段：端口的一次性
  回复通道与「丢掉回复通道 = 没有答案」；`questions[].options[]` / `label` / `(Recommended)` /
  `description` 与两个同名 `Choice` 的分家；`selected` / `custom` / `highlight` / `skipped` 四字段与
  「跳过编码成 `selected: []`」；`render::wording` 的 `questionnaire_*` 文案前缀族。
- **价目表** → `docs/observability.md` 的「钱需要一个模型」一节：加了 `cached` / `miss` / `output`
  三档、命中与未命中分开计价、费用只作显示、未登记 model 显示「无价格」而非 0。
- **落点** → `docs/discussion.md` 的「跑一场」清单：加了「全系统只有两个落点（合成器与执行者、
  可配置覆盖），讨论者绝不是落点」。

### 14 个「会空掉」的词条各补的一句纯领域定义

- 提示符色相：输入区提示符 `❱ ` 的颜色，界面里唯一在动、且只在一次运行进行中时动的东西。
- 脉冲：驱动提示符色相的那个帧计数器（一次运行进行中才启动、结束归零）。
- 下落短横：一条 `▀▀▀▀` 从上往下落、落到底再从上面出现的动画，且已退出屏幕。
- 状态行：输入区上方那一行，内容是「模型 / 模式 / 上下文占比」三段、永远在场。
- 输入区（Input Zone）：问卷里键盘让给文本的那一区（问卷两区之一）。
- 选项区：问卷里高亮落在选项上的那一区。
- 焦点回合：回合条上被高亮的那一格，含义是「正在看的那一段」、不是最新回合。
- 回合条：转录右缘最右 1 列的竖条，一格 = 会话的一个单位。
- 会话桶：按会话 cwd 分出的目录，是 `--continue` 找会话时的第一层范围。
- 翻页：结束当前会话、开一个新的那条内部动作。
- 阻塞边：票顶上那一行记的依赖边。
- 问卷：一次提问调用里的整批问题，以及前端为它持有的键盘状态。
- 询问：harness 发起的问句、答案是闸门。
- 权限模式：会话对「写」的一档立场（四档各是什么的领域定义）。
每个都点过，不是抽样；四个造名槽位（`PromptHue` / `FallingDash` / `StatusRow` / `Pulse`）保留，
第 10 行的规矩改成票面写的那半句。

### `docs/render.md` 的两处

- **L279-288 那块复述**：4 条顶层清单项（渲染器怎么选 / `--continue` / `--mode` / 命令与手势）
  整块删掉，在删掉处留一句指针——「渲染器怎么选、`--continue` 怎么续哪一场、`--mode` 覆盖哪一档，
  都见 `README.md` 的「跑」一节」。**三处独有信息并回 `README.md` 不归本票**，按票面留给 Lead 的
  另一张票。
- **它那个 ≥500 的清单项**：是「外壳」里的**「标记」项 569 字符**（不是 L279-288 本身——那块整块
  526，按单元拆开后 4 项都合格）。按 research/03 §6 切成两个顶层项：「标记本体 + 色坡道」与
  「**这里什么都不动**：动效退休记录 + `PULSE_PALETTE` 不上屏」。切完 `docs/render.md` 的违规数
  也归 0，`check-doc-size.py` 仍退出 0。

### 与票面数字对不上的地方

- 票面说 `CONTEXT.md` **348 行 / 44KB**；`git show HEAD:CONTEXT.md` 实测是 **349 行 / 44,169 字节**
  （脚本口径 `text.count("\n") + 1` 也是 349）。以实测为准，不另改。
- 票面说 6 处裸 `§N`「渲染四处在 `docs/render.md`，其余按落点表」；实测这 6 处**全在渲染节**，
  所以 6 处全落 `docs/render.md`，没有第六份文件要碰。
- research/02 §7.3 落点表里 `会话桶`（`.scratch/continue-by-id/spec.md`）与 `发言归属` / `投影`
  等词的「一份完整来源」只在 `.scratch/*/spec.md`；按票面「指针必须落到真实存在且按需读得到的
  来源」，这些改指到 `docs/observability.md` / `docs/discussion.md`，没有 `.scratch/` 链接进正文。
- **基线没有收紧**：`CONTEXT.md` 的 8 只是清到了 0 违规，`VIOLATION_BASELINE` 里的 8 原样留着，
  由**票 12** 统一收。提交信息里若要写，写「本文件违规清到 0」，不写「基线已收紧到 0」。
- 造名槽位的真实代码形态（`PROMPT_HUE_PER_SECOND` / `DASH_FALL` / `wording::status_row()`）已随指针
  写进 `docs/render.md`（色相计时器、标记动效、状态行各一处），没有进 `CONTEXT.md` 正文。

**2026-10-04 `/code-review` 后的修正**：上面那 6 条「独有内容搬进 `docs/`」的搬运**只搬了、没从词条里删**，
于是与 `docs/render.md` / `observability.md` / `discussion.md` 的新段逐句重复（同一条规则两处维护）。
已把词条一侧压到「一句定义 + 硬约束句 + 指针」：删掉细节与原因（都在 `docs/` 那一份里），
留下的硬约束句是「丢掉发送端就等于没有答案」「跳过是显式的『不作答』」「费用只作显示」「讨论者绝不是落点」。

