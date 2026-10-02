# 08 — `scripts/lifecycle-check.py` + 它的测试

Type: implement
Status: done
Part of: ../map.md
Blocked by: 06

> 规格：[`../spec.md`](../spec.md) §6 与 Testing Decisions。设计（含误报取舍与失效清单）见
> [`../research/02-lifecycle-check-design.md`](../research/02-lifecycle-check-design.md)。

## 目标

`python3 scripts/lifecycle-check.py` 对写好的 `docs/lifecycle.md` **退出 0**；对它做两处手工破坏
（改一个节点 id、删一行证据）后**退出 1 并指出是哪一项**。`python3 -m unittest` 全绿。
这一票之后，图与代码脱节这件事第一次有了机器信号。

## 现状（2026-10-03 核实，改前先复核）

- **护栏脚本的先例**：`scripts/wayfinder-check.py`（分行标签的报告格式、退出码 0/1/2、对历史遗留的
  宽容处理）与 `scripts/check-language.py`（`--list` 用来核对清单本身）。
- **没有 CI、也没有 Makefile**：脚本靠 `README.md` 的「开发」一节手工调用，**输出文本就是接口**，
  退出码是唯一机器信号。
- **仓库现有四个 Python 脚本都没有测试** —— 本票新建 `scripts/tests/`，这是第一个 Python 测试；
  spec 的 Testing Decisions 解释了为什么值得破一次这个一致性（脚本最危险的失效模式是**假绿**）。
- **mermaid 在仓库里零先例**，所以脚本可以自圈方言，没有历史包袱。

## 落点

`scripts/lifecycle-check.py`（新）、`scripts/tests/test_lifecycle_check.py`（新）、`README.md` 的
「开发」一节。

## 具体行为

1. **七条计失败**（退出 1）：C1 `路径:行号` 的文件存在、行数够（区间 `路径:a-b` 一并认）；
   C2 点名的符号仍在该文件里（按词边界找，不搜裸名字）；C3 `docs/lifecycle.md` 仍被 `README.md`
   引用（「文档」表或「架构」节任一命中即可）；C4/C5 证据表的节点键集合 **==** 图的节点集合
   （**按图分别比对**，两个方向分开打印）；C6 节点 id 不重复定义；C7 每条边两端都已**显式**定义；
   C8 只用 §1 的方言 —— **不认识的构造报红，不静默跳过**。
2. **C9（行号精确性）只告警**：打印「`src/x.rs:120` 的 `fn run` 现在在 137 行」，退出仍 0；
   `--strict` 时才算失败。理由写进注释（参照 `scripts/check-language.py:148-150` 留余量的写法）。
3. **CLI**：缺省盯 `docs/lifecycle.md`；`--list` 打印解析出的节点 / 边 / 证据；`--strict`；
   退出码 `0` 通过 / `1` 有失败 / `2` 用法错；文档不存在返回 1。
4. **docstring 写清定位**：只守**指称完整性**，不守语义正确性 —— 「**通过 ≠ 图是对的**」，以及
   「不认识的构造一律报红」这条天花板声明。
5. **mermaid 解析**：切块（认 ``` 与 ~~~）→ 摘引号字符串 → 剥 `%%` 注释 → 按行分类；产出
   `nodes` / `edges` / `problems` 三样。任何一行既不认识、也不是已知语句形态，就进 `problems`。
6. **11 条测试用例**（spec 的 Testing Decisions 逐条列了）：干净文档 PASS · C1 · C2（含 C9 告警那条
   要退出 0）· C3 · C4/C5 两个方向 · C6 · C7（含「同 id 在另一张图里有定义仍要报红」）· C8
   （`subgraph` / `&` / 链式 / `classDef` 各来一个，必须报「不认识的构造」）· `--list` · `--strict` ·
   用法错。断言只打在**退出码 + stdout 里有没有那一项**上。
7. **README「开发」一节**加 `python3 -m unittest`。

## 验证

1. 对 `docs/lifecycle.md` 退出 0；`--list` 打印的节点集合与文档里的一致。
2. 手工破坏两次（改一个节点 id、删一行证据）→ 退出 1 且报出正确的那一项。
3. `python3 -m unittest` 全绿。
4. 往 fixture 里放一个 `subgraph` → 报「不认识的构造」，**不静默通过**。
5. 在 `src/` 顶部插一个 `use` 让下行号整体 +1 → 仍然退出 0，只多一条提示（C9 是告警）。

## 不做什么

- 不接 CI。
- 不验证语义（图是否如实描述运行时）。
- 不校验其它文档里的图（v1 只盯 `docs/lifecycle.md`）。
- 不改 `check-language.py`（那一行归票 06）。
- 不与 `check-language.py` 的清单联动（越界耦合，见票 03 的 C10）。
- 不为 `sequenceDiagram` 写解析（图 3 不进 C4/C5）。
