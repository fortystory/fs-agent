# 08 — 护栏落地：`scripts/check-doc-size.py` 与它的测试（tracer bullet）

Type: implement
Status: ready-for-agent
Part of: ../map.md
Blocked by: —

> 规格：[`../spec.md`](../spec.md) §1（单元口径）与 §2（护栏脚本契约）。契约的完整理由与取舍在
> [护栏脚本的契约](05-grilling-guardrail-contract.md) 的 `## Answer` 里；可直接实现的判据片段与校准样本在
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
