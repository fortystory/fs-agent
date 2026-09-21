# 10: 全屏骨架：终端生命周期、四分区几何、降级阶梯与渲染管线

**What to build:** 把 TUI 从 inline 视口换成 **alt screen 全屏四分区**，并让**整个壳**跑起来：四区（header / 对话面板 / 右栏 / 底部块）各自带 `Block` 边框画出正确几何，header 显示名称+版本 / cwd / 模式 / 时钟，底部块显示输入区与提示行，降级阶梯按宽度高度逐档生效。同时删掉 inline 路径，并把渲染循环改成「整批 drain + 每轮一帧 + 脏标记」。**对话面板这一票只做「能画出来且不滚动」**（滚动在 11），**右栏只画空框**（内容在 13）。

Blocked by: None

Status: done

**参考:** spec §1（全屏与终端生命周期）、§2（几何与降级阶梯）、§10（提示行与措辞层）、§11（渲染管线）、§13（不动的东西）

- [x] 用 `ratatui::init()`（不是 `init_with_options`）进 alt screen；`TerminalOptions` 只需默认 `Viewport::Fullscreen`
- [x] 进入时 `EnableMouseCapture` + `EnableBracketedPaste`；**三条退出路径**（`/quit`、空闲 `Ctrl-C`、panic）都成对撤销；自装一层 panic hook（ratatui 那个只 restore 终端面，不管鼠标与 paste）
- [x] **删除 inline 路径**：底部锚定 `MoveTo(0, rows-1)`、`insert_before` 插入循环、`paint_scrollback`（含 `src/render/mod.rs` 的再导出）、`Viewport::Inline(LIVE_HEIGHT)`、`LIVE_HEIGHT`/`LIVE_ROWS`、每帧 `hide_cursor()`
- [x] `TuiOptions` 从 `{ port }` 变成 `{ port, facts }`，构造点 `src/cli.rs:299`；`SessionFacts` = 会话 id / cwd / 模型 / `context::usable_input(&caps)` / `budget.limit`
- [x] 四区几何：各 1 行 `Block` 边框；**header 底边框即分隔横线**（不重复画）；中左中右**共用接缝 1 列**并补 `┬`/`┴`；airy = header 下与输入区上各 1 行；右栏 `clamp(⌊26%·w⌋, 25, 31)`
- [x] 固定 chrome = **7 行**，`h = header内容 + 中段内容 + 输入区行 + airy(0|2) + 7`
- [x] header：名称+版本 / cwd / 模式 / `YYYY-MM-DD HH:MM`（本地、分钟粒度）；单行 header 用 ` · ` 分隔
- [x] **模式从流推**，不进 `SessionFacts`：进计划模式 = `ContextInjected{source: PlanMode}`，离开 = `HistorySuperseded{reason: ModeChange}`
- [x] 降级阶梯**顺序固定（先隐藏右栏、再压 header）**：`w<40 或 h<10` → 只显示 `终端太小：至少 40×10`；`w<80` 或中段内容 <4 行 → 隐藏整栏；右栏最小出现尺寸 80×16；`w<60` → header 压 1 行并丢 cwd；`h≤11` → 丢 airy
- [x] **40×10 渲染完整降级布局**（1 header + 1 转录 + 1 输入 + 1 提示），不是「太小」
- [x] 底部块：输入区行数 `clamp(草稿折行数, 1, min(10, h−7−header内容行−airy))`，超出显示尾部；最后一行是提示行（内宽 `w−2`）
- [x] 提示行：状态词在最左 + **六条**提示（`enter 发送` / `ctrl-j 换行` / `esc 取消` / `shift+tab 计划` / `PgUp/PgDn 滚动` / `ctrl-c 退出`）；**降级算法：提示从左边填、`ctrl-c 退出` 预留，状态词只在还放得下时加在最左**（票面原措辞「状态词先占位」按实测修正，见 `## Comments`）；实测条目数 40→3 / 60→5 / 80→6 / 120→7（数与票面差 1 是因为票面那组数字没把状态词算进去）
- [x] 提示集与任何文案里**绝不出现 `shift+enter`**
- [x] 措辞层新增：`终端太小：至少 40×10`、`identity` / `mode_field` / `clock` / `clock_short`、提示集条目；配色沿用现有六种 + BOLD，不加主题配置项。**面板标签 / `—` / 配对函数有意未落**，见 `## Comments`
- [x] 渲染管线：保留 `select!` 四路；**每轮末尾 `try_recv` 整批 drain（单轮上限 4096）再画一帧**；synchronized update **只包 `terminal.draw`**；脏标记驱动、**无脏不画**；tick 120ms 只比较时钟分钟
- [x] 流式增量仍进 `live` 尾缓冲（上限 4 000 字符），`Message` 块到达时清空并作为永久副本
- [x] **plain 与 headless 一行不动**；`sse_stream` 的 `YIELD_EVERY` 不动
- [x] `TestBackend` 用例：尺寸矩阵（`40×10` / `40×12` / `60×24` / `80×16` / `80×24` / `120×24` / `174×50` / `39×24`）上四区锚点与几何；降级阶梯逐档；`ctrl-c 退出` 恒在；提示行条目数；无 `shift+enter`

## Comments

## Comments

**实现完成（2026-09-21）**。落点：新增 `src/render/layout.rs`（几何与降级阶梯，纯函数）；`src/render/tui.rs` 删 inline 路径并重写循环；`src/render/wording.rs` 改提示集与降级算法、新增 `too_small` / `identity` / `mode_field` / `clock` / `clock_short`；`src/render/mod.rs` 换导出；`src/cli.rs` 注入 `SessionFacts`；新增 `tests/render_layout.rs`（6 个用例）。

- **生命周期**：`ratatui::init()` 进 alt screen；`TerminalModes` 这个 guard 负责 `EnableMouseCapture` + `EnableBracketedPaste` 的成对撤销，且在 `ratatui::init` 装好的 panic hook 外面再包一层，panic 路径也会先把鼠标与粘贴还回去。
- **管线**：`select!` 四路不变；每轮末尾 `try_recv` 整批 drain（上限 4096）后画一帧；synchronized update 只包 `terminal.draw`；脏标记驱动（`apply` / `key` / `request` / `Resize` / 时钟跨分钟），无脏不画；不再显式 `autoresize`（`draw` 内部会做），也不再有 `hide_cursor`。
- **删除**：底部 `MoveTo` 锚定、`insert_before` 循环、`paint_scrollback`（含 `mod.rs` 的再导出与同名的宽字符用例 —— 那个 bug 只存在于 insert 的 scratch buffer，全屏走 ratatui 自己的 buffer diff）、`Viewport::Inline`、`LIVE_HEIGHT` / `LIVE_ROWS`。
- **状态**：`TuiState` 新增 `facts` / `mode` / `source`（已完成块的渲染行）/ `clock`（`DateTime<Local>`）/ `dirty`；`ready` + `take_ready()` 换成 `source`；模式从流推（`ContextInjected{PlanMode}` → 计划，`HistorySuperseded{ModeChange}` → 询问）。
- **两个设计接缝**：`draw_frame(frame, &TuiState)` 是布局测试的唯一接缝（`TestBackend`，无 pty）；`layout::plan` 是私有的纯函数，测试只断言画出来的东西。
- **一处按实测修正**：`status_line` 的降级优先级做成「**提示优先、状态词最后加**」。原票面写「状态词与 `ctrl-c 退出` 先占位」，但那样 40 列内宽 38 会把 `ctrl-j 换行` 挤掉，而票 02 那张已批准的 40×10 快照是三条提示、无状态词。实测阶梯 3 / 5 / 6 / 7 与快照一致。已回改 `spec.md` §10 与票 06。
- **两处未在本票落地**（有意）：
  1. 票面 checklist 里的「面板标签（模型 / 上下文 / token / 回合 / 输入 / 输出 / 缓存）、`—`、`token_pair` / `context_pair` / `cache_pair`」**留给票 13** —— 本票只画右栏的空框，那批措辞在被消费的那张票里加才有测试可言。
  2. `paste()` 目前只把粘贴文本按字符插入（已比「终端把粘贴拆成按键、第一个换行就提交」好），`\r\n` 归一、控制字符过滤与 10 万字符确认**留给票 12**。
- **基线**：`cargo test` **494 passed / 0 failed**（+5：新增 6 个布局用例，删 1 个宽字符用例）；`cargo clippy --all-targets` 干净；`cargo fmt --check` 只剩 `src/context/repo_map.rs` 的既有漂移。
