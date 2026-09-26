# 领域文档

各个工程技能在探索代码库时，该怎么消费本仓库的这些领域文档。

## 探索之前，先读这些

- 仓库根目录的 **`CONTEXT.md`**，或者
- 仓库根目录的 **`CONTEXT-MAP.md`**（如果存在的话）：它给每个 context 各指一个
  `CONTEXT.md`。把与当前题目相关的那些逐个读掉。
- **`docs/adr/`**：读那些碰到你即将动手的那块地方的 ADR。多 context 的仓库里，还要去
  `src/<context>/docs/adr/` 看那些 context 范围内的决定。

这些文件里有哪一个不存在时，**安静地继续**就好。不要去提它缺席这件事，也不要建议先把它们
建出来。`/domain-modeling` 技能（经 `/grill-with-docs` 与
`/improve-codebase-architecture` 到达）会在术语或决定真的被解决的那一刻，惰性地把它们建
出来。

## 文件结构

单 context 的仓库（大多数仓库都长这样）：

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-event-sourced-orders.md
│   └── 0002-postgres-for-write-model.md
└── src/
```

多 context 的仓库（根目录下放着 `CONTEXT-MAP.md`）：

```
/
├── CONTEXT-MAP.md
├── docs/adr/                          ← system-wide decisions
└── src/
    ├── ordering/
    │   ├── CONTEXT.md
    │   └── docs/adr/                  ← context-specific decisions
    └── billing/
        ├── CONTEXT.md
        └── docs/adr/
```

## 用词汇表里的词

你的产出里点名一个领域概念时（在 issue 标题里、在一次重构提案里、在一个假设里、在一个
测试名里），就用 `CONTEXT.md` 里定义的那个词。不要漂向词汇表明确避开的那些同义词。

你需要的那个概念还没收进词汇表里，那本身就是一个信号：要么你正在发明这个项目根本不用的
语言（那就重新想一想），要么那里确实有一个真缺口（把它记下来交给 `/domain-modeling`）。

## 把 ADR 冲突挑明

你的产出与某一条既有 ADR 相矛盾时，明着把它说出来，而不是默默把它推翻：

> _Contradicts ADR-0007 (event-sourced orders), but worth reopening because…_
