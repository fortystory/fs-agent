# 08 — 护栏落地：`scripts/check-doc-size.py` 与它的测试（tracer bullet）

Type: implement
Status: done
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1（单元口径）与 §2（护栏脚本契约）。契约的完整理由与取舍在
> [护栏脚本的契约](05-grilling-guardrail-contract.md) 的 `## 作答` 里；可直接实现的判据片段与校准样本在
> [`../research/03-long-paragraph-triage.md`](../research/03-long-paragraph-triage.md) §4.1–4.3。
> **本票是这一轮唯一的 prefactor**：它是后面每一张票的尺子，所以它先落地。

## 目标

在仓库根跑 `python3 scripts/check-doc-size.py`，能一眼看到 36 份活文档里**哪个单元超了 500**、
**哪份入口文档超了字符 / 行数预算**、**哪份中文占比余量不足**；`--list` 给出全部单元与每条豁免规则的
当日命中数。它**装上即绿**（基线取首次运行的实测值）——守的是「不许恶化」，不是「瘦身做完了没」。

## 现状（2026-10-04 核实，改前先复核）

- **邻居形态**：`scripts/check-language.py` —— 硬编码字典（`DOCS_MIN_RATIO`）、各检查函数返回
  `problems: list[str]`、`main()` 统一打印后返回 0/1、`--list` 打印细节而不做判定。
- **测试形态**：`scripts/tests/test_lifecycle_check.py` —— **只断言 CLI**（退出码 + stdout 里有没有那一项），
  fixture 全在 `tempfile`，跑法是 `python3 -m unittest`。
- **接线点**：`README.md` 的「开发」一节那个代码块（`python3 -m unittest` 与 `check-language.py` 就在里面）。
- **没有 CI 入口**（无 `.github/workflows`、无 justfile / Makefile / pre-commit）—— 所以接线只有「README 一行 + unittest 发现」两处。
- **今天的实测底账**（票 05 的 Answer，用于核对首跑输出）：有效违规 **23 项**；单元口径 ≥500 共 **22 个**
  （整块口径 79，别用）；单元格 ≥500 共 **5 个**，其中 `README.md` 的 2 个被 R2 自动豁免；
  最紧的中文占比余量是 `docs/tui-manual-checklist.md` 的 **0.19 个百分点**。

## 落点

`scripts/check-doc-size.py`（新）、`scripts/tests/test_check_doc_size.py`（新）、`README.md`。

## 具体行为

1. **单元切分**（四步）：按空行切块，遇表格行 / 围栏代码块 / `#` 标题即断；清单块按**顶层清单项**连同续行与
   嵌套子项各算一个单元、块首到第一个清单项之间的散文另算一个单元；其余整块一个单元；**表格单元格单独算单元**。
2. **判据**：长度 ≤500 合格免检；**>500 才进豁免判定**，不命中即违规。单元格并入同一 500 指标。
3. **豁免**：R1 引文（`>` 行字符 ≥50%）与 R2 纯指针枚举（链接跨度 ≥50% 且 `·`/`、` ≥4，只对清单项与单元格）
   自动放行；R3 / R4 / R5 命中时打一行 `review:`、**退出码不变**。判据照 `../research/03` §4.2–4.3
   （含 `README.md` L141 那个必须继续被否掉的假阳性）。
4. **36 条硬编码字典**：`README.md`、`CONTEXT.md`、`AGENTS.md`、`.scratch/README.md`、`docs/` 逐面 18 份、
   `docs/adr/` 11 份、`docs/agents/` 3 份。两条自检：清单里的文件不存在 → 非零退出；`docs/` 下存在但不在
   清单里的 `.md` → 打印提示、不非零退出。
5. **入口三份的预算**：两张硬编码表（非空白字符数 ≤ 上限、行数 ≤ 上限），**初始值取今天实测**
   （`README.md` 19,954 / 286、`.scratch/README.md` 18,100 / 82、`AGENTS.md` 1,225 / 26），
   票 04 的目标（≤13,500 / ≤18,500 / ≤950；≤100 / ≤300 / ≤30）写进注释当终点。
6. **违规计数基线**：**按文件**记（初值 = 首次运行实测），永远打印每一项违规的位置，失败判据是
   「某文件的违规数 > 该文件的基线」；收紧是一次显式动作，注释风格照 `MODEL_TEXT_FLOOR` / `COMMENT_FLOOR`。
7. **占比余量警告**：顺带逐份算 `100 × CJK 数 / 字符数`，余量 < 0.5 个百分点打一行 `warn:`、不非零退出。
8. **`--list`**：按长度排序打印全部单元（长度 + `文件:行` + 类型 + 是否命中规则），并打印**每条规则的当日
   命中数与位置**。
9. **退出码与接线**：0 / 非 0；`problems` 的打印格式照 `check-language.py`；`README.md` 的「开发」一节加一行
   `python3 scripts/check-doc-size.py`。

## 验证

1. `python3 scripts/check-doc-size.py` **退出 0**；`--list` 能打印出 ≥500 的单元，数量与本票「现状」一节的
   底账对得上（22 个单元 + 5 个单元格，其中 2 个标 R2 豁免）；数量对不上时以脚本输出为准并在票底记一句。
2. `python3 -m unittest` 全绿，且新测试**只断言 CLI**（退出码 + stdout 里有没有那一项）。
3. **手工破坏两次**：把某个 fixture 的单元拉过 500 → 退出 1 并指出位置；把「清单外 md 提示」与「占比余量 warn」
   各造一次 → 提示出现但**退出码仍为 0**。
4. `python3 scripts/check-language.py` 仍绿（本票**不改任何文档内容**）。
5. `git status` 只看得到本票的三个落点。

## 不做什么

- 不改 `scripts/check-language.py`（那是另一条线，见 `../spec.md` 的「明确不做」）。
- 不在这张票里改任何文档内容 —— 入口三份、`CONTEXT.md`、手工清单、lifecycle 各有自己的票。
- 不加 CI 配置：仓库没有 CI 入口，spec 已确认没有「加进 CI」这一步。
- 不为「好看」调阈值：500 是冻结项。

## 评论

**2026-10-04 落地。** 三个落点：`scripts/check-doc-size.py`（新，约 380 行）、
`scripts/tests/test_check_doc_size.py`（新，15 条只打 CLI 的用例）、`README.md` 的「开发」一节加一行。

- **首装即绿**：`python3 scripts/check-doc-size.py` 退出 0；`python3 -m unittest` 33 条全绿
  （原有 18 + 新 15）；`check-language.py`、`lifecycle-check.py` 未受影响。
- **单元口径核对**：`--list` 实测 **2,997 个单元**，>500 的 27 个 = 散文 / 清单侧 **22** + 单元格侧 **5**，
  与票 05 的 22 + 5 精确对上；R2 自动豁免 2 项（`README.md:242` 571、`README.md:244` 1,241），
  R1 / R3 / R4 / R5 今日命中 0。
- **有效违规实测 25 项**（票 05 的底账是 23，差 2 项，在 spec 允许的 1–3 项口径差内）：
  `CONTEXT.md` 8 / `.scratch/README.md` 6 / `README.md` 5 / `docs/tui-manual-checklist.md` **2** /
  `docs/render.md` 1 / `docs/skills.md` **1** / `docs/adr/0003` 1 / `docs/adr/0008` 1。
  多出来的两处：`docs/tui-manual-checklist.md:126`（908 字符 —— L126 是散文行、L128 起才是 `3.`
  编号项，按「块首匹配清单才算清单块」的定义整块算一个单元，不是票 05 记的抬头那 737 一处）与
  `docs/skills.md:39`（778 字符，票 05 没列）。**两处都没有别的票管，归票 12 一并清**。
- **基线初值**（按文件，实测）：上表八个数；`> 基线` 才报红，每一项违规的位置永远打印。
- **入口三份的上限**：`README.md` 那条改成 **20,017 字符 / 287 行** —— 是**接线这一行之后**的实测
  （19,954 → 20,017），棘轮从今天的真实体量起步，票 10 再往 ≤18,500 / ≤300 收。
  `.scratch/README.md` 实测 18,309 / 82（票面 18,100 略低，以脚本为准）、`AGENTS.md` 1,225 / 26。
- **手工破坏两次**（票面「验证」第 3 条）：① 往 `docs/bash.md` 尾巴塞一个 524 字符段落 →
  退出 1 并指出 `docs/bash.md:162`、报「违规 1 项 > 基线 0」，改回后复原；
  ② 建一份 `docs/zz-probe.md` → 打出 `warn:` 提示且**退出码仍为 0**，删掉后复原。
  占比 warn 在真实仓库上今天只有 `docs/tui-manual-checklist.md` 一条（余量 0.19 点）。
- **`--root <目录>`** 这个口子不在票面契约里，是测试需要的：fixture 全在临时目录里，不依赖真实
  仓库文件（与 `test_lifecycle_check.py` 同一形态）。默认 `.`，仓库里的用法一个字不变。

**2026-10-04 `/code-review` 后的修正**（Standards + Spec 两轴的发现）：

- **单元定义第 ② 步补全**：块首是散文、块里又出现清单项时，**块首到第一个清单项之间的散文
  另算一个单元**（此前只在块首本身是清单项时才拆项）—— 票里那两处「加空行绕过」从此不再需要
  （空行留着，markdown 上也更清楚）。
- **`--list` 不再吞掉自检**：缺清单里的文件时它也非零退出（此前无条件 `return 0`）。
- **去掉 `--root`**：它不在票 05 / spec §2 的 CLI 契约里（那里只有 `--list`）。测试改成把**工作
  目录**切到 fixture —— 脚本的路径本来就相对当前目录，少一个旗标就少一条契约外的口子。
- **测试补到 19 条**：R4（不可断因果链）与 R5（次序操作序列）各一条、单元第 ② 步一条、
  「缺文件 + `--list`」一条（原 15 条）。
- `python3 scripts/check-doc-size.py --list | head` 不再抛 `BrokenPipeError` 栈（下游关管道不是错误）。

- **2026-10-06 同上**：`AGENTS.md` 的预算从 905 / 26 放宽到 **5,000 / 140**（终点 5,500 / 150）——
  本票第 5 条「初始值取今天实测、只许降」的这次例外由维护者显式给出，来由与细节见
  [`05` 的 `## 评论`](05-grilling-guardrail-contract.md)。
