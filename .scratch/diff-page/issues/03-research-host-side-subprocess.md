# 03 — research：宿主侧子进程、ANSI 与 `[ui]` 配置的接线

Type: research
Status: resolved
Part of: ../map.md
Blocked by: —

## 问题

[01 号票](01-charting-decisions.md) 定了：**git 由渲染器自己在宿主侧跑**（不进事件流、不进
`messages`、不过权限门与沙箱），**上色内置兜底**，另有 `[ui] diff_viewer` 把那份 diff 交给一个
外部命令（一次性收 stdout、当静态正文画）。这一票只查接线的事实，不写实现。

### 已知起点（charting 时已查，别重复查）

- 渲染层里**没有**「跑一次子进程、收 stdout、带超时」的形状：`src/render/opener.rs:25-37` 用同步
  `std::process::Command` spawn、三条 stdio 全 null、**不等退出**（`child.wait()` 丢给后台线程），
  没有超时也没有 kill；`src/render/viewer.rs` 的 nvim 那一档是交互式 pty（`portable_pty` +
  vt100 解析 + `respond_to`），对一次 `git diff` 过重。完整形状在 `src/tools/process.rs:105-164`
  （tokio + `Stdio::piped` + 进程组 + 超时），但它在**工具域**、过沙箱与权限门。
- 回执习惯：`opener` 的失败由调用侧处理（`src/render/tui.rs:533-541` 的 `take_open_request()` →
  `wording::opened` / `wording::open_failed` → 提示行）。
- **diff 上色的件现成但没有调用方**：`diff_tag(&str) -> DiffTag`（`src/render/highlight.rs:208-218`，
  吃一整行、吐 `Context`/`Added`/`Removed`/`Hunk`）、`DiffTag::style`（`highlight.rs:195-204`，
  吐**背景**样式）、`DiffTag::ansi`（`highlight.rs:185-192`，另一张表，服务 `ansi_line`）、
  `highlight_diff`（`highlight.rs:413-450`，吃整段吐逐行 span，但**写死按 Rust 高亮**）。
  diff 三色**不在** `palette.rs` 里。
- 详情覆盖层加变体的接线：`DetailKind` 五个变体（`src/render/tui.rs:7793-7817`）、`detail_body`
  是唯一 switch（`tui.rs:7976-8075`）、加一个变体 = enum 一项 + switch 一支 + 一个
  `open_*_detail`（照 `open_file_detail`，`tui.rs:3405-3419`）+ `DetailOpener` 一个变体
  （照 `DetailOpener::Files`，`tui.rs:7826-7836`）；正文排版 `folded_text`（`tui.rs:7887-7903`）
  与带前缀列的 `file_body_lines`（`tui.rs:8085-8123`）。
- 页签条加第四签的接线：`Tab` 枚举（`tui.rs:5789`）+ `draw_tab_bar` 的 entries
  （`tui.rs:5435-5471`）+ `draw_sidebar_page` 的 match（`tui.rs:5315`）+ `SwitchTab` 的键盘交接
  （`tui.rs:3157-3168`）+ `sidebar_key` 的吃键范围（`tui.rs:3403`）。**窄档 28 列放得下**
  （三签 16 列 → 四签 21 列）。

### 要查清的

1. **一次只读子进程该长在哪一层、跑在哪个线程上**：渲染器今天**异步回填**的现成形状是哪一个
   （`file_index` 重扫那套 `spawn_blocking` + `mpsc` + `files_loaded` 位？还是别处）？
   主循环在一帧里能等什么、不能等什么（`src/render/tui.rs` 的主循环与 tick）。
   超时与 kill 加在哪一处最省（std 的 `Command` 没有超时；要不要 `wait_timeout` 那一类件，
   还是 tokio 的 `timeout`）—— **查清今天 `Cargo.toml` 里已有哪些依赖能白用**（tokio 的哪些
   feature、`wait-timeout`、`portable-pty` 之类），不要引入新依赖。给出可编译的最小形状。
2. **进程卫生**：要不要 `setpgid` / 进程组（`src/tools/process.rs` 怎么做、为什么）？
   子进程没退出就被丢掉会怎样（opener 那条路今天是什么代价）？要不要清环境变量
   （`GIT_DIR` / `GIT_WORK_TREE` / `PAGER` / `GIT_PAGER`）？
3. **ANSI 那一件到底是什么**：`ansi_line`（`highlight.rs` 里那个）吃什么吐什么、谁来调用、
   它是不是一个**通用 ANSI → span** 的解析器，还是只认得 diff 行首那一个标记？
   `#[cfg(test)]` 之外有没有生产调用方？若它只处理 diff 标记，那么「外部工具吐的 ANSI 输出」
   今天**没有**现成的解析件 —— 如实说明（内部内容 `vt100` 是给整屏网格的，不是给一行的 span）。
4. **`[ui]` 配置的接线**：`[ui] file_viewer` 这一项在配置结构里的位置与文法（`src/config.rs`、
   缺省值、非法值的报错路径），加一个 `diff_viewer` 要动哪几处（结构、缺省、文档
   `docs/render.md` / `docs/configuration.md` 一类、启动检查脚本）。
   顺带查清：这个值是「一个程序名」还是「一条带参数的命令」在今天更适合本仓库的既有文法
   （`CustomTool` 的 argv 模板是什么形状，见 `docs/custom-tools.md`）。
5. **「工作区刚被改过」那条信号今天怎么走到前端**：`WorkspaceChanged` → `file_scan_wanted` 位
   → `FileIndex::begin()` 那条链，逐处给出 `文件:行号`；要再挂第三件事（改动页重取）动哪一处？
   有没有现成的「合并 N 次触发成一次」的守卫可照抄？
6. **加一个左栏页要不要动 `layout.rs` / 手工清单 / 启动检查**：今天加页的既有清单是什么
   （查 `docs/agents/*` 或 `scripts/tui-startup-check.py` 有没有「新增左栏页」这类 checklist）；
   没有就如实说没有。

### 产物

`.scratch/diff-page/research/03-renderer-subprocess-and-config.md`：逐条回答，每条给
`文件:行号` 证据（要引 `docs/` 的写清小节名）。查不到的明说「查不到」，不要推断。
最后一节用 5 行以内说清：**哪几件是顺手可借的，哪几件是新的活**。

## 作答

findings 在 [`.scratch/diff-page/research/03-renderer-subprocess-and-config.md`](../research/03-renderer-subprocess-and-config.md)。

1. 异步回填只有一套现成形状：`file_index` 的「三态 + `take_file_scan` 位 + `mpsc` + `files_loaded`」（`src/render/file_index.rs:24-60`、`src/render/tui.rs:424-425, 454-462, 490, 4434-4462`）；状态机只置位收结果、循环才起任务（`tui.rs:533-541` 是同一个分工）。一帧的 `select!` 只等通道与两个定时器（`tui.rs:486-497`），节拍 60 ms（`tui.rs:6029`）—— git 取数必须 `tokio::spawn` 出去，不能在任何一支里 await。
2. 超时与 kill 零新依赖：`tokio` 的 `time` / `process` / `rt` 与 `libc` 都在（`Cargo.toml:54, 57`），**`wait-timeout` 查不到**。最小形状（`tokio::process::Command` + `process_group(0)` + `kill_on_drop` + `tokio::time::timeout` + 自己 `killpg`）已用一个临时 example `cargo check` / `cargo run` 过，探针已删；`ProcessGroup` 在 `process.rs:236` 是**私有**的。
3. 进程卫生：进程组与 `killpg` 照 `process.rs:126, 236-267`；`GIT_DIR` / `GIT_WORK_TREE` **必清**（实测污染时 git 走 `--no-index` 语义或直接致命错误），`PAGER` / `GIT_PAGER` 清是保险（本机实测非 tty 下 git 不分页）。opener 那条路不 wait、不 kill、无超时，对 git 不适用。
4. `ansi_line` / `DiffTag::ansi` 是**产出** ANSI 的表，不是解析器；`#[cfg(test)]` 之外**没有生产调用方**（`tests/render_highlight.rs:61-66`、`docs/highlight.md`「diff 层为什么还没有调用方」）—— 外部工具吐的 ANSI 今天**没有**现成解析件（`vt100` 是整屏网格，`src/render/viewer.rs:145, 202, 247`）。
5. `[ui]` 接线：`RawUi`（`config.rs:1493-1502`，`deny_unknown_fields`）→ `resolve_ui`（`1147-1185`）→ `Config` 平铺字段，非法值是**启动错误**（`config.rs:2437-2444`）。加 `diff_viewer` 要动 8 处（含 `SessionFacts` 与 10 个完整字面量）；带参命令在 `[tools.*]` 里是 argv 数组（`docs/custom-tools.md`「声明」「校验」），在 `[ui]` 里是新文法。
6. `WorkspaceChanged` 链的逐处行号在 findings 表格里（`mod.rs:206-210` → `tui.rs:2087-2093` → `4434-4440` → `490` → `4444`）；合并守卫照抄「一个位 + 在飞时不发、位留着补发」（`file_index.rs:52-60`）；再挂第三件事动 5 处（位、`take_*`、发活、通道分支、`*_loaded`）。
7. 加一个左栏页：`layout.rs` **不用动**（页签条高度是常量 `TAB_ROWS` / `TAB_RULE_ROWS`，标签宽度画时算，`tui.rs:5506-5546`）；启动检查脚本**不用动**（`scripts/tui-startup-check.py:84-88, 741-751` 只查身份与虚线）；**「新增左栏页」的清单查不到**，既有的 `docs/tui-manual-checklist.md` 要自己加一节。
