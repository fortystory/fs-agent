# 03 research：宿主侧子进程、ANSI 与 `[ui]` 配置的接线

对象是左栏新 `改动` 页（票 01 已定：**git 由渲染器自己在宿主侧跑**，不进事件流、不过权限门与
沙箱；上色内置兜底；另一档 `[ui] diff_viewer` 把 diff 交给外部命令、一次性收 stdout 当静态正
文画）。这一票只查接线，**不写实现**：`src/` 与 `docs/` 一个字没动，只新建了本文。

证据一律 `文件:行号`；引 `docs/` 的写清小节名。查不到的明说「查不到」。票面「已知起点」里
已经查过的那几条（opener、viewer、`diff_tag` 系列、详情覆盖层、页签条、四签列宽）不重复。

---

## 一、一次只读子进程长在哪一层、跑在哪根线程上

### 现成的异步回填形状就是 `file_index` 那一套

渲染器里「后台做一件慢事、结果回主循环」今天只有一套形状，四件套齐全：

1. **一个值带三态**：`FileIndex` = `Idle | Loading | Ready(Arc<Vec<PathBuf>>)`
   （`src/render/file_index.rs:24-35`）。`Loading` 不是装饰 —— 它是并发守卫：`begin()`
   在 `Loading` 上回答 `false`（`file_index.rs:47-60`），于是同一时刻只有一次遍历在飞。
2. **一个请求位**：`TuiState::file_scan_wanted`（`src/render/tui.rs:795`），由状态机置位
   （构造时预热 `tui.rs:1841`、提交一条消息 `tui.rs:4422`、关掉 nvim 浮层 `tui.rs:3342`、
   `WorkspaceChanged` 到达 `tui.rs:2090-2092`）。
3. **一根 `mpsc` 通道**：`files_tx` / `files_rx`（`tui.rs:424-425`），与文件查看器的唤醒通道
   `viewer_tx` / `viewer_rx`（`tui.rs:419-420`）并排。
4. **主循环里发活 + 收结果**：发活在 `tui.rs:454-462`（`take_file_scan()` → `spawn_blocking`
   → `tx.send(file_index::scan(&root))`），收结果在 `tui.rs:490` 的 select 分支 →
   `TuiState::files_loaded`（`tui.rs:4444-4462`）。

遍历本身是同步函数、刻意不知道 tokio：`file_index.rs:149` 的 `scan` 上方注释写着「这是同步的：
调用方负责把它放进 `tokio::task::spawn_blocking`」，模块头 `file_index.rs:1-6` 同一句话。

**这条形状的关键分工**：状态机只置位、只收结果，**自己不 spawn**，所以测试直接调
`TuiState`（`take_file_scan` / `files_loaded`）就能走完整条路，不必起任务 ——
`tests/render_layout.rs:2299-2322` 就是这么测的。改动页照抄这条分工即可。

### 主循环一帧里能等什么、不能等什么

TUI 的 `run` 是一个 async 函数，跑在 `cli.rs:92-105` 的 multi-thread runtime 上
（`tokio::runtime::Builder::new_multi_thread().enable_all()`，`block_on(run(...))`）。

一帧 = 一轮 `loop`（`tui.rs:450-568`）：

- **能等的只有 `select!` 里那几样**：渲染事件、键盘事件流、console 端口、`files_rx`、
  `viewer_rx`、以及两个定时器（`tui.rs:486-497`；重放那一支是 `tui.rs:466-475`）。它们都
  是「已经在动的通道/时钟」，不是新起的 I/O。
- **一帧的节拍是 60 ms**：`PULSE_FRAME`（`tui.rs:6029`）常驻（`tui.rs:446-447`，注释解释了
  为什么用 `interval` 而不是每轮新建 `sleep`）。所以任何一次超过几十毫秒的阻塞都会同时冻住
  键盘与状态行的字形循环。
- **不能等的**：`select!` 之外的那几段是同步的 —— 事件排空 `tui.rs:501-517`、出口处那几件
  （写剪贴板 `tui.rs:525-530`、`take_open_request` → `opener::open` `tui.rs:533-541`）、
  绘制 `tui.rs:544-567`。唯一的例外是 `suspend_and_resume`（`tui.rs:547`），它**故意**阻塞到
  用户 `fg` 回来 —— 那条路已经交还了终端。
- **因此**：一次 `git diff` 绝不能出现在 `select!` 的任何一支里 —— 那会把这一帧挂住。它得
  照文件索引的样子：置位 → 循环起任务 → 结果回通道 → 下一轮（或下一帧的 `select`）收。

`opener` 那条先例把这个模式写全了：状态机交出**一个字符串**（`take_open_request`，
`tui.rs:3086`），循环拿字符串去跑进程、把回执写回（`note_open_receipt`，`tui.rs:3094`；
调用处 `tui.rs:533-541`）。改动页的 git 取数照这个分工，只是回执换成「拿到的那段文本」。

### 超时与 kill 加在哪：今天 Cargo.toml 里能白用的件

`Cargo.toml` 里与这件事有关的现有依赖（**无需新增**）：

| 件 | 位置 | 能干什么 |
| --- | --- | --- |
| `tokio` features `rt-multi-thread, macros, sync, net, time, process, io-util` | `Cargo.toml:54` | `tokio::process::Command`（`process`）、`tokio::time::timeout`（`time`）、`mpsc`（`sync`）、`spawn_blocking`（`rt`）全都在 |
| `libc` | `Cargo.toml:57` | `killpg`（整组杀，见下节） |
| `process-wrap`（含 `tokio1`） | `Cargo.toml:112` | rmcp 的 stdio 传输在用（`src/mcp/rmcp_client.rs:358-361`）；它做的是「`CommandWrap` + `ProcessGroup::leader()`」，渲染层也用得上，但比手写多一层间接 |
| `portable-pty` / `vt100` | `Cargo.toml:119-120` | 只服务 nvim 浮层（`src/render/viewer.rs:145,202,247`），对一次 `git diff` 过重 |

**`wait-timeout` 查不到**：`Cargo.toml` 里没有，`Cargo.lock` 里也 grep 不到（连传递依赖都没有）。

最省的做法是 `tokio` 那三件：`tokio::process::Command` + `tokio::time::timeout` +
`process.rs` 那套 `ProcessGroup`。注意超时**丢弃** future 时：`kill_on_drop(true)` 只杀直接子
进程（`src/tools/process.rs:130-131` 的注释自己写着那是「第二道防线」，够不到孙子进程），所以
要整组杀还得自己 `killpg`。

### 可编译的最小形状（已编译验证）

下面这段**原样**在一个临时 example 里编过、跑过（`cargo check --offline --example …` 通过；
`cargo run` 打印 `Ok(Some(0))` —— 工作区当时干净，`git diff` 输出 0 字节）。探针文件跑完已删，
`examples/` 目录也已删；`git status` 只剩本次会话之前就在的那两个未跟踪项。

```rust
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

/// 子进程的进程组；调用提前散场时杀整棵。
/// 形状照 `src/tools/process.rs:236-267`（那边是私有的，见下）。
struct ProcessGroup {
    pid: u32,
    armed: bool,
}

impl ProcessGroup {
    fn new(pid: u32) -> Self {
        Self { pid, armed: true }
    }
    fn disarm(&mut self) {
        self.armed = false;
    }
    fn kill(&self) {
        // 安全性：`killpg` 只读它拿到的那个 id。
        unsafe {
            libc::killpg(self.pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if self.armed {
            self.kill();
        }
    }
}

/// 跑一次 `git diff`，带墙钟上限；超时或提前散场杀整个组。
async fn git_diff(cwd: &Path, limit: Duration) -> std::io::Result<Option<String>> {
    let child = Command::new("git")
        .args(["diff", "--no-color", "--"])
        .current_dir(cwd)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()?;
    let pid = child.id().unwrap_or(0);
    let mut group = ProcessGroup::new(pid);
    match tokio::time::timeout(limit, child.wait_with_output()).await {
        Ok(result) => {
            let output = result?;
            group.disarm();
            Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
        }
        // 超时：`group` 的 Drop 里 `killpg` 整组。
        Err(_) => Ok(None),
    }
}
```

接线处（循环里，照 `tui.rs:454-462`）：

```rust
if state.take_diff_request() {
    let root = state.cwd().to_path_buf();
    let tx = diff_tx.clone();
    tokio::spawn(async move {
        let _ = tx.send(git_diff(&root, DIFF_LIMIT).await);
    });
}
```

两点推论，写下来免得下一个人重推：`git` 用 `current_dir` 而不是 `git -C`（`tui.rs` 里
state 本来就有 `cwd()`，`file_index` 那条也是这么拿的）；结果通道与 `files_rx` 并排即可
（`tui.rs:424-425` 的邻居位置）。

### 「借用 `process.rs` 那一半」的实情

`src/tools/process.rs` 里有完整形状（tokio + `Stdio::piped` + `process_group(0)` + 超时 +
`killpg`：`process.rs:105-229`），但它是**工具域**的：

- `run` 是 `pub`（`process.rs:105`），签名要一个 `&Sandbox`（`process.rs:109`），也就是要过
  sandbox 的 `wrap`（`process.rs:111`）—— 而票 01 定的是「不过沙箱」，所以直接调用它反而是
  绕路。
- 真正想借的那两件里，`ProcessGroup` 是**私有**的（`process.rs:236`，`struct ProcessGroup`
  没有 `pub`）。要么在渲染层复制这一小段，要么把它提到一个公共处 —— 后者是新的活。

---

## 二、进程卫生

### 进程组：为什么要它

`process.rs` 的模块注释说得最清楚（`process.rs:9-13`）：`Command::process_group(0)`（等价于
`setsid`）让命令进它自己的进程组，于是超时或调用被丢时，`killpg` 杀掉的是**整棵树**，不只是
直接子进程。落地三处：

- `process.rs:126` 的 `.process_group(0)`；
- `process.rs:130-131` 的 `.kill_on_drop(true)`，注释明说它「只够直接子进程，够不到那棵树」；
- `process.rs:236-267` 的 `ProcessGroup`：`kill()` 是 `libc::killpg(pid, SIGKILL)`
  （`process.rs:252-258`），`Drop` 里 `armed` 时自动杀（`process.rs:261-267`），
  `disarm()` 防 PID 复用（`process.rs:247-250`）。

对一次 `git diff` 要不要这套：**要**，但理由与 `bash` 略有不同。`git diff` 自己会退出，可它会
先跑 `core.pager` / hooks / 外部 diff driver —— 那些都在同一个组里，而一次卡住的调用会一直占
着输出管道和一个 task。`mcp` 那侧同一条纪律（`src/mcp/rmcp_client.rs:13-14、358-361`）。

### 「子进程没退出就被丢掉」今天的代价（opener 那条路）

`src/render/opener.rs:25-37`：同步 `Command` spawn、三条 stdio 全 null、**不等退出** ——
`child.wait()` 丢给一条后台线程（`opener.rs:34-36`）。代价：

- **不会留僵尸**：后台线程会 `wait` 收尸。
- **没有超时、没有 kill**：被丢掉的进程照样活着，heng 退出也带不走它。对 `xdg-open` 这是
  好事（浏览器该活多久与我们无关，注释 `opener.rs:23-24` 明说这是有意的）。
- 对 git 不适用：我们要的恰恰是**它的输出**，所以必须等；于是「等的上限」与「超时之后杀谁」
  两件都得自己写，opener 那条路上一个都没有。

### 要不要清环境变量

**要，而且至少 `GIT_DIR` / `GIT_WORK_TREE` 是实测必需的**（本机 git 实测，stdout 是管道）：

- `GIT_DIR=/nonexistent git diff --no-color --` → `警告：不是 git 仓库。使用 --no-index 比较
  工作区之外的两个路径` + usage 行：它不再对着本仓库跑。
- `GIT_WORK_TREE=/nonexistent git diff --no-color --` → `致命错误：该操作必须在一个工作区中运行`。

也就是说，用户 shell 里（或某个 hook 里）留下的这两个变量会让改动页静默地看错东西、或者直接
空。清掉它们是零成本的。

**`PAGER` / `GIT_PAGER` 是保险而不是实测必需**：本机实测 `GIT_PAGER='echo PAGER-RAN' git log
-1 --oneline | head -3` 与 `git -c core.pager='echo PAGER-RAN' --paginate log -1 --oneline`
都没有跑 pager —— stdout 是管道，git 自己就不分页。但这条事实**依赖 git 的实现与版本**，而清
两个变量、再加一个 `--no-pager`（或 `diff --no-ext-diff`）是一行的事。探针里两种都写了
（`env("GIT_PAGER", "cat")` / `env("PAGER", "cat")`）。

顺带记一句查得到的事实：`core.pager` 是用户配置里的一项，值可以是任意命令行（`git help
config` 那一项），所以「谁替我分页」并不由我们决定。

---

## 三、`ansi_line` / `DiffTag::ansi` 到底是什么

**结论：它们是「产出 ANSI」的表与组合器，不是「读 ANSI」的解析器。外部工具吐的 ANSI 今天没
有现成的解析件。**

- `DiffTag::ansi`（`src/render/highlight.rs:185-192`）：一张「标签 → SGR 前缀」的表
  （`Added` → `\x1b[32m`，`Removed` → `\x1b[31m`，`Hunk` → `\x1b[36m`，`Context` → `""`）。
  它的 doc 注释自己写明那是**另一张表**，服务 `ansi_line`。
- `ansi_line`（`highlight.rs:470-490`）：吃**一整行**、吐**带 ANSI 的字符串**。逻辑是
  「`diff_tag(line)` 不是 `Context` 就把整行用一个色包起来；是 `Context` 就走
  `highlight_diff`（`highlight.rs:413-450`）拿逐行 span，再用 `Class::ansi`
  （`highlight.rs:129-141`）拼回 ANSI 文本」。两道输入都通向「吐 ANSI」，没有一处读 ANSI。
- `diff_tag`（`highlight.rs:208-218`）只认**行首那一两个字符**：`+++` / `---` / `@@` → `Hunk`，
  `+` → `Added`，`-` → `Removed`，其余 → `Context`。它不是任何意义上的转义序列扫描器。
- **生产调用方：没有。** `grep -rn ansi_line src/` 只命中定义处与其 doc 引用；真正的调用只
  在 `tests/render_highlight.rs:61-66`。`git log` 里那份说明在 `docs/highlight.md` 的
  「diff 层为什么还没有调用方」一节：工具输出不再直接显示之后，「那个组合函数被删掉
  （提交 `940cd43`），`highlight_diff` 与 `ansi_line` 再也没有调用者」。
- 仓库里唯一会**读** ANSI 的件是 `vt100`，而它是**整屏网格**：`src/render/viewer.rs:145`
  （`vt100::Parser`）、`viewer.rs:202`（`vt100::Parser::new(rows, cols, 0)`）、
  `viewer.rs:247`（`screen()`）、`viewer.rs:406-433`（逐格取样式）。没有一行 span 级的
  ANSI 解析件。`src/render/plain.rs:359` 与 `src/render/severity.rs:46` 也都是产出方向的。

所以：`diff_viewer` 那一档若希望外部工具的颜色**留得住**，那是一块全新的活（自己写一个
SGR → `Style` 的一行解析器，或者退回「把 ANSI 剥掉、只当纯文本画」）。内置兜底那档不受影响，
它要的 `DiffTag::style`（`highlight.rs:195-204`，背景色）+ `diff_tag` 都在（票面已查）。

---

## 四、`[ui]` 配置的接线

### `file_viewer` 今天长什么样

| 环节 | 位置 |
| --- | --- |
| 原始表 | `RawUi`（`src/config.rs:1493-1502`），`#[serde(deny_unknown_fields)]`，字段 `number_style: Option<String>` / `file_viewer: Option<String>` / `file_viewer_width: Option<u16>` |
| 挂在根上 | `RawConfig.ui: Option<RawUi>`（`config.rs:1471-1472`） |
| 解析 | `resolve_ui`（`config.rs:1147-1185`），由 `config.rs:1348` 调一次 |
| 已解析的值 | `UiSettings`（`config.rs:509-521`），**只在 config.rs 内部用**，随后平铺进 `Config` 字段（`config.rs:454-459` 的 `number_style` / `file_viewer`；搬运在 `config.rs:1379-1380`） |
| 查看器枚举 | `FileViewer { Builtin, Nvim }`，缺省 `Builtin`（`config.rs:523-535`）；`FileViewerSettings { kind, width }`（`config.rs:537-556`） |
| 缺省与下界 | `DEFAULT_FILE_VIEWER_WIDTH = 135`（`config.rs:558-559`）、`MIN_FILE_VIEWER_WIDTH = 20`（`config.rs:561-562`） |
| 非法值 | 未知词 → `ConfigError::UnknownFileViewer`（抛出 `config.rs:1165-1169`，文案 `config.rs:2437-2439`、定义 `config.rs:2440`）；宽度太窄 → `ConfigError::FileViewerWidthTooNarrow`（抛出 `config.rs:1176-1179`，文案 `config.rs:2441-2443`、定义 `config.rs:2444`）。两者都是**启动错误**，不是静默回退（注释 `config.rs:1159-1161` 说了理由：写了 `vim` 的人以为配好了） |
| 文档 | 配置示例在 `README.md:110-112`（`[ui]` 三行），说明在 `README.md:144`（nvim 那一档）与 `README.md:146`（`number_style`）；渲染层的说法在 `docs/render.md` 的「外壳」小节（页签、`Ctrl-O`）与「文件页」小节的「内容弹窗有两档」那一条（`docs/render.md:251-258`）。**没有** `docs/configuration.md` 这个文件 —— 配置项的说明今天散在 `README.md` 与各 `docs/*.md` 里 |

渲染器侧是另一个 seam：`SessionFacts`（`src/render/tui.rs:319-354`，派生 `Default`）里
`number_style`（`tui.rs:347`）与 `file_viewer`（`tui.rs:352`）两个字段，组装时由
`src/cli.rs:350-359` 与 `src/cli.rs:697-714` 注入（「渲染器从不伸手去够配置」，`tui.rs:311-315`
是这条纪律的原文）。

### 加一个 `diff_viewer` 要动哪几处

1. `RawUi` 一个字段（`config.rs:1493-1502`）；
2. `resolve_ui` 一支 + 它的错误路径（`config.rs:1147-1185`，照 `file_viewer` 那支
   `config.rs:1160-1170`）；
3. `UiSettings` 一个字段（`config.rs:509-521`）与 `Config` 一个字段（`config.rs:454-459`）
   + 搬运（`config.rs:1379-1380`）；
4. 缺省值常量（照 `config.rs:558-559`）与 `ConfigError` 一个变体（照 `config.rs:2437-2440`）；
5. `SessionFacts` 一个字段（`tui.rs:347-354`）+ 两处注入（`cli.rs:359`、`cli.rs:714`）；
6. **所有 `SessionFacts { … }` 完整字面量**：`src/cli.rs:350`、`src/cli.rs:697`、
   `src/render/tui.rs:8872`，以及 `tests/ask_user_question_tui.rs:33`、
   `tests/history_replay.rs:36`、`tests/render_layout.rs:34`、`tests/render_layout.rs:5850`、
   `tests/render_layout.rs:6960`、`tests/render_tui.rs:47`、`tests/render_answer.rs:25`
   （它们都没有 `..Default::default()`，加字段就得逐个补一行）；
7. 文档：`README.md:110-112` 的 `[ui]` 示例 + 说明段，`docs/render.md` 的「文件页」小节；
8. **启动检查脚本不用动**（见第六节）；手工清单要加一节（见第六节）。

一个附带的事实：`UiSettings` 今天派生 `Copy`（`config.rs:509`）。若 `diff_viewer` 取
`Option<String>`，`Copy` 就得去掉 —— 代价不大，因为 `UiSettings` 只在 `config.rs` 内部出现
（`grep -rn UiSettings src/ tests/` 只有 `config.rs:515, 1147, 1181` 三处）。

### 这个值是「一个程序名」还是「一条带参数的命令」

本仓库两条文法各有先例：

- **argv 数组**：动态工具的 `command = ["git", "status", "--porcelain"]`
  （`docs/custom-tools.md` 的「声明」小节），Rust 侧是 `RawTool.command: Vec<String>`
  （`config.rs:1546`）；「argv 替换：整个元素，永不过 shell」与「`command` 不能为空、第一个
  元素必须是字面量程序名」写在同一份文档的「argv 替换」「校验」两节
  （`docs/custom-tools.md:56-71`、`:93`）。
- **一小撮词、不认识就报错**：`[ui] file_viewer` 与 `[ui] number_style`
  （`config.rs:1152-1170`）。

`[ui]` 这一节今天**没有**数组值的先例：`RawUi` 三个字段一个 `String` 两个 `u16`
（`config.rs:1493-1502`）。所以「带参数的命令」在 `[ui]` 里是**新文法**，而在 `[tools.*]` 里
是既有文法；「一个程序名」则与 `[ui] file_viewer` 完全同形。这一条是事实陈述 —— 选哪条属于
票 06（`06-grilling-external-viewer.md`）的决策，不在这里下结论。

---

## 五、`WorkspaceChanged` → `file_scan_wanted` → `FileIndex::begin()`：逐处行号

| # | 环节 | 位置 |
| --- | --- | --- |
| 1 | 触发（工具收尾，判据是既有的 `Effect`，不是工具名单） | `src/agent.rs:1293-1297`（`touches_workspace(&effect)` → `render.workspace_changed()`） |
| 2 | 触发（`/undo` 自己补一次） | `src/agent/history.rs:195-197` |
| 3 | 发送端 | `RenderHandle::workspace_changed`（`src/render/mod.rs:206-210`，`self.sender.send(RenderEvent::WorkspaceChanged)`） |
| 4 | 事件本体 | `RenderEvent::WorkspaceChanged`（`src/render/mod.rs:96-102`，枚举项在 101 行；注释说明它是静默信号、三个渲染器里只有 TUI 养索引） |
| 5 | 前端接收（唯一置位处） | `TuiState::apply`（`src/render/tui.rs:2087-2093`）；`live_event`（`tui.rs:2613-2622`）非重放时走 `apply`，重放期间压进 `live_buffer` |
| 6 | 请求位 | `tui.rs:795`（字段）+ `tui.rs:1841`（进 TUI 预热）、`tui.rs:4422`（提交）、`tui.rs:3342`（关掉 nvim 浮层）也置位 |
| 7 | 一帧里发活 | `tui.rs:454-462`（`take_file_scan()` → `spawn_blocking(move || tx.send(file_index::scan(&root)))`） |
| 8 | **合并守卫** | `TuiState::take_file_scan`（`tui.rs:4434-4440`：位为假或 `files.begin()` 回答 `false` 就直接返回）+ `FileIndex::begin`（`src/render/file_index.rs:52-60` 的 `Loading` 位） |
| 9 | 结果回来 | 通道 `tui.rs:424-425`、select 分支 `tui.rs:490`、`files_loaded`（`tui.rs:4444-4462`，顺带清掉索引里没有的展开路径、`sync_tokens()`、置脏） |
| 10 | 另外两个渲染器 | `src/render/headless.rs:95`、`src/render/transcript.rs:226` 把它当没看见 |
| 11 | 事件时刻（轨迹页用） | `tui.rs:7360` |
| 12 | 测试 | `tests/render_layout.rs:2299-2322`：连着三次 `WorkspaceChanged` = 一次遍历；一次遍历在飞时不并发；飞着时又改一次，位留着、结果落地后的下一轮补发 |
| 13 | 文档 | `docs/render.md` 的「文件页」小节的**「重扫」**那一条（`docs/render.md:259-263`），原文写「前端那个位把连着几次触发合并成一次遍历：一次遍历在飞时不并发，最多延后一轮」 |

**要再挂第三件事（改动页重取）动哪一处**：五个点，全都在上表里 ——

1. `apply` 里 `WorkspaceChanged` 那一支再置一个位（`tui.rs:2090-2092`）；
2. 一个 `take_diff_request()`，形状与 `take_file_scan` 同（`tui.rs:4434-4440`）；
3. 循环里第二个发活块（照 `tui.rs:454-462`，用 `tokio::spawn` 而不是 `spawn_blocking`，因为
   要 await `tokio::process` 与 `timeout`）；
4. 第二条 `mpsc` 通道（照 `tui.rs:424-425`）与 select 里一个分支（照 `tui.rs:490`）；
5. 一个 `diff_loaded(...)`（照 `tui.rs:4444`，至少置脏）。

**有没有现成的「把 N 次触发合并成一次」的守卫可照抄**：有，就是 8 号那一对 ——「一个 bool
位 + 一个在飞的位」。语义要照抄全：合并的是**位**（连着 N 次只留一次请求），而在飞时不重发、
**不丢**（位留着，下一轮补发）。注意 git 取数与文件扫描有一处不同：文件扫描的 `Loading` 位在
`FileIndex` 里，是因为那个值本身有状态；改动页若只存一份文本，等的位也要自己拿一个。

---

## 六、加一个左栏页，要不要动 `layout.rs` / 手工清单 / 启动检查

**「新增左栏页」的 checklist：查不到。** 全仓（`docs/`、`.scratch/`、`AGENTS.md`）grep
「新增左栏页」「加一页」「新页签」只有本票自己命中。今天的两处清单都不是那种东西：

- `docs/tui-manual-checklist.md`：真终端手工项。它开头有一份**「来源」列表**
  （`docs/tui-manual-checklist.md:5-26`），每轮 feature 把自己的 spec 与编号节加进去
  （最近的例子：文件页 → ㉛ 在 `docs/tui-manual-checklist.md:915`，nvim 查看器 → ㉜ 在
  `docs/tui-manual-checklist.md:950`）。
  也就是说：**新增页要在这里加一节**，但没有一份「加页要动什么」的清单。
- `scripts/tui-startup-check.py`：pty 自动检查。它自己的注释把界线写死了
  （`scripts/tui-startup-check.py:84-88`）：「这个脚本只要求身份在场与虚线的数量，从不要求
  某一页上写着什么」。实际断言就是身份/标记在场 + 虚线数 ≥ 3
  （`scripts/tui-startup-check.py:741-751`）。**加一页不必改这个脚本**：它没有页签数量或页
  内容的常量。（它给首帧留的时间预算是既有的：capture 的 `timeout=20.0`
  `scripts/tui-startup-check.py:283`、安定窗口 6.0 s `:303-327` —— 若新页在启动首帧同步跑
  git，那是唯一会被这个预算约束的地方。）

**`src/render/layout.rs` 不用动**（就几何而言）：

- 左栏的宽与高只看**终端宽**与用户意愿：`sidebar_tier`（`src/render/layout.rs:477-493`）、
  `sidebar_content`（`layout.rs:506-533`）；页签条那两行是常量 `TAB_ROWS = 3`
  （`layout.rs:43`）与 `TAB_RULE_ROWS = 1`（`layout.rs:47`），页区矩形按它们算
  （`layout.rs:451-467`）—— 与**页数**无关。
- 页签标签的宽度是**画的时候算的**：`draw_tab_bar` 交出一串 entries
  （`tui.rs:5435-5472`，`todo` 那条还是有条件的），`draw_label_bar` 按文字宽度逐个摆、用
  `┆` 分隔、剩下的用 `┄` 填满（`tui.rs:5506-5546`）。所以加一签只动 entries，不动几何常量。
- 页内容的匹配是 `draw_sidebar_page` 的 `match state.tab`（`tui.rs:5306-5324`），键位交接在
  `SwitchTab`（`tui.rs:3157-3168`）与 `sidebar_key`（`tui.rs:3581`，只对文件页吃键）。
  （这几处票面「已知起点」已给，此处只标出与本节结论相关的那一句：`layout.rs` 不在其中。）

---

## 七、哪几件顺手可借，哪几件是新的活

- 顺手可借五件：`file_index` 的「请求位 + `mpsc` + 在飞守卫 + `*_loaded` 回填」（`src/render/file_index.rs:24-60`、`src/render/tui.rs:454-462, 490, 4434-4462`）；`src/tools/process.rs` 的进程组与超时形状（`process.rs:105-229, 236-267`）；`diff_tag` / `DiffTag::style` 的内置上色（`src/render/highlight.rs:195-218`）；`[ui] file_viewer` 整条接线（`src/config.rs:1147-1185, 1493-1502`）；`WorkspaceChanged` → 位的合并守卫（`src/render/mod.rs:206-210`、`tui.rs:2087-2093, 4434-4440`）。全部零新增依赖（`Cargo.toml:54, 57`）。
- 新的活五件：渲染层第一个会 await 的子进程 runner（`opener` 不等退出、`process.rs` 在工具域过沙箱）；`ProcessGroup` 要么复制要么提成公共件；`[ui] diff_viewer` 与「接不接 argv」的新文法（归票 06）；`SessionFacts` 加字段牵动的 10 处完整字面量；外部命令的 ANSI 输出**没有**解析件，要留色得自己写。
