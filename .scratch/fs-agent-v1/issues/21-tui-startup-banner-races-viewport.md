# 21: TUI 启动横幅野写入，落在 live region 的状态行上

**What to build:** 让启动横幅不再绕过渲染接缝——横幅（以及 `interactive_loop` 里那几处 `eprintln!`）必须经 `render` 通道作为 scrollback 块输出，或至少在渲染器启动之前写出。总之 TUI 活着时终端只有一个写入者（spec §19、票 18）。

Blocked by: None

Status: done

**参考:** spec §A.12（启动时看到模型 / 模式 / 预算）、§19（渲染器互斥：一个 trait 三个实现、**不是并发订阅者**）、票 18（渲染接缝：plain 与 TUI）

## 现象

`cargo run`，**尚未开始任何对话**，状态行是：

```
ready · enter send · esc cancel · shift+tab plan · ctrl-c quitlash · mode ask · /home/forty/.local/share/fs-agent/sessions/-home-forty-code-fortystory-fs-agent-986aee694bc228c4/20260921T101442Z-142215e5
```

`quit` 和 `lash` 之间没有任何分隔符——那不是一行坏文本，是**两行不同的文本被画在了同一行上**：TUI 的状态行，和 `cli.rs` 的启动横幅。横幅自己的开头（`fs-agent: session … · model deepseek-f`）已经看不见了。

## 根因（已用 pty 复现，非推测）

两个写入者同时拥有 tty，而 ratatui 的 inline viewport 是**差分渲染**、假定自己独占终端。

1. `cli.rs:326` 构造 `Harness` → `lib.rs:225` 的 `renderer.spawn(receiver)` 立刻启动 `Tui::run` → `ratatui::init_with_options(Viewport::Inline(8))`。**TUI 从这一刻起拥有终端。**
2. `cli.rs:337` 才 `eprintln!` 启动横幅。它在 TUI 启动**之后**执行（本机 pty 下 6/6 次；这是竞态，取决于被 spawn 的任务何时被调度）。
3. TUI 用裸 `\n` 预留 8 行 live region，然后在最后两行画输入行与状态行。横幅恰好落在这段预留区里的**状态行那一行**。
4. ratatui 的差分缓冲不知道横幅的字形存在（它认为那些格子仍是空格），所以不会擦掉状态行没盖到的部分。

原始字节流顺序（`ESC[6n` 是 ratatui init 的光标位置查询）：

```
<ESC>[6n                              ← renderer init 开始
\n ×7                                 ← 预留 live region
fs-agent: session … <path>\n          ← 野写入的横幅，落进状态行
<ESC>[7;1H >                          ← 画输入行
<ESC>[8;1H ready · … ctrl-c quit      ← 画状态行，覆盖横幅前 62 列
```

算术完全吻合：状态行长 **62 列**，而 `lash` 在横幅里的下标正好是 **62**。所以屏幕上留下 `…ctrl-c quit` + `lash · mode ask · <path>`，与报告里的一字不差。

## 复现

需要一个会回答 CPR（`ESC[6n` → `ESC[r;cR`）的 pty；不回答的话 ratatui 会先 panic 在 `failed to initialize terminal: The cursor position could not be read within a normal duration`（本机第一次尝试就是这样，与 TUI 无关，是假终端的锅）。

- 判据（今天为红）：**启动横幅出现在渲染器首个 `ESC[6n` 之后**。本机 6/6 次命中。
- harness：`target/repro/repro2.py`（throwaway，`target/` 已 gitignore）。跑法：`XDG_DATA_HOME=$PWD/target/repro/data python3 target/repro/repro2.py target/debug/fs-agent $HOME 6`。数据目录必须指向可写处，否则会话建不起来。

## 同类写入（同一根因，未逐一复现）

TUI 活着时 `cli.rs` 还有几处裸 `eprintln!`，都会落进 live region：`380`、`381`、`386`、`390`（`/undo`、`/plan`、`/endplan`、未知命令的反馈）。修法应当一次覆盖这一类，而不是只挪 banner。

对照（这是**对的**做法）：`[context injected: …]` 走 `tui.rs:600` 经渲染接缝输出——原始流里能看到它被 ratatui 带样式、带整行 padding 地推入 scrollback。

## 修复方向

- **推荐**：把启动横幅做成接缝上的一个块（与 `[context injected: …]` 同路），由渲染器 `insert_before` 推进 scrollback。这样「只有一个写入者」由结构保证而不是靠调用顺序，顺带它变得可测（`tests/render_tui.rs` 那种无终端测试）。
- 次选：把 `eprintln!` 挪到 `Harness` 构造之前。能修好 banner，但「渲染器启动后不许写 tty」这条纪律仍然没有落点，上面那几处 `eprintln!` 还会复现同一个 bug。

## Comments

- 本票是 raw bug report 的记录（`/triage` 的产物）。**标 `needs-triage` 的原因**：修复方向二选一需要维护者拍板，以及要确认范围是「只修 banner」还是「这一类写入一起修」。拍板后即可改 `ready-for-agent`。
- 观察（未验证，可能超范围）：spec §A.12 要的是「模型、模式与**预算**」，而横幅只打了 model / mode / 会话目录，没打预算。做接缝化时可能顺手补齐——先确认预算是否已在别处显示。

**落地（2026-09-21）。** 按推荐方向修：横幅走渲染接缝，不是一个字一个字的调用顺序问题。

- **新接缝**：`RenderEvent::Notice(String)` + `Block::Notice(String)` + `RenderHandle::notice`，三个渲染器各自落地（TUI：逐字进 scrollback，DarkGray；plain / headless：逐字进 `stderr_diagnostic`）。**不复用 `Diagnostic`**，因为 TUI 会加 `[diag]` 前缀、plain / headless 同理——那会改写用户看到的那行字。
- **`Harness::notice(&self, message)`**：cli 侧唯一的出口。`Harness` 现在可以把话交给前端，而不必自己碰终端。
- **cli.rs 的五处野写入全部改道**：启动横幅、`/undo` 的两条、`/endplan`、未知命令、`run_one_turn` 的失败，以及 `enter_plan` / `toggle_plan` 两个 helper。
- 组装前的错误（root 拒绝、会话建不起来等）仍是 `eprintln!`——那时渲染器还不存在，本来就该直接写。

### 验证：Phase 1 的 loop 双向都跑过

`scripts/tui-startup-check.py`（pty harness，回答 CPR，等待收敛而不是固定窗口）。判据是**用户那个确切症状**：冻结在状态行首帧，该行不得有状态行之外的文字；横幅必须恰好到达终端一次。

- 修复前（`git stash` 回 src/ 重建的二进制）：**3/3 RED** — `the status row holds foreign text: 'lash · mode ask · <path>'`
- 修复后：**3/3 GREEN** — `status row clean, banner shown once`
- 全量 `cargo test`：438 passed / 0 failed；`cargo clippy --all-targets` 零 warning；`cargo fmt --check` 对本次改动的文件干净（`src/context/repo_map.rs` 与 `tests/repo_map.rs` 在 HEAD 上就未格式化，未动）。

接缝上的 regression test：`tests/render_tui.rs::a_notice_is_a_scrollback_line_shown_as_it_is`、`tests/render_plain.rs::a_notice_reaches_the_diagnostic_sink_verbatim`。

### 一个真实的缺口：CLI 的终端所有权没有 seam

**没有 Rust test 能锁住这个 bug 本身。** TUI 只在真终端上被选中（`IsTerminal`），而 `cli::run` 在内部自建 stdout / stderr sink，所以 `cargo test` 观察不到「什么到达了 tty」。上面那条 pty 脚本是目前唯一的 red-capable 检查——它不该是唯一的那条。

候选深化方向（留给 `/improve-codebase-architecture`，不在本次修复范围）：让 `cli::run` 的 sink / renderer 可注入，于是「启动横幅必须经渲染器」可以是一条普通的集成断言，而不是一个 python 脚本。

### 复盘：什么本可以预防它

- 同一份代码里 `[context injected: …]` **已经**走接缝（`tui.rs:600`），横幅是唯一的例外。规则本身是清楚的，缺的是**落点**：`Harness` 没有「对前端说话」的方法，于是想输出的人只能伸手去够 `eprintln!`。加 `Harness::notice` 比修那一行更重要。
- `tui.rs` 顶部的文档早就写明「渲染器拥有键盘……这就是让输入和输出不打架的原因」——同一条推理对输出同样成立，只是没人写下来。已写进 `Harness::notice` 的 doc。

### 遗留（本次未动，各自独立）

- `OpenedSession::open` 在 `renderer.spawn` **之后**还可能失败（`EventLog` 打不开等）。那条路径上 TUI 已经进了 raw mode，而 `cli.rs` 直接 `eprintln!` 后返回失败——没有 `ratatui::restore()`，终端可能留在 raw mode。与本票同族但不同触发条件，值得单独一票。
- §A.12 的「预算」仍未显示（见上一条观察）。
