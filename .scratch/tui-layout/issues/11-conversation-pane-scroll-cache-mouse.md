# 11: 对话面板：滚动缓冲、换行缓存、吸底与鼠标滚动

**What to build:** 让对话面板真正成为**可回看的转录**：自己持有源行缓冲（上限 20 000 源行、超出丢最旧）、按宽度缓存换行结果与其索引、吸底但用户上滚后不抢、右缘恒留滚动条、脱离吸底时底部右缘显示可点击的「点此到底」。同时把**用户自己输入的消息**从「单行 + 截断 500 字符」改成多行原样渲染。

Blocked by: 10

Status: done

**参考:** spec §3（面板行缓冲与换行缓存）、§4（滚动、吸底与鼠标）、§6（键位表里转录那几行）

- [x] 源行缓冲：`Block` 展开成未换行行；**20 000 上限按源行算**（宽度无关），超出丢最旧
- [x] 换行缓存：全量换行后的行 + **源行 → 显示行起点**索引
- [x] 换行口径：**逐字符、CJK 2 列，复用 `wrap_take`**；**不用** `Paragraph::line_count`、**也不开** `unstable-rendered-line-info`
- [x] 失效：宽度变化 → 全量重算；流式增量 → 只重算尾部；新块 → 只增量换行尾部并追加索引
- [x] 缓存**在 `draw` 闭包里按 `frame.area().width` 失效**（不写单独的 resize 分支）
- [x] **非 assistant 消息改多行原样渲染**（保留换行、不做 Markdown），续行按 speaker 前缀显示宽度缩进；**assistant 的消息保持全文 Markdown 不动**；工具输出保持 4 000 字符 preview 与 `summarize_args`
- [x] 描画：`Paragraph` + 自算 `scroll`，**滚动位置自己夹紧**在 `0..=max`（`Paragraph::scroll` 不夹紧）
- [x] 吸底：默认吸底；**任何上滚立即脱离**；脱离后新内容不改变视口；**提交（Enter）无条件回到底部并恢复吸底**
- [x] 键与手势：`PgUp`/`PgDn` = 整页**减 2 行重叠**；`Ctrl-G` = 一次到底；**滚轮每格 3 行**；左键点击「点此到底」= 到底
- [x] **除「点此到底」外，所有鼠标点击一律忽略**（不抢焦点、不选择、不改视口）
- [x] **滚动条**：右缘**恒预留 1 列**（不因出现而改变转录宽度），只在内容超出时画字符；`ScrollbarState` 三个量都按**显示行**填；脱离吸底时要有可区分的视觉差异
- [x] 脱离吸底时在转录区**底部右缘**显示 `↓ {n} 行新内容 · 点此到底`（无新内容时 `点此到底`），整块是一个鼠标命中矩形，`n` 按显示行算
- [x] resize 锚点：吸底态保持吸底；脱离态保持顶部可见的**源行**
- [x] 措辞：`↓ {n} 行新内容 · 点此到底` 与 `点此到底` 进 `wording.rs`
- [x] 用例：滚动/吸底/脱离/提交回底；20 000 源行上限丢最旧；resize 锚点（两种态）；滚轮 3 行；「点此到底」命中矩形；其它区域点击无效；非 assistant 消息多行渲染；assistant 仍走 Markdown

## Comments

## Comments

**实现完成（2026-09-21）**。落点：新增 `src/render/pane.rs`（面板本体：源行缓冲、换行缓存、索引、吸底、命中矩形）；`src/render/tui.rs` 换成持有一个 `Pane`、新增鼠标入口与滚动键；`src/render/wording.rs` 新增两条指示文案；`tests/render_layout.rs` 新增 6 个用例；`tests/render_tui.rs` 改写 1 个、新增 1 个；`src/render/tui.rs` 内新增一个 `#[cfg(test)]` 单元测试。

**三处与票面写法的偏差（都是有意，且理由写在代码里）**：

1. **没有复用 `wrap_take`，而是新写了 `pane::wrap_line`**。`wrap_take` 只回答「多少字节放得下」，源行却带样式（Markdown 标题、diff 底色、speaker 前缀），换行必须把 span 切开并保留各自样式。列口径（CJK = 2 列、逐字符）逐字沿用，`wrap_take` 的理由也搬进了 `wrap_line` 的文档；`tui.rs` 里那个 `wrap_take` 因此删除（已无消费者）。
2. **描画用「自己取窗口行」而不是 `Paragraph::scroll(y)`**。票面两者都允许（「描画：`Paragraph` + 自算 `scroll`，滚动位置自己夹紧」）；自己切片把夹紧与窗口取用做在同一处，也就不必依赖 `Paragraph` 不夹紧这个事实。
3. **缓存失效用的是「转录内容宽 − 1」（文本宽），不是 `frame.area().width`**。票面写 `frame.area().width` 是为了强调「按真正画出来的宽度失效」而不是另存 resize 状态；实际决定换行的是文本宽（滚动条那一列恒被预留），所以按文本宽失效才是那个意思。

**两处测试当场抓出来的 bug**：

- `wrap_text("")` 返回**一行**（`split('\n')` 给一个空串，`wrap_line` 就产出一行空行）—— 没有流式尾巴时白占一行，把「跟底」的行数整体推偏一格。已改成空文本零行，并加了守卫注释。
- **指示块压到滚动条那一列**：`点此到底` 是 8 列，原来右对齐到内容右边界（第 88 列），最后一个 `底` 跨 87–88，与滚动条同列。ratatui 写宽字符**不会**覆盖它右侧的遮蔽格，所以滚动条的 `▼` 还在，肉眼看是两者叠在一起。已改成停在滚动条内侧一列。

**新增的措辞与配色（补齐票 06 的调色板记录）**：`wording::new_content(n)`（`↓ {n} 行新内容 · 点此到底`）与 `wording::back_to_bottom()`（`点此到底`）；指示块用 **Yellow + BOLD**（与权限询问同色，但两者不会同屏 —— 模态会盖住面板），滚动条「脱离吸底」的差异用 **BOLD 拇指**而不是换颜色，避免再引入一个颜色语义。

**一处语义按票面澄清**：指示块的 `n` 是「**自离开底部以来到达的显示行**」，不是「视口下方还有多少行」—— 这正是「仅上滚、无新内容时只显示 `点此到底`」那一支成立的前提。已回改 spec §4 写明。

**基线**：`cargo test` **502 passed / 0 failed**（495 → +6 布局用例 +1 单元测试）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移；pty 启动检查仍 **3/3 GREEN**。

**变异检验**（确认新测试不是摆设）：把 `evict()` 摘掉 → 上限用例红；把 resize 锚点改成 `top = 0` → 锚点用例红；把指示块右边界放开一列 → 指示块用例红。三处全部被抓到。

**评审收口**（`/code-review` 双轴，2026-09-21）：

Standards 轴：

- `char_columns` 在 `pane.rs` 与 `tui.rs` 逐字重复 —— 抽出 **`src/render/width.rs`**（`text_columns` / `char_columns` / `truncate_columns`），两处共用。这也给票 12 的编辑器一个稳定的家（票 12 与 spec §5 已相应改写：不再「从 tui.rs 搬一份」）。
- 保留列用裸 `1`（三处 `saturating_sub(1)`）—— 归位到几何：`layout::Regions::transcript_text()`（内容去掉滚动条那一列）与 `Regions::scrollbar()`，渲染器不再自己减。
- `draw_scrollbar` 的注释说「the pane always reserves」—— 保留列的 owner 是 `layout`，注释已改。
- `pane_rows` 只是转发 `pane.view` —— 删掉，直接调用。
- 两条 Message 臂的「前缀 + 悬挂缩进」重复 —— 抽出 `attribute(speaker, rows)`，归属规则只留一处。
- `layout::Panes` 与 `pane::Pane` 同屏两个名字 —— `Panes` 改名 **`Regions`**。
- `indicator: Option<Rect>` + `hits_indicator` 挂在滚动缓冲上（屏幕坐标跑进了缓冲）—— 移到 `TuiState`；`Pane` 现在完全不知道屏幕坐标。
- `Pane::height()` 无人调用 —— 删除。

Spec 轴：

- **上限用例用的数据每条只占一行**，所以「按源行而非显示行」这条其实没被证明 —— 改成每条折三行的数据（20 001 条 = 60 003 显示行）。现在把 `CAP` 调成 1/3（等于按显示行算）这条用例会红，已用变异检验确认。
- spec §3 与新写的 `wrap_line` 不一致（spec 还写着「复用 `wrap_take`」，而那个函数已删）—— 已回改 spec §3，并把「列宽算术在 `width.rs`」写进 §3/§5。
- 两处**不成立**的 finding，未改：①`track_style(DarkGray)` 不是多余 —— ratatui 的默认 track 是 `Style::new()`（无色），不加就与边框不同色；②`fresh()` 不会被 evict 干扰 —— evict 只从**最旧**那头丢行，`total` 与 `seen` 同减同一个高度，差值不变。
- 一处**打折但不改**：resize 锚点落在源行的**起点**（票面就是「保持顶部可见的源行」），当该行在新宽度下折成多行时，视口顶回到这一行的开头而不是行内的同一偏移。这是票面的字面要求，也是更能读的选择。
