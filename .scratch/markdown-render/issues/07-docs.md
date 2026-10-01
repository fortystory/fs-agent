# 文档收口：render、highlight、ADR 索引、scratch 索引与真机清单

Type: docs
Status: done

> 规格：`.scratch/markdown-render/spec.md` §8。
> Blocked by: 02、03、04、05、06（文档写的是它们落地**之后**的事实）。

## 目标

把这一轮改动落到该落的地方，让下一个读代码的人不必重新推一遍。

## 落点与具体行为

1. **[`docs/highlight.md`](../../../docs/highlight.md)：整篇重写。** 它现在的主题是「这个模块**没有生产消费者**」（两处带日期的状态复核都在说这件事）。重写后主题变成「它是代码块的语法高亮提供者，管 10 种语言」：
   - 开头换成「它是什么、谁在用」；
   - **删掉**「为什么现在没人用」那一节的历史叙述（它记的是 `tui-ux` 票 02 的旧账），或压缩成一句交叉引用；
   - 「什么会让它回来 / 什么会让它消失」两节改写成现状（回来了；diff 层仍然没有调用方，为工具输出留着）；
   - 记上 10 种语言、per-language `OnceLock` 延迟编译、以及单数 `HIGHLIGHT_QUERY` 那两个坑——它们是下次升 grammar 时最省时间的两条。
2. **[`docs/render.md`](../../../docs/render.md)**：
   - 「一个呈现层，两个画家」那一节里 `[speaker]` 前缀的说明要改——现在 assistant 与非 assistant **分叉**（spec §5）；
   - 渲染边界那一节加一句：`markdown::to_lines` 收可用宽度，因此**源行也不再宽度无关**，宽度变化时会重跑 markdown 渲染（chain 到 spec §1）。
3. **[`README.md`](../../../README.md) 的文档表**：在 ADR 那一格加 [ADR 0008](../../../docs/adr/0008-markdown-parsing-by-pulldown-cmark.md) 的条目（按那一格现有的写法：链接 + 一句话）。
4. **[`.scratch/README.md`](../../README.md)**：给 `markdown-render/` 加索引行（形态 `spec`、一句话、票数）。同时更新那句由 `ls` / `grep` 核过的日期与说明。
5. **[`tui-layout/spec.md`](../../tui-layout/spec.md) §3**：把「assistant 的消息本来就是全文 Markdown，**不动**」与「续行按 speaker 前缀显示宽度缩进」两句改成**指向本 spec §5 的交叉引用**；「源行……宽度无关，可复现」那半句改成指向本 spec §1。**不要改写历史**——补一条带日期的注记说明哪一句被推翻、被谁推翻。
6. **[`docs/tui-manual-checklist.md`](../../../docs/tui-manual-checklist.md)**：加一条真机项——表格（装得下 / 装不下各一张）、代码块（语言名那一行、折行续行的缩进）、10 种语言各一段、图片语法、assistant 续行顶格。

## 测试

文档票没有单测。验收靠三件事：链接不自相矛盾、被引用的行号/函数名在代码里真实存在、`.scratch/README.md` 的票数与 `grep '^Status:'` 一致。

## 验收

- [ ] `python3 scripts/check-language.py`（若存在于 `scripts/`）通过——这些文件都是中文散文。
- [ ] 全仓链接扫描：本票改过的相对链接没有死链（注意 `.scratch/` 里的路径是**仓库内**的，不是上游的）。
- [ ] `.scratch/README.md` 里 `markdown-render` 那一行的票数与 `ls .scratch/markdown-render/issues/*.md | wc -l` 一致。
- [ ] `docs/highlight.md` 里不再有「没有生产消费者」这类陈述。

## Comments

- 2026-10-01 落地：`docs/highlight.md` 整篇重写（主题从「没有生产消费者」换成「代码块的高亮提供者，管十种语言」，两个单数常量名、两个对不上语言的 crate 名、延迟编译这三条坑记在里面）；`docs/render.md` 的 `[speaker]` 与高亮两节改写，并补上「`to_lines` 收宽度，所以源行也不再宽度无关」；`README.md` 的 ADR 格补 0008；`.scratch/README.md` 的索引行与日期说明更新成 `7/7 done`；`.scratch/tui-layout/spec.md` §3 的三处（源行宽度无关、assistant 不动、续行缩进）都留了带日期的交棒注记，原文没有被改写；`docs/tui-manual-checklist.md` 加 ㉑。
- 复核：`python3 scripts/check-language.py` 通过（`docs/highlight.md` 的中文占比一度掉到 23.6%，重写成散文后回到下限之上）；本票改过的相对链接逐个核过，唯一一处错路径（从 `docs/` 出发多了一级目录）已修，其余命中都是文档里当例子的 `![图片](url)` 语法；`.scratch/README.md` 里 `markdown-render` 那一行的票数与 `ls .scratch/markdown-render/issues/*.md | wc -l` 都是 7；`docs/highlight.md` 里不再有「没有生产消费者」这类陈述。
- 真机那一格没勾：这台机器上没有 provider key，`docs/tui-manual-checklist.md` ㉑ 的八条只写好、没有逐条手工跑过。
