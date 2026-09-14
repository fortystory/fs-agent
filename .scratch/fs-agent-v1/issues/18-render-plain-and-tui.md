# 18: 渲染接缝：plain 与 TUI

**What to build:** 一个真正能读多 agent 讨论的终端界面——谁在说话、现在是第几轮、分歧在哪——并且与 headless / plain 共用**同一个事件序列**（不是三套渲染逻辑围着同一堆数据各写一遍）。

Blocked by: 10

Status: ready-for-agent

**参考:** spec §19（渲染与 CLI 组装）、§5（两套前缀）

- [ ] **一个 trait 三个实现**，启动时选定、**互斥**（不是并发订阅者）；三模式各一个任务消费**同一条**广播通道，通道由组装期注入
- [ ] **增量文本与日志事件同通道**（分两条则相对顺序无定义）
- [ ] plain：说话人前缀**逐行**、轮次分节线、分歧**缩进块**、工具调用一行摘要、hook 反馈与工具结果**归成一处**（同 `tool_call_id`）
- [ ] TUI = `ratatui` + `crossterm` 默认特性 + **inline viewport**（定稿内容 `insert_before` 推进 scrollback）；**不进 alt screen**（转录要能滚动 / 复制）
- [ ] TUI 的 `select!` 处理广播 / tick / 键盘；**输入归渲染器**（终端独占），答案经注入的 channel 回循环
- [ ] `tree-sitter-highlight` 做语法高亮，**diff 着色与语法高亮是两层**；不引入 C 构建依赖（不引 syntect 的 Oniguruma 路径）
- [ ] 终止原因在显示上可区分（`Completed` 与 `Aborted` / `Error` **不能同色**）
- [ ] headless 的 stdout 纯净性**不被本次改动破坏**（第 01 票那条回归断言仍然绿）
