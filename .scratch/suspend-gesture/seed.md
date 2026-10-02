# 挂起手势（种子与访谈存档）

**这不是 spec，也不是票。** 它是 2026-10-02 一次 `/ask-matt` 里那句话的原始记载，外加随后同一轮 `/grill-with-docs` 的九问九答。产物是 [`spec.md`](spec.md)。

## 原始意向

> 让 fs-agent 支持 ctrl-z 挂起到后台

## 访谈（2026-10-02，九问，答案全是 A）

| # | 问题 | 答案 |
| --- | --- | --- |
| Q1 | 「挂起到后台」是**真暂停**（SIGTSTP，`fg` 回来继续），还是**后台继续跑**（画面让位、回合不停）？ | (A) 真暂停 —— 后者要 daemon 化加重连，是另一个 feature |
| Q2 | 射程盖哪几个前端？ | (B) TUI 显式实现，另用 pty 回归与文档把 plain 的天然行为钉住 |
| Q3 | 忙碌态（回合跑着）允许挂起吗？ | (A) 一律允许，接受 provider / `bash` 超时照走 |
| Q4 | 单下直接挂起，还是像 exit-gesture 那样举手？ | (A) 单下 —— plain 那边必然单下，TUI 对齐它 |
| Q5 | 挂起时终端交还到什么程度？ | (A) 完全交还（离开 alt screen、关 raw、还标题） |
| Q6 | `fg` 回来后界面上说不说话？ | (A) 只重绘，不说话 |
| Q7 | SIGTSTP 处置不是 `SIG_DFL` 时怎么办？ | (A) 发信号前置回默认、恢复后还原 |
| Q8 | 挂起期间子进程与超时怎么办？ | (A) 都不管，也不冻结 deadline |
| Q9 | plain 的「天然行为」用什么钉住？ | (C) 文档 + 一条 pty 回归 |

## 访谈里核过的一手事实

派了一个只读子代理去查，事实已并进 [`spec.md`](spec.md)：

- crossterm 0.29 的 raw 模式经 `cfmakeraw` 清掉 `ISIG|ICANON|ECHO|IEXTEN`，所以 TUI 里 Ctrl-Z 是按键不是信号；crossterm / ratatui 都没有 suspend/resume 原语。
- `ratatui::restore()` 就是 `disable_raw_mode` + `LeaveAlternateScreen`，不重绘不清屏；`ratatui::init()` 每次都会再包一层 panic hook。
- `raise` 多线程下只到当前线程，glibc 手册要求发给**进程组**；`SIGTSTP` 可被忽略，被忽略的处置跨 `execve` 继承。
- tokio timer 基于 `CLOCK_MONOTONIC`，进程停止期间照走；`bash` 超时与 provider 超时因此都不会因为在后台而暂停。
