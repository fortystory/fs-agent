# `src/render/highlight.rs`：代码块的语法高亮，外加补丁那一层 diff

**结论先行：语法高亮现在是转录里代码块的高亮提供者。** 渲染器按围栏上那个语言名挑一份文法，
把这里给出的 `Class` 铺到代码行上（规格见 [`.scratch/markdown-render/spec.md`](../.scratch/markdown-render/spec.md)
的 §3 与 §4）。

> **状态（2026-10-08 复核）**：两层都有生产调用点了 —— 语法层是转录里代码块的提供者
> （`highlight_code`），diff 层是**改动页**点开的那份 diff（`highlight_diff_with`，
> `.scratch/diff-page/spec.md` §6；那一档按文件的扩展名认语言，认不出就退纯文本）。
> 十种语言的文法都已经是硬依赖。

## 它是什么

这个模块提供两层互不查询的着色。**语法层**（`Class` 与 `highlight_code`）回答「这段代码是
什么语法元素」；**diff 层**（`DiffTag` 与 `diff_tag`）回答「这一行在补丁里是新增、删除、
hunk 头还是上下文」。两层可以同时成立：一个被删掉的关键字既是关键字、也是一次删除，所以
调用方把语法样式盖在 diff 样式上，而不是二选一。

语法层跑在 `tree-sitter-highlight` 上。它避开的是 syntect 那条要编 Oniguruma 的 C 路径
（规格 §19 的「明确不做」）；代价是十种文法自己各要编一个 C parser，首次构建因此并不比纯
Rust 依赖快。

## 谁在用它，怎么用

渲染器是唯一的调用点。围栏块里的整段代码先交给 `highlight_code(语言名, 源码)`，拿回来的是
**按行**切好的样式片段；渲染器再把这些行折到可用宽度，续行保持代码块那两格缩进。

语法层给 TUI 的取色住在 `palette` 的**内容域**一节（`Class::style`）—— 那里与界面域分家、
允许撞值（[`render.md`](render.md) 的「语义色板」两条）；`Class::ansi` 是**另一张表**，服务
diff 那套 ANSI 组合，不是 TUI 色板。

语言认不出、或者那份文法的 query 编不出来时，这个函数交回空值，渲染器就按纯文本画 ——
**代码不会消失，只是不上色**。

`highlight_rust` 保留成「固定用 Rust 那一份文法」的包装，`highlight_diff` 与 `ansi_line`
仍然从它拼出来，所以 diff 那一层的行为一个字没变。

## 十种语言

十种语言全是普通依赖，没有 feature 门控。大部分语言的名字、crate 的名字与 Rust 里的常量名
一一对应，读起来一眼就懂：`rust` 用 `tree-sitter-rust` 的 `HIGHLIGHTS_QUERY`，`json`、`html`
与 `python` 各用同名的 crate，`typescript` 取 `LANGUAGE_TYPESCRIPT` 而不是 `LANGUAGE_TSX`，
`php` 取 `LANGUAGE_PHP` 而不是 `LANGUAGE_PHP_ONLY`。

三件下次升文法时最省时间的事：

1. **有两个 crate 的常量名是单数**：`bash` 与 `javascript` 导出的是 `HIGHLIGHT_QUERY`，其余
   八种都是复数 `HIGHLIGHTS_QUERY`。两个 crate 都带同一份 `highlights.scm`，只有 Rust 常量
   的名字不同；猜错只会在编译期报一个找不到名字的错。
2. **有两个 crate 的名字和语言对不上**：`toml` 要 `tree-sitter-toml-ng`，因为直觉会去够的
   `tree-sitter-toml` 停在 2022 年、锁着 `^0.20`，和这里的 0.27 合不到一起；`sql` 要
   `tree-sitter-sequel`，同样是名字对得上的那个 `tree-sitter-sql` 停在 2021 年。
3. **每种语言第一次用到才编 query**：每种一个延迟初始化的格子，实测首次编译从 0.05 毫秒
   （json）到 70 毫秒（php）不等。十份全在启动时算会白付几百毫秒，而大多数会话只用得到
   Rust 与 json。高亮器本身也按上游的建议复用，按线程存一份。

另外两条刻意的取舍。**别名归我们管**：`rs` 就是 `rust`，`py` 就是 `python`，`sh` 与 `shell`
都是 `bash`，`js` 与 `ts` 同理；别名收在一张 `canonical_language` 表里，不散在匹配分支里
（`config_for` 那张「按语言名取文法」的表是另一回事，两者各管一段）。`yaml` 不在十种语言里，
所以**不映射** —— 名单之外的一律按纯文本画。**typescript 的 JSX 那份 query 不拼接**：
`.tsx` 里的标签不上色，这是知情的取舍（规格 §7）。

## capture 的覆盖面

`CAPTURES` 是这份渲染器认得的 capture 名。查询点到、而表里没有的名字退回纯文本，所以一次
文法升级弄不坏渲染 —— 它只能让东西不上色。十种文法里只有三处需要专门补：html 的 `tag` 与
`tag.error`；php 的 `module`、`module.builtin` 与 `tag`；`tree-sitter-sequel` 的
`conditional`、`field`、`float`、`parameter` 与 `storageclass`。

`Class::of` 的前缀映射和这张表同步：`tag.*` 与 `module.*` 归 `Type`，`conditional` 与
`storageclass` 归 `Keyword`，`float` 归 `Number`，`field` 与 `parameter` 归 `Variable`。
`tree-sitter-sequel` 还给出一个 `spell`，那是它给「没归类的词」的兜底，**有意**留在 `Plain`：
那本来就是「不知道是什么」。

## diff 层这一路是怎么回来的

TUI 从前直接显示工具输出，并用这一层上色。后来 `.scratch/tui-ux/` 的票 02 决定工具输出
**不再直接显示**：转录里只留一行调用行，正文进详情覆盖层，而详情层画的是**纯文本** ——
于是那个组合函数被删掉（提交 `940cd43`），`highlight_diff` 与 `ansi_line` 安静了一阵。

2026-10-08 那一轮（`.scratch/diff-page/spec.md` §6）把它接了回来：**改动页**点开一份 diff
时，标记剥掉、代码按扩展名认的语言高亮、标记再贴回去 —— 正是当初设想的用法，只是入口从
「工具输出」换成了「改动页的弹窗」。`ansi_line`（产出 ANSI 的那条路，给 plain 用的）仍然
只有测试在调；真要删，它和 `tests/render_highlight.rs` 里的那一半还是一起删。但
`tree-sitter` 与 `tree-sitter-rust` **不能**一起删 —— `src/context/repo_map.rs` 在读侧用它
们做符号抽取，那是另一条独立用途。

## 想自己确认

要确认语法层真的接上了，看渲染器里挑文法的那一处：`grep -rn highlight_code src/`；要确认
diff 层也接上了，看 `grep -rn highlight_diff src/`（改动页那一档走的是
`highlight_diff_with`）。要确认
十种文法都活着，跑 `cargo test --test render_markdown all_ten_grammars`：十种语言各一个最小
样例，每一种都要求至少拿到一片非 `Plain` 的样式，接错了、常量名写错了、选错了语言都会在
那里报红。
