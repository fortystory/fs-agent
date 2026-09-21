# 18: 渲染接缝：plain 与 TUI

**What to build:** 一个真正能读多 agent 讨论的终端界面——谁在说话、现在是第几轮、分歧在哪——并且与 headless / plain 共用**同一个事件序列**（不是三套渲染逻辑围着同一堆数据各写一遍）。

Blocked by: 10

Status: done

**参考:** spec §19（渲染与 CLI 组装）、§5（两套前缀）

- [x] **一个 trait 三个实现**，启动时选定、**互斥**（不是并发订阅者）；三模式各一个任务消费**同一条**广播通道，通道由组装期注入
- [x] **增量文本与日志事件同通道**（分两条则相对顺序无定义）
- [x] plain：说话人前缀**逐行**、轮次分节线、分歧**缩进块**、工具调用一行摘要、hook 反馈与工具结果**归成一处**（同 `tool_call_id`）
- [x] TUI = `ratatui` + `crossterm` 默认特性 + **inline viewport**（定稿内容 `insert_before` 推进 scrollback）；**不进 alt screen**（转录要能滚动 / 复制）
- [x] TUI 的 `select!` 处理广播 / tick / 键盘；**输入归渲染器**（终端独占），答案经注入的 channel 回循环
- [x] `tree-sitter-highlight` 做语法高亮，**diff 着色与语法高亮是两层**；不引入 C 构建依赖（不引 syntect 的 Oniguruma 路径）
- [x] 终止原因在显示上可区分（`Completed` 与 `Aborted` / `Error` **不能同色**）
- [x] headless 的 stdout 纯净性**不被本次改动破坏**（第 01 票那条回归断言仍然绿）

## Comments

**落地（2026-09-22）。** `render` 从单文件变成边界：

- **接缝**：`Render` trait + `Renderer` 值类型（`Headless` / `Plain` / `Tui` 三选一）；`assembly` 用 `render::channel()` 建唯一通道，把消费端注入选中的实现（`Renderer::spawn`）。`AssemblyParts.sinks` / `DiscussionParts.sinks` 换成 `renderer: Renderer`，全部调用点（约 20 处测试夹具）随之改为 `Renderer::headless(RenderSinks { … })`。
- **共享呈现**：`render::transcript` 把事件转成 `Block`（工具调用 + 结果 + 后置 hook 归成一个块；后置 hook 无 `tool_call_id`，靠「遇到下一个无关事件才关块」合并；增量文本原样透传）。plain 与 TUI 只负责绘制。
- **plain**：逐行 `[speaker]` 前缀、轮次分节线、分歧缩进块、工具一行摘要 + 缩进结果 + `[hook]`，终止原因按 `render::severity` 上色。
- **TUI**：`ratatui` + `crossterm`（额外开 `event-stream`）；`init_with_options(TerminalOptions { viewport: Inline })` 只进 raw mode、**不进 alt screen**，定稿块走 `insert_before` 推进 scrollback；`select!` 广播 / tick / 键盘；`TuiState` 是脱离终端的可测状态机，`Key` 是自己的键词汇表。
- **高亮**：`render::highlight` 两层——`diff_tag`（added/removed/hunk/context）与 `highlight_rust`（tree-sitter，复用已有 Rust 语法，无 C 构建）；`highlight_diff` 先剥掉 diff 标记再整篇高亮，所以被删的 `fn` 既是删除又是关键字。
- **键盘接缝**：`render::input`。循环持 `ConsoleHandle`（按需取一行 / 问一个问题）与 `ConsoleEvents`（Esc / Shift+Tab / Ctrl-C 手势）；前端持 `ConsolePort`，TUI 在自己的 `select!` 里服务它，plain 由 `spawn_plain_console` 读 stdin。`ConsoleAsker` 让权限门走同一条通道。
- **CLI**：无子命令即进入交互会话（`--plain` / `--tui` 二选一，stdout 非终端时回落 plain；`--continue` / `--config` / `--model` / `--cwd`），命令 `/undo`、`/plan`、`/endplan`、`/quit`；可在飞的回合里接取消手势（第二次取消强退）。
- **测试**：新增 `tests/render_plain.rs`（8）、`render_console.rs`（7）、`render_tui.rs`（12）、`render_highlight.rs`（8）；第 01 票的 headless stdout 回归断言未改、仍绿。文档 `docs/render.md` + `CONTEXT.md` 词条（渲染器 / 转录 / 终端端口）。

**未做**：交互式默认仍是单 agent 会话（渲染器本身对讨论事件完全支持，测试直接喂讨论事件流断言）；讨论入口（两个讨论者的 CLI 名册）不在本票清单内。
