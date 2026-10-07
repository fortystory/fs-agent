# 03 — 详情覆盖层与语法高亮的接线

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

[01 号票](01-page-shape.md) 决定：文件内容落在既有的**详情覆盖层**上，带**语法高亮**与**行号**，
内容由渲染器**直接读盘**并有界截断。这一票只查清接入这些时的代码事实，不写实现。

1. **详情覆盖层今天怎么画**：`DetailKind` 的四个变体（`Message` / `Thinking` / `Context` / `Tool`）、
   `Detail` / `DetailView` / `DetailLine` 的结构、正文从哪一层拿到可画的行（`pane::wrap_text`？）、
   滚动与尺寸上限（那个 135 列）各在哪一层。加第五个变体要动哪几处。
2. **高亮那一层的入口形状**：`src/render/highlight.rs`（或它的实际位置）按什么单位吃输入
   （整段？逐行？），语言怎么定（扩展名？围栏里的语言名？），产物是什么（带样式的 span？ANSI？）。
   markdown 代码块那条调用点在哪。
3. **行号与折行怎么共存**：一条逻辑行折成多条显示行时，行号只画第一条 —— 那一层有没有现成的
   形状可借（比如轨迹视图的时间戳前缀只画块的第一行，[`trace-in-main/spec.md`](../../trace-in-main/spec.md) §5）。
4. **有界读盘的既有件**：`read_file` 的默认 2000 行与续读提示、`bash` 输出的截断与 `outputs/`
   落盘那套 —— 哪些可以直接借、哪些绑死在工具层上用不了。二进制检测今天在哪一层有（如果有）。
5. **核实一条前提**：01 号票说这个弹窗「不进事件流、不进模型上下文」—— 渲染器读盘这条路有没有
   会写流或写盘的副作用（`outputs/` 那类），如实回报。

## 收尾

findings 写进 `.scratch/files-page/research/01-overlay-and-highlight.md`：逐条回答上面五问，
每条给 `文件:行号` 证据；查不到的要明说「查不到」，不要推断。

## 作答

findings（逐条带 `文件:行号` 证据）在 [`../research/01-overlay-and-highlight.md`](../research/01-overlay-and-highlight.md)。

1. **覆盖层今天怎么画、加第五个变体要动哪几处**：它是一份「打开那一刻排好版」的静态正文加一个
   `top` 偏移 —— 正文由 `detail_body`（`tui.rs:6206`）经 `folded_text`（`tui.rs:6114`）与
   `pane::wrap_line` 排成 `Vec<DetailLine>`（**不是** `pane::wrap_text`），`draw_detail`
   （`tui.rs:6328`）只做裁剪与滚动算术。`DetailKind` 有四处构造、一处消费（`detail_body` 那个
   match）；加第五个变体是「定义 + match 加一支 + 一条新入口调 `open_detail`」三处。**两处要
   注意**：① `detail_opener` 与 `close_detail` 里有一处「打开方恒为轨迹页」的硬编码记账
   （`tui.rs:6151`、`6168`、`6342`），文件页是第二个打开方就必须处理它；② 正文的排版宽度今天用
   的是**框宽**（`tui.rs:2732` → `layout.rs:205-210`），而实际文本区比它窄 4 列 —— 往每行前面
   加行号之前这个口径要先对齐，否则行号本身也会被算错。
2. **高亮那一层的入口形状**：`highlight_code`（`src/render/highlight.rs:344`）吃「整段源码 +
   语言名」、吐逐行的 `Vec<Vec<Span>>`（`Class::style` 在 `highlight.rs:148`）。语言今天**只**来自
   markdown 围栏的 info string（`markdown.rs:188-196`、`649`）—— **按扩展名挑语言的映射不存在**，
   要新写。
3. **行号与折行**：`folded_text` 先按 `\n` 拆逻辑行、再逐条 `wrap_line`（`tui.rs:6114-6130`），
   所以在折行**前**给逻辑行插前缀，续行自然不带行号 —— 形状可借，但借的是「逻辑行级」，不是
   `stamp_lines` 那个「块级」。**顺带查明一处 spec 与代码不一致**：`trace-in-main/spec.md` §5 说的
   「续行补 9 个空格」在代码里查不到，代码是不补。
4. **有界读盘的既有件**：`read_file` 那一套（默认 2000 行等）**绑死在工具层**（要 `ToolContext`），
   借不了；渲染层唯一能借的是 `read_tool_body`（`tui.rs:6293`）加 `DETAIL_MAX_CHARS`
   （`tui.rs:6142`），但它按**字符**封顶、且绑死 `session_dir/outputs/<tool_call_id>.txt` 这条命名。
   **行数上限、字节上限、按扩展名定语言、二进制检测这四件在渲染层都没有现成件** —— 二进制检测
   全仓库只有 `grep` 工具有（`grep.rs:157-161`），`read_file` 靠 `read_to_string` 隐式要求 UTF-8。
   所以 01 票 D 节那条「行数、字节、行宽都封顶，二进制只报一句」在这一层是**新写的活**。
   **一条好消息**：渲染器手里有注入的 `cwd`（`tui.rs:645`，读出口 `tui.rs:3412`），按相对路径读
   工作区文件不缺「工作区根从哪来」这个答案。
5. **「不进事件流、不进模型上下文」成立**：渲染层对文件系统的唯一调用就是那一次只读
   `read_to_string`（`tui.rs:6311`，只在打开那一刻发生，`tui.rs:6155`），整个 `src/render/` 没有
   `fs::write` 那类调用，渲染器不 emit 事件、手里根本没有 `Session`。两条要如实记下的**既有
   副作用**：打开任何详情都会把**轨迹页**冻住、关掉时无条件还给轨迹页（`tui.rs:6151`、`6168`、
   `6342`）—— 文件页当第二个打开方，要么复用要么扩展这套记账；`note_rows` 会把详情正文记进屏幕
   文本层（`tui.rs:6410`），也就是**可以被拖选复制**的那份文本，行号与截断提示要不要一起被复制
   在这里决定。
